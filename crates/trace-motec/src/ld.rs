use std::{
    collections::BTreeMap,
    io::Cursor,
    panic::{AssertUnwindSafe, catch_unwind},
};

use i3rs_core::LdFile;
use quick_xml::{Reader as XmlReader, events::Event as XmlEvent};
use trace_domain::{
    CoordinateFrame, ElapsedNanoseconds, EnvironmentState, FrameSequence, Gear, MotionState,
    NativeTelemetrySample, TelemetryFrame, Vector3, WheelCorner, WheelState,
};

const LD_MAGIC: u8 = 0x40;
const MINIMUM_LD_BYTES: usize = 0x6e2;
const NATIVE_SCHEMA: &str = "motec.i2.ld/community-3";
const U64_EXCLUSIVE_UPPER_F64: f64 = 18_446_744_073_709_551_616.0;

/// Resource limits applied before and after decoding a native log pair.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LdImportLimits {
    pub max_ld_bytes: usize,
    pub max_ldx_bytes: usize,
    pub max_channels: usize,
    pub max_samples_per_channel: u32,
    pub max_output_frames: u64,
    pub max_sample_rate_hz: u16,
    pub max_lap_boundaries: usize,
}

impl Default for LdImportLimits {
    fn default() -> Self {
        Self {
            max_ld_bytes: 512 * 1024 * 1024,
            max_ldx_bytes: 1024 * 1024,
            max_channels: 512,
            max_samples_per_channel: 20_000_000,
            max_output_frames: 20_000_000,
            max_sample_rate_hz: 500,
            max_lap_boundaries: 10_000,
        }
    }
}

/// One native channel available in an `.ld` file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LdChannel {
    pub name: String,
    pub short_name: String,
    pub unit: String,
    pub sample_rate_hz: u16,
    pub sample_count: u32,
    pub data_type: String,
    pub imported: bool,
}

/// One lap crossing reported by the `.ldx` sidecar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LapBoundary {
    pub name: String,
    pub elapsed_ns: u64,
}

/// Bounded metadata decoded from an `.ldx` sidecar.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LdxMetadata {
    pub version: Option<String>,
    pub total_laps: Option<u32>,
    pub fastest_lap: Option<u32>,
    pub fastest_time_ns: Option<u64>,
    pub boundaries: Vec<LapBoundary>,
}

/// Session and channel information decoded before frame iteration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LdImportMetadata {
    pub date: String,
    pub time: String,
    pub driver: String,
    pub vehicle_id: String,
    pub venue: String,
    pub session_type: Option<String>,
    pub event_name: Option<String>,
    pub source_comment: Option<String>,
    pub channels: Vec<LdChannel>,
    pub ldx: Option<LdxMetadata>,
    pub output_rate_hz: u16,
    pub frame_count: u64,
}

/// Validation or decoding failure for a native log pair.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LdImportError {
    InvalidLimits,
    SourceTooLarge {
        bytes: usize,
        maximum: usize,
    },
    SidecarTooLarge {
        bytes: usize,
        maximum: usize,
    },
    InvalidHeader,
    ParserFailed(String),
    ParserPanicked,
    TooManyChannels {
        channels: usize,
        maximum: usize,
    },
    InvalidSampleRate {
        channel: String,
        rate_hz: u16,
    },
    TooManySamples {
        channel: String,
        samples: u32,
        maximum: u32,
    },
    TooManyFrames {
        frames: u64,
        maximum: u64,
    },
    NoReadableChannels,
    InvalidSidecar(String),
    TooManyLapBoundaries {
        maximum: usize,
    },
    LapBoundaryMovedBackwards,
    LapBoundaryOutsideLog {
        elapsed_ns: u64,
        duration_ns: u64,
    },
}

struct DecodedChannel {
    name: String,
    unit: String,
    sample_rate_hz: u16,
    values: Vec<f64>,
    mapping: LdMapping,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LdMapping {
    Native,
    Throttle,
    Brake,
    Clutch,
    Steering,
    Speed,
    EngineRpm,
    Fuel,
    FuelCapacity,
    Gear,
    PositionX,
    PositionY,
    PositionZ,
    LapPosition,
    TyresOut,
    LastSectorTime,
    AmbientTemperature,
    TrackTemperature,
    TrackGrip,
    WheelAngularSpeed(WheelCorner),
    TyrePressure(WheelCorner),
    TyreCoreTemperature(WheelCorner),
    SuspensionTravel(WheelCorner),
}

/// Streaming canonical view of one bounded `.ld`/`.ldx` pair.
pub struct MotecLdReader {
    metadata: LdImportMetadata,
    channels: Vec<DecodedChannel>,
    next_sequence: u64,
    previous_completed_laps: Option<usize>,
    previous_last_sector_time_ns: Option<u64>,
    current_sector_index: u32,
}

impl MotecLdReader {
    /// Parses a native log and optional sidecar, then decodes bounded source columns.
    ///
    /// The underlying community parser is isolated behind pre-validation, explicit
    /// limits, and panic containment. TRACE does not expose its writer.
    ///
    /// # Errors
    ///
    /// Rejects malformed, oversized, unsupported, or internally inconsistent input.
    #[allow(clippy::too_many_lines)]
    pub fn new(
        ld_bytes: Vec<u8>,
        sidecar_bytes: Option<&[u8]>,
        limits: LdImportLimits,
    ) -> Result<Self, LdImportError> {
        validate_limits(limits)?;
        if ld_bytes.len() > limits.max_ld_bytes {
            return Err(LdImportError::SourceTooLarge {
                bytes: ld_bytes.len(),
                maximum: limits.max_ld_bytes,
            });
        }
        if ld_bytes.len() < MINIMUM_LD_BYTES || ld_bytes.first() != Some(&LD_MAGIC) {
            return Err(LdImportError::InvalidHeader);
        }
        let log = catch_unwind(AssertUnwindSafe(|| LdFile::from_bytes(ld_bytes)))
            .map_err(|_| LdImportError::ParserPanicked)?
            .map_err(LdImportError::ParserFailed)?;
        if log.channels.len() > limits.max_channels {
            return Err(LdImportError::TooManyChannels {
                channels: log.channels.len(),
                maximum: limits.max_channels,
            });
        }

        let mut channel_metadata = Vec::with_capacity(log.channels.len());
        let mut channels = Vec::with_capacity(log.channels.len());
        let mut output_rate_hz = 0_u16;
        let mut native_names = BTreeMap::<String, usize>::new();
        for channel in &log.channels {
            if channel.freq == 0 || channel.freq > limits.max_sample_rate_hz {
                return Err(LdImportError::InvalidSampleRate {
                    channel: channel.name.clone(),
                    rate_hz: channel.freq,
                });
            }
            if channel.n_data > limits.max_samples_per_channel {
                return Err(LdImportError::TooManySamples {
                    channel: channel.name.clone(),
                    samples: channel.n_data,
                    maximum: limits.max_samples_per_channel,
                });
            }
            let values = catch_unwind(AssertUnwindSafe(|| log.read_channel_data(channel)))
                .map_err(|_| LdImportError::ParserPanicked)?;
            let imported = values.is_some();
            channel_metadata.push(LdChannel {
                name: channel.name.clone(),
                short_name: channel.short_name.clone(),
                unit: channel.unit.clone(),
                sample_rate_hz: channel.freq,
                sample_count: channel.n_data,
                data_type: channel.data_type.name().into(),
                imported,
            });
            let Some(values) = values else {
                continue;
            };
            if values.len() != channel.n_data as usize {
                return Err(LdImportError::ParserFailed(format!(
                    "channel {:?} ended before its declared sample count",
                    channel.name
                )));
            }
            output_rate_hz = output_rate_hz.max(channel.freq);
            channels.push(DecodedChannel {
                name: unique_name(&channel.name, channel.index, &mut native_names),
                unit: channel.unit.clone(),
                sample_rate_hz: channel.freq,
                values,
                mapping: ld_mapping(&channel.name, &channel.unit),
            });
        }
        if channels.is_empty() || output_rate_hz == 0 {
            return Err(LdImportError::NoReadableChannels);
        }
        let frame_count = channels
            .iter()
            .map(|channel| {
                div_ceil(
                    u64::try_from(channel.values.len())
                        .unwrap_or(u64::MAX)
                        .saturating_mul(u64::from(output_rate_hz)),
                    u64::from(channel.sample_rate_hz),
                )
            })
            .max()
            .unwrap_or(0);
        if frame_count > limits.max_output_frames {
            return Err(LdImportError::TooManyFrames {
                frames: frame_count,
                maximum: limits.max_output_frames,
            });
        }
        let duration_ns = elapsed_ns(frame_count.saturating_sub(1), output_rate_hz);
        let ldx = sidecar_bytes
            .map(|bytes| parse_ldx(bytes, duration_ns, limits))
            .transpose()?;
        let session_type = nonempty(&log.event.session);
        let event_name = nonempty(&log.event.event_name);
        let source_comment = [
            nonempty(&log.session.short_comment),
            nonempty(&log.event.comment),
            nonempty(&log.event.vehicle_comment),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        let metadata = LdImportMetadata {
            date: log.session.date.clone(),
            time: log.session.time.clone(),
            driver: log.session.driver.clone(),
            vehicle_id: log.session.vehicle_id.clone(),
            venue: log.session.venue.clone(),
            session_type,
            event_name,
            source_comment: (!source_comment.is_empty()).then_some(source_comment),
            channels: channel_metadata,
            ldx,
            output_rate_hz,
            frame_count,
        };
        Ok(Self {
            metadata,
            channels,
            next_sequence: 0,
            previous_completed_laps: None,
            previous_last_sector_time_ns: None,
            current_sector_index: 0,
        })
    }

    /// Returns decoded identity, channel inventory, lap markers, and output shape.
    pub fn metadata(&self) -> &LdImportMetadata {
        &self.metadata
    }

    /// Decodes the next frame on the highest native sample-rate grid.
    pub fn next_frame(&mut self) -> Option<TelemetryFrame> {
        if self.next_sequence >= self.metadata.frame_count {
            return None;
        }
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        let elapsed = elapsed_ns(sequence, self.metadata.output_rate_hz);
        let mut frame = TelemetryFrame {
            sequence: FrameSequence(sequence),
            elapsed: ElapsedNanoseconds(elapsed),
            native: Some(Box::new(NativeTelemetrySample {
                schema: NATIVE_SCHEMA.into(),
                ..NativeTelemetrySample::default()
            })),
            ..TelemetryFrame::default()
        };
        let mut source_position = [None; 3];
        for channel in &self.channels {
            let index = sample_index(
                sequence,
                self.metadata.output_rate_hz,
                channel.sample_rate_hz,
                channel.values.len(),
            );
            let Some(value) = channel
                .values
                .get(index)
                .copied()
                .filter(|value| value.is_finite())
            else {
                continue;
            };
            if let Some(native) = frame.native.as_deref_mut() {
                native.float_fields.insert(channel.name.clone(), value);
                if sequence == 0 && !channel.unit.is_empty() {
                    native
                        .text_fields
                        .insert(format!("unit.{}", channel.name), channel.unit.clone());
                }
            }
            apply_ld_mapping(&mut frame, &mut source_position, channel.mapping, value);
        }
        // ACTI names its horizontal plane X/Y and height Z. TRACE's map consumes X/Z.
        if let [Some(source_x), Some(source_y), Some(source_z)] = source_position {
            frame.motion = MotionState {
                position_m: Some(Vector3 {
                    x: source_x,
                    y: source_z,
                    z: source_y,
                    frame: CoordinateFrame::SourceWorld,
                }),
                ..MotionState::default()
            };
        }
        if let Some(ldx) = &self.metadata.ldx {
            let completed = ldx
                .boundaries
                .partition_point(|boundary| boundary.elapsed_ns <= elapsed);
            frame.lap.completed_laps = u32::try_from(completed).ok();
            let start = completed
                .checked_sub(1)
                .and_then(|index| ldx.boundaries.get(index))
                .map_or(0, |boundary| boundary.elapsed_ns);
            frame.lap.current_lap_time_ns = Some(elapsed.saturating_sub(start));
            if self.previous_completed_laps != Some(completed) {
                self.current_sector_index = 0;
            } else if let Some(last_sector_time_ns) = frame.lap.last_sector_time_ns
                && self
                    .previous_last_sector_time_ns
                    .is_some_and(|previous| previous != last_sector_time_ns)
            {
                self.current_sector_index = self.current_sector_index.saturating_add(1);
            }
            self.previous_completed_laps = Some(completed);
            self.previous_last_sector_time_ns = frame.lap.last_sector_time_ns;
            frame.lap.current_sector_index = Some(self.current_sector_index);
        }
        Some(frame)
    }
}

fn validate_limits(limits: LdImportLimits) -> Result<(), LdImportError> {
    if limits.max_ld_bytes < MINIMUM_LD_BYTES
        || limits.max_ldx_bytes == 0
        || limits.max_channels == 0
        || limits.max_samples_per_channel == 0
        || limits.max_output_frames == 0
        || limits.max_sample_rate_hz == 0
        || limits.max_lap_boundaries == 0
    {
        return Err(LdImportError::InvalidLimits);
    }
    Ok(())
}

fn parse_ldx(
    bytes: &[u8],
    duration_ns: u64,
    limits: LdImportLimits,
) -> Result<LdxMetadata, LdImportError> {
    if bytes.len() > limits.max_ldx_bytes {
        return Err(LdImportError::SidecarTooLarge {
            bytes: bytes.len(),
            maximum: limits.max_ldx_bytes,
        });
    }
    let mut reader = XmlReader::from_reader(Cursor::new(bytes));
    reader.config_mut().trim_text(true);
    let mut metadata = LdxMetadata::default();
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(XmlEvent::Start(element)) if element.name().as_ref() == b"LDXFile" => {
                for attribute in element.attributes().with_checks(true) {
                    let attribute = attribute
                        .map_err(|error| LdImportError::InvalidSidecar(error.to_string()))?;
                    if attribute.key.as_ref() == b"Version" {
                        metadata.version = Some(
                            attribute
                                .decode_and_unescape_value(reader.decoder())
                                .map_err(|error| LdImportError::InvalidSidecar(error.to_string()))?
                                .into_owned(),
                        );
                    }
                }
            }
            Ok(XmlEvent::Empty(element)) if element.name().as_ref() == b"String" => {
                let attributes = attributes(&element, &reader)?;
                match attributes.get("Id").map(String::as_str) {
                    Some("Total Laps") => {
                        metadata.total_laps =
                            attributes.get("Value").and_then(|value| value.parse().ok());
                    }
                    Some("Fastest Lap") => {
                        metadata.fastest_lap =
                            attributes.get("Value").and_then(|value| value.parse().ok());
                    }
                    Some("Fastest Time") => {
                        metadata.fastest_time_ns = attributes
                            .get("Value")
                            .and_then(|value| parse_duration_ns(value));
                    }
                    _ => {}
                }
            }
            Ok(XmlEvent::Empty(element)) if element.name().as_ref() == b"Marker" => {
                if metadata.boundaries.len() >= limits.max_lap_boundaries {
                    return Err(LdImportError::TooManyLapBoundaries {
                        maximum: limits.max_lap_boundaries,
                    });
                }
                let values = attributes(&element, &reader)?;
                let Some(raw_time) = values.get("Time") else {
                    buffer.clear();
                    continue;
                };
                let elapsed_ns = microseconds_to_ns(raw_time)?;
                if metadata
                    .boundaries
                    .last()
                    .is_some_and(|previous| elapsed_ns <= previous.elapsed_ns)
                {
                    return Err(LdImportError::LapBoundaryMovedBackwards);
                }
                if elapsed_ns > duration_ns.saturating_add(1_000_000_000) {
                    return Err(LdImportError::LapBoundaryOutsideLog {
                        elapsed_ns,
                        duration_ns,
                    });
                }
                metadata.boundaries.push(LapBoundary {
                    name: values.get("Name").cloned().unwrap_or_default(),
                    elapsed_ns,
                });
            }
            Ok(XmlEvent::Eof) => break,
            Err(error) => return Err(LdImportError::InvalidSidecar(error.to_string())),
            _ => {}
        }
        buffer.clear();
    }
    if metadata
        .total_laps
        .is_some_and(|total| usize::try_from(total).ok() != Some(metadata.boundaries.len() + 1))
    {
        return Err(LdImportError::InvalidSidecar(
            "total laps does not match the number of lap boundaries".into(),
        ));
    }
    Ok(metadata)
}

fn attributes(
    element: &quick_xml::events::BytesStart<'_>,
    reader: &XmlReader<Cursor<&[u8]>>,
) -> Result<BTreeMap<String, String>, LdImportError> {
    let mut values = BTreeMap::new();
    for attribute in element.attributes().with_checks(true) {
        let attribute =
            attribute.map_err(|error| LdImportError::InvalidSidecar(error.to_string()))?;
        let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
        let value = attribute
            .decode_and_unescape_value(reader.decoder())
            .map_err(|error| LdImportError::InvalidSidecar(error.to_string()))?
            .into_owned();
        values.insert(key, value);
    }
    Ok(values)
}

fn microseconds_to_ns(value: &str) -> Result<u64, LdImportError> {
    let microseconds = value
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| LdImportError::InvalidSidecar("invalid marker time".into()))?;
    let nanoseconds = microseconds * 1_000.0;
    if !(0.0..U64_EXCLUSIVE_UPPER_F64).contains(&nanoseconds) {
        return Err(LdImportError::InvalidSidecar(
            "marker time is outside the supported range".into(),
        ));
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(nanoseconds.round() as u64)
}

fn parse_duration_ns(value: &str) -> Option<u64> {
    let mut parts = value.split(':');
    let first = parts.next()?;
    let second = parts.next();
    if parts.next().is_some() {
        return None;
    }
    let seconds = if let Some(second) = second {
        first.parse::<f64>().ok()? * 60.0 + second.parse::<f64>().ok()?
    } else {
        first.parse::<f64>().ok()?
    };
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    let nanoseconds = seconds * 1_000_000_000.0;
    if !(0.0..U64_EXCLUSIVE_UPPER_F64).contains(&nanoseconds) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(nanoseconds.round() as u64)
}

fn ld_mapping(name: &str, unit: &str) -> LdMapping {
    let name = normalized(name);
    let unit = normalized(unit);
    match name.as_str() {
        "throttlepos" if unit == "%" => LdMapping::Throttle,
        "brakepos" if unit == "%" => LdMapping::Brake,
        "clutchpos" if unit == "%" => LdMapping::Clutch,
        "steeringangle" if unit == "deg" => LdMapping::Steering,
        "groundspeed" if unit == "km/h" => LdMapping::Speed,
        "enginerpm" if unit == "rpm" => LdMapping::EngineRpm,
        "fuellevel" if matches!(unit.as_str(), "l" | "litre" | "litres") => LdMapping::Fuel,
        "maxfuel" if matches!(unit.as_str(), "l" | "litre" | "litres") => LdMapping::FuelCapacity,
        "gear" => LdMapping::Gear,
        "carcoordx" if unit == "m" => LdMapping::PositionX,
        "carcoordy" if unit == "m" => LdMapping::PositionY,
        "carcoordz" if unit == "m" => LdMapping::PositionZ,
        "carposnorm" => LdMapping::LapPosition,
        "numtiresofftrack" => LdMapping::TyresOut,
        "lastsectortime" if unit == "s" => LdMapping::LastSectorTime,
        "airtemp" if unit == "c" => LdMapping::AmbientTemperature,
        "roadtemp" if unit == "c" => LdMapping::TrackTemperature,
        "surfacegrip" if unit == "%" => LdMapping::TrackGrip,
        _ => wheel_mapping(&name, &unit).unwrap_or(LdMapping::Native),
    }
}

fn wheel_mapping(name: &str, unit: &str) -> Option<LdMapping> {
    let (prefix, corner) = split_corner(name)?;
    match (prefix, unit) {
        ("wheelangularspeed", "rad/s") => Some(LdMapping::WheelAngularSpeed(corner)),
        ("tirepressure", "psi") => Some(LdMapping::TyrePressure(corner)),
        ("tiretempcore", "c") => Some(LdMapping::TyreCoreTemperature(corner)),
        ("suspensiontravel", "mm") => Some(LdMapping::SuspensionTravel(corner)),
        _ => None,
    }
}

fn split_corner(name: &str) -> Option<(&str, WheelCorner)> {
    for (suffix, corner) in [
        ("fl", WheelCorner::FrontLeft),
        ("fr", WheelCorner::FrontRight),
        ("rl", WheelCorner::RearLeft),
        ("rr", WheelCorner::RearRight),
    ] {
        if let Some(prefix) = name.strip_suffix(suffix) {
            return Some((prefix, corner));
        }
    }
    None
}

#[allow(clippy::too_many_lines)]
fn apply_ld_mapping(
    frame: &mut TelemetryFrame,
    source_position: &mut [Option<f64>; 3],
    mapping: LdMapping,
    value: f64,
) {
    match mapping {
        LdMapping::Throttle => frame.inputs.throttle = percent(value),
        LdMapping::Brake => frame.inputs.brake = percent(value),
        LdMapping::Clutch => frame.inputs.clutch = percent(value),
        LdMapping::Steering => {
            frame.inputs.steering_angle_rad = finite_f32(value.to_radians());
        }
        LdMapping::Speed => frame.vehicle.speed_mps = nonnegative_f32(value / 3.6),
        LdMapping::EngineRpm => frame.vehicle.engine_rpm = nonnegative_f32(value),
        LdMapping::Fuel => frame.vehicle.fuel_litres = nonnegative_f32(value),
        LdMapping::FuelCapacity => {
            if value.is_finite()
                && value > 0.0
                && let Some(native) = frame.native.as_deref_mut()
            {
                native
                    .float_fields
                    .insert("static.max_fuel_litres".into(), value);
            }
        }
        LdMapping::Gear => {
            frame.vehicle.gear = integral_i16(value).map(|gear| match gear {
                -1 => Gear::Reverse,
                0 => Gear::Neutral,
                1..=255 => Gear::Forward(u8::try_from(gear).expect("guarded gear range")),
                _ => Gear::Unknown(gear),
            });
        }
        // ACTI reflects AC's world X axis relative to the fast-lane spline frame.
        LdMapping::PositionX => source_position[0] = Some(-value),
        LdMapping::PositionY => source_position[1] = Some(value),
        LdMapping::PositionZ => source_position[2] = Some(value),
        LdMapping::LapPosition => {
            frame.lap.normalized_position =
                finite_f32(value).filter(|value| (0.0..=1.0).contains(value));
        }
        LdMapping::TyresOut => {
            frame.lap.tyres_out = integral_i16(value).and_then(|value| u8::try_from(value).ok());
        }
        LdMapping::LastSectorTime => {
            frame.lap.last_sector_time_ns = seconds_to_ns(value);
        }
        LdMapping::AmbientTemperature => {
            environment(frame).ambient_temperature_c = finite_f32(value);
        }
        LdMapping::TrackTemperature => environment(frame).track_temperature_c = finite_f32(value),
        LdMapping::TrackGrip => environment(frame).track_grip = percent(value),
        LdMapping::WheelAngularSpeed(corner) => {
            wheel(frame, corner).angular_speed_rad_s = finite_f32(value);
        }
        LdMapping::TyrePressure(corner) => {
            wheel(frame, corner).tyre_pressure_pa = nonnegative_f32(value * 6_894.757_293_168);
        }
        LdMapping::TyreCoreTemperature(corner) => {
            wheel(frame, corner).tyre_core_temperature_c = finite_f32(value);
        }
        LdMapping::SuspensionTravel(corner) => {
            wheel(frame, corner).suspension_travel_m = finite_f32(value / 1_000.0);
        }
        LdMapping::Native => {}
    }
}

fn environment(frame: &mut TelemetryFrame) -> &mut EnvironmentState {
    frame
        .environment
        .get_or_insert_with(EnvironmentState::default)
}

fn wheel(frame: &mut TelemetryFrame, corner: WheelCorner) -> &mut WheelState {
    frame.wheels.entry(corner).or_default()
}

fn percent(value: f64) -> Option<f32> {
    finite_f32(value / 100.0).filter(|value| (0.0..=1.0).contains(value))
}

fn nonnegative_f32(value: f64) -> Option<f32> {
    (value >= 0.0).then(|| finite_f32(value)).flatten()
}

fn finite_f32(value: f64) -> Option<f32> {
    #[allow(clippy::cast_possible_truncation)]
    let narrowed = value as f32;
    narrowed.is_finite().then_some(narrowed)
}

fn integral_i16(value: f64) -> Option<i16> {
    if value.fract() != 0.0 || value < f64::from(i16::MIN) || value > f64::from(i16::MAX) {
        return None;
    }
    value.to_string().parse().ok()
}

fn seconds_to_ns(value: f64) -> Option<u64> {
    let nanoseconds = value * 1_000_000_000.0;
    if !nanoseconds.is_finite() || !(0.0..U64_EXCLUSIVE_UPPER_F64).contains(&nanoseconds) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(nanoseconds.round() as u64)
}

fn sample_index(sequence: u64, output_rate_hz: u16, channel_rate_hz: u16, samples: usize) -> usize {
    let index = sequence.saturating_mul(u64::from(channel_rate_hz)) / u64::from(output_rate_hz);
    usize::try_from(index)
        .unwrap_or(usize::MAX)
        .min(samples.saturating_sub(1))
}

fn elapsed_ns(sequence: u64, rate_hz: u16) -> u64 {
    sequence.saturating_mul(1_000_000_000) / u64::from(rate_hz)
}

fn div_ceil(value: u64, divisor: u64) -> u64 {
    value / divisor + u64::from(!value.is_multiple_of(divisor))
}

fn unique_name(name: &str, index: usize, counts: &mut BTreeMap<String, usize>) -> String {
    let base = if name.is_empty() {
        format!("channel_{index}")
    } else {
        name.to_owned()
    };
    let count = counts.entry(base.clone()).or_default();
    *count += 1;
    if *count == 1 {
        base
    } else {
        format!("{base}#{count}")
    }
}

fn nonempty(value: &str) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.trim().to_owned())
}

fn normalized(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .filter(|character| !character.is_whitespace() && !matches!(character, '_' | '-'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STINT_7_LD: &[u8] = include_bytes!("../tests/fixtures/acti-zandvoort/stint-7.ld");
    const STINT_7_LDX: &[u8] = include_bytes!("../tests/fixtures/acti-zandvoort/stint-7.ldx");
    const STINT_9_LD: &[u8] = include_bytes!("../tests/fixtures/acti-zandvoort/stint-9.ld");
    const STINT_9_LDX: &[u8] = include_bytes!("../tests/fixtures/acti-zandvoort/stint-9.ldx");

    #[test]
    fn acti_fixture_metadata_and_lap_markers_are_stable() {
        let reader = MotecLdReader::new(
            STINT_7_LD.to_vec(),
            Some(STINT_7_LDX),
            LdImportLimits::default(),
        )
        .expect("authorised ACTI fixture");
        let metadata = reader.metadata();
        assert_eq!(metadata.driver, "E. Cavalli");
        assert_eq!(metadata.vehicle_id, "ks_mazda_mx5_cup");
        assert_eq!(metadata.venue, "zandvoort2023");
        assert_eq!(metadata.session_type.as_deref(), Some("HOTLAP"));
        assert_eq!(metadata.channels.len(), 169);
        assert_eq!(metadata.output_rate_hz, 20);
        assert_eq!(metadata.frame_count, 11_394);
        let ldx = metadata.ldx.as_ref().expect("sidecar");
        assert_eq!(ldx.version.as_deref(), Some("1.5"));
        assert_eq!(ldx.total_laps, Some(5));
        assert_eq!(ldx.fastest_lap, Some(1));
        assert_eq!(ldx.fastest_time_ns, Some(111_996_000_000));
        assert_eq!(ldx.boundaries.len(), 4);
        assert_eq!(ldx.boundaries[0].elapsed_ns, 116_780_000_000);
        assert_eq!(ldx.boundaries[3].elapsed_ns, 461_560_000_000);
    }

    #[test]
    fn acti_fixture_maps_core_and_rich_channels_without_losing_native_values() {
        let mut reader = MotecLdReader::new(
            STINT_9_LD.to_vec(),
            Some(STINT_9_LDX),
            LdImportLimits::default(),
        )
        .expect("authorised ACTI fixture");
        let frame = reader.next_frame().expect("first frame");
        assert_eq!(frame.elapsed, ElapsedNanoseconds(0));
        assert_eq!(frame.inputs.throttle, Some(1.0));
        assert_eq!(frame.inputs.brake, Some(0.0));
        assert_approx(frame.inputs.steering_angle_rad, -0.271_398_7, 0.000_001);
        assert_approx(frame.vehicle.speed_mps, 3.594_444_5, 0.000_001);
        assert_approx(frame.vehicle.engine_rpm, 5_982.222, 0.001);
        assert_eq!(frame.vehicle.gear, Some(Gear::Forward(1)));
        assert_eq!(frame.lap.completed_laps, Some(0));
        assert_eq!(frame.lap.current_lap_time_ns, Some(0));
        assert_approx(frame.lap.normalized_position, 0.028_633_334, 0.000_001);
        let position = frame.motion.position_m.expect("ACTI position");
        assert!((position.x - -213.116_666_666_7).abs() < 0.000_001);
        assert!((position.y - 19.356_166_666_7).abs() < 0.000_001);
        assert!((position.z - -438.942_857_142_9).abs() < 0.000_001);
        assert!(frame.environment.is_some());
        assert_eq!(frame.wheels.len(), 4);
        let native = frame.native.expect("native telemetry");
        assert_eq!(native.schema, "motec.i2.ld/community-3");
        assert_eq!(
            native.float_fields.get("static.max_fuel_litres"),
            Some(&45.0)
        );
        assert_eq!(native.float_fields.len(), 170);
        assert_eq!(native.float_fields.get("Lap Invalidated"), Some(&0.0));
        assert_eq!(
            native
                .text_fields
                .get("unit.Ground Speed")
                .map(String::as_str),
            Some("km/h")
        );
    }

    #[test]
    fn ldx_boundaries_rebase_laps_and_current_lap_time() {
        let mut reader = MotecLdReader::new(
            STINT_9_LD.to_vec(),
            Some(STINT_9_LDX),
            LdImportLimits::default(),
        )
        .expect("authorised ACTI fixture");
        reader.next_sequence = 2_363;
        let before = reader.next_frame().expect("before crossing");
        assert_eq!(before.elapsed.0, 118_150_000_000);
        assert_eq!(before.lap.completed_laps, Some(1));
        assert_eq!(before.lap.current_lap_time_ns, Some(39_000_000));

        reader.next_sequence = 4_600;
        let second_crossing = reader.next_frame().expect("second crossing");
        assert_eq!(second_crossing.elapsed.0, 230_000_000_000);
        assert_eq!(second_crossing.lap.completed_laps, Some(2));
        assert_eq!(second_crossing.lap.current_lap_time_ns, Some(4_000_000));
    }

    #[test]
    fn acti_sector_transitions_are_mapped_to_the_complete_lap() {
        let mut reader = MotecLdReader::new(
            STINT_9_LD.to_vec(),
            Some(STINT_9_LDX),
            LdImportLimits::default(),
        )
        .expect("authorised ACTI fixture");
        let mut observed = BTreeMap::new();
        while let Some(frame) = reader.next_frame() {
            if matches!(frame.sequence.0, 2_363 | 3_324 | 3_890) {
                observed.insert(
                    frame.sequence.0,
                    (
                        frame.lap.current_sector_index,
                        frame.lap.last_sector_time_ns,
                    ),
                );
            }
            if frame.sequence.0 == 3_890 {
                break;
            }
        }
        assert_eq!(observed[&2_363], (Some(0), Some(36_022_000_000)));
        assert_eq!(observed[&3_324], (Some(1), Some(48_038_000_000)));
        assert_eq!(observed[&3_890], (Some(2), Some(28_319_000_000)));
    }

    #[test]
    fn malformed_or_oversized_native_inputs_are_rejected() {
        let limits = LdImportLimits {
            max_ld_bytes: MINIMUM_LD_BYTES,
            ..LdImportLimits::default()
        };
        assert!(matches!(
            MotecLdReader::new(STINT_9_LD.to_vec(), None, limits),
            Err(LdImportError::SourceTooLarge { .. })
        ));
        assert!(matches!(
            MotecLdReader::new(vec![0; MINIMUM_LD_BYTES], None, LdImportLimits::default()),
            Err(LdImportError::InvalidHeader)
        ));
    }

    fn assert_approx(actual: Option<f32>, expected: f32, tolerance: f32) {
        assert!((actual.expect("available value") - expected).abs() <= tolerance);
    }
}
