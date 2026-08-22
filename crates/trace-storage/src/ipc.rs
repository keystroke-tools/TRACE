//! Versioned Apache Arrow IPC telemetry representation spike.

use std::{
    collections::HashMap,
    io::{Cursor, Read, Seek, Write},
    sync::Arc,
};

use arrow_array::{
    Array, BinaryArray, Float32Array, Float64Array, Int8Array, Int16Array, MapArray, RecordBatch,
    StringArray, StructArray, UInt32Array, UInt64Array,
    builder::{Float64Builder, Int64Builder, MapBuilder, StringBuilder},
};
use arrow_ipc::{
    CompressionType,
    reader::FileReader,
    writer::{FileWriter, IpcWriteOptions},
};
use arrow_schema::{DataType, Field, Schema};
use trace_domain::{CoordinateFrame, Gear, TelemetryFrame, WheelCorner};

const FORMAT_NAME: &str = "trace.telemetry";
const SCHEMA_VERSION: &str = "4";

/// Current full-fidelity telemetry schema version written by TRACE.
pub const TELEMETRY_SCHEMA_VERSION: u32 = 4;

/// Compression policies supported by the standard Arrow IPC file writer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IpcCompression {
    /// Store record-batch buffers without compression.
    None,
    /// Compress record-batch buffers with the LZ4 frame codec.
    Lz4Frame,
    /// Compress record-batch buffers with Zstandard's default level.
    #[default]
    Zstd,
}

/// Compression used by TRACE capture writers unless a caller selects another policy.
pub const DEFAULT_IPC_COMPRESSION: IpcCompression = IpcCompression::Zstd;

const SHARED_NATIVE_FLOAT_FIELDS: [&str; 7] = [
    "static.max_fuel_litres",
    "static.track_spline_length_m",
    "physics.steer_angle",
    "physics.tyre_wear.0",
    "physics.tyre_wear.1",
    "physics.tyre_wear.2",
    "physics.tyre_wear.3",
];
const SHARED_NATIVE_TEXT_FIELDS: [&str; 1] = ["static.track_configuration"];

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
    pub lap_time_ns: Vec<Option<u64>>,
    pub steering_angle_rad: Vec<Option<f32>>,
    pub ambient_temperature_c: Vec<Option<f32>>,
    pub track_temperature_c: Vec<Option<f32>>,
    pub position_x_m: Vec<Option<f64>>,
    pub position_z_m: Vec<Option<f64>>,
    pub gear_kind: Vec<Option<i8>>,
    pub gear_value: Vec<Option<i16>>,
    pub sector_index: Vec<Option<u32>>,
    pub track_length_m: Option<f64>,
    pub track_configuration: Option<String>,
}

/// Bounded summary derived from one lap's immutable telemetry sample range.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LapTelemetryMetrics {
    pub fuel_start_litres: Option<f32>,
    pub fuel_end_litres: Option<f32>,
    pub fuel_capacity_litres: Option<f32>,
    pub max_speed_mps: Option<f32>,
    pub tyre_wear_start: [Option<f32>; 4],
    pub tyre_wear_end: [Option<f32>; 4],
    pub tyre_wear_minimum: [Option<f32>; 4],
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
            lap_time_ns: frames
                .iter()
                .map(|frame| frame.lap.current_lap_time_ns)
                .collect(),
            steering_angle_rad: frames
                .iter()
                .map(|frame| frame.inputs.steering_angle_rad)
                .collect(),
            ambient_temperature_c: frames
                .iter()
                .map(|frame| {
                    frame
                        .environment
                        .and_then(|value| value.ambient_temperature_c)
                })
                .collect(),
            track_temperature_c: frames
                .iter()
                .map(|frame| {
                    frame
                        .environment
                        .and_then(|value| value.track_temperature_c)
                })
                .collect(),
            position_x_m: frames
                .iter()
                .map(|frame| frame.motion.position_m.map(|position| position.x))
                .collect(),
            position_z_m: frames
                .iter()
                .map(|frame| frame.motion.position_m.map(|position| position.z))
                .collect(),
            gear_kind: frames
                .iter()
                .map(|frame| frame.vehicle.gear.map(gear_kind))
                .collect(),
            gear_value: frames
                .iter()
                .map(|frame| frame.vehicle.gear.map(gear_value))
                .collect(),
            sector_index: frames
                .iter()
                .map(|frame| frame.lap.current_sector_index)
                .collect(),
            track_length_m: frames.iter().find_map(|frame| {
                frame
                    .native
                    .as_deref()
                    .and_then(|native| native.float_fields.get("static.track_spline_length_m"))
                    .copied()
                    .filter(|value| value.is_finite() && *value > 0.0)
            }),
            track_configuration: frames.iter().find_map(|frame| {
                frame
                    .native
                    .as_deref()
                    .and_then(|native| native.text_fields.get("static.track_configuration"))
                    .filter(|value| !value.is_empty())
                    .cloned()
            }),
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
            lap_time_ns: Vec::new(),
            steering_angle_rad: Vec::new(),
            ambient_temperature_c: Vec::new(),
            track_temperature_c: Vec::new(),
            position_x_m: Vec::new(),
            position_z_m: Vec::new(),
            gear_kind: Vec::new(),
            gear_value: Vec::new(),
            sector_index: Vec::new(),
            track_length_m: None,
            track_configuration: None,
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
        let mut writer = FileWriter::try_new_with_options(
            &mut output,
            &schema,
            ipc_write_options(DEFAULT_IPC_COMPRESSION)?,
        )
        .map_err(IpcError::from)?;
        writer.write(&batch).map_err(IpcError::from)?;
        writer.finish().map_err(IpcError::from)?;
    }
    Ok(output.into_inner())
}

/// Rewrites a full-fidelity telemetry file into the compact representation used for
/// session sharing. Canonical channels are retained; the opaque source page payload
/// is omitted because it duplicates decoded fields and cannot be inspected by TRACE
/// directly. Repeated native maps are reduced to the small set of values required by
/// current analysis features.
///
/// # Errors
///
/// Returns [`IpcError`] for an unsupported source schema or Arrow read/write failure.
pub fn compact_for_sharing<R: Read + Seek, W: Write>(
    reader: R,
    writer: W,
) -> Result<u64, IpcError> {
    let mut reader = FileReader::try_new_buffered(reader, None).map_err(IpcError::from)?;
    validate_schema(reader.schema().as_ref())?;
    let schema = reader.schema();
    let payload_index = schema.index_of("native_payload").ok();
    let float_index = schema.index_of("native_float_fields").ok();
    let integer_index = schema.index_of("native_integer_fields").ok();
    let text_index = schema.index_of("native_text_fields").ok();
    let mut writer = FileWriter::try_new_with_options(
        writer,
        &schema,
        ipc_write_options(DEFAULT_IPC_COMPRESSION)?,
    )
    .map_err(IpcError::from)?;
    let mut samples = 0_u64;
    for batch in &mut reader {
        let batch = batch.map_err(IpcError::from)?;
        let rows = batch.num_rows();
        let batch = if payload_index.is_some()
            || float_index.is_some()
            || integer_index.is_some()
            || text_index.is_some()
        {
            let mut columns = batch.columns().to_vec();
            if let Some(index) = payload_index {
                columns[index] =
                    Arc::new(std::iter::repeat_n(None::<&[u8]>, rows).collect::<BinaryArray>());
            }
            if let Some(index) = float_index {
                let source = batch.column(index).as_any().downcast_ref::<MapArray>();
                columns[index] = Arc::new(shared_native_float_map(source, rows)?);
            }
            if let Some(index) = integer_index {
                columns[index] = Arc::new(empty_native_integer_map(rows)?);
            }
            if let Some(index) = text_index {
                let source = batch.column(index).as_any().downcast_ref::<MapArray>();
                columns[index] = Arc::new(shared_native_text_map(source, rows)?);
            }
            RecordBatch::try_new(schema.clone(), columns).map_err(IpcError::from)?
        } else {
            batch
        };
        writer.write(&batch).map_err(IpcError::from)?;
        samples = samples
            .checked_add(u64::try_from(rows).map_err(|_| IpcError::SampleOverflow)?)
            .ok_or(IpcError::SampleOverflow)?;
    }
    if samples == 0 {
        return Err(IpcError::EmptyBatch);
    }
    writer.finish().map_err(IpcError::from)?;
    Ok(samples)
}

fn shared_native_float_map(source: Option<&MapArray>, rows: usize) -> Result<MapArray, IpcError> {
    let mut builder = MapBuilder::new(None, StringBuilder::new(), Float64Builder::new());
    for row in 0..rows {
        let mut found = false;
        if let Some(source) = source {
            for key in SHARED_NATIVE_FLOAT_FIELDS {
                if let Some(value) = native_float_value(source, row, key) {
                    builder.keys().append_value(key);
                    builder.values().append_value(value);
                    found = true;
                }
            }
        }
        builder.append(found).map_err(IpcError::from)?;
    }
    Ok(builder.finish())
}

fn empty_native_integer_map(rows: usize) -> Result<MapArray, IpcError> {
    let mut builder = MapBuilder::new(None, StringBuilder::new(), Int64Builder::new());
    for _ in 0..rows {
        builder.append(false).map_err(IpcError::from)?;
    }
    Ok(builder.finish())
}

fn shared_native_text_map(source: Option<&MapArray>, rows: usize) -> Result<MapArray, IpcError> {
    let mut builder = MapBuilder::new(None, StringBuilder::new(), StringBuilder::new());
    for row in 0..rows {
        let mut found = false;
        if let Some(source) = source {
            for key in SHARED_NATIVE_TEXT_FIELDS {
                if let Some(value) = native_text_value(source, row, key) {
                    builder.keys().append_value(key);
                    builder.values().append_value(value);
                    found = true;
                }
            }
        }
        builder.append(found).map_err(IpcError::from)?;
    }
    Ok(builder.finish())
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
        Self::with_compression(writer, batch_size, DEFAULT_IPC_COMPRESSION)
    }

    /// Starts an Arrow IPC file using an explicit compression policy.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError::InvalidBatchSize`] for zero or an Arrow write error.
    pub fn with_compression(
        writer: W,
        batch_size: usize,
        compression: IpcCompression,
    ) -> Result<Self, IpcError> {
        if batch_size == 0 {
            return Err(IpcError::InvalidBatchSize);
        }
        let schema = Arc::new(schema());
        Ok(Self {
            writer: FileWriter::try_new_with_options(
                writer,
                &schema,
                ipc_write_options(compression)?,
            )
            .map_err(IpcError::from)?,
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

fn ipc_write_options(compression: IpcCompression) -> Result<IpcWriteOptions, IpcError> {
    let compression = match compression {
        IpcCompression::None => None,
        IpcCompression::Lz4Frame => Some(CompressionType::LZ4_FRAME),
        IpcCompression::Zstd => Some(CompressionType::ZSTD),
    };
    IpcWriteOptions::default()
        .try_with_compression(compression)
        .map_err(IpcError::from)
}

#[allow(clippy::too_many_lines, clippy::from_iter_instead_of_collect)]
fn record_batch(frames: &[TelemetryFrame]) -> Result<RecordBatch, IpcError> {
    let columns = TelemetryColumns::from_frames(frames);
    let schema = Arc::new(schema());
    let native_float_fields = native_float_map(frames)?;
    let native_integer_fields = native_integer_map(frames)?;
    let native_text_fields = native_text_map(frames)?;
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
            Arc::new(UInt32Array::from_iter(
                frames.iter().map(|frame| frame.lap.current_sector_index),
            )),
            Arc::new(UInt64Array::from_iter(
                frames.iter().map(|frame| frame.lap.last_sector_time_ns),
            )),
            Arc::new(StringArray::from_iter(frames.iter().map(|frame| {
                frame.native.as_ref().map(|native| native.schema.as_str())
            }))),
            Arc::new(BinaryArray::from_iter(frames.iter().map(|frame| {
                frame
                    .native
                    .as_ref()
                    .map(|native| native.payload.as_slice())
            }))),
            Arc::new(native_float_fields),
            Arc::new(native_integer_fields),
            Arc::new(native_text_fields),
        ],
    )
    .map_err(IpcError::from)
}

fn native_float_map(frames: &[TelemetryFrame]) -> Result<arrow_array::MapArray, IpcError> {
    let mut builder = MapBuilder::new(None, StringBuilder::new(), Float64Builder::new());
    for frame in frames {
        if let Some(native) = &frame.native {
            for (key, value) in &native.float_fields {
                builder.keys().append_value(key);
                builder.values().append_value(*value);
            }
            builder.append(true).map_err(IpcError::from)?;
        } else {
            builder.append(false).map_err(IpcError::from)?;
        }
    }
    Ok(builder.finish())
}

fn native_integer_map(frames: &[TelemetryFrame]) -> Result<arrow_array::MapArray, IpcError> {
    let mut builder = MapBuilder::new(None, StringBuilder::new(), Int64Builder::new());
    for frame in frames {
        if let Some(native) = &frame.native {
            for (key, value) in &native.integer_fields {
                builder.keys().append_value(key);
                builder.values().append_value(*value);
            }
            builder.append(true).map_err(IpcError::from)?;
        } else {
            builder.append(false).map_err(IpcError::from)?;
        }
    }
    Ok(builder.finish())
}

fn native_text_map(frames: &[TelemetryFrame]) -> Result<arrow_array::MapArray, IpcError> {
    let mut builder = MapBuilder::new(None, StringBuilder::new(), StringBuilder::new());
    for frame in frames {
        if let Some(native) = &frame.native {
            for (key, value) in &native.text_fields {
                builder.keys().append_value(key);
                builder.values().append_value(value);
            }
            builder.append(true).map_err(IpcError::from)?;
        } else {
            builder.append(false).map_err(IpcError::from)?;
        }
    }
    Ok(builder.finish())
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

/// Validates a telemetry Arrow file and returns its exact number of samples without
/// retaining record batches in memory.
///
/// # Errors
///
/// Rejects malformed Arrow data, unsupported TRACE schemas, empty recordings, and
/// sample-count overflow.
pub fn sample_count<R: Read + Seek>(reader: R) -> Result<u64, IpcError> {
    let mut reader = FileReader::try_new_buffered(reader, None).map_err(IpcError::from)?;
    validate_schema(reader.schema().as_ref())?;
    let mut samples = 0_u64;
    for batch in &mut reader {
        let batch = batch.map_err(IpcError::from)?;
        samples = samples
            .checked_add(u64::try_from(batch.num_rows()).map_err(|_| IpcError::SampleOverflow)?)
            .ok_or(IpcError::SampleOverflow)?;
    }
    if samples == 0 {
        return Err(IpcError::EmptyBatch);
    }
    Ok(samples)
}

/// Streams the stable core telemetry projection as numeric CSV.
///
/// The export deliberately includes only the seven channels shared by every supported
/// TRACE telemetry schema. Missing optional values are emitted as empty fields, and SI
/// units remain explicit in the column names.
///
/// # Errors
///
/// Rejects malformed or unsupported Arrow input, an empty recording, or an output
/// write failure.
pub fn export_core_csv<R: Read + Seek, W: Write>(
    reader: R,
    mut writer: W,
) -> Result<u64, IpcError> {
    let mut reader = FileReader::try_new_buffered(reader, None).map_err(IpcError::from)?;
    validate_schema(reader.schema().as_ref())?;
    writeln!(
        writer,
        "sequence,elapsed_ns,throttle,brake,speed_mps,engine_rpm,lap_position"
    )
    .map_err(IpcError::from)?;

    let mut sample_count = 0_u64;
    for batch in &mut reader {
        let batch = batch.map_err(IpcError::from)?;
        let sequence = required_u64(&batch, 0)?;
        let elapsed_ns = required_u64(&batch, 1)?;
        let throttle = nullable_f32(&batch, 2)?;
        let brake = nullable_f32(&batch, 3)?;
        let speed_mps = nullable_f32(&batch, 4)?;
        let engine_rpm = nullable_f32(&batch, 5)?;
        let lap_position = nullable_f32(&batch, 6)?;

        for index in 0..batch.num_rows() {
            writeln!(
                writer,
                "{},{},{},{},{},{},{}",
                sequence[index],
                elapsed_ns[index],
                csv_optional(throttle[index]),
                csv_optional(brake[index]),
                csv_optional(speed_mps[index]),
                csv_optional(engine_rpm[index]),
                csv_optional(lap_position[index]),
            )
            .map_err(IpcError::from)?;
        }
        sample_count = sample_count
            .checked_add(u64::try_from(batch.num_rows()).map_err(|_| IpcError::SampleOverflow)?)
            .ok_or(IpcError::SampleOverflow)?;
    }
    if sample_count == 0 {
        return Err(IpcError::EmptyBatch);
    }
    writer.flush().map_err(IpcError::from)?;
    Ok(sample_count)
}

fn csv_optional(value: Option<f32>) -> String {
    value.map_or_else(String::new, |value| value.to_string())
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

/// Derives lightweight lap-level metrics without loading a whole recording into memory.
///
/// Fuel and speed use portable canonical columns. Tyre wear is read from AC's native
/// float map when present, so recordings from other simulators or older schemas simply
/// return unavailable tyre values.
///
/// # Errors
///
/// Rejects malformed telemetry and invalid or out-of-bounds sample ranges.
pub fn read_lap_metrics<R: Read + Seek>(
    reader: R,
    sample_start: u64,
    sample_count: u64,
) -> Result<LapTelemetryMetrics, IpcError> {
    if sample_count == 0 {
        return Err(IpcError::InvalidSampleRange);
    }
    let requested_end = sample_start
        .checked_add(sample_count)
        .ok_or(IpcError::InvalidSampleRange)?;
    let mut reader = FileReader::try_new_buffered(reader, None).map_err(IpcError::from)?;
    validate_schema(reader.schema().as_ref())?;
    let mut metrics = LapTelemetryMetrics::default();
    let mut batch_start = 0_u64;
    let mut observed = 0_u64;

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
            observe_lap_metrics(&mut metrics, &batch, local_start, length)?;
            observed = observed
                .checked_add(u64::try_from(length).map_err(|_| IpcError::SampleOverflow)?)
                .ok_or(IpcError::SampleOverflow)?;
        }
        batch_start = batch_end;
        if batch_start >= requested_end {
            break;
        }
    }
    if observed != sample_count {
        return Err(IpcError::InvalidSampleRange);
    }
    Ok(metrics)
}

fn observe_lap_metrics(
    metrics: &mut LapTelemetryMetrics,
    batch: &RecordBatch,
    start: usize,
    length: usize,
) -> Result<(), IpcError> {
    let schema = batch.schema();
    let fuel_index = schema.index_of("fuel_litres").map_err(IpcError::from)?;
    let speed_index = schema.index_of("speed_mps").map_err(IpcError::from)?;
    let fuel = batch
        .column(fuel_index)
        .as_any()
        .downcast_ref::<Float32Array>()
        .ok_or(IpcError::UnsupportedSchema)?;
    let speed = batch
        .column(speed_index)
        .as_any()
        .downcast_ref::<Float32Array>()
        .ok_or(IpcError::UnsupportedSchema)?;
    let native = schema
        .index_of("native_float_fields")
        .ok()
        .and_then(|index| batch.column(index).as_any().downcast_ref::<MapArray>());

    for row in start..start + length {
        if !fuel.is_null(row) && fuel.value(row).is_finite() {
            let value = fuel.value(row);
            metrics.fuel_start_litres.get_or_insert(value);
            metrics.fuel_end_litres = Some(value);
        }
        if !speed.is_null(row) && speed.value(row).is_finite() {
            let value = speed.value(row);
            metrics.max_speed_mps = Some(metrics.max_speed_mps.map_or(value, |max| max.max(value)));
        }
        if let Some(native) = native {
            if let Some(value) = native_float_value(native, row, "static.max_fuel_litres")
                .filter(|value| value.is_finite() && *value > 0.0)
                .map(narrow_native_float)
            {
                metrics.fuel_capacity_litres.get_or_insert(value);
            }
            for corner in 0..4 {
                let key = format!("physics.tyre_wear.{corner}");
                if let Some(value) = native_float_value(native, row, &key)
                    .filter(|value| value.is_finite())
                    .map(narrow_native_float)
                {
                    metrics.tyre_wear_start[corner].get_or_insert(value);
                    metrics.tyre_wear_end[corner] = Some(value);
                    metrics.tyre_wear_minimum[corner] = Some(
                        metrics.tyre_wear_minimum[corner]
                            .map_or(value, |minimum| minimum.min(value)),
                    );
                }
            }
        }
    }
    Ok(())
}

fn native_float_value(map: &MapArray, row: usize, key: &str) -> Option<f64> {
    if map.is_null(row) {
        return None;
    }
    let entries = map.value(row);
    let entries = entries.as_any().downcast_ref::<StructArray>()?;
    let keys = entries.column(0).as_any().downcast_ref::<StringArray>()?;
    let values = entries.column(1).as_any().downcast_ref::<Float64Array>()?;
    (0..entries.len())
        .find(|index| !keys.is_null(*index) && keys.value(*index) == key)
        .and_then(|index| (!values.is_null(index)).then(|| values.value(index)))
}

fn native_text_value(map: &MapArray, row: usize, key: &str) -> Option<String> {
    if map.is_null(row) {
        return None;
    }
    let entries = map.value(row);
    let entries = entries.as_any().downcast_ref::<StructArray>()?;
    let keys = entries.column(0).as_any().downcast_ref::<StringArray>()?;
    let values = entries.column(1).as_any().downcast_ref::<StringArray>()?;
    (0..entries.len())
        .find(|index| !keys.is_null(*index) && keys.value(*index) == key)
        .and_then(|index| (!values.is_null(index)).then(|| values.value(index).to_owned()))
        .filter(|value| !value.is_empty())
}

#[allow(clippy::cast_possible_truncation)]
fn narrow_native_float(value: f64) -> f32 {
    value as f32
}

#[allow(clippy::too_many_lines)]
fn extend_projection(
    decoded: &mut TelemetryColumns,
    batch: &RecordBatch,
    start: usize,
    length: usize,
) -> Result<(), IpcError> {
    let projection_start = decoded.steering_angle_rad.len();
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
    decoded.lap_time_ns.extend(
        optional_u64(batch, "lap_current_time_ns")?
            .into_iter()
            .skip(start)
            .take(length),
    );
    decoded.steering_angle_rad.extend(
        optional_f32(batch, "steering_angle_rad")?
            .into_iter()
            .skip(start)
            .take(length),
    );
    decoded.ambient_temperature_c.extend(
        optional_f32(batch, "ambient_temperature_c")?
            .into_iter()
            .skip(start)
            .take(length),
    );
    decoded.track_temperature_c.extend(
        optional_f32(batch, "track_temperature_c")?
            .into_iter()
            .skip(start)
            .take(length),
    );
    decoded.position_x_m.extend(
        optional_f64(batch, "position_x_m")?
            .into_iter()
            .skip(start)
            .take(length),
    );
    decoded.position_z_m.extend(
        optional_f64(batch, "position_z_m")?
            .into_iter()
            .skip(start)
            .take(length),
    );
    decoded.gear_kind.extend(
        optional_i8(batch, "gear_kind")?
            .into_iter()
            .skip(start)
            .take(length),
    );
    decoded.gear_value.extend(
        optional_i16(batch, "gear_value")?
            .into_iter()
            .skip(start)
            .take(length),
    );
    decoded.sector_index.extend(
        optional_u32(batch, "lap_current_sector_index")?
            .into_iter()
            .skip(start)
            .take(length),
    );
    if let Ok(index) = batch.schema().index_of("native_float_fields")
        && let Some(native) = batch.column(index).as_any().downcast_ref::<MapArray>()
    {
        for offset in 0..length {
            let source_row = start + offset;
            let projected_row = projection_start + offset;
            backfill_native_float(
                &mut decoded.steering_angle_rad[projected_row],
                native,
                source_row,
                "physics.steer_angle",
            );
            backfill_native_float(
                &mut decoded.ambient_temperature_c[projected_row],
                native,
                source_row,
                "physics.air_temperature_c",
            );
            backfill_native_float(
                &mut decoded.track_temperature_c[projected_row],
                native,
                source_row,
                "physics.road_temperature_c",
            );
        }
    }
    if decoded.track_length_m.is_none()
        && let Ok(index) = batch.schema().index_of("native_float_fields")
        && let Some(native) = batch.column(index).as_any().downcast_ref::<MapArray>()
    {
        decoded.track_length_m = (start..start + length).find_map(|row| {
            native_float_value(native, row, "static.track_spline_length_m")
                .filter(|value| value.is_finite() && *value > 0.0)
        });
    }
    if decoded.track_configuration.is_none()
        && let Ok(index) = batch.schema().index_of("native_text_fields")
        && let Some(native) = batch.column(index).as_any().downcast_ref::<MapArray>()
    {
        decoded.track_configuration = (start..start + length)
            .find_map(|row| native_text_value(native, row, "static.track_configuration"));
    }
    Ok(())
}

fn backfill_native_float(destination: &mut Option<f32>, native: &MapArray, row: usize, key: &str) {
    if destination.is_none() {
        *destination = native_float_value(native, row, key)
            .filter(|value| value.is_finite())
            .map(narrow_native_float);
    }
}

fn optional_f32(batch: &RecordBatch, name: &str) -> Result<Vec<Option<f32>>, IpcError> {
    batch.schema().index_of(name).map_or_else(
        |_| Ok(vec![None; batch.num_rows()]),
        |index| nullable_f32(batch, index),
    )
}

fn optional_u64(batch: &RecordBatch, name: &str) -> Result<Vec<Option<u64>>, IpcError> {
    batch.schema().index_of(name).map_or_else(
        |_| Ok(vec![None; batch.num_rows()]),
        |index| nullable_u64(batch, index),
    )
}

fn optional_f64(batch: &RecordBatch, name: &str) -> Result<Vec<Option<f64>>, IpcError> {
    batch.schema().index_of(name).map_or_else(
        |_| Ok(vec![None; batch.num_rows()]),
        |index| nullable_f64(batch, index),
    )
}

fn optional_u32(batch: &RecordBatch, name: &str) -> Result<Vec<Option<u32>>, IpcError> {
    batch.schema().index_of(name).map_or_else(
        |_| Ok(vec![None; batch.num_rows()]),
        |index| nullable_u32(batch, index),
    )
}

fn optional_i8(batch: &RecordBatch, name: &str) -> Result<Vec<Option<i8>>, IpcError> {
    batch.schema().index_of(name).map_or_else(
        |_| Ok(vec![None; batch.num_rows()]),
        |index| nullable_i8(batch, index),
    )
}

fn optional_i16(batch: &RecordBatch, name: &str) -> Result<Vec<Option<i16>>, IpcError> {
    batch.schema().index_of(name).map_or_else(
        |_| Ok(vec![None; batch.num_rows()]),
        |index| nullable_i16(batch, index),
    )
}

fn schema_v2() -> Schema {
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
            ("trace.schema_version".into(), "2".into()),
            ("trace.units".into(), "si".into()),
        ]),
    )
}

fn schema_v3() -> Schema {
    let mut fields = schema_v2()
        .fields()
        .iter()
        .map(|field| field.as_ref().clone())
        .collect::<Vec<_>>();
    fields.push(Field::new(
        "lap_current_sector_index",
        DataType::UInt32,
        true,
    ));
    fields.push(Field::new(
        "lap_last_sector_time_ns",
        DataType::UInt64,
        true,
    ));
    Schema::new_with_metadata(
        fields,
        HashMap::from([
            ("trace.format".into(), FORMAT_NAME.into()),
            ("trace.schema_version".into(), "3".into()),
            ("trace.units".into(), "si".into()),
        ]),
    )
}

fn schema() -> Schema {
    let mut fields = schema_v3()
        .fields()
        .iter()
        .map(|field| field.as_ref().clone())
        .collect::<Vec<_>>();
    fields.push(Field::new("native_schema", DataType::Utf8, true));
    fields.push(Field::new("native_payload", DataType::Binary, true));
    fields.push(native_map_field("native_float_fields", DataType::Float64));
    fields.push(native_map_field("native_integer_fields", DataType::Int64));
    fields.push(native_map_field("native_text_fields", DataType::Utf8));
    Schema::new_with_metadata(
        fields,
        HashMap::from([
            ("trace.format".into(), FORMAT_NAME.into()),
            ("trace.schema_version".into(), SCHEMA_VERSION.into()),
            ("trace.units".into(), "si".into()),
        ]),
    )
}

fn native_map_field(name: &str, value_type: DataType) -> Field {
    Field::new(
        name,
        DataType::Map(
            Arc::new(Field::new(
                "entries",
                DataType::Struct(
                    vec![
                        Field::new("keys", DataType::Utf8, false),
                        Field::new("values", value_type, true),
                    ]
                    .into(),
                ),
                false,
            )),
            false,
        ),
        true,
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
        Some("2") => value.fields() == schema_v2().fields(),
        Some("3") => value.fields() == schema_v3().fields(),
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

fn nullable_f64(batch: &RecordBatch, index: usize) -> Result<Vec<Option<f64>>, IpcError> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<Float64Array>()
        .ok_or(IpcError::UnsupportedSchema)?;
    Ok(array.iter().collect())
}

fn nullable_u32(batch: &RecordBatch, index: usize) -> Result<Vec<Option<u32>>, IpcError> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<UInt32Array>()
        .ok_or(IpcError::UnsupportedSchema)?;
    Ok(array.iter().collect())
}

fn nullable_u64(batch: &RecordBatch, index: usize) -> Result<Vec<Option<u64>>, IpcError> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<UInt64Array>()
        .ok_or(IpcError::UnsupportedSchema)?;
    Ok(array.iter().collect())
}

fn nullable_i8(batch: &RecordBatch, index: usize) -> Result<Vec<Option<i8>>, IpcError> {
    let array = batch
        .column(index)
        .as_any()
        .downcast_ref::<Int8Array>()
        .ok_or(IpcError::UnsupportedSchema)?;
    Ok(array.iter().collect())
}

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
    Io(String),
}

impl From<arrow_schema::ArrowError> for IpcError {
    fn from(error: arrow_schema::ArrowError) -> Self {
        Self::Arrow(error.to_string())
    }
}

impl From<std::io::Error> for IpcError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use trace_domain::{
        CoordinateFrame, DriverInputs, ElapsedNanoseconds, EnvironmentState, FrameSequence, Gear,
        LapObservation, MotionState, NativeTelemetrySample, Vector3, VehicleState, WheelState,
    };

    #[test]
    fn csv_export_streams_the_stable_projection_with_explicit_units() {
        let frames = vec![TelemetryFrame {
            sequence: FrameSequence(7),
            elapsed: ElapsedNanoseconds(42),
            inputs: DriverInputs {
                throttle: Some(0.75),
                ..DriverInputs::default()
            },
            vehicle: VehicleState {
                speed_mps: Some(31.5),
                ..VehicleState::default()
            },
            ..TelemetryFrame::default()
        }];
        let bytes = encode_frames(&frames).expect("encoded");
        let mut csv = Vec::new();

        assert_eq!(
            export_core_csv(Cursor::new(bytes), &mut csv).expect("exported"),
            1
        );
        assert_eq!(
            String::from_utf8(csv).expect("UTF-8"),
            "sequence,elapsed_ns,throttle,brake,speed_mps,engine_rpm,lap_position\n\
             7,42,0.75,,31.5,,\n"
        );
    }

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
                native: Some(Box::new(NativeTelemetrySample {
                    text_fields: BTreeMap::from([(
                        "static.track_configuration".into(),
                        "layout_gp".into(),
                    )]),
                    ..NativeTelemetrySample::default()
                })),
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
        let projected = read_columns_range(Cursor::new(bytes), 0, 2).expect("full projection");
        assert_eq!(projected.track_configuration.as_deref(), Some("layout_gp"));
    }

    #[test]
    fn compact_share_profile_keeps_analysis_data_and_strips_redundant_native_data() {
        let frames = (0..8)
            .map(|sequence| TelemetryFrame {
                sequence: FrameSequence(sequence),
                elapsed: ElapsedNanoseconds(sequence * 10),
                inputs: DriverInputs {
                    throttle: Some(0.75),
                    ..DriverInputs::default()
                },
                native: Some(Box::new(NativeTelemetrySample {
                    schema: "fixture.native/1".into(),
                    payload: vec![u8::try_from(sequence).expect("byte"); 1_500],
                    float_fields: BTreeMap::from([
                        ("physics.steer_angle".into(), 0.4),
                        ("physics.tyre_wear.0".into(), 98.0),
                        ("static.max_fuel_litres".into(), 60.0),
                        ("physics.unused".into(), 123.0),
                    ]),
                    integer_fields: BTreeMap::from([("physics.unused".into(), 42)]),
                    text_fields: BTreeMap::from([
                        ("static.track_configuration".into(), "gp".into()),
                        ("graphics.unused".into(), "value".into()),
                    ]),
                })),
                ..TelemetryFrame::default()
            })
            .collect::<Vec<_>>();
        let full = encode_frames(&frames).expect("full telemetry");
        let mut compact = Vec::new();
        assert_eq!(
            compact_for_sharing(Cursor::new(&full), &mut compact).expect("compact telemetry"),
            8
        );
        assert!(compact.len() < full.len());

        let projected = read_columns_range(Cursor::new(&compact), 0, 8).expect("projection");
        assert_eq!(projected.throttle, vec![Some(0.75); 8]);
        assert_eq!(projected.steering_angle_rad, vec![Some(0.4); 8]);
        assert_eq!(projected.track_configuration.as_deref(), Some("gp"));
        let metrics = read_lap_metrics(Cursor::new(&compact), 0, 8).expect("metrics");
        assert_eq!(metrics.fuel_capacity_litres, Some(60.0));
        assert_eq!(metrics.tyre_wear_start[0], Some(98.0));

        let mut reader = FileReader::try_new(Cursor::new(compact), None).expect("reader");
        let batch = reader.next().expect("batch").expect("valid batch");
        let payload = batch
            .column(batch.schema().index_of("native_payload").expect("payload"))
            .as_any()
            .downcast_ref::<BinaryArray>()
            .expect("binary payload");
        assert_eq!(payload.null_count(), 8);
        let floats = batch
            .column(
                batch
                    .schema()
                    .index_of("native_float_fields")
                    .expect("float map"),
            )
            .as_any()
            .downcast_ref::<MapArray>()
            .expect("float map");
        assert_eq!(
            native_float_value(floats, 0, "physics.tyre_wear.0"),
            Some(98.0)
        );
        assert_eq!(native_float_value(floats, 0, "physics.unused"), None);
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
    fn every_compression_policy_produces_a_readable_arrow_file() {
        let frames = (0..8)
            .map(|sequence| TelemetryFrame {
                sequence: FrameSequence(sequence),
                elapsed: ElapsedNanoseconds(sequence * 10),
                vehicle: VehicleState {
                    speed_mps: Some(42.0),
                    ..VehicleState::default()
                },
                ..TelemetryFrame::default()
            })
            .collect::<Vec<_>>();

        for compression in [
            IpcCompression::None,
            IpcCompression::Lz4Frame,
            IpcCompression::Zstd,
        ] {
            let mut writer =
                TelemetryIpcWriter::with_compression(Vec::new(), 3, compression).expect("writer");
            for frame in &frames {
                writer.push(frame.clone()).expect("frame");
            }
            let (bytes, samples) = writer.finish().expect("finished");
            assert_eq!(samples, 8);
            assert_eq!(
                decode_columns(&bytes).expect("decoded").sequence,
                (0..8).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn schema_v4_preserves_canonical_channels_and_native_payload() {
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
                current_sector_index: Some(1),
                last_sector_time_ns: Some(11),
                tyres_out: Some(2),
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
            native: Some(Box::new(NativeTelemetrySample {
                schema: "fixture.native/1".into(),
                payload: vec![1, 2, 3],
                float_fields: BTreeMap::from([("physics.speed_kmh".into(), 120.0)]),
                integer_fields: BTreeMap::from([("physics.rpm".into(), 6_000)]),
                text_fields: BTreeMap::from([("graphics.tyre_compound".into(), "SM".into())]),
            })),
        };
        let bytes = encode_frames(&[frame]).expect("encoded");
        let mut reader = FileReader::try_new(Cursor::new(bytes), None).expect("reader");
        assert_eq!(
            reader.schema().metadata().get("trace.schema_version"),
            Some(&"4".to_owned())
        );
        assert_eq!(reader.schema().fields().len(), 53);
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
        assert_eq!(nullable_u32(&batch, 46).expect("sector"), vec![Some(1)]);
        assert_eq!(
            nullable_u64(&batch, 47).expect("sector time"),
            vec![Some(11)]
        );
        let native_schema = batch
            .column(48)
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("native schema");
        let native_payload = batch
            .column(49)
            .as_any()
            .downcast_ref::<BinaryArray>()
            .expect("native payload");
        assert_eq!(native_schema.value(0), "fixture.native/1");
        assert_eq!(native_payload.value(0), &[1, 2, 3]);
        for index in 50..=52 {
            let fields = batch
                .column(index)
                .as_any()
                .downcast_ref::<arrow_array::MapArray>()
                .expect("native field map");
            assert_eq!(fields.value_length(0), 1);
        }
    }

    #[test]
    fn projection_recovers_ac_channels_recorded_before_canonical_mapping() {
        let frame = TelemetryFrame {
            native: Some(Box::new(NativeTelemetrySample {
                schema: "assetto-corsa.shared-memory/1".into(),
                float_fields: BTreeMap::from([
                    ("physics.steer_angle".into(), -0.42),
                    ("physics.air_temperature_c".into(), 19.0),
                    ("physics.road_temperature_c".into(), 27.0),
                ]),
                ..NativeTelemetrySample::default()
            })),
            ..TelemetryFrame::default()
        };

        let bytes = encode_frames(&[frame]).expect("encoded");
        let decoded = read_columns_range(Cursor::new(bytes), 0, 1).expect("decoded");

        assert_eq!(decoded.steering_angle_rad, vec![Some(-0.42)]);
        assert_eq!(decoded.ambient_temperature_c, vec![Some(19.0)]);
        assert_eq!(decoded.track_temperature_c, vec![Some(27.0)]);
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
    fn schema_v2_remains_supported_after_sector_fields_are_added() {
        assert_eq!(validate_schema(&schema_v2()), Ok(()));
    }

    #[test]
    fn schema_v3_remains_supported_after_native_payload_is_added() {
        assert_eq!(validate_schema(&schema_v3()), Ok(()));
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

    #[test]
    fn lap_metrics_are_derived_from_a_bounded_sample_range() {
        let frames = (0..5_u64)
            .map(|sequence| {
                let value = f32::from(u16::try_from(sequence).expect("bounded sequence"));
                TelemetryFrame {
                    sequence: FrameSequence(sequence),
                    elapsed: ElapsedNanoseconds(sequence * 10),
                    vehicle: VehicleState {
                        speed_mps: Some(value),
                        fuel_litres: Some(10.0 - value),
                        ..VehicleState::default()
                    },
                    native: Some(Box::new(NativeTelemetrySample {
                        schema: "assetto-corsa.shared-memory/1".into(),
                        float_fields: (0..4)
                            .map(|corner| {
                                (
                                    format!("physics.tyre_wear.{corner}"),
                                    f64::from(100.0 - value),
                                )
                            })
                            .chain([("static.max_fuel_litres".into(), 30.0)])
                            .collect(),
                        ..NativeTelemetrySample::default()
                    })),
                    ..TelemetryFrame::default()
                }
            })
            .collect::<Vec<_>>();
        let bytes = encode_frames(&frames).expect("encoded");

        let metrics = read_lap_metrics(Cursor::new(bytes), 1, 3).expect("metrics");
        assert_eq!(metrics.fuel_start_litres, Some(9.0));
        assert_eq!(metrics.fuel_end_litres, Some(7.0));
        assert_eq!(metrics.fuel_capacity_litres, Some(30.0));
        assert_eq!(metrics.max_speed_mps, Some(3.0));
        assert_eq!(metrics.tyre_wear_start, [Some(99.0); 4]);
        assert_eq!(metrics.tyre_wear_end, [Some(97.0); 4]);
        assert_eq!(metrics.tyre_wear_minimum, [Some(97.0); 4]);
    }
}
