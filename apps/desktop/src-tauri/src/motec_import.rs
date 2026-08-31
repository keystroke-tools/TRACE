use std::{fs, io::Write, path::Path};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use trace_domain::TelemetryFrame;
use trace_motec::{LdImportLimits, LdImportMetadata, MotecLdReader};
use trace_storage::{
    BlobCommit, BlobFormat, FileBlobStore, RelativeBlobPath, TelemetryBlobStore,
    ipc::{TELEMETRY_SCHEMA_VERSION, encode_frames},
    metadata::{MetadataStore, NewLap, NewSession, SessionConditions},
};

const MAX_NATIVE_FRAMES: u64 = 5_000_000;

pub(crate) struct MotecImportSummary {
    pub lap_count: usize,
    pub sample_count: u64,
}

pub(crate) fn import_motec_session(
    data_directory: &Path,
    source_path: &Path,
    session_id: &str,
    maximum_session_bytes: u64,
) -> Result<MotecImportSummary, String> {
    let (source_metadata, frames) = read_source(source_path, maximum_session_bytes)?;
    let sample_count = u64::try_from(frames.len())
        .map_err(|_| "MoTeC sample count does not fit TRACE storage".to_owned())?;
    let laps = build_laps(session_id, &source_metadata, &frames)?;
    let arrow = encode_frames(&frames)
        .map_err(|error| format!("failed to encode imported MoTeC telemetry: {error:?}"))?;
    if u64::try_from(arrow.len()).unwrap_or(u64::MAX) > maximum_session_bytes {
        return Err("Imported MoTeC telemetry exceeds TRACE's session storage limit".into());
    }

    fs::create_dir_all(data_directory)
        .map_err(|error| format!("failed to prepare TRACE storage: {error}"))?;
    let mut metadata = MetadataStore::open(&data_directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let mut blobs =
        FileBlobStore::open(&data_directory.join("telemetry"), maximum_session_bytes)
            .map_err(|error| format!("failed to open TRACE telemetry storage: {error:?}"))?;
    let imported_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| format!("failed to timestamp imported session: {error}"))?;
    metadata
        .create_session(&new_session(
            session_id,
            &source_metadata,
            &frames,
            &imported_at,
        ))
        .map_err(|error| format!("failed to create imported session: {error:?}"))?;

    let relative_path = RelativeBlobPath::parse(format!("sessions/{session_id}.arrow"))
        .map_err(|error| format!("failed to create imported telemetry path: {error:?}"))?;
    let import_result = (|| {
        let mut writer = blobs
            .begin_writer()
            .map_err(|error| format!("failed to stage imported telemetry: {error:?}"))?;
        writer
            .write_all(&arrow)
            .map_err(|error| format!("failed to write imported telemetry: {error}"))?;
        writer
            .flush()
            .map_err(|error| format!("failed to flush imported telemetry: {error}"))?;
        let blob = blobs
            .commit(
                &writer.into_pending(),
                BlobCommit {
                    path: relative_path.clone(),
                    format: BlobFormat::ArrowIpc,
                    schema_version: TELEMETRY_SCHEMA_VERSION,
                    sample_count,
                    expected_sha256: None,
                },
            )
            .map_err(|error| format!("failed to commit imported telemetry: {error:?}"))?;
        metadata
            .complete_session(session_id, &imported_at, &blob, &laps)
            .map_err(|error| format!("failed to index imported laps: {error:?}"))?;
        metadata
            .update_session_details(
                session_id,
                None,
                nonempty(&source_metadata.driver).as_deref(),
                "other",
                &["MoTeC".into()],
            )
            .map_err(|error| format!("failed to restore MoTeC attribution: {error:?}"))?;
        Ok::<(), String>(())
    })();
    if let Err(error) = import_result {
        let _ = metadata.delete_session(session_id);
        let _ = fs::remove_file(
            data_directory
                .join("telemetry")
                .join(relative_path.as_str()),
        );
        return Err(error);
    }

    Ok(MotecImportSummary {
        lap_count: laps.len(),
        sample_count,
    })
}

fn read_source(
    source_path: &Path,
    maximum_session_bytes: u64,
) -> Result<(LdImportMetadata, Vec<TelemetryFrame>), String> {
    let source_size = fs::metadata(source_path)
        .map_err(|error| format!("failed to inspect MoTeC log: {error}"))?
        .len();
    let limits = LdImportLimits {
        max_ld_bytes: usize::try_from(maximum_session_bytes.min(512 * 1024 * 1024))
            .unwrap_or(usize::MAX),
        max_output_frames: MAX_NATIVE_FRAMES,
        ..LdImportLimits::default()
    };
    if source_size > u64::try_from(limits.max_ld_bytes).unwrap_or(u64::MAX) {
        return Err("MoTeC log is larger than the supported import limit".into());
    }
    let source =
        fs::read(source_path).map_err(|error| format!("failed to read MoTeC log: {error}"))?;
    let sidecar_path = source_path.with_extension("ldx");
    let sidecar = match fs::read(&sidecar_path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("failed to read matching MoTeC sidecar: {error}")),
    };
    let mut reader = MotecLdReader::new(source, sidecar.as_deref(), limits)
        .map_err(|error| format!("invalid or unsupported MoTeC log: {error:?}"))?;
    let source_metadata = reader.metadata().clone();
    if source_metadata.ldx.is_none() {
        return Err(format!(
            "No matching {} was found. This first import path requires the .ldx sidecar so lap boundaries are not guessed.",
            sidecar_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(".ldx file")
        ));
    }
    let capacity = usize::try_from(source_metadata.frame_count)
        .map_err(|_| "MoTeC sample count does not fit this system".to_owned())?;
    let mut frames = Vec::with_capacity(capacity);
    while let Some(frame) = reader.next_frame() {
        frames.push(frame);
    }
    if let Some(track_length_m) = derive_track_length(&source_metadata, &frames) {
        for frame in &mut frames {
            if let Some(native) = frame.native.as_deref_mut() {
                native
                    .float_fields
                    .insert("trace.derived_track_length_m".into(), track_length_m);
            }
        }
    }
    Ok((source_metadata, frames))
}

fn derive_track_length(metadata: &LdImportMetadata, frames: &[TelemetryFrame]) -> Option<f64> {
    let boundaries = &metadata.ldx.as_ref()?.boundaries;
    let mut lengths = boundaries
        .windows(2)
        .filter_map(|window| {
            let start = usize::try_from(boundary_sample(
                window[0].elapsed_ns,
                metadata.output_rate_hz,
            ))
            .ok()?
            .min(frames.len());
            let end = usize::try_from(boundary_sample(
                window[1].elapsed_ns,
                metadata.output_rate_hz,
            ))
            .ok()?
            .min(frames.len());
            let samples = frames.get(start..end)?;
            let mut observed_steps = 0_usize;
            let distance = samples.windows(2).fold(0.0, |total, pair| {
                let Some(left) = pair[0].motion.position_m else {
                    return total;
                };
                let Some(right) = pair[1].motion.position_m else {
                    return total;
                };
                let dx = right.x - left.x;
                let dz = right.z - left.z;
                let step = dx.hypot(dz);
                if step.is_finite() && step <= 100.0 {
                    observed_steps += 1;
                    total + step
                } else {
                    total
                }
            });
            let expected_steps = samples.len().saturating_sub(1);
            (expected_steps > 0
                && observed_steps.saturating_mul(10) >= expected_steps.saturating_mul(9)
                && (500.0..=30_000.0).contains(&distance))
            .then_some(distance)
        })
        .collect::<Vec<_>>();
    lengths.sort_by(f64::total_cmp);
    lengths.get(lengths.len() / 2).copied()
}

fn new_session(
    session_id: &str,
    source: &LdImportMetadata,
    frames: &[TelemetryFrame],
    imported_at: &str,
) -> NewSession {
    let simulator_key = if source.event_name.as_deref() == Some("AC_LIVE") {
        "assetto-corsa"
    } else {
        "unknown-simulator"
    };
    let environment = frames.first().and_then(|frame| frame.environment);
    NewSession {
        id: session_id.into(),
        simulator_id: format!("sim-{simulator_key}"),
        simulator_key: simulator_key.into(),
        simulator_version: None,
        track_id: nonempty(&source.venue).map(|value| format!("track-{}", hex_identity(&value))),
        source_track_id: nonempty(&source.venue),
        layout_id: None,
        track_display_name: nonempty(&source.venue),
        car_id: nonempty(&source.vehicle_id).map(|value| format!("car-{}", hex_identity(&value))),
        source_car_id: nonempty(&source.vehicle_id),
        car_display_name: nonempty(&source.vehicle_id),
        started_at: imported_at.into(),
        session_type: source.session_type.as_deref().and_then(nonempty),
        source_kind: "imported".into(),
        conditions: SessionConditions {
            ambient_temperature_c: environment
                .and_then(|value| value.ambient_temperature_c)
                .map(|value| value.round().to_string()),
            road_temperature_c: environment
                .and_then(|value| value.track_temperature_c)
                .map(|value| value.round().to_string()),
            weather_name: None,
            track_grip_percent: environment
                .and_then(|value| value.track_grip)
                .map(|value| (value * 100.0).round())
                .and_then(float_to_u8),
        },
    }
}

fn build_laps(
    session_id: &str,
    metadata: &LdImportMetadata,
    frames: &[TelemetryFrame],
) -> Result<Vec<NewLap>, String> {
    let sidecar = metadata
        .ldx
        .as_ref()
        .ok_or_else(|| "MoTeC lap sidecar is missing".to_owned())?;
    let mut starts = Vec::with_capacity(sidecar.boundaries.len() + 2);
    starts.push((0_u64, 0_usize));
    for boundary in &sidecar.boundaries {
        let sample = boundary_sample(boundary.elapsed_ns, metadata.output_rate_hz);
        let sample = usize::try_from(sample)
            .map_err(|_| "MoTeC lap marker does not fit this system".to_owned())?
            .min(frames.len());
        starts.push((boundary.elapsed_ns, sample));
    }
    starts.push((
        frames.last().map_or(0, |frame| frame.elapsed.0),
        frames.len(),
    ));

    let mut laps = starts
        .windows(2)
        .enumerate()
        .filter_map(|(index, window)| {
            let (start_ns, sample_start) = window[0];
            let (end_ns, sample_end) = window[1];
            (sample_end > sample_start).then(|| {
                let partial = index == 0 || index + 2 == starts.len();
                let samples = &frames[sample_start..sample_end];
                let invalidated = samples.iter().any(|frame| {
                    native_value(frame, "Lap Invalidated").is_some_and(|value| value > 0.5)
                });
                let has_validity = samples
                    .iter()
                    .any(|frame| native_value(frame, "Lap Invalidated").is_some());
                let max_tyres_out = samples.iter().filter_map(|frame| frame.lap.tyres_out).max();
                let (validity, validity_reason) = if partial {
                    ("invalid", "partial lap at the edge of the MoTeC outing")
                } else if invalidated {
                    (
                        "invalid",
                        "source telemetry reported that the lap was invalidated",
                    )
                } else if has_validity {
                    ("valid", "source telemetry reported no lap invalidation")
                } else {
                    ("unknown", "source telemetry did not expose lap validity")
                };
                NewLap {
                    id: format!("{session_id}-lap-{}", index + 1),
                    lap_index: u32::try_from(index + 1).unwrap_or(u32::MAX),
                    started_offset_ns: Some(start_ns),
                    duration_ns: (!partial).then_some(end_ns.saturating_sub(start_ns)),
                    validity: validity.into(),
                    validity_reason: Some(validity_reason.into()),
                    max_tyres_out,
                    sample_start: u64::try_from(sample_start).unwrap_or(u64::MAX),
                    sample_count: u64::try_from(sample_end - sample_start).unwrap_or(u64::MAX),
                    is_personal_best: false,
                    sectors: Vec::new(),
                }
            })
        })
        .collect::<Vec<_>>();
    let fastest = laps
        .iter()
        .filter(|lap| lap.validity == "valid")
        .filter_map(|lap| lap.duration_ns)
        .min();
    for lap in &mut laps {
        lap.is_personal_best = lap.duration_ns == fastest && fastest.is_some();
    }
    Ok(laps)
}

fn boundary_sample(elapsed_ns: u64, rate_hz: u16) -> u64 {
    elapsed_ns
        .saturating_mul(u64::from(rate_hz))
        .div_ceil(1_000_000_000)
}

fn native_value(frame: &TelemetryFrame, name: &str) -> Option<f64> {
    frame
        .native
        .as_deref()
        .and_then(|native| native.float_fields.get(name))
        .copied()
}

fn nonempty(value: &str) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.trim().to_owned())
}

fn hex_identity(value: &str) -> String {
    use std::fmt::Write as _;
    value.as_bytes().iter().fold(
        String::with_capacity(value.len() * 2),
        |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to String cannot fail");
            output
        },
    )
}

fn float_to_u8(value: f32) -> Option<u8> {
    if !value.is_finite() || !(0.0..=100.0).contains(&value) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(value as u8)
}

#[cfg(test)]
mod tests {
    use std::{fs::File, path::PathBuf};

    use super::*;
    use trace_storage::ipc::read_columns_range;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../crates/trace-motec/tests/fixtures/acti-zandvoort")
            .join(name)
    }

    #[test]
    fn imports_authorised_acti_pair_into_normal_session_storage() {
        let directory = tempfile::tempdir().expect("temporary data directory");
        let summary = import_motec_session(
            directory.path(),
            &fixture("stint-9.ld"),
            "motec-fixture",
            64 * 1024 * 1024,
        )
        .expect("import ACTI fixture");
        assert_eq!(summary.sample_count, 5_019);
        assert_eq!(summary.lap_count, 3);

        let store =
            MetadataStore::open(&directory.path().join("trace.sqlite")).expect("imported metadata");
        let session = store
            .recent_sessions(10)
            .expect("sessions")
            .into_iter()
            .find(|session| session.id == "motec-fixture")
            .expect("imported session");
        assert_eq!(session.simulator_key, "assetto-corsa");
        assert_eq!(session.source_track_id.as_deref(), Some("zandvoort2023"));
        assert_eq!(session.source_car_id.as_deref(), Some("ks_mazda_mx5_cup"));
        assert_eq!(session.user_driver.as_deref(), Some("E. Cavalli"));
        assert_eq!(session.session_type.as_deref(), Some("HOTLAP"));
        assert_eq!(session.laps.len(), 3);
        assert_eq!(session.laps[0].validity, "invalid");
        assert_eq!(session.laps[1].duration_ns, Some(111_885_000_000));
        assert!(session.laps[1].is_personal_best);
        assert_eq!(session.laps[2].validity, "invalid");
        let telemetry = store.session_telemetry("motec-fixture").expect("telemetry");
        assert_eq!(telemetry.sample_count, 5_019);
        let lap = store
            .lap_telemetry(&session.laps[1].id)
            .expect("complete lap telemetry");
        let columns = read_columns_range(
            File::open(
                directory
                    .path()
                    .join("telemetry")
                    .join(telemetry.blob_path.as_str()),
            )
            .expect("Arrow file"),
            lap.sample_start,
            lap.sample_count,
        )
        .expect("visualizer columns");
        assert!(
            columns
                .track_length_m
                .is_some_and(|length| (4_000.0..=5_000.0).contains(&length))
        );
        assert_eq!(columns.position_x_m.len(), 2_237);
    }
}
