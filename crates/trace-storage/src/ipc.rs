//! Versioned Apache Arrow IPC telemetry representation spike.

use std::{collections::HashMap, io::Cursor, sync::Arc};

use arrow_array::{Array, Float32Array, RecordBatch, UInt64Array};
use arrow_ipc::{reader::FileReader, writer::FileWriter};
use arrow_schema::{DataType, Field, Schema};
use trace_domain::TelemetryFrame;

const FORMAT_NAME: &str = "trace.telemetry";
const SCHEMA_VERSION: &str = "1";

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
    let columns = TelemetryColumns::from_frames(frames);
    let schema = Arc::new(schema());
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(UInt64Array::from(columns.sequence)),
            Arc::new(UInt64Array::from(columns.elapsed_ns)),
            Arc::new(Float32Array::from(columns.throttle)),
            Arc::new(Float32Array::from(columns.brake)),
            Arc::new(Float32Array::from(columns.speed_mps)),
            Arc::new(Float32Array::from(columns.engine_rpm)),
            Arc::new(Float32Array::from(columns.lap_position)),
        ],
    )
    .map_err(IpcError::from)?;

    let mut output = Cursor::new(Vec::new());
    {
        let mut writer = FileWriter::try_new(&mut output, &schema).map_err(IpcError::from)?;
        writer.write(&batch).map_err(IpcError::from)?;
        writer.finish().map_err(IpcError::from)?;
    }
    Ok(output.into_inner())
}

/// Decodes and validates an Arrow IPC telemetry spike file.
///
/// # Errors
///
/// Rejects malformed Arrow data, an unknown TRACE schema, or wrong columns.
pub fn decode_columns(bytes: &[u8]) -> Result<TelemetryColumns, IpcError> {
    let mut reader = FileReader::try_new(Cursor::new(bytes), None).map_err(IpcError::from)?;
    validate_schema(reader.schema().as_ref())?;
    let mut decoded = TelemetryColumns {
        sequence: Vec::new(),
        elapsed_ns: Vec::new(),
        throttle: Vec::new(),
        brake: Vec::new(),
        speed_mps: Vec::new(),
        engine_rpm: Vec::new(),
        lap_position: Vec::new(),
    };
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
        ],
        HashMap::from([
            ("trace.format".into(), FORMAT_NAME.into()),
            ("trace.schema_version".into(), SCHEMA_VERSION.into()),
            ("trace.units".into(), "si".into()),
        ]),
    )
}

fn validate_schema(value: &Schema) -> Result<(), IpcError> {
    if value.metadata().get("trace.format").map(String::as_str) != Some(FORMAT_NAME)
        || value
            .metadata()
            .get("trace.schema_version")
            .map(String::as_str)
            != Some(SCHEMA_VERSION)
        || value.fields() != schema().fields()
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

/// Arrow representation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IpcError {
    EmptyBatch,
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
        DriverInputs, ElapsedNanoseconds, FrameSequence, LapObservation, VehicleState,
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
}
