//! Assetto Corsa-specific acquisition and canonical mapping.
//!
//! Vanilla AC shared-memory details remain private to this crate. Phase 1 provides
//! validated page-prefix readers and deterministic mapping; live Windows mapping is
//! Phase 2 work.

mod adapter;
mod capture;
mod pages;

pub use adapter::{AcAdapter, AcSource, SystemAcSource};
pub use capture::{
    AC_NATIVE_SCHEMA, AcAvailability, AcCaptureError, AcNativePages, AcRedactedFixture,
    AcSharedMemory, AcSnapshot, decode_native_payload,
};

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
            current_sector_index: non_negative_i32(graphics.current_sector_index()),
            last_sector_time_ns: milliseconds_to_nanoseconds(graphics.last_sector_time_ms()),
            tyres_out: physics
                .number_of_tyres_out()
                .and_then(non_negative_i32)
                .and_then(|value| u8::try_from(value).ok())
                .filter(|value| *value <= 4),
        },
        inputs: DriverInputs {
            throttle: ratio(physics.gas()),
            brake: ratio(physics.brake()),
            clutch: physics.clutch().and_then(ratio),
            steering_angle_rad: finite(physics.steering_angle_rad()),
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
        environment: map_environment(&physics),
        native: None,
    })
}

/// Extracts session identity from the static AC page.
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
        layout_id: page.track_configuration(),
        session_type: None,
    };
    Ok((session, None))
}

fn map_environment(physics: &PhysicsPage<'_>) -> Option<EnvironmentState> {
    let ambient_temperature_c = physics.air_temperature_c().and_then(temperature_c);
    let track_temperature_c = physics.road_temperature_c().and_then(temperature_c);
    (ambient_temperature_c.is_some() || track_temperature_c.is_some()).then_some(EnvironmentState {
        ambient_temperature_c,
        track_temperature_c,
        track_grip: None,
    })
}

fn temperature_c(value: f32) -> Option<f32> {
    (value.is_finite() && value != 0.0 && (-50.0..=100.0).contains(&value)).then_some(value)
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
        let mut physics = vec![0; pages::PHYSICS_PAGE_LENGTH];
        put_f32(&mut physics, 4, 0.75);
        put_f32(&mut physics, 8, 0.25);
        put_f32(&mut physics, 12, 20.0);
        put_i32(&mut physics, 16, 4); // third gear in AC encoding
        put_i32(&mut physics, 20, 6_000);
        put_f32(&mut physics, 24, -0.3);
        put_f32(&mut physics, 28, 180.0);
        put_f32(&mut physics, 32, 50.0);
        put_f32(&mut physics, 44, 1.0);
        put_f32(&mut physics, 152, 90.0);
        put_f32(&mut physics, 184, 0.04);
        put_i32(&mut physics, 244, 2);
        put_f32(&mut physics, 288, 24.0);
        put_f32(&mut physics, 292, 31.0);

        let mut graphics = vec![0; pages::GRAPHICS_PREFIX_LENGTH];
        put_i32(&mut graphics, 132, 2);
        put_i32(&mut graphics, 140, 12_345);
        put_i32(&mut graphics, 164, 1);
        put_i32(&mut graphics, 168, 36_370);
        put_f32(&mut graphics, 248, 0.5);
        put_f32(&mut graphics, 252, 100.0);

        let frame = map_frame(
            &physics,
            &graphics,
            FrameSequence(7),
            ElapsedNanoseconds(99),
        )
        .expect("valid pages");

        assert_eq!(frame.vehicle.speed_mps, Some(50.0));
        assert_eq!(frame.vehicle.gear, Some(Gear::Forward(3)));
        assert_eq!(frame.lap.completed_laps, Some(2));
        assert_eq!(frame.lap.normalized_position, Some(0.5));
        assert_eq!(frame.lap.current_lap_time_ns, Some(12_345_000_000));
        assert_eq!(frame.lap.current_sector_index, Some(1));
        assert_eq!(frame.lap.last_sector_time_ns, Some(36_370_000_000));
        assert_eq!(frame.lap.tyres_out, Some(2));
        assert_eq!(frame.inputs.steering_angle_rad, Some(-0.3));
        assert_eq!(
            frame
                .environment
                .and_then(|value| value.track_temperature_c),
            Some(31.0)
        );
        assert_eq!(frame.motion.position_m.map(|value| value.x), Some(100.0));
        assert_eq!(
            frame.wheels[&WheelCorner::FrontLeft].tyre_core_temperature_c,
            Some(90.0)
        );
    }

    #[test]
    fn static_utf16_identity_is_mapped_without_deprecated_conditions() {
        let mut page = vec![0; pages::STATIC_PAGE_LENGTH];
        put_utf16(&mut page, 68, 33, "tatuusfa1");
        put_utf16(&mut page, 134, 33, "mugello");
        put_utf16(&mut page, 524, 33, "layout_gp");

        let (session, environment) = map_session(&page).expect("valid static page");
        assert_eq!(session.car_id.as_deref(), Some("tatuusfa1"));
        assert_eq!(session.track_id.as_deref(), Some("mugello"));
        assert_eq!(session.layout_id.as_deref(), Some("layout_gp"));
        assert_eq!(environment, None);
    }

    #[test]
    fn documented_native_inventory_reaches_the_end_of_every_page() {
        let mut physics = vec![0; pages::PHYSICS_PAGE_LENGTH];
        let mut graphics = vec![0; pages::GRAPHICS_PAGE_LENGTH];
        let mut static_page = vec![0; pages::STATIC_PAGE_LENGTH];
        put_f32(&mut physics, 576, 12.5);
        put_f32(&mut graphics, 292, 270.0);
        put_i32(&mut static_page, 680, 42);
        put_utf16(&mut static_page, 604, 33, "skin_01");
        let snapshot = AcSnapshot::from_pages(physics, graphics, static_page).expect("snapshot");

        let frame = snapshot
            .map_frame(FrameSequence(0), ElapsedNanoseconds(0))
            .expect("frame");
        let native = frame.native.expect("native fields");

        assert!((native.float_fields["physics.local_velocity.2"] - 12.5).abs() < f64::EPSILON);
        assert!(
            (native.float_fields["graphics.wind_direction_degrees"] - 270.0).abs() < f64::EPSILON
        );
        assert_eq!(native.integer_fields["static.pit_window_end"], 42);
        assert_eq!(native.text_fields["static.car_skin"], "skin_01");
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
        assert_eq!(frame.environment, None);
    }

    #[test]
    fn maps_the_documented_clutch_input_when_the_full_page_is_available() {
        let mut physics = vec![0; pages::PHYSICS_PAGE_LENGTH];
        put_f32(&mut physics, 364, 0.42);
        let frame = map_frame(
            &physics,
            &[0; pages::GRAPHICS_PREFIX_LENGTH],
            FrameSequence(0),
            ElapsedNanoseconds(0),
        )
        .expect("valid page lengths");

        assert_eq!(frame.inputs.clutch, Some(0.42));
    }
}
