//! Versioned Apache Arrow IPC telemetry representation spike.

use std::{
    collections::HashMap,
    io::{Cursor, Read, Seek},
    sync::Arc,
};

use arrow_array::{
    Array, Float32Array, Float64Array, Int8Array, Int16Array, RecordBatch, UInt32Array, UInt64Array,
};
use arrow_ipc::{reader::FileReader, writer::FileWriter};
use arrow_schema::{DataType, Field, Schema};
use trace_domain::{CoordinateFrame, Gear, TelemetryFrame, WheelCorner};

use std::io::Write;

const FORMAT_NAME: &str = "trace.telemetry";
const SCHEMA_VERSION: &str = "2";

/// Current full-fidelity telemetry schema version written by TRACE.
pub const TELEMETRY_SCHEMA_VERSION: u32 = 2;

/// Minimal decoded columns used to validate the Arrow storage choice.
/// This is not yet the final full-resolution persistence schema.
#[derive(Clone, Debug, PartialEq)]
pub struct TelemetryColumns {
    pub sequence: Vec<u64>,
    pub elapsed_ns: Vec<u64>,
    pub throttle: Vec<Option<f32>>,
    pub brake: Vec<Option<f32>>,
    pub speed_mps: Vec<Option<f32>>,
    pub engine_rpm: Vec<Option<f32>>,
    pub lap_position: Vec<Option<f32>>,
}

impl TelemetryColumns {
    /// Converts canonical frames into aligned storage columns.
    pub fn from_frames(frames: &[TelemetryFrame]) -> Self {
        Self {
            sequence: frames.iter().map(|frame| frame.sequence.0).collect(),
            elapsed_ns: frames.iter().map(|frame| frame.elapsed.0).collect(),
            throttle: frames.iter().map(|frame| frame.inputs.throttle).collect(),
            brake: frames.iter().map(|frame| frame.inputs.brake).collect(),
            speed_mps: frames.iter().map(|frame| frame.vehicle.speed_mps).collect(),
            engine_rpm: frames
                .iter()
                .map(|frame| frame.vehicle.engine_rpm)
                .collect(),
            lap_position: frames
                .iter()
                .map(|frame| frame.lap.normalized_position)
                .collect(),
        }
    }

    /// Number of aligned samples.
    pub fn len(&self) -> usize {
        self.sequence.len()
    }

    /// Whether no samples are present.
    pub fn is_empty(&self) -> bool {
        self.sequence.is_empty()
    }

    fn empty() -> Self {
        Self {
            sequence: Vec::new(),
            elapsed_ns: Vec::new(),
            throttle: Vec::new(),
            brake: Vec::new(),
            speed_mps: Vec::new(),
            engine_rpm: Vec::new(),
            lap_position: Vec::new(),
        }
    }
}

/// Encodes a canonical frame batch as an Arrow IPC random-access file.
///
/// # Errors
///
/// Returns [`IpcError`] for empty input or Arrow schema/write failures.
pub fn encode_frames(frames: &[TelemetryFrame]) -> Result<Vec<u8>, IpcError> {
    if frames.is_empty() {
        return Err(IpcError::EmptyBatch);
    }
    let schema = Arc::new(schema());
    let batch = record_batch(frames)?;

    let mut output = Cursor::new(Vec::new());
    {
        let mut writer = FileWriter::try_new(&mut output, &schema).map_err(IpcError::from)?;
        writer.write(&batch).map_err(IpcError::from)?;
        writer.finish().map_err(IpcError::from)?;
    }
    Ok(output.into_inner())
}

/// Incremental Arrow IPC file encoder with a fixed maximum in-memory frame batch.
pub struct TelemetryIpcWriter<W: Write> {
    writer: FileWriter<W>,
    pending: Vec<TelemetryFrame>,
    batch_size: usize,
    sample_count: u64,
}

impl<W: Write> TelemetryIpcWriter<W> {
    /// Starts an Arrow IPC file on `writer`.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError::InvalidBatchSize`] for zero or an Arrow write error.
    pub fn new(writer: W, batch_size: usize) -> Result<Self, IpcError> {
        if batch_size == 0 {
            return Err(IpcError::InvalidBatchSize);
        }
        let schema = Arc::new(schema());
        Ok(Self {
            writer: FileWriter::try_new(writer, &schema).map_err(IpcError::from)?,
            pending: Vec::with_capacity(batch_size),
            batch_size,
            sample_count: 0,
        })
    }

    /// Adds one frame and flushes a record batch when the bound is reached.
    ///
    /// # Errors
    ///
    /// Returns an Arrow or underlying writer error.
    pub fn push(&mut self, frame: TelemetryFrame) -> Result<(), IpcError> {
        if self.pending.len() >= self.batch_size {
            self.flush_batch()?;
        }
        self.pending.push(frame);
        if self.pending.len() == self.batch_size {
            self.flush_batch()?;
        }
        Ok(())
    }

    /// Finalizes the Arrow footer and returns the underlying writer and sample count.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError::EmptyBatch`] when no frames were written, or a write error.
    pub fn finish(mut self) -> Result<(W, u64), IpcError> {
        self.flush_batch()?;
        if self.sample_count == 0 {
            return Err(IpcError::EmptyBatch);
        }
        let writer = self.writer.into_inner().map_err(IpcError::from)?;
        Ok((writer, self.sample_count))
    }

    /// Number of frames buffered but not yet written as a record batch.
    pub fn buffered_frames(&self) -> usize {
        self.pending.len()
    }

    fn flush_batch(&mut self) -> Result<(), IpcError> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let count = u64::try_from(self.pending.len()).map_err(|_| IpcError::SampleOverflow)?;
        let batch = record_batch(&self.pending)?;
        self.writer.write(&batch).map_err(IpcError::from)?;
        self.sample_count = self
            .sample_count
            .checked_add(count)
            .ok_or(IpcError::SampleOverflow)?;
        self.pending.clear();
        Ok(())
    }
}

#[allow(clippy::too_many_lines, clippy::from_iter_instead_of_collect)]
fn record_batch(frames: &[TelemetryFrame]) -> Result<RecordBatch, IpcError> {
    let columns = TelemetryColumns::from_frames(frames);
    let schema = Arc::new(schema());
    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(UInt64Array::from(columns.sequence)),
            Arc::new(UInt64Array::from(columns.elapsed_ns)),
            Arc::new(Float32Array::from(columns.throttle)),
            Arc::new(Float32Array::from(columns.brake)),
            Arc::new(Float32Array::from(columns.speed_mps)),
            Arc::new(Float32Array::from(columns.engine_rpm)),
            Arc::new(Float32Array::from(columns.lap_position)),
            Arc::new(UInt32Array::from_iter(
                frames.iter().map(|frame| frame.lap.completed_laps),
            )),
            Arc::new(UInt64Array::from_iter(
                frames.iter().map(|frame| frame.lap.current_lap_time_ns),
            )),
            Arc::new(Float64Array::from_iter(
                frames.iter().map(|frame| frame.lap.simulator_distance_m),
            )),
            Arc::new(Float32Array::from_iter(
                frames.iter().map(|frame| frame.inputs.clutch),
            )),
            Arc::new(Float32Array::from_iter(
                frames.iter().map(|frame| frame.inputs.steering_angle_rad),
            )),
            Arc::new(Float32Array::from_iter(
                frames.iter().map(|frame| frame.vehicle.fuel_litres),
            )),
            Arc::new(Int8Array::from_iter(
                frames.iter().map(|frame| frame.vehicle.gear.map(gear_kind)),
            )),
            Arc::new(Int16Array::from_iter(
                frames
                    .iter()
                    .map(|frame| frame.vehicle.gear.map(gear_value)),
            )),
            vector_f64(frames, |frame| frame.motion.position_m, |value| value.x),
            vector_f64(frames, |frame| frame.motion.position_m, |value| value.y),
            vector_f64(frames, |frame| frame.motion.position_m, |value| value.z),
            vector_frame(frames, |frame| frame.motion.position_m),
            vector_f64(frames, |frame| frame.motion.velocity_mps, |value| value.x),
            vector_f64(frames, |frame| frame.motion.velocity_mps, |value| value.y),
            vector_f64(frames, |frame| frame.motion.velocity_mps, |value| value.z),
            vector_frame(frames, |frame| frame.motion.velocity_mps),
            vector_f64(
                frames,
                |frame| frame.motion.acceleration_mps2,
                |value| value.x,
            ),
            vector_f64(
                frames,
                |frame| frame.motion.acceleration_mps2,
                |value| value.y,
            ),
            vector_f64(
                frames,
                |frame| frame.motion.acceleration_mps2,
                |value| value.z,
            ),
            vector_frame(frames, |frame| frame.motion.acceleration_mps2),
            Arc::new(Float32Array::from_iter(frames.iter().map(|frame| {
                frame
                    .environment
                    .and_then(|value| value.ambient_temperature_c)
            }))),
            Arc::new(Float32Array::from_iter(frames.iter().map(|frame| {
                frame
                    .environment
                    .and_then(|value| value.track_temperature_c)
            }))),
            Arc::new(Float32Array::from_iter(frames.iter().map(|frame| {
                frame.environment.and_then(|value| value.track_grip)
            }))),
            wheel_f32(frames, WheelCorner::FrontLeft, |value| {
                value.angular_speed_rad_s
            }),
            wheel_f32(frames, WheelCorner::FrontLeft, |value| {
                value.tyre_pressure_pa
            }),
            wheel_f32(frames, WheelCorner::FrontLeft, |value| {
                value.tyre_core_temperature_c
            }),
            wheel_f32(frames, WheelCorner::FrontLeft, |value| {
                value.suspension_travel_m
            }),
            wheel_f32(frames, WheelCorner::FrontRight, |value| {
                value.angular_speed_rad_s
            }),
            wheel_f32(frames, WheelCorner::FrontRight, |value| {
                value.tyre_pressure_pa
            }),
            wheel_f32(frames, WheelCorner::FrontRight, |value| {
                value.tyre_core_temperature_c
            }),
            wheel_f32(frames, WheelCorner::FrontRight, |value| {
                value.suspension_travel_m
            }),
            wheel_f32(frames, WheelCorner::RearLeft, |value| {
                value.angular_speed_rad_s
            }),
            wheel_f32(frames, WheelCorner::RearLeft, |value| {
                value.tyre_pressure_pa
            }),
            wheel_f32(frames, WheelCorner::RearLeft, |value| {
                value.tyre_core_temperature_c
            }),
            wheel_f32(frames, WheelCorner::RearLeft, |value| {
                value.suspension_travel_m
            }),
            wheel_f32(frames, WheelCorner::RearRight, |value| {
                value.angular_speed_rad_s
            }),
            wheel_f32(frames, WheelCorner::RearRight, |value| {
                value.tyre_pressure_pa
            }),
            wheel_f32(frames, WheelCorner::RearRight, |value| {
                value.tyre_core_temperature_c
            }),
            wheel_f32(frames, WheelCorner::RearRight, |value| {
                value.suspension_travel_m
            }),
        ],
    )
    .map_err(IpcError::from)
}

fn gear_kind(gear: Gear) -> i8 {
    match gear {
        Gear::Reverse => -1,
        Gear::Neutral => 0,
        Gear::Forward(_) => 1,
        Gear::Unknown(_) => 2,
    }
}

fn gear_value(gear: Gear) -> i16 {
    match gear {
        Gear::Reverse => -1,
        Gear::Neutral => 0,
        Gear::Forward(value) => i16::from(value),
        Gear::Unknown(value) => value,
    }
}

fn coordinate_frame(frame: CoordinateFrame) -> i8 {
    match frame {
        CoordinateFrame::SourceWorld => 0,
        CoordinateFrame::TraceWorld => 1,
        CoordinateFrame::Vehicle => 2,
    }
}

#[allow(clippy::from_iter_instead_of_collect)]
fn vector_f64(
    frames: &[TelemetryFrame],
    vector: impl Fn(&TelemetryFrame) -> Option<trace_domain::Vector3>,
    component: impl Fn(trace_domain::Vector3) -> f64,
) -> Arc<Float64Array> {
    Arc::new(Float64Array::from_iter(
        frames.iter().map(|frame| vector(frame).map(&component)),
    ))
}

#[allow(clippy::from_iter_instead_of_collect)]
fn vector_frame(
    frames: &[TelemetryFrame],
    vector: impl Fn(&TelemetryFrame) -> Option<trace_domain::Vector3>,
) -> Arc<Int8Array> {
    Arc::new(Int8Array::from_iter(frames.iter().map(|frame| {
        vector(frame).map(|value| coordinate_frame(value.frame))
    })))
}

#[allow(clippy::from_iter_instead_of_collect)]
fn wheel_f32(
    frames: &[TelemetryFrame],
    corner: WheelCorner,
    channel: impl Fn(trace_domain::WheelState) -> Option<f32>,
) -> Arc<Float32Array> {
    Arc::new(Float32Array::from_iter(frames.iter().map(|frame| {
        frame.wheels.get(&corner).copied().and_then(&channel)
    })))
}

/// Decodes and validates an Arrow IPC telemetry spike file.
///
/// # Errors
///
/// Rejects malformed Arrow data, an unknown TRACE schema, or wrong columns.
pub fn decode_columns(bytes: &[u8]) -> Result<TelemetryColumns, IpcError> {
    let mut reader = FileReader::try_new(Cursor::new(bytes), None).map_err(IpcError::from)?;
    validate_schema(reader.schema().as_ref())?;
    let mut decoded = TelemetryColumns::empty();
    for batch in &mut reader {
        let batch = batch.map_err(IpcError::from)?;
        decoded.sequence.extend(required_u64(&batch, 0)?);
        decoded.elapsed_ns.extend(required_u64(&batch, 1)?);
        decoded.throttle.extend(nullable_f32(&batch, 2)?);
        decoded.brake.extend(nullable_f32(&batch, 3)?);
        decoded.speed_mps.extend(nullable_f32(&batch, 4)?);
        decoded.engine_rpm.extend(nullable_f32(&batch, 5)?);
        decoded.lap_position.extend(nullable_f32(&batch, 6)?);
    }
    if decoded.is_empty() {
        return Err(IpcError::EmptyBatch);
    }
    Ok(decoded)
}

/// Reads a bounded sample range while holding at most one Arrow record batch.
///
/// The returned projection contains the seven analysis-entry columns common to
/// schemas v1 and v2. Batches outside the requested range are discarded immediately.
///
/// # Errors
///
/// Returns [`IpcError::InvalidSampleRange`] for a zero count, overflow, or a range
/// extending beyond the file, and other IPC errors for malformed data.
pub fn read_columns_range<R: Read + Seek>(
    reader: R,
    sample_start: u64,
    sample_count: u64,
) -> Result<TelemetryColumns, IpcError> {
    if sample_count == 0 {
        return Err(IpcError::InvalidSampleRange);
    }
    let requested_end = sample_start
        .checked_add(sample_count)
        .ok_or(IpcError::InvalidSampleRange)?;
    let mut reader = FileReader::try_new_buffered(reader, None).map_err(IpcError::from)?;
    validate_schema(reader.schema().as_ref())?;
    let mut decoded = TelemetryColumns::empty();
    let mut batch_start = 0_u64;
    for batch in &mut reader {
        let batch = batch.map_err(IpcError::from)?;
        let rows = u64::try_from(batch.num_rows()).map_err(|_| IpcError::SampleOverflow)?;
        let batch_end = batch_start
            .checked_add(rows)
            .ok_or(IpcError::SampleOverflow)?;
        let overlap_start = sample_start.max(batch_start);
        let overlap_end = requested_end.min(batch_end);
        if overlap_start < overlap_end {
            let local_start = usize::try_from(overlap_start - batch_start)
                .map_err(|_| IpcError::SampleOverflow)?;
            let length = usize::try_from(overlap_end - overlap_start)
                .map_err(|_| IpcError::SampleOverflow)?;
            extend_projection(&mut decoded, &batch, local_start, length)?;
        }
        batch_start = batch_end;
        if batch_start >= requested_end {
            break;
        }
    }
    if u64::try_from(decoded.len()).map_err(|_| IpcError::SampleOverflow)? != sample_count {
        return Err(IpcError::InvalidSampleRange);
    }
    Ok(decoded)
}

fn extend_projection(
    decoded: &mut TelemetryColumns,
    batch: &RecordBatch,
    start: usize,
    length: usize,
) -> Result<(), IpcError> {
    decoded
        .sequence
        .extend(required_u64(batch, 0)?.into_iter().skip(start).take(length));
    decoded
        .elapsed_ns
        .extend(required_u64(batch, 1)?.into_iter().skip(start).take(length));
    decoded
        .throttle
        .extend(nullable_f32(batch, 2)?.into_iter().skip(start).take(length));
    decoded
        .brake
        .extend(nullable_f32(batch, 3)?.into_iter().skip(start).take(length));
    decoded
        .speed_mps
        .extend(nullable_f32(batch, 4)?.into_iter().skip(start).take(length));
    decoded
        .engine_rpm
        .extend(nullable_f32(batch, 5)?.into_iter().skip(start).take(length));
    decoded
        .lap_position
        .extend(nullable_f32(batch, 6)?.into_iter().skip(start).take(length));
    Ok(())
}

fn schema() -> Schema {
    Schema::new_with_metadata(
        vec![
            Field::new("sequence", DataType::UInt64, false),
            Field::new("elapsed_ns", DataType::UInt64, false),
            Field::new("throttle", DataType::Float32, true),
            Field::new("brake", DataType::Float32, true),
            Field::new("speed_mps", DataType::Float32, true),
            Field::new("engine_rpm", DataType::Float32, true),
            Field::new("lap_position", DataType::Float32, true),
            Field::new("lap_completed", DataType::UInt32, true),
            Field::new("lap_current_time_ns", DataType::UInt64, true),
            Field::new("lap_simulator_distance_m", DataType::Float64, true),
            Field::new("clutch", DataType::Float32, true),
            Field::new("steering_angle_rad", DataType::Float32, true),
            Field::new("fuel_litres", DataType::Float32, true),
            Field::new("gear_kind", DataType::Int8, true),
            Field::new("gear_value", DataType::Int16, true),
            Field::new("position_x_m", DataType::Float64, true),
            Field::new("position_y_m", DataType::Float64, true),
            Field::new("position_z_m", DataType::Float64, true),
            Field::new("position_frame", DataType::Int8, true),
            Field::new("velocity_x_mps", DataType::Float64, true),
            Field::new("velocity_y_mps", DataType::Float64, true),
            Field::new("velocity_z_mps", DataType::Float64, true),
            Field::new("velocity_frame", DataType::Int8, true),
            Field::new("acceleration_x_mps2", DataType::Float64, true),
            Field::new("acceleration_y_mps2", DataType::Float64, true),
            Field::new("acceleration_z_mps2", DataType::Float64, true),
            Field::new("acceleration_frame", DataType::Int8, true),
            Field::new("ambient_temperature_c", DataType::Float32, true),
            Field::new("track_temperature_c", DataType::Float32, true),
            Field::new("track_grip", DataType::Float32, true),
            wheel_field("front_left", "angular_speed_rad_s"),
            wheel_field("front_left", "tyre_pressure_pa"),
            wheel_field("front_left", "tyre_core_temperature_c"),
            wheel_field("front_left", "suspension_travel_m"),
            wheel_field("front_right", "angular_speed_rad_s"),
            wheel_field("front_right", "tyre_pressure_pa"),
            wheel_field("front_right", "tyre_core_temperature_c"),
            wheel_field("front_right", "suspension_travel_m"),
            wheel_field("rear_left", "angular_speed_rad_s"),
            wheel_field("rear_left", "tyre_pressure_pa"),
            wheel_field("rear_left", "tyre_core_temperature_c"),
            wheel_field("rear_left", "suspension_travel_m"),
            wheel_field("rear_right", "angular_speed_rad_s"),
            wheel_field("rear_right", "tyre_pressure_pa"),
            wheel_field("rear_right", "tyre_core_temperature_c"),
            wheel_field("rear_right", "suspension_travel_m"),
        ],
        HashMap::from([
            ("trace.format".into(), FORMAT_NAME.into()),
            ("trace.schema_version".into(), SCHEMA_VERSION.into()),
            ("trace.units".into(), "si".into()),
        ]),
    )
}

fn schema_v1() -> Schema {
    Schema::new_with_metadata(
        vec![
            Field::new("sequence", DataType::UInt64, false),
            Field::new("elapsed_ns", DataType::UInt64, false),
            Field::new("throttle", DataType::Float32, true),
            Field::new("brake", DataType::Float32, true),
            Field::new("speed_mps", DataType::Float32, true),
            Field::new("engine_rpm", DataType::Float32, true),
            Field::new("lap_position", DataType::Float32, true),
        ],
        HashMap::from([
            ("trace.format".into(), FORMAT_NAME.into()),
            ("trace.schema_version".into(), "1".into()),
            ("trace.units".into(), "si".into()),
        ]),
    )
}

fn wheel_field(corner: &str, channel: &str) -> Field {
    Field::new(format!("wheel_{corner}_{channel}"), DataType::Float32, true)
}

fn validate_schema(value: &Schema) -> Result<(), IpcError> {
    let version = value
        .metadata()
        .get("trace.schema_version")
        .map(String::as_str);
    let fields_match = match version {
        Some("1") => value.fields() == schema_v1().fields(),
        Some(SCHEMA_VERSION) => value.fields() == schema().fields(),
        _ => false,
    };
    if value.metadata().get("trace.format").map(String::as_str) != Some(FORMAT_NAME)
        || !fields_match
    {
        Err(IpcError::UnsupportedSchema)
    } else {
        Ok(())
    }
}

fn required_u64(batch: &RecordBatch, index: usize) -> Result<Vec<u64>, IpcError> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<UInt64Array>()
        .ok_or(IpcError::UnsupportedSchema)?;
    if array.null_count() != 0 {
        return Err(IpcError::UnexpectedNull);
    }
    Ok(array.values().to_vec())
}

fn nullable_f32(batch: &RecordBatch, index: usize) -> Result<Vec<Option<f32>>, IpcError> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<Float32Array>()
        .ok_or(IpcError::UnsupportedSchema)?;
    Ok(array.iter().collect())
}

#[cfg(test)]
fn nullable_f64(batch: &RecordBatch, index: usize) -> Result<Vec<Option<f64>>, IpcError> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<Float64Array>()
        .ok_or(IpcError::UnsupportedSchema)?;
    Ok(array.iter().collect())
}

#[cfg(test)]
fn nullable_u32(batch: &RecordBatch, index: usize) -> Result<Vec<Option<u32>>, IpcError> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<UInt32Array>()
        .ok_or(IpcError::UnsupportedSchema)?;
    Ok(array.iter().collect())
}

#[cfg(test)]
fn nullable_i8(batch: &RecordBatch, index: usize) -> Result<Vec<Option<i8>>, IpcError> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<Int8Array>()
        .ok_or(IpcError::UnsupportedSchema)?;
    Ok(array.iter().collect())
}

#[cfg(test)]
fn nullable_i16(batch: &RecordBatch, index: usize) -> Result<Vec<Option<i16>>, IpcError> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<Int16Array>()
        .ok_or(IpcError::UnsupportedSchema)?;
    Ok(array.iter().collect())
}

/// Arrow representation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IpcError {
    EmptyBatch,
    InvalidBatchSize,
    SampleOverflow,
    InvalidSampleRange,
    UnsupportedSchema,
    UnexpectedNull,
    Arrow(String),
}

impl From<arrow_schema::ArrowError> for IpcError {
    fn from(error: arrow_schema::ArrowError) -> Self {
        Self::Arrow(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trace_domain::{
        CoordinateFrame, DriverInputs, ElapsedNanoseconds, EnvironmentState, FrameSequence, Gear,
        LapObservation, MotionState, Vector3, VehicleState, WheelState,
    };

    #[test]
    fn round_trip_preserves_alignment_units_and_missing_channels() {
        let frames = vec![
            TelemetryFrame {
                sequence: FrameSequence(10),
                elapsed: ElapsedNanoseconds(1_000),
                inputs: DriverInputs {
                    throttle: Some(0.5),
                    ..DriverInputs::default()
                },
                vehicle: VehicleState {
                    speed_mps: Some(30.0),
                    engine_rpm: Some(6_000.0),
                    ..VehicleState::default()
                },
                lap: LapObservation {
                    normalized_position: Some(0.25),
                    ..LapObservation::default()
                },
                ..TelemetryFrame::default()
            },
            TelemetryFrame {
                sequence: FrameSequence(11),
                elapsed: ElapsedNanoseconds(2_000),
                inputs: DriverInputs {
                    brake: Some(0.8),
                    ..DriverInputs::default()
                },
                ..TelemetryFrame::default()
            },
        ];
        let bytes = encode_frames(&frames).expect("encoded Arrow file");
        assert!(bytes.starts_with(b"ARROW1"));
        let decoded = decode_columns(&bytes).expect("decoded Arrow file");
        assert_eq!(decoded.sequence, vec![10, 11]);
        assert_eq!(decoded.throttle, vec![Some(0.5), None]);
        assert_eq!(decoded.brake, vec![None, Some(0.8)]);
        assert_eq!(decoded.speed_mps, vec![Some(30.0), None]);
    }

    #[test]
    fn rejects_empty_and_non_arrow_input() {
        assert_eq!(encode_frames(&[]), Err(IpcError::EmptyBatch));
        assert!(matches!(
            decode_columns(b"not arrow"),
            Err(IpcError::Arrow(_))
        ));
    }

    #[test]
    fn incremental_writer_bounds_batches_and_produces_one_valid_file() {
        let mut writer = TelemetryIpcWriter::new(Vec::new(), 2).expect("writer");
        for sequence in 0..5 {
            writer
                .push(TelemetryFrame {
                    sequence: trace_domain::FrameSequence(sequence),
                    elapsed: trace_domain::ElapsedNanoseconds(sequence * 10),
                    ..TelemetryFrame::default()
                })
                .expect("frame");
            assert!(writer.buffered_frames() < 2);
        }
        let (bytes, samples) = writer.finish().expect("finished");
        assert_eq!(samples, 5);
        assert_eq!(
            decode_columns(&bytes).expect("decoded").sequence,
            vec![0, 1, 2, 3, 4]
        );
    }

    #[test]
    fn schema_v2_preserves_full_canonical_channel_families() {
        let mut wheels = trace_domain::WheelStates::new();
        wheels.insert(
            WheelCorner::FrontLeft,
            WheelState {
                angular_speed_rad_s: Some(42.0),
                tyre_pressure_pa: Some(190_000.0),
                tyre_core_temperature_c: Some(88.0),
                suspension_travel_m: Some(0.04),
            },
        );
        let frame = TelemetryFrame {
            sequence: FrameSequence(7),
            elapsed: ElapsedNanoseconds(8),
            lap: LapObservation {
                completed_laps: Some(3),
                normalized_position: Some(0.4),
                current_lap_time_ns: Some(9),
                simulator_distance_m: Some(10.0),
            },
            inputs: DriverInputs {
                clutch: Some(0.2),
                steering_angle_rad: Some(-0.3),
                ..DriverInputs::default()
            },
            vehicle: VehicleState {
                gear: Some(Gear::Unknown(12)),
                fuel_litres: Some(22.0),
                ..VehicleState::default()
            },
            motion: MotionState {
                position_m: Some(Vector3 {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                    frame: CoordinateFrame::SourceWorld,
                }),
                velocity_mps: Some(Vector3 {
                    x: 4.0,
                    y: 5.0,
                    z: 6.0,
                    frame: CoordinateFrame::TraceWorld,
                }),
                acceleration_mps2: Some(Vector3 {
                    x: 7.0,
                    y: 8.0,
                    z: 9.0,
                    frame: CoordinateFrame::Vehicle,
                }),
            },
            wheels,
            environment: Some(EnvironmentState {
                ambient_temperature_c: Some(21.0),
                track_temperature_c: Some(34.0),
                track_grip: Some(0.98),
            }),
        };
        let bytes = encode_frames(&[frame]).expect("encoded");
        let mut reader = FileReader::try_new(Cursor::new(bytes), None).expect("reader");
        assert_eq!(
            reader.schema().metadata().get("trace.schema_version"),
            Some(&"2".to_owned())
        );
        assert_eq!(reader.schema().fields().len(), 46);
        let batch = reader.next().expect("batch").expect("valid batch");
        assert_eq!(nullable_u32(&batch, 7).expect("laps"), vec![Some(3)]);
        assert_eq!(nullable_i8(&batch, 13).expect("gear kind"), vec![Some(2)]);
        assert_eq!(
            nullable_i16(&batch, 14).expect("gear value"),
            vec![Some(12)]
        );
        assert_eq!(
            nullable_f64(&batch, 15).expect("position x"),
            vec![Some(1.0)]
        );
        assert_eq!(
            nullable_i8(&batch, 22).expect("velocity frame"),
            vec![Some(1)]
        );
        assert_eq!(
            nullable_i8(&batch, 26).expect("acceleration frame"),
            vec![Some(2)]
        );
        assert_eq!(nullable_f32(&batch, 27).expect("ambient"), vec![Some(21.0)]);
        assert_eq!(
            nullable_f32(&batch, 30).expect("wheel speed"),
            vec![Some(42.0)]
        );
        assert_eq!(nullable_f32(&batch, 34).expect("missing wheel"), vec![None]);
    }

    #[test]
    fn schema_v1_projection_remains_readable() {
        let schema = Arc::new(schema_v1());
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![
                Arc::new(UInt64Array::from(vec![1])),
                Arc::new(UInt64Array::from(vec![2])),
                Arc::new(Float32Array::from(vec![Some(0.3)])),
                Arc::new(Float32Array::from(vec![None])),
                Arc::new(Float32Array::from(vec![Some(40.0)])),
                Arc::new(Float32Array::from(vec![Some(7_000.0)])),
                Arc::new(Float32Array::from(vec![Some(0.5)])),
            ],
        )
        .expect("batch");
        let mut bytes = Vec::new();
        {
            let mut writer = FileWriter::try_new(&mut bytes, &schema).expect("writer");
            writer.write(&batch).expect("write");
            writer.finish().expect("finish");
        }
        let decoded = decode_columns(&bytes).expect("v1 projection");
        assert_eq!(decoded.sequence, vec![1]);
        assert_eq!(decoded.speed_mps, vec![Some(40.0)]);
    }

    #[test]
    fn range_reader_slices_across_record_batch_boundaries() {
        let mut writer = TelemetryIpcWriter::new(Vec::new(), 2).expect("writer");
        for sequence in 0..6 {
            writer
                .push(TelemetryFrame {
                    sequence: FrameSequence(sequence),
                    elapsed: ElapsedNanoseconds(sequence * 10),
                    vehicle: VehicleState {
                        speed_mps: Some(f32::from(
                            u16::try_from(sequence).expect("bounded sequence"),
                        )),
                        ..VehicleState::default()
                    },
                    ..TelemetryFrame::default()
                })
                .expect("push");
        }
        let (bytes, _) = writer.finish().expect("finish");
        let range = read_columns_range(Cursor::new(bytes.clone()), 1, 4).expect("range");
        assert_eq!(range.sequence, vec![1, 2, 3, 4]);
        assert_eq!(
            range.speed_mps,
            vec![Some(1.0), Some(2.0), Some(3.0), Some(4.0)]
        );
        assert_eq!(
            read_columns_range(Cursor::new(bytes), 5, 2),
            Err(IpcError::InvalidSampleRange)
        );
    }
}
