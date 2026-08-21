//! Assetto Corsa-specific acquisition and canonical mapping.
//!
//! Vanilla AC shared-memory details remain private to this crate. Phase 1 provides
//! validated page-prefix readers and deterministic mapping; live Windows mapping is
//! Phase 2 work.

mod capture;
mod pages;

pub use capture::{AcAvailability, AcCaptureError, AcSharedMemory, AcSnapshot};

use trace_domain::{
    CoordinateFrame, DriverInputs, ElapsedNanoseconds, EnvironmentState, FrameSequence, Gear,
    LapObservation, MotionState, SessionSeed, TelemetryFrame, Vector3, VehicleState, WheelCorner,
    WheelState, WheelStates,
};

use pages::{GraphicsPage, PhysicsPage, StaticPage};

const STANDARD_GRAVITY_MPS2: f64 = 9.806_65;

/// Failure to parse a documented vanilla AC shared-memory page prefix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcPageError {
    TooShort { expected: usize, actual: usize },
}

/// Maps owned AC page bytes into canonical telemetry.
///
/// This function does not read shared memory itself. A Phase 2 reader must first
/// obtain a packet-stable owned copy.
///
/// # Errors
///
/// Returns [`AcPageError`] when either changing page lacks its documented prefix.
pub fn map_frame(
    physics_bytes: &[u8],
    graphics_bytes: &[u8],
    sequence: FrameSequence,
    elapsed: ElapsedNanoseconds,
) -> Result<TelemetryFrame, AcPageError> {
    let physics = PhysicsPage::parse(physics_bytes)?;
    let graphics = GraphicsPage::parse(graphics_bytes)?;

    let mut wheels = WheelStates::new();
    for (index, corner) in [
        WheelCorner::FrontLeft,
        WheelCorner::FrontRight,
        WheelCorner::RearLeft,
        WheelCorner::RearRight,
    ]
    .into_iter()
    .enumerate()
    {
        wheels.insert(
            corner,
            WheelState {
                tyre_core_temperature_c: finite(physics.tyre_core_temperature(index)),
                suspension_travel_m: finite(physics.suspension_travel(index)),
                ..WheelState::default()
            },
        );
    }

    Ok(TelemetryFrame {
        sequence,
        elapsed,
        lap: LapObservation {
            completed_laps: non_negative_i32(graphics.completed_laps()),
            normalized_position: ratio(graphics.normalized_car_position()),
            current_lap_time_ns: milliseconds_to_nanoseconds(graphics.current_time_ms()),
            // AC's distanceTraveled semantics are not treated as lap distance.
            simulator_distance_m: None,
        },
        inputs: DriverInputs {
            throttle: ratio(physics.gas()),
            brake: ratio(physics.brake()),
            clutch: None,
            // Published AC material does not define a reliable unit here.
            steering_angle_rad: None,
        },
        vehicle: VehicleState {
            speed_mps: non_negative(physics.speed_kmh()).map(|speed| speed / 3.6),
            engine_rpm: u16::try_from(physics.rpm()).ok().map(f32::from),
            gear: Some(map_gear(physics.gear())),
            fuel_litres: non_negative(physics.fuel()),
        },
        motion: MotionState {
            position_m: vector(graphics.car_coordinates(), CoordinateFrame::SourceWorld),
            velocity_mps: vector(physics.velocity(), CoordinateFrame::SourceWorld),
            acceleration_mps2: vector(physics.acceleration_g(), CoordinateFrame::Vehicle).map(
                |value| Vector3 {
                    x: value.x * STANDARD_GRAVITY_MPS2,
                    y: value.y * STANDARD_GRAVITY_MPS2,
                    z: value.z * STANDARD_GRAVITY_MPS2,
                    frame: value.frame,
                },
            ),
        },
        wheels,
        environment: None,
    })
}

/// Extracts session identity and environment from the static AC page.
///
/// # Errors
///
/// Returns [`AcPageError`] when the page lacks its documented prefix.
pub fn map_session(
    static_bytes: &[u8],
) -> Result<(SessionSeed, Option<EnvironmentState>), AcPageError> {
    let page = StaticPage::parse(static_bytes)?;
    let session = SessionSeed {
        source_session_id: None,
        car_id: page.car_model(),
        track_id: page.track(),
        layout_id: None,
        session_type: None,
    };
    let ambient_temperature_c = finite(page.air_temperature());
    let track_temperature_c = finite(page.road_temperature());
    let environment = (ambient_temperature_c.is_some() || track_temperature_c.is_some()).then_some(
        EnvironmentState {
            ambient_temperature_c,
            track_temperature_c,
            track_grip: None,
        },
    );
    Ok((session, environment))
}

fn finite(value: f32) -> Option<f32> {
    value.is_finite().then_some(value)
}

fn non_negative(value: f32) -> Option<f32> {
    (value.is_finite() && value >= 0.0).then_some(value)
}

fn ratio(value: f32) -> Option<f32> {
    (value.is_finite() && (0.0..=1.0).contains(&value)).then_some(value)
}

fn non_negative_i32(value: i32) -> Option<u32> {
    u32::try_from(value).ok()
}

fn milliseconds_to_nanoseconds(value: i32) -> Option<u64> {
    u64::try_from(value).ok()?.checked_mul(1_000_000)
}

fn map_gear(value: i32) -> Gear {
    match value {
        0 => Gear::Reverse,
        1 => Gear::Neutral,
        2..=257 => Gear::Forward(u8::try_from(value - 1).expect("bounded forward gear")),
        other => Gear::Unknown(i16::try_from(other).unwrap_or(if other < 0 {
            i16::MIN
        } else {
            i16::MAX
        })),
    }
}

fn vector(values: [f32; 3], frame: CoordinateFrame) -> Option<Vector3> {
    values
        .iter()
        .all(|value| value.is_finite())
        .then_some(Vector3 {
            x: f64::from(values[0]),
            y: f64::from(values[1]),
            z: f64::from(values[2]),
            frame,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_f32(bytes: &mut [u8], offset: usize, value: f32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_i32(bytes: &mut [u8], offset: usize, value: i32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_utf16(bytes: &mut [u8], offset: usize, slots: usize, value: &str) {
        for (index, unit) in value.encode_utf16().take(slots - 1).enumerate() {
            let start = offset + index * 2;
            bytes[start..start + 2].copy_from_slice(&unit.to_le_bytes());
        }
    }

    #[test]
    fn documented_offsets_map_to_canonical_units() {
        let mut physics = vec![0; pages::PHYSICS_PREFIX_LENGTH];
        put_f32(&mut physics, 4, 0.75);
        put_f32(&mut physics, 8, 0.25);
        put_f32(&mut physics, 12, 20.0);
        put_i32(&mut physics, 16, 4); // third gear in AC encoding
        put_i32(&mut physics, 20, 6_000);
        put_f32(&mut physics, 28, 180.0);
        put_f32(&mut physics, 32, 50.0);
        put_f32(&mut physics, 44, 1.0);
        put_f32(&mut physics, 152, 90.0);
        put_f32(&mut physics, 184, 0.04);

        let mut graphics = vec![0; pages::GRAPHICS_PREFIX_LENGTH];
        put_i32(&mut graphics, 140, 2);
        put_i32(&mut graphics, 148, 12_345);
        put_f32(&mut graphics, 256, 0.5);
        put_f32(&mut graphics, 260, 100.0);

        let frame = map_frame(
            &physics,
            &graphics,
            FrameSequence(7),
            ElapsedNanoseconds(99),
        )
        .expect("valid pages");

        assert_eq!(frame.vehicle.speed_mps, Some(50.0));
        assert_eq!(frame.vehicle.gear, Some(Gear::Forward(3)));
        assert_eq!(frame.lap.current_lap_time_ns, Some(12_345_000_000));
        assert_eq!(frame.motion.position_m.map(|value| value.x), Some(100.0));
        assert_eq!(
            frame.wheels[&WheelCorner::FrontLeft].tyre_core_temperature_c,
            Some(90.0)
        );
    }

    #[test]
    fn static_utf16_identity_and_conditions_are_mapped() {
        let mut page = vec![0; pages::STATIC_PREFIX_LENGTH];
        put_utf16(&mut page, 72, 33, "tatuusfa1");
        put_utf16(&mut page, 140, 33, "mugello");
        put_f32(&mut page, 468, 24.0);
        put_f32(&mut page, 472, 31.0);

        let (session, environment) = map_session(&page).expect("valid static page");
        assert_eq!(session.car_id.as_deref(), Some("tatuusfa1"));
        assert_eq!(session.track_id.as_deref(), Some("mugello"));
        assert_eq!(
            environment.and_then(|value| value.track_temperature_c),
            Some(31.0)
        );
    }

    #[test]
    fn short_and_invalid_values_degrade_without_panicking() {
        assert_eq!(
            map_frame(
                &[0; 12],
                &[0; pages::GRAPHICS_PREFIX_LENGTH],
                FrameSequence(0),
                ElapsedNanoseconds(0)
            ),
            Err(AcPageError::TooShort {
                expected: pages::PHYSICS_PREFIX_LENGTH,
                actual: 12,
            })
        );

        let mut physics = vec![0; pages::PHYSICS_PREFIX_LENGTH];
        put_f32(&mut physics, 4, 1.5);
        put_f32(&mut physics, 28, f32::NAN);
        let frame = map_frame(
            &physics,
            &[0; pages::GRAPHICS_PREFIX_LENGTH],
            FrameSequence(0),
            ElapsedNanoseconds(0),
        )
        .expect("valid page lengths");
        assert_eq!(frame.inputs.throttle, None);
        assert_eq!(frame.vehicle.speed_mps, None);
    }
}
