//! Le Mans Ultimate's built-in shared-memory interface mapped into TRACE.
//!
//! LMU installs its authoritative SDK headers under
//! `Support/SharedMemoryInterface`. Those proprietary headers are not copied or
//! redistributed here. This crate contains a bounded decoder for the stable V01
//! fields TRACE consumes, with byte offsets validated against the locally installed
//! SDK using its required four-byte structure packing.

use std::collections::BTreeMap;

use trace_domain::{
    CoordinateFrame, DriverInputs, ElapsedNanoseconds, EnvironmentState, FrameSequence, Gear,
    LapObservation, MotionState, NativeTelemetrySample, SessionSeed, TelemetryFrame, Vector3,
    VehicleState, WheelCorner, WheelState, WheelStates,
};

mod adapter;
mod capture;

pub use adapter::{LmuAdapter, LmuSource, SystemLmuSource};
pub use capture::{LmuAvailability, LmuCaptureError, LmuSharedMemory};

const SHARED_MEMORY_SIZE: usize = 324_824;
const SCORING_OFFSET: usize = 1_632;
const SCORING_INFO_SIZE: usize = 548;
const SCORING_VEHICLES_OFFSET: usize = SCORING_OFFSET + 560;
const VEHICLE_SCORING_SIZE: usize = 584;
const TELEMETRY_OFFSET: usize = 128_464;
const TELEMETRY_VEHICLES_OFFSET: usize = TELEMETRY_OFFSET + 4;
const TELEMETRY_SIZE: usize = 1_888;
const MAX_VEHICLES: usize = 104;
const WHEEL_OFFSET: usize = 848;
const WHEEL_SIZE: usize = 260;
const LMU_NATIVE_SCHEMA: &str = "le-mans-ultimate.shared-memory/v01";
const NATIVE_PAYLOAD_MAGIC: &[u8; 4] = b"LMUS";
const NATIVE_PAYLOAD_VERSION: u16 = 1;
type NativeFields = (BTreeMap<String, f64>, BTreeMap<String, i64>);

/// Why an LMU shared-memory snapshot could not be decoded safely.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LmuPageError {
    TooShort { expected: usize, actual: usize },
    InvalidVehicleCount { telemetry: usize, scoring: usize },
    MissingPlayer,
    InvalidValue(&'static str),
}

/// One owned copy of LMU's `LMU_Data` mapping.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LmuSnapshot {
    bytes: Vec<u8>,
    player_telemetry_offset: usize,
    player_scoring_offset: usize,
}

/// Privacy-redacted LMU mapping suitable for a checked-in regression fixture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LmuRedactedFixture {
    pub bytes: Vec<u8>,
    pub simulator_version: Option<String>,
    pub car_model: Option<String>,
    pub track: Option<String>,
    pub session_type: Option<String>,
}

impl LmuSnapshot {
    /// Validates an owned shared-memory copy and resolves the local player in
    /// telemetry and scoring independently.
    ///
    /// # Errors
    ///
    /// Rejects truncated pages, impossible vehicle counts, invalid player indexes,
    /// and snapshots without a matching player scoring record.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, LmuPageError> {
        if bytes.len() < SHARED_MEMORY_SIZE {
            return Err(LmuPageError::TooShort {
                expected: SHARED_MEMORY_SIZE,
                actual: bytes.len(),
            });
        }
        let page = Page::new(&bytes);
        let telemetry_count = usize::from(page.u8(TELEMETRY_OFFSET)?);
        let player_index = usize::from(page.u8(TELEMETRY_OFFSET + 1)?);
        let player_has_vehicle = page.u8(TELEMETRY_OFFSET + 2)? != 0;
        let scoring_count = usize::try_from(page.i32(SCORING_OFFSET + 104)?).map_err(|_| {
            LmuPageError::InvalidVehicleCount {
                telemetry: telemetry_count,
                scoring: 0,
            }
        })?;
        if telemetry_count > MAX_VEHICLES || scoring_count > MAX_VEHICLES {
            return Err(LmuPageError::InvalidVehicleCount {
                telemetry: telemetry_count,
                scoring: scoring_count,
            });
        }
        if !player_has_vehicle || player_index >= telemetry_count {
            return Err(LmuPageError::MissingPlayer);
        }
        let player_telemetry_offset = TELEMETRY_VEHICLES_OFFSET
            .checked_add(player_index.saturating_mul(TELEMETRY_SIZE))
            .ok_or(LmuPageError::MissingPlayer)?;
        let player_id = page.i32(player_telemetry_offset)?;
        let player_scoring_offset = (0..scoring_count)
            .map(|index| SCORING_VEHICLES_OFFSET + index * VEHICLE_SCORING_SIZE)
            .find(|offset| {
                page.u8(*offset + 196).ok() == Some(1) || page.i32(*offset).ok() == Some(player_id)
            })
            .ok_or(LmuPageError::MissingPlayer)?;
        Ok(Self {
            bytes,
            player_telemetry_offset,
            player_scoring_offset,
        })
    }

    /// Converts this snapshot to one canonical TRACE frame.
    ///
    /// # Errors
    ///
    /// Rejects non-finite required values or invalid source ranges.
    pub fn map_frame(
        &self,
        sequence: FrameSequence,
        elapsed: ElapsedNanoseconds,
    ) -> Result<TelemetryFrame, LmuPageError> {
        let page = Page::new(&self.bytes);
        let telemetry = self.player_telemetry_offset;
        let scoring = self.player_scoring_offset;
        let track_length = positive(page.f64(SCORING_OFFSET + 88)?);
        let lap_distance = nonnegative(page.f64(scoring + 104)?);
        let normalized_position = lap_distance
            .zip(track_length)
            .and_then(|(distance, length)| finite_f32((distance / length).clamp(0.0, 1.0)));
        let local_velocity = page.vector3(telemetry + 184, CoordinateFrame::Vehicle)?;
        let physical_rotation_degrees = f64::from(page.f32(telemetry + 692)?);
        let steering_input = page.f64(telemetry + 404)?;
        let steering_angle_rad = (physical_rotation_degrees.is_finite()
            && (90.0..=2_160.0).contains(&physical_rotation_degrees)
            && (-1.0..=1.0).contains(&steering_input))
        .then(|| finite_f32(steering_input * physical_rotation_degrees.to_radians() / 2.0))
        .flatten();
        let wheels = map_wheels(&page, telemetry)?;
        let current_lap_time_ns =
            nonnegative(page.f64(telemetry + 12)? - page.f64(telemetry + 24)?)
                .and_then(seconds_to_ns);
        let sector = current_sector(page.i32(telemetry + 600)?);
        let last_sector_time_ns =
            last_sector_duration(&page, scoring, sector).and_then(seconds_to_ns);
        let gear = page.i32(telemetry + 352)?;
        let (float_fields, integer_fields) = native_fields(&page, telemetry, scoring)?;
        Ok(TelemetryFrame {
            sequence,
            elapsed,
            lap: LapObservation {
                completed_laps: u32::try_from(page.i16(scoring + 100)?).ok(),
                normalized_position,
                current_lap_time_ns,
                simulator_distance_m: lap_distance,
                current_sector_index: sector,
                last_sector_time_ns,
                invalidated: Some(page.u8(telemetry + 745)? != 0),
                tyres_out: None,
            },
            inputs: DriverInputs {
                throttle: ratio(page.f64(telemetry + 388)?),
                brake: ratio(page.f64(telemetry + 396)?),
                clutch: ratio(page.f64(telemetry + 412)?),
                steering_angle_rad,
            },
            vehicle: VehicleState {
                speed_mps: finite_f32(
                    (local_velocity.x.mul_add(
                        local_velocity.x,
                        local_velocity
                            .y
                            .mul_add(local_velocity.y, local_velocity.z * local_velocity.z),
                    ))
                    .sqrt(),
                ),
                engine_rpm: nonnegative_f32(page.f64(telemetry + 356)?),
                gear: Some(map_gear(gear)),
                fuel_litres: nonnegative_f32(page.f64(telemetry + 524)?),
            },
            motion: MotionState {
                position_m: Some(page.vector3(telemetry + 160, CoordinateFrame::SourceWorld)?),
                velocity_mps: Some(local_velocity),
                acceleration_mps2: Some(page.vector3(telemetry + 208, CoordinateFrame::Vehicle)?),
            },
            wheels,
            environment: Some(EnvironmentState {
                ambient_temperature_c: finite_f32(page.f64(SCORING_OFFSET + 228)?),
                track_temperature_c: finite_f32(page.f64(SCORING_OFFSET + 236)?),
                track_grip: None,
            }),
            native: Some(Box::new(NativeTelemetrySample {
                schema: LMU_NATIVE_SCHEMA.into(),
                payload: self.native_payload(),
                float_fields,
                integer_fields,
                text_fields: BTreeMap::new(),
            })),
        })
    }

    /// Extracts session identity from scoring and player telemetry.
    ///
    /// # Errors
    ///
    /// Returns an error when required fields fall outside the validated snapshot.
    pub fn map_session(&self) -> Result<SessionSeed, LmuPageError> {
        let page = Page::new(&self.bytes);
        let telemetry = self.player_telemetry_offset;
        Ok(SessionSeed {
            source_session_id: None,
            car_id: page
                .string(telemetry + 796, 30)
                .or_else(|| page.string(telemetry + 32, 64)),
            track_id: page
                .string(SCORING_OFFSET, 64)
                .or_else(|| page.string(telemetry + 96, 64)),
            layout_id: None,
            session_type: session_type(page.i32(SCORING_OFFSET + 64)?).map(str::to_owned),
        })
    }

    /// Player name exposed by the scoring stream.
    pub fn driver_name(&self) -> Option<String> {
        Page::new(&self.bytes).string(self.player_scoring_offset + 4, 32)
    }

    /// Whether LMU reports this lap as invalid.
    pub fn lap_invalidated(&self) -> bool {
        Page::new(&self.bytes)
            .u8(self.player_telemetry_offset + 745)
            .is_ok_and(|value| value != 0)
    }

    /// Current LMU game phase (0–9 in the validated V01 SDK).
    ///
    /// # Errors
    ///
    /// Returns an error when the scoring region is truncated.
    pub fn game_phase(&self) -> Result<u8, LmuPageError> {
        Page::new(&self.bytes).u8(SCORING_OFFSET + 108)
    }

    /// Whether scoring reports real-time driving rather than the monitor.
    ///
    /// # Errors
    ///
    /// Returns an error when the scoring region is truncated.
    pub fn in_realtime(&self) -> Result<bool, LmuPageError> {
        Ok(Page::new(&self.bytes).u8(SCORING_OFFSET + 115)? != 0)
    }

    /// Numeric game version published in LMU's generic shared-memory region.
    ///
    /// # Errors
    ///
    /// Returns an error when the generic region is truncated.
    pub fn simulator_version(&self) -> Result<Option<String>, LmuPageError> {
        let version = Page::new(&self.bytes).i32(60)?;
        Ok((version > 0).then(|| version.to_string()))
    }

    /// Rebuilds a one-player mapping without driver names, Steam IDs, other cars,
    /// result streams, filesystem paths, or unused expansion bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when session metadata cannot be decoded from this snapshot.
    pub fn redacted_fixture(&self) -> Result<LmuRedactedFixture, LmuPageError> {
        let session = self.map_session()?;
        let simulator_version = self.simulator_version()?;
        let mut bytes = vec![0; SHARED_MEMORY_SIZE];
        copy_bytes(&self.bytes, &mut bytes, 60, 4);
        copy_bytes(&self.bytes, &mut bytes, SCORING_OFFSET, 96);
        copy_bytes(&self.bytes, &mut bytes, SCORING_OFFSET + 104, 12);
        copy_bytes(&self.bytes, &mut bytes, SCORING_OFFSET + 228, 16);
        bytes[TELEMETRY_OFFSET] = 1;
        bytes[TELEMETRY_OFFSET + 1] = 0;
        bytes[TELEMETRY_OFFSET + 2] = 1;
        copy_bytes(
            &self.bytes,
            &mut bytes,
            self.player_telemetry_offset,
            TELEMETRY_SIZE,
        );
        bytes[TELEMETRY_VEHICLES_OFFSET + 828..TELEMETRY_VEHICLES_OFFSET + WHEEL_OFFSET].fill(0);
        for index in 0..4 {
            let expansion = TELEMETRY_VEHICLES_OFFSET + WHEEL_OFFSET + index * WHEEL_SIZE + 242;
            bytes[expansion..expansion + 18].fill(0);
        }
        let source = self.player_scoring_offset;
        let target = SCORING_VEHICLES_OFFSET;
        for (offset, length) in [
            (0, 4),
            (36, 164),
            (200, 32),
            (264, 72),
            (457, 1),
            (464, 8),
            (507, 1),
        ] {
            copy_between(
                &self.bytes,
                &mut bytes,
                source + offset,
                target + offset,
                length,
            );
        }
        // Keep only one player scoring record, resolved by both the player flag and ID.
        bytes[target + 196] = 1;
        bytes[SCORING_OFFSET + 104..SCORING_OFFSET + 108].copy_from_slice(&1_i32.to_le_bytes());
        Ok(LmuRedactedFixture {
            bytes,
            simulator_version,
            car_model: session.car_id,
            track: session.track_id,
            session_type: session.session_type,
        })
    }

    fn native_payload(&self) -> Vec<u8> {
        let segments = [
            &self.bytes[SCORING_OFFSET..SCORING_OFFSET + SCORING_INFO_SIZE],
            &self.bytes
                [self.player_scoring_offset..self.player_scoring_offset + VEHICLE_SCORING_SIZE],
            &self.bytes
                [self.player_telemetry_offset..self.player_telemetry_offset + TELEMETRY_SIZE],
        ];
        let mut payload = Vec::with_capacity(
            8 + segments
                .iter()
                .map(|segment| 4 + segment.len())
                .sum::<usize>(),
        );
        payload.extend_from_slice(NATIVE_PAYLOAD_MAGIC);
        payload.extend_from_slice(&NATIVE_PAYLOAD_VERSION.to_le_bytes());
        payload.extend_from_slice(&0_u16.to_le_bytes());
        for segment in segments {
            payload.extend_from_slice(
                &u32::try_from(segment.len())
                    .expect("native LMU segment fits u32")
                    .to_le_bytes(),
            );
            payload.extend_from_slice(segment);
        }
        payload
    }
}

fn copy_bytes(source: &[u8], target: &mut [u8], offset: usize, length: usize) {
    copy_between(source, target, offset, offset, length);
}

fn copy_between(
    source: &[u8],
    target: &mut [u8],
    source_offset: usize,
    target_offset: usize,
    length: usize,
) {
    target[target_offset..target_offset + length]
        .copy_from_slice(&source[source_offset..source_offset + length]);
}

fn map_wheels(page: &Page<'_>, telemetry: usize) -> Result<WheelStates, LmuPageError> {
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
        let wheel = telemetry + WHEEL_OFFSET + index * WHEEL_SIZE;
        wheels.insert(
            corner,
            WheelState {
                angular_speed_rad_s: finite_f32(page.f64(wheel + 40)?),
                tyre_load_n: nonnegative_f32(page.f64(wheel + 104)?),
                tyre_pressure_pa: nonnegative_f32(page.f64(wheel + 120)? * 1_000.0),
                tyre_core_temperature_c: finite_f32(page.f64(wheel + 204)? - 273.15),
                suspension_travel_m: finite_f32(page.f64(wheel)?),
                ..WheelState::default()
            },
        );
    }
    Ok(wheels)
}

fn native_fields(
    page: &Page<'_>,
    telemetry: usize,
    scoring: usize,
) -> Result<NativeFields, LmuPageError> {
    let mut integer_fields = BTreeMap::new();
    integer_fields.insert(
        "lap_invalidated".into(),
        i64::from(page.u8(telemetry + 745)?),
    );
    integer_fields.insert(
        "track_limits_steps".into(),
        i64::from(page.u8(telemetry + 767)?),
    );
    integer_fields.insert("in_pits".into(), i64::from(page.u8(scoring + 198)?));
    integer_fields.insert("pit_state".into(), i64::from(page.u8(scoring + 457)?));
    let mut float_fields = BTreeMap::new();
    float_fields.insert("fuel_capacity_litres".into(), page.f64(telemetry + 608)?);
    float_fields.insert("delta_best_seconds".into(), page.f64(telemetry + 696)?);
    float_fields.insert("rear_brake_bias".into(), page.f64(telemetry + 664)?);
    Ok((float_fields, integer_fields))
}

#[derive(Clone, Copy)]
struct Page<'a> {
    bytes: &'a [u8],
}

impl<'a> Page<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    fn take<const N: usize>(&self, offset: usize) -> Result<[u8; N], LmuPageError> {
        self.bytes
            .get(offset..offset.saturating_add(N))
            .and_then(|value| value.try_into().ok())
            .ok_or(LmuPageError::TooShort {
                expected: offset.saturating_add(N),
                actual: self.bytes.len(),
            })
    }

    fn u8(&self, offset: usize) -> Result<u8, LmuPageError> {
        self.bytes
            .get(offset)
            .copied()
            .ok_or(LmuPageError::TooShort {
                expected: offset.saturating_add(1),
                actual: self.bytes.len(),
            })
    }

    fn i16(&self, offset: usize) -> Result<i16, LmuPageError> {
        Ok(i16::from_le_bytes(self.take(offset)?))
    }

    fn i32(&self, offset: usize) -> Result<i32, LmuPageError> {
        Ok(i32::from_le_bytes(self.take(offset)?))
    }

    fn f32(&self, offset: usize) -> Result<f32, LmuPageError> {
        Ok(f32::from_le_bytes(self.take(offset)?))
    }

    fn f64(&self, offset: usize) -> Result<f64, LmuPageError> {
        Ok(f64::from_le_bytes(self.take(offset)?))
    }

    fn vector3(&self, offset: usize, frame: CoordinateFrame) -> Result<Vector3, LmuPageError> {
        let vector = Vector3 {
            x: self.f64(offset)?,
            y: self.f64(offset + 8)?,
            z: self.f64(offset + 16)?,
            frame,
        };
        (vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite())
            .then_some(vector)
            .ok_or(LmuPageError::InvalidValue("vector"))
    }

    fn string(&self, offset: usize, length: usize) -> Option<String> {
        let bytes = self.bytes.get(offset..offset.checked_add(length)?)?;
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len());
        let value = String::from_utf8_lossy(&bytes[..end]).trim().to_owned();
        (!value.is_empty()).then_some(value)
    }
}

fn session_type(value: i32) -> Option<&'static str> {
    match value {
        0 => Some("test day"),
        1..=4 => Some("practice"),
        5..=8 => Some("qualifying"),
        9 => Some("warmup"),
        10..=13 => Some("race"),
        _ => None,
    }
}

fn current_sector(value: i32) -> Option<u32> {
    let without_pit = value & 0x7fff_ffff;
    match without_pit {
        0 => Some(2),
        1 => Some(0),
        2 => Some(1),
        _ => None,
    }
}

fn last_sector_duration(page: &Page<'_>, scoring: usize, sector: Option<u32>) -> Option<f64> {
    let first = positive(page.f64(scoring + 152).ok()?);
    let first_two = positive(page.f64(scoring + 160).ok()?);
    let lap = positive(page.f64(scoring + 168).ok()?);
    match sector {
        Some(0) => lap.zip(first_two).map(|(lap, first_two)| lap - first_two),
        Some(1) => first,
        Some(2) => first_two
            .zip(first)
            .map(|(first_two, first)| first_two - first),
        _ => None,
    }
    .filter(|value| value.is_finite() && *value > 0.0)
}

fn map_gear(value: i32) -> Gear {
    match value {
        -1 => Gear::Reverse,
        0 => Gear::Neutral,
        1..=255 => Gear::Forward(u8::try_from(value).expect("bounded forward gear")),
        other => Gear::Unknown(i16::try_from(other).unwrap_or(if other < 0 {
            i16::MIN
        } else {
            i16::MAX
        })),
    }
}

fn ratio(value: f64) -> Option<f32> {
    finite_f32(value).filter(|value| (0.0..=1.0).contains(value))
}

fn positive(value: f64) -> Option<f64> {
    (value.is_finite() && value > 0.0).then_some(value)
}

fn nonnegative(value: f64) -> Option<f64> {
    (value.is_finite() && value >= 0.0).then_some(value)
}

fn nonnegative_f32(value: f64) -> Option<f32> {
    nonnegative(value).and_then(finite_f32)
}

fn finite_f32(value: f64) -> Option<f32> {
    #[allow(clippy::cast_possible_truncation)]
    let value = value as f32;
    value.is_finite().then_some(value)
}

fn seconds_to_ns(value: f64) -> Option<u64> {
    let value = value * 1_000_000_000.0;
    #[allow(clippy::cast_precision_loss)]
    if !value.is_finite() || !(0.0..u64::MAX as f64).contains(&value) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(value.round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_i16(bytes: &mut [u8], offset: usize, value: i16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn write_i32(bytes: &mut [u8], offset: usize, value: i32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_f32(bytes: &mut [u8], offset: usize, value: f32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_f64(bytes: &mut [u8], offset: usize, value: f64) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn write_text(bytes: &mut [u8], offset: usize, value: &str) {
        bytes[offset..offset + value.len()].copy_from_slice(value.as_bytes());
    }

    fn snapshot() -> LmuSnapshot {
        let mut bytes = vec![0; SHARED_MEMORY_SIZE];
        bytes[TELEMETRY_OFFSET] = 1;
        bytes[TELEMETRY_OFFSET + 1] = 0;
        bytes[TELEMETRY_OFFSET + 2] = 1;
        write_i32(&mut bytes, SCORING_OFFSET + 104, 1);
        write_i32(&mut bytes, TELEMETRY_VEHICLES_OFFSET, 42);
        write_i32(&mut bytes, SCORING_VEHICLES_OFFSET, 42);
        bytes[SCORING_VEHICLES_OFFSET + 196] = 1;
        let telemetry = TELEMETRY_VEHICLES_OFFSET;
        let scoring = SCORING_VEHICLES_OFFSET;
        write_text(&mut bytes, SCORING_OFFSET, "Circuit de la Sarthe");
        write_i32(&mut bytes, SCORING_OFFSET + 64, 10);
        write_f64(&mut bytes, SCORING_OFFSET + 88, 13_626.0);
        write_text(&mut bytes, telemetry + 796, "Oreca 07");
        write_text(&mut bytes, scoring + 4, "TRACE DRIVER");
        write_i16(&mut bytes, scoring + 100, 4);
        write_f64(&mut bytes, scoring + 104, 6_813.0);
        write_f64(&mut bytes, scoring + 152, 31.0);
        write_f64(&mut bytes, scoring + 160, 63.0);
        write_f64(&mut bytes, scoring + 168, 95.0);
        write_f64(&mut bytes, telemetry + 12, 700.0);
        write_f64(&mut bytes, telemetry + 24, 652.0);
        write_f64(&mut bytes, telemetry + 160, 10.0);
        write_f64(&mut bytes, telemetry + 168, 2.0);
        write_f64(&mut bytes, telemetry + 176, -5.0);
        write_f64(&mut bytes, telemetry + 184, 0.0);
        write_f64(&mut bytes, telemetry + 192, 0.0);
        write_f64(&mut bytes, telemetry + 200, -70.0);
        write_i32(&mut bytes, telemetry + 352, 4);
        write_f64(&mut bytes, telemetry + 356, 7_500.0);
        write_f64(&mut bytes, telemetry + 388, 0.82);
        write_f64(&mut bytes, telemetry + 396, 0.15);
        write_f64(&mut bytes, telemetry + 404, -0.5);
        write_f64(&mut bytes, telemetry + 412, 0.0);
        write_f64(&mut bytes, telemetry + 524, 57.0);
        write_i32(&mut bytes, telemetry + 600, 2);
        write_f32(&mut bytes, telemetry + 692, 540.0);
        write_f64(&mut bytes, telemetry + 608, 75.0);
        write_f64(&mut bytes, SCORING_OFFSET + 228, 21.0);
        write_f64(&mut bytes, SCORING_OFFSET + 236, 29.0);
        for index in 0..4 {
            let wheel = telemetry + WHEEL_OFFSET + index * WHEEL_SIZE;
            write_f64(&mut bytes, wheel + 40, 62.0);
            write_f64(&mut bytes, wheel + 104, 3_200.0);
            write_f64(&mut bytes, wheel + 120, 180.0);
            write_f64(&mut bytes, wheel + 204, 363.15);
        }
        LmuSnapshot::from_bytes(bytes).expect("valid synthetic LMU snapshot")
    }

    #[test]
    fn rejects_truncated_or_playerless_snapshots() {
        assert!(matches!(
            LmuSnapshot::from_bytes(vec![0; 10]),
            Err(LmuPageError::TooShort { .. })
        ));
        assert_eq!(
            LmuSnapshot::from_bytes(vec![0; SHARED_MEMORY_SIZE]),
            Err(LmuPageError::MissingPlayer)
        );
    }

    #[test]
    fn maps_session_and_rich_player_telemetry() {
        let snapshot = snapshot();
        let session = snapshot.map_session().expect("session");
        assert_eq!(session.track_id.as_deref(), Some("Circuit de la Sarthe"));
        assert_eq!(session.car_id.as_deref(), Some("Oreca 07"));
        assert_eq!(session.session_type.as_deref(), Some("race"));
        assert_eq!(snapshot.driver_name().as_deref(), Some("TRACE DRIVER"));

        let frame = snapshot
            .map_frame(FrameSequence(7), ElapsedNanoseconds(123))
            .expect("frame");
        assert_eq!(frame.lap.completed_laps, Some(4));
        assert_eq!(frame.lap.normalized_position, Some(0.5));
        assert_eq!(frame.lap.current_sector_index, Some(1));
        assert_eq!(frame.lap.last_sector_time_ns, Some(31_000_000_000));
        assert!((frame.inputs.throttle.expect("throttle") - 0.82).abs() < 0.000_1);
        assert_eq!(frame.vehicle.gear, Some(Gear::Forward(4)));
        assert_eq!(frame.vehicle.speed_mps, Some(70.0));
        assert_eq!(frame.wheels.len(), 4);
        assert_eq!(
            frame.native.as_ref().map(|native| native.payload.len()),
            Some(3_040)
        );
        assert_eq!(
            frame.wheels[&WheelCorner::FrontLeft].tyre_pressure_pa,
            Some(180_000.0)
        );
    }

    #[test]
    fn redacted_fixture_removes_identity_and_other_scoring_records() {
        let snapshot = snapshot();
        let original_frame = snapshot
            .map_frame(FrameSequence(1), ElapsedNanoseconds(2))
            .expect("original frame");
        let fixture = snapshot.redacted_fixture().expect("redacted fixture");
        let redacted = LmuSnapshot::from_bytes(fixture.bytes).expect("redacted snapshot");
        assert_eq!(redacted.driver_name(), None);
        assert_eq!(redacted.map_session(), snapshot.map_session());
        let redacted_frame = redacted
            .map_frame(FrameSequence(1), ElapsedNanoseconds(2))
            .expect("redacted frame");
        assert_eq!(redacted_frame.inputs, original_frame.inputs);
        assert_eq!(redacted_frame.vehicle, original_frame.vehicle);
        assert_eq!(redacted_frame.lap, original_frame.lap);
        assert_eq!(redacted_frame.wheels, original_frame.wheels);
    }
}
