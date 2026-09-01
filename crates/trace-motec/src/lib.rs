//! Bounded import primitives for telemetry exported by `MoTeC` i2.
//!
//! The native `.ld` layout is not treated as a public interchange format here.
//! This crate starts with the documented i2 CSV export path and deliberately
//! maps only source channels whose name and unit have an unambiguous canonical
//! TRACE meaning.

use std::{collections::BTreeMap, io::Read};

use csv::{Reader, ReaderBuilder, StringRecord, Trim};
use trace_domain::{
    ChannelId, CoordinateFrame, ElapsedNanoseconds, FrameSequence, Gear, MotionState,
    NativeTelemetrySample, TelemetryFrame, Unit, Vector3,
};

mod ld;

pub use ld::{
    LapBoundary, LdChannel, LdImportError, LdImportLimits, LdImportMetadata, LdxMetadata,
    MotecLdReader,
};

const FORMAT_FIELD: &str = "Format";
const FORMAT_VALUE: &str = "MoTeC CSV File";
const NATIVE_SCHEMA: &str = "motec.i2.csv/provisional-1";

/// Hard resource limits applied while reading an imported CSV file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImportLimits {
    pub max_bytes: u64,
    pub max_rows: u64,
    pub max_columns: usize,
    pub max_field_bytes: usize,
    pub max_preamble_rows: usize,
}

impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            max_bytes: 512 * 1024 * 1024,
            max_rows: 5_000_000,
            max_columns: 512,
            max_field_bytes: 16 * 1024,
            max_preamble_rows: 256,
        }
    }
}

/// One source detail from the CSV preamble. Duplicate names remain distinct.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceDetail {
    pub name: String,
    pub value: String,
}

/// One channel declared by the CSV header and unit row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceChannel {
    pub name: String,
    pub unit: String,
    pub canonical_id: Option<ChannelId>,
    pub canonical_unit: Option<Unit>,
}

/// Source metadata available before sample iteration begins.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportMetadata {
    pub format: String,
    pub details: Vec<SourceDetail>,
    pub channels: Vec<SourceChannel>,
}

/// A recoverable validation or parsing failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportError {
    InvalidLimits,
    SourceTooLarge { bytes: u64, maximum: u64 },
    InvalidCsv(String),
    UnsupportedFormat,
    MissingChannelHeader,
    MissingUnitRow,
    TooManyPreambleRows,
    TooManyColumns { columns: usize, maximum: usize },
    FieldTooLarge { bytes: usize, maximum: usize },
    ColumnCountChanged { expected: usize, actual: usize },
    MissingTimeChannel,
    UnsupportedTimeUnit(String),
    InvalidTime(String),
    TimeMovedBackwards,
    TooManyRows { maximum: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mapping {
    Time(TimeScale),
    Throttle(LinearScale),
    Brake(LinearScale),
    Clutch(LinearScale),
    Steering(LinearScale),
    Speed(LinearScale),
    EngineRpm,
    Fuel(LinearScale),
    Gear,
    PositionX(LinearScale),
    PositionY(LinearScale),
    PositionZ(LinearScale),
    Native,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LinearScale(f64);

#[derive(Clone, Copy, Debug, PartialEq)]
struct TimeScale(f64);

/// Streaming reader for one bounded `MoTeC` i2 CSV export.
pub struct MotecCsvReader<R: Read> {
    reader: Reader<std::io::Take<R>>,
    metadata: ImportMetadata,
    mappings: Vec<Mapping>,
    native_keys: Vec<String>,
    record: StringRecord,
    limits: ImportLimits,
    rows: u64,
    source_start_ns: Option<i64>,
    last_source_ns: Option<i64>,
    finished: bool,
}

impl<R: Read> MotecCsvReader<R> {
    /// Inspects the preamble, channel header, and units without buffering samples.
    ///
    /// `source_bytes` must come from the opened file's metadata. Reads are also
    /// capped independently to handle a file growing after that check.
    ///
    /// # Errors
    ///
    /// Rejects invalid limits, oversized input, malformed CSV, unsupported format,
    /// or a header that cannot establish an unambiguous time channel.
    pub fn new(reader: R, source_bytes: u64, limits: ImportLimits) -> Result<Self, ImportError> {
        validate_limits(limits)?;
        if source_bytes > limits.max_bytes {
            return Err(ImportError::SourceTooLarge {
                bytes: source_bytes,
                maximum: limits.max_bytes,
            });
        }
        let bounded = reader.take(limits.max_bytes.saturating_add(1));
        let mut reader = ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .trim(Trim::All)
            .from_reader(bounded);
        let (metadata, mappings, native_keys) =
            inspect_preamble(&mut reader, limits).map_err(|error| {
                if reader.get_ref().limit() == 0 {
                    ImportError::SourceTooLarge {
                        bytes: limits.max_bytes.saturating_add(1),
                        maximum: limits.max_bytes,
                    }
                } else {
                    error
                }
            })?;
        Ok(Self {
            reader,
            metadata,
            mappings,
            native_keys,
            record: StringRecord::new(),
            limits,
            rows: 0,
            source_start_ns: None,
            last_source_ns: None,
            finished: false,
        })
    }

    /// Returns source details and channel mappings discovered in the preamble.
    pub fn metadata(&self) -> &ImportMetadata {
        &self.metadata
    }

    /// Number of sample rows successfully decoded so far.
    pub fn rows_read(&self) -> u64 {
        self.rows
    }

    /// Decodes the next canonical frame while retaining unknown source values.
    ///
    /// # Errors
    ///
    /// Rejects malformed rows, invalid or decreasing timestamps, and resource
    /// limit violations. After an error, this reader is exhausted.
    pub fn next_frame(&mut self) -> Result<Option<TelemetryFrame>, ImportError> {
        if self.finished {
            return Ok(None);
        }
        self.record.clear();
        let has_record = match self.reader.read_record(&mut self.record) {
            Ok(value) => value,
            Err(error) => return self.fail(csv_error(&error)),
        };
        if !has_record {
            self.finished = true;
            if self.reader.get_ref().limit() == 0 {
                return Err(ImportError::SourceTooLarge {
                    bytes: self.limits.max_bytes.saturating_add(1),
                    maximum: self.limits.max_bytes,
                });
            }
            return Ok(None);
        }
        if let Err(error) = validate_record(&self.record, self.limits) {
            return self.fail(error);
        }
        if self.record.len() != self.mappings.len() {
            return self.fail(ImportError::ColumnCountChanged {
                expected: self.mappings.len(),
                actual: self.record.len(),
            });
        }
        if self.rows >= self.limits.max_rows {
            return self.fail(ImportError::TooManyRows {
                maximum: self.limits.max_rows,
            });
        }
        match decode_frame(
            &self.record,
            &self.mappings,
            &self.native_keys,
            self.rows,
            &mut self.source_start_ns,
            &mut self.last_source_ns,
        ) {
            Ok(frame) => {
                self.rows += 1;
                Ok(Some(frame))
            }
            Err(error) => self.fail(error),
        }
    }

    fn fail<T>(&mut self, error: ImportError) -> Result<T, ImportError> {
        self.finished = true;
        Err(error)
    }
}

fn validate_limits(limits: ImportLimits) -> Result<(), ImportError> {
    if limits.max_bytes == 0
        || limits.max_rows == 0
        || limits.max_columns == 0
        || limits.max_field_bytes == 0
        || limits.max_preamble_rows == 0
    {
        return Err(ImportError::InvalidLimits);
    }
    Ok(())
}

fn inspect_preamble<R: Read>(
    reader: &mut Reader<R>,
    limits: ImportLimits,
) -> Result<(ImportMetadata, Vec<Mapping>, Vec<String>), ImportError> {
    let mut record = StringRecord::new();
    let mut details = Vec::new();
    let mut format_seen = false;
    for _ in 0..limits.max_preamble_rows {
        record.clear();
        if !reader
            .read_record(&mut record)
            .map_err(|error| csv_error(&error))?
        {
            return Err(if format_seen {
                ImportError::MissingChannelHeader
            } else {
                ImportError::UnsupportedFormat
            });
        }
        validate_record(&record, limits)?;
        let first = record.get(0).map(strip_bom).unwrap_or_default();
        if first.eq_ignore_ascii_case(FORMAT_FIELD) {
            format_seen = record
                .get(1)
                .is_some_and(|value| value.eq_ignore_ascii_case(FORMAT_VALUE));
            if !format_seen {
                return Err(ImportError::UnsupportedFormat);
            }
            continue;
        }
        if first.eq_ignore_ascii_case("Time") {
            if !format_seen {
                return Err(ImportError::UnsupportedFormat);
            }
            return finish_header(reader, details, &record, limits);
        }
        if format_seen && !first.is_empty() {
            details.push(SourceDetail {
                name: first.to_owned(),
                value: record.iter().skip(1).collect::<Vec<_>>().join(", "),
            });
        }
    }
    Err(ImportError::TooManyPreambleRows)
}

fn finish_header<R: Read>(
    reader: &mut Reader<R>,
    details: Vec<SourceDetail>,
    header: &StringRecord,
    limits: ImportLimits,
) -> Result<(ImportMetadata, Vec<Mapping>, Vec<String>), ImportError> {
    let mut units = StringRecord::new();
    if !reader
        .read_record(&mut units)
        .map_err(|error| csv_error(&error))?
    {
        return Err(ImportError::MissingUnitRow);
    }
    validate_record(&units, limits)?;
    if units.len() != header.len() {
        return Err(ImportError::ColumnCountChanged {
            expected: header.len(),
            actual: units.len(),
        });
    }
    let mut channels = Vec::with_capacity(header.len());
    let mut mappings = Vec::with_capacity(header.len());
    let mut native_keys = Vec::with_capacity(header.len());
    let mut key_counts = BTreeMap::<String, usize>::new();
    for (index, (name, unit)) in header.iter().zip(units.iter()).enumerate() {
        let mapping = mapping(name, unit)?;
        let (canonical_id, canonical_unit) = canonical_descriptor(mapping);
        channels.push(SourceChannel {
            name: name.to_owned(),
            unit: unit.to_owned(),
            canonical_id,
            canonical_unit,
        });
        mappings.push(mapping);
        native_keys.push(unique_native_key(name, index, &mut key_counts));
    }
    if !matches!(mappings.first(), Some(Mapping::Time(_))) {
        return Err(ImportError::MissingTimeChannel);
    }
    Ok((
        ImportMetadata {
            format: FORMAT_VALUE.into(),
            details,
            channels,
        },
        mappings,
        native_keys,
    ))
}

fn mapping(name: &str, unit: &str) -> Result<Mapping, ImportError> {
    let name = normalized(name);
    let unit = normalized(unit);
    if name == "time" {
        return time_scale(&unit).map(Mapping::Time);
    }
    let percent = percent_scale(&unit);
    let result = match name.as_str() {
        "throttle" | "throttlepos" | "throttleposition" => {
            percent.map_or(Mapping::Native, Mapping::Throttle)
        }
        "brake" | "brakepos" | "brakeposition" => percent.map_or(Mapping::Native, Mapping::Brake),
        "clutch" | "clutchpos" | "clutchposition" => {
            percent.map_or(Mapping::Native, Mapping::Clutch)
        }
        "steerangle" | "steeringangle" | "steeredangle" => {
            angle_scale(&unit).map_or(Mapping::Native, Mapping::Steering)
        }
        "speed" | "groundspeed" | "vehiclespeed" => {
            speed_scale(&unit).map_or(Mapping::Native, Mapping::Speed)
        }
        "enginerpm" | "rpm" if matches!(unit.as_str(), "rpm" | "r/min") => Mapping::EngineRpm,
        "fuel" | "fuellevel" | "fuelremaining"
            if matches!(unit.as_str(), "l" | "litre" | "litres" | "liter" | "liters") =>
        {
            Mapping::Fuel(LinearScale(1.0))
        }
        "gear" if unit.is_empty() || matches!(unit.as_str(), "count" | "gear") => Mapping::Gear,
        "positionx" | "gpsx" => distance_scale(&unit).map_or(Mapping::Native, Mapping::PositionX),
        "positiony" | "gpsy" => distance_scale(&unit).map_or(Mapping::Native, Mapping::PositionY),
        "positionz" | "gpsz" => distance_scale(&unit).map_or(Mapping::Native, Mapping::PositionZ),
        _ => Mapping::Native,
    };
    Ok(result)
}

fn canonical_descriptor(mapping: Mapping) -> (Option<ChannelId>, Option<Unit>) {
    let descriptor = match mapping {
        Mapping::Time(_) => ("time.elapsed", Unit::Second),
        Mapping::Throttle(_) => ("inputs.throttle", Unit::Ratio),
        Mapping::Brake(_) => ("inputs.brake", Unit::Ratio),
        Mapping::Clutch(_) => ("inputs.clutch", Unit::Ratio),
        Mapping::Steering(_) => ("inputs.steering_angle", Unit::Radian),
        Mapping::Speed(_) => ("vehicle.speed", Unit::MetresPerSecond),
        Mapping::EngineRpm => ("vehicle.engine_rpm", Unit::RevolutionsPerMinute),
        Mapping::Fuel(_) => ("vehicle.fuel", Unit::Litre),
        Mapping::Gear => ("vehicle.gear", Unit::Count),
        Mapping::PositionX(_) => ("motion.position_x", Unit::Metre),
        Mapping::PositionY(_) => ("motion.position_y", Unit::Metre),
        Mapping::PositionZ(_) => ("motion.position_z", Unit::Metre),
        Mapping::Native => return (None, None),
    };
    (
        Some(ChannelId::parse(descriptor.0).expect("static canonical channel ID is valid")),
        Some(descriptor.1),
    )
}

fn decode_frame(
    record: &StringRecord,
    mappings: &[Mapping],
    native_keys: &[String],
    sequence: u64,
    source_start_ns: &mut Option<i64>,
    last_source_ns: &mut Option<i64>,
) -> Result<TelemetryFrame, ImportError> {
    let source_ns = source_time_ns(record.get(0).unwrap_or_default(), mappings[0])?;
    if last_source_ns.is_some_and(|previous| source_ns < previous) {
        return Err(ImportError::TimeMovedBackwards);
    }
    let start = *source_start_ns.get_or_insert(source_ns);
    let elapsed = u64::try_from(source_ns - start).map_err(|_| ImportError::TimeMovedBackwards)?;
    *last_source_ns = Some(source_ns);

    let mut frame = TelemetryFrame {
        sequence: FrameSequence(sequence),
        elapsed: ElapsedNanoseconds(elapsed),
        native: Some(Box::new(NativeTelemetrySample {
            schema: NATIVE_SCHEMA.into(),
            ..NativeTelemetrySample::default()
        })),
        ..TelemetryFrame::default()
    };
    let mut position = [None; 3];
    for ((value, mapping), key) in record.iter().zip(mappings).zip(native_keys).skip(1) {
        if value.is_empty() {
            continue;
        }
        let numeric = value.parse::<f64>().ok().filter(|value| value.is_finite());
        retain_native(frame.native.as_deref_mut(), key, value, numeric);
        apply_mapping(&mut frame, &mut position, *mapping, value, numeric);
    }
    if position[0].is_some() && position[2].is_some() {
        frame.motion = MotionState {
            position_m: Some(Vector3 {
                x: position[0].unwrap_or(0.0),
                y: position[1].unwrap_or(0.0),
                z: position[2].unwrap_or(0.0),
                frame: CoordinateFrame::SourceWorld,
            }),
            ..MotionState::default()
        };
    }
    Ok(frame)
}

fn apply_mapping(
    frame: &mut TelemetryFrame,
    position: &mut [Option<f64>; 3],
    mapping: Mapping,
    raw: &str,
    numeric: Option<f64>,
) {
    let scaled = |scale: LinearScale| numeric.map(|value| value * scale.0);
    match mapping {
        Mapping::Throttle(scale) => frame.inputs.throttle = ratio(scaled(scale)),
        Mapping::Brake(scale) => frame.inputs.brake = ratio(scaled(scale)),
        Mapping::Clutch(scale) => frame.inputs.clutch = ratio(scaled(scale)),
        Mapping::Steering(scale) => frame.inputs.steering_angle_rad = finite_f32(scaled(scale)),
        Mapping::Speed(scale) => frame.vehicle.speed_mps = nonnegative_f32(scaled(scale)),
        Mapping::EngineRpm => frame.vehicle.engine_rpm = nonnegative_f32(numeric),
        Mapping::Fuel(scale) => frame.vehicle.fuel_litres = nonnegative_f32(scaled(scale)),
        Mapping::Gear => frame.vehicle.gear = parse_gear(raw),
        Mapping::PositionX(scale) => position[0] = scaled(scale),
        Mapping::PositionY(scale) => position[1] = scaled(scale),
        Mapping::PositionZ(scale) => position[2] = scaled(scale),
        Mapping::Time(_) | Mapping::Native => {}
    }
}

fn retain_native(
    native: Option<&mut NativeTelemetrySample>,
    key: &str,
    raw: &str,
    numeric: Option<f64>,
) {
    let Some(native) = native else {
        return;
    };
    if let Some(value) = numeric {
        native.float_fields.insert(key.into(), value);
    } else {
        native.text_fields.insert(key.into(), raw.into());
    }
}

fn source_time_ns(raw: &str, mapping: Mapping) -> Result<i64, ImportError> {
    let Mapping::Time(TimeScale(scale)) = mapping else {
        return Err(ImportError::MissingTimeChannel);
    };
    let value = raw
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| ImportError::InvalidTime(raw.into()))?;
    let nanoseconds = value * scale;
    if !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&nanoseconds) {
        return Err(ImportError::InvalidTime(raw.into()));
    }
    // The explicit finite range check above makes the rounded conversion defined.
    #[allow(clippy::cast_possible_truncation)]
    Ok(nanoseconds.round() as i64)
}

fn time_scale(unit: &str) -> Result<TimeScale, ImportError> {
    let scale = match unit {
        "s" | "sec" | "second" | "seconds" => 1_000_000_000.0,
        "ms" | "millisecond" | "milliseconds" => 1_000_000.0,
        "us" | "µs" | "microsecond" | "microseconds" => 1_000.0,
        _ => return Err(ImportError::UnsupportedTimeUnit(unit.into())),
    };
    Ok(TimeScale(scale))
}

fn percent_scale(unit: &str) -> Option<LinearScale> {
    match unit {
        "%" | "percent" => Some(LinearScale(0.01)),
        "ratio" => Some(LinearScale(1.0)),
        _ => None,
    }
}

fn angle_scale(unit: &str) -> Option<LinearScale> {
    match unit {
        "deg" | "degree" | "degrees" | "°" => Some(LinearScale(std::f64::consts::PI / 180.0)),
        "rad" | "radian" | "radians" => Some(LinearScale(1.0)),
        _ => None,
    }
}

fn speed_scale(unit: &str) -> Option<LinearScale> {
    match unit {
        "km/h" | "kmh" | "kph" => Some(LinearScale(1.0 / 3.6)),
        "m/s" | "mps" => Some(LinearScale(1.0)),
        "mph" => Some(LinearScale(0.447_04)),
        _ => None,
    }
}

fn distance_scale(unit: &str) -> Option<LinearScale> {
    match unit {
        "m" | "metre" | "metres" | "meter" | "meters" => Some(LinearScale(1.0)),
        "mm" => Some(LinearScale(0.001)),
        "km" => Some(LinearScale(1_000.0)),
        _ => None,
    }
}

fn parse_gear(raw: &str) -> Option<Gear> {
    if raw.eq_ignore_ascii_case("r") || raw.eq_ignore_ascii_case("reverse") {
        return Some(Gear::Reverse);
    }
    if raw.eq_ignore_ascii_case("n") || raw.eq_ignore_ascii_case("neutral") {
        return Some(Gear::Neutral);
    }
    let value = raw.parse::<i16>().ok()?;
    match value {
        -1 => Some(Gear::Reverse),
        0 => Some(Gear::Neutral),
        value if (1..=i16::from(u8::MAX)).contains(&value) => {
            Some(Gear::Forward(u8::try_from(value).ok()?))
        }
        _ => Some(Gear::Unknown(value)),
    }
}

fn ratio(value: Option<f64>) -> Option<f32> {
    value
        .filter(|value| (0.0..=1.0).contains(value))
        .and_then(|value| finite_f32(Some(value)))
}

fn nonnegative_f32(value: Option<f64>) -> Option<f32> {
    finite_f32(value.filter(|value| *value >= 0.0))
}

fn finite_f32(value: Option<f64>) -> Option<f32> {
    value.and_then(|value| {
        // Values outside the f32 range become infinities and are rejected below.
        #[allow(clippy::cast_possible_truncation)]
        let narrowed = value as f32;
        narrowed.is_finite().then_some(narrowed)
    })
}

fn validate_record(record: &StringRecord, limits: ImportLimits) -> Result<(), ImportError> {
    if record.len() > limits.max_columns {
        return Err(ImportError::TooManyColumns {
            columns: record.len(),
            maximum: limits.max_columns,
        });
    }
    if let Some(bytes) = record
        .iter()
        .map(str::len)
        .find(|bytes| *bytes > limits.max_field_bytes)
    {
        return Err(ImportError::FieldTooLarge {
            bytes,
            maximum: limits.max_field_bytes,
        });
    }
    Ok(())
}

fn unique_native_key(name: &str, index: usize, counts: &mut BTreeMap<String, usize>) -> String {
    let base = if name.is_empty() {
        format!("column_{index}")
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

fn normalized(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .filter(|character| !character.is_whitespace() && !matches!(character, '_' | '-'))
        .collect()
}

fn strip_bom(value: &str) -> &str {
    value.strip_prefix('\u{feff}').unwrap_or(value)
}

fn csv_error(error: &csv::Error) -> ImportError {
    ImportError::InvalidCsv(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use trace_domain::DriverInputs;

    use super::*;

    const SAMPLE: &str = include_str!("../tests/fixtures/csv/canonical.csv");
    const DECREASING_TIME: &str = include_str!("../tests/fixtures/csv/decreasing-time.csv");

    fn reader(source: &str) -> MotecCsvReader<Cursor<&[u8]>> {
        MotecCsvReader::new(
            Cursor::new(source.as_bytes()),
            source.len() as u64,
            ImportLimits::default(),
        )
        .expect("valid synthetic export")
    }

    #[test]
    fn inspects_source_details_and_conservative_channel_mappings() {
        let reader = reader(SAMPLE);
        assert_eq!(reader.metadata().format, FORMAT_VALUE);
        assert_eq!(reader.metadata().details[0].name, "Venue");
        assert_eq!(reader.metadata().details[0].value, "Synthetic Circuit");
        let channels = &reader.metadata().channels;
        assert_eq!(channels.len(), 13);
        assert_eq!(
            channels[1].canonical_id.as_ref().map(ChannelId::as_str),
            Some("inputs.throttle")
        );
        assert_eq!(channels[1].canonical_unit, Some(Unit::Ratio));
        assert_eq!(channels[12].canonical_id, None);
        assert_eq!(channels[12].unit, "mm");
    }

    #[test]
    fn streams_frames_with_units_normalized_and_unknown_values_retained() {
        let mut reader = reader(SAMPLE);
        let first = reader.next_frame().expect("first row").expect("frame");
        assert_eq!(first.elapsed, ElapsedNanoseconds(0));
        assert_eq!(first.inputs.throttle, Some(0.5));
        assert_eq!(first.inputs.brake, Some(0.0));
        assert_eq!(first.inputs.clutch, Some(0.1));
        assert_eq!(first.vehicle.speed_mps, Some(50.0));
        assert_eq!(first.vehicle.engine_rpm, Some(7_000.0));
        assert_eq!(
            first.inputs.steering_angle_rad,
            Some(-std::f32::consts::FRAC_PI_2)
        );
        assert_eq!(first.vehicle.fuel_litres, Some(20.5));
        assert_eq!(first.vehicle.gear, Some(Gear::Forward(3)));
        assert_eq!(
            first
                .motion
                .position_m
                .map(|position| (position.x, position.z)),
            Some((10.0, 20.0))
        );
        assert_eq!(
            first.motion.position_m.map(|position| position.y),
            Some(2.0)
        );
        let native = first.native.expect("native values");
        assert_eq!(native.schema, NATIVE_SCHEMA);
        assert_eq!(native.float_fields.get("Damper FL"), Some(&42.5));

        let second = reader.next_frame().expect("second row").expect("frame");
        assert_eq!(second.elapsed, ElapsedNanoseconds(50_000_000));
        assert_eq!(second.vehicle.gear, Some(Gear::Neutral));
        assert_eq!(
            second
                .native
                .expect("native values")
                .text_fields
                .get("Damper FL")
                .map(String::as_str),
            Some("not available")
        );
        assert!(reader.next_frame().expect("end").is_none());
        assert_eq!(reader.rows_read(), 2);
    }

    #[test]
    fn pressure_is_not_mapped_as_normalized_brake_input() {
        let source = concat!(
            "Format,MoTeC CSV File\n",
            "Time,Brake Pressure\n",
            "s,bar\n",
            "0,80\n",
        );
        let mut reader = reader(source);
        assert_eq!(reader.metadata().channels[1].canonical_id, None);
        let frame = reader.next_frame().expect("row").expect("frame");
        assert_eq!(frame.inputs, DriverInputs::default());
        assert_eq!(
            frame
                .native
                .expect("native")
                .float_fields
                .get("Brake Pressure"),
            Some(&80.0)
        );
    }

    #[test]
    fn incomplete_planar_position_is_not_exposed_as_a_track_point() {
        let source = concat!(
            "Format,MoTeC CSV File\n",
            "Time,Position X\n",
            "s,m\n",
            "0,80\n",
        );
        let mut reader = reader(source);
        let frame = reader.next_frame().expect("row").expect("frame");
        assert_eq!(frame.motion, MotionState::default());
        assert_eq!(
            frame.native.expect("native").float_fields.get("Position X"),
            Some(&80.0)
        );
    }

    #[test]
    fn rejects_unsupported_format_and_ambiguous_time_units() {
        let unsupported = "Time,Speed\ns,km/h\n0,1\n";
        assert!(matches!(
            MotecCsvReader::new(
                Cursor::new(unsupported.as_bytes()),
                unsupported.len() as u64,
                ImportLimits::default()
            ),
            Err(ImportError::UnsupportedFormat)
        ));
        let ambiguous = "Format,MoTeC CSV File\nTime,Speed\nticks,km/h\n0,1\n";
        assert!(matches!(
            MotecCsvReader::new(
                Cursor::new(ambiguous.as_bytes()),
                ambiguous.len() as u64,
                ImportLimits::default()
            ),
            Err(ImportError::UnsupportedTimeUnit(unit)) if unit == "ticks"
        ));
    }

    #[test]
    fn rejects_decreasing_time_and_row_limit_overflow() {
        let mut reader = reader(DECREASING_TIME);
        reader.next_frame().expect("first row").expect("frame");
        assert_eq!(reader.next_frame(), Err(ImportError::TimeMovedBackwards));

        let limited = "Format,MoTeC CSV File\nTime,Speed\ns,km/h\n0,1\n1,2\n";
        let limits = ImportLimits {
            max_rows: 1,
            ..ImportLimits::default()
        };
        let mut reader = MotecCsvReader::new(
            Cursor::new(limited.as_bytes()),
            limited.len() as u64,
            limits,
        )
        .expect("valid header");
        reader.next_frame().expect("first row").expect("frame");
        assert_eq!(
            reader.next_frame(),
            Err(ImportError::TooManyRows { maximum: 1 })
        );
    }

    #[test]
    fn rejects_size_and_column_shape_violations() {
        let source = "Format,MoTeC CSV File\nTime,Speed\ns,km/h\n0,1\n";
        let limits = ImportLimits {
            max_bytes: 10,
            ..ImportLimits::default()
        };
        assert!(matches!(
            MotecCsvReader::new(Cursor::new(source.as_bytes()), source.len() as u64, limits),
            Err(ImportError::SourceTooLarge { .. })
        ));

        let changed = "Format,MoTeC CSV File\nTime,Speed\ns,km/h\n0,1,2\n";
        let mut reader = reader(changed);
        assert_eq!(
            reader.next_frame(),
            Err(ImportError::ColumnCountChanged {
                expected: 2,
                actual: 3,
            })
        );
    }
}
