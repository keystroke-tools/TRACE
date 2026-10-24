use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use crate::ac_content::AcContentNames;

use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use trace_domain::TelemetryFrame;
use trace_motec::{LdImportLimits, LdImportMetadata, MotecLdReader};
use trace_storage::{
    BlobCommit, BlobFormat, FileBlobStore, RelativeBlobPath, TelemetryBlobStore,
    ipc::{TELEMETRY_SCHEMA_VERSION, encode_frames},
    metadata::{MetadataStore, NewLap, NewSector, NewSession, SessionConditions},
};

const MAX_NATIVE_FRAMES: u64 = 5_000_000;

pub(crate) struct MotecImportSummary {
    pub lap_count: usize,
    pub sample_count: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MotecImportOptions {
    pub simulator_key: String,
    pub track: String,
    pub car: String,
    pub driver: String,
    pub session_type: String,
    pub title: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MotecImportPreview {
    pub simulator_key: String,
    pub track: String,
    pub car: String,
    pub driver: String,
    pub session_type: String,
    pub title: String,
    pub complete_lap_count: usize,
    pub sample_count: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MotecFolderEntry {
    pub path: String,
    pub file_name: String,
    pub sidecar_present: bool,
    pub preview: Option<MotecImportPreview>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MotecFolderScan {
    pub folder: String,
    pub entries: Vec<MotecFolderEntry>,
    pub limited: bool,
}

const MAX_FOLDER_LOGS: usize = 250;

pub(crate) fn scan_motec_folder(
    folder: &Path,
    maximum_session_bytes: u64,
) -> Result<MotecFolderScan, String> {
    if !folder.is_dir() {
        return Err("Choose a folder containing MoTeC .ld logs".into());
    }
    let (logs, limited) = find_motec_logs(folder, MAX_FOLDER_LOGS)?;
    let entries = logs
        .into_iter()
        .map(|path| {
            let sidecar_present = path.with_extension("ldx").is_file();
            let file_name = path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into(),
            );
            match preview_motec_session(&path, maximum_session_bytes) {
                Ok(preview) => MotecFolderEntry {
                    path: path.display().to_string(),
                    file_name,
                    sidecar_present,
                    preview: Some(preview),
                    error: None,
                },
                Err(error) => MotecFolderEntry {
                    path: path.display().to_string(),
                    file_name,
                    sidecar_present,
                    preview: None,
                    error: Some(error),
                },
            }
        })
        .collect();
    Ok(MotecFolderScan {
        folder: folder.display().to_string(),
        entries,
        limited,
    })
}

fn find_motec_logs(folder: &Path, limit: usize) -> Result<(Vec<PathBuf>, bool), String> {
    let mut pending = vec![folder.to_path_buf()];
    let mut logs = Vec::new();
    let mut limited = false;
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory).map_err(|error| {
            format!(
                "failed to read MoTeC folder {}: {error}",
                directory.display()
            )
        })?;
        for entry in entries {
            let entry =
                entry.map_err(|error| format!("failed to inspect MoTeC folder: {error}"))?;
            let file_type = entry.file_type().map_err(|error| {
                format!("failed to inspect {}: {error}", entry.path().display())
            })?;
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                pending.push(path);
                continue;
            }
            let is_log = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("ld"));
            if is_log {
                if logs.len() == limit {
                    limited = true;
                    continue;
                }
                logs.push(path);
            }
        }
    }
    logs.sort_by(|left, right| left.as_os_str().cmp(right.as_os_str()));
    Ok((logs, limited))
}

pub(crate) fn preview_motec_session(
    source_path: &Path,
    maximum_session_bytes: u64,
) -> Result<MotecImportPreview, String> {
    let (metadata, _) = open_source(source_path, maximum_session_bytes)?;
    let inferred = inferred_options(source_path, &metadata);
    Ok(MotecImportPreview {
        simulator_key: inferred.simulator_key,
        track: inferred.track,
        car: inferred.car,
        driver: inferred.driver,
        session_type: inferred.session_type,
        title: inferred.title,
        complete_lap_count: metadata.lap_boundaries.len().saturating_sub(1),
        sample_count: metadata.frame_count,
    })
}

#[cfg(test)]
pub(crate) fn import_motec_session(
    data_directory: &Path,
    source_path: &Path,
    session_id: &str,
    maximum_session_bytes: u64,
) -> Result<MotecImportSummary, String> {
    import_motec_session_with_options(
        data_directory,
        source_path,
        session_id,
        maximum_session_bytes,
        None,
    )
}

pub(crate) fn import_motec_session_with_options(
    data_directory: &Path,
    source_path: &Path,
    session_id: &str,
    maximum_session_bytes: u64,
    options: Option<&MotecImportOptions>,
) -> Result<MotecImportSummary, String> {
    let (source_metadata, frames) = read_source(source_path, maximum_session_bytes)?;
    let inferred;
    let options = if let Some(options) = options {
        options
    } else {
        inferred = inferred_options(source_path, &source_metadata);
        &inferred
    };
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
    let (source_track_id, layout_id) = resolve_track_identity(&metadata, options)?;
    metadata
        .create_session(&new_session(
            session_id,
            &frames,
            &imported_at,
            options,
            source_track_id.as_deref(),
            layout_id.as_deref(),
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
        let tags = vec!["MoTeC".into()];
        metadata
            .update_session_details(
                session_id,
                nonempty(&options.title).as_deref(),
                nonempty(&options.driver).as_deref(),
                "other",
                &tags,
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
    let (source_metadata, mut reader) = open_source(source_path, maximum_session_bytes)?;
    let capacity = usize::try_from(source_metadata.frame_count)
        .map_err(|_| "MoTeC sample count does not fit this system".to_owned())?;
    let mut frames = Vec::with_capacity(capacity);
    while let Some(frame) = reader.next_frame() {
        frames.push(frame);
    }
    normalize_lap_positions(&source_metadata, &mut frames);
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

fn open_source(
    source_path: &Path,
    maximum_session_bytes: u64,
) -> Result<(LdImportMetadata, MotecLdReader), String> {
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
    let reader = MotecLdReader::new(source, sidecar.as_deref(), limits)
        .map_err(|error| format!("invalid or unsupported MoTeC log: {error:?}"))?;
    let source_metadata = reader.metadata().clone();
    if source_metadata.lap_boundaries.is_empty() {
        return Err(missing_lap_boundaries_error(
            &sidecar_path,
            sidecar.is_some(),
        ));
    }
    Ok((source_metadata, reader))
}

fn missing_lap_boundaries_error(sidecar_path: &Path, sidecar_present: bool) -> String {
    let sidecar_name = sidecar_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(".ldx file");
    if sidecar_present {
        format!(
            "This MoTeC log contains no completed laps. TRACE read the matching {sidecar_name}, but it contains no lap markers; the outing may have ended before the first lap was completed."
        )
    } else {
        format!(
            "No completed lap markers were found. Add the matching {sidecar_name}, or export a MoTeC log with a Lap Number channel."
        )
    }
}

fn derive_track_length(metadata: &LdImportMetadata, frames: &[TelemetryFrame]) -> Option<f64> {
    let boundaries = &metadata.lap_boundaries;
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
    frames: &[TelemetryFrame],
    imported_at: &str,
    options: &MotecImportOptions,
    resolved_source_track_id: Option<&str>,
    resolved_layout_id: Option<&str>,
) -> NewSession {
    let simulator_key =
        nonempty(&options.simulator_key).unwrap_or_else(|| "unknown-simulator".into());
    let track = nonempty(&options.track);
    let source_track_id = resolved_source_track_id
        .map(identity_key)
        .or_else(|| track.as_deref().map(identity_key));
    let car = nonempty(&options.car);
    let environment = frames.first().and_then(|frame| frame.environment);
    NewSession {
        id: session_id.into(),
        simulator_id: format!("sim-{simulator_key}"),
        simulator_key,
        simulator_version: None,
        track_id: source_track_id
            .as_ref()
            .map(|value| format!("track-{}", hex_identity(value))),
        source_track_id,
        layout_id: resolved_layout_id.map(identity_key),
        track_display_name: track,
        car_id: car
            .as_ref()
            .map(|value| format!("car-{}", hex_identity(&identity_key(value)))),
        source_car_id: car.as_deref().map(identity_key),
        car_display_name: car,
        started_at: imported_at.into(),
        session_type: nonempty(&options.session_type),
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

fn resolve_track_identity(
    metadata: &MetadataStore,
    options: &MotecImportOptions,
) -> Result<(Option<String>, Option<String>), String> {
    let Some(reported) = nonempty(&options.track) else {
        return Ok((None, None));
    };
    if options.simulator_key != "assetto-corsa" {
        return Ok((Some(reported), None));
    }
    let configured_path = metadata
        .simulator_install_path("assetto-corsa")
        .map_err(|error| format!("failed to read Assetto Corsa settings: {error:?}"))?
        .map(PathBuf::from);
    let names = AcContentNames::discover(configured_path.as_deref());
    let (source_track_id, layout_id) = names.resolve_track_identity(&reported);
    Ok((Some(source_track_id), layout_id))
}

fn inferred_options(source_path: &Path, source: &LdImportMetadata) -> MotecImportOptions {
    let stem = source_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let parts = stem.split(" - ").map(str::trim).collect::<Vec<_>>();
    let timestamped_name =
        parts.len() >= 4 && looks_like_date(parts[0]) && looks_like_clock(parts[1]);
    let inferred_track = timestamped_name
        .then(|| parts[2..parts.len() - 1].join(" - "))
        .filter(|value| !value.is_empty());
    let run_label = timestamped_name.then(|| parts[parts.len() - 1]);
    let path_hint = source_path.to_string_lossy().to_ascii_lowercase();
    let simulator_key = if source.event_name.as_deref() == Some("AC_LIVE") {
        "assetto-corsa"
    } else if path_hint.contains("lmu") || path_hint.contains("le mans ultimate") {
        "le-mans-ultimate"
    } else {
        "unknown-simulator"
    };
    MotecImportOptions {
        simulator_key: simulator_key.into(),
        track: nonempty(&source.venue)
            .or(inferred_track)
            .unwrap_or_default(),
        car: source.vehicle_id.trim().to_owned(),
        driver: source.driver.trim().to_owned(),
        session_type: source
            .session_type
            .as_deref()
            .and_then(nonempty)
            .or_else(|| run_label.and_then(infer_session_type))
            .unwrap_or_default(),
        title: String::new(),
    }
}

fn looks_like_date(value: &str) -> bool {
    let parts = value.split('-').collect::<Vec<_>>();
    parts.len() == 3
        && parts[0].len() == 4
        && parts[1].len() == 2
        && parts[2].len() == 2
        && parts
            .iter()
            .all(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn looks_like_clock(value: &str) -> bool {
    let parts = value.split('-').collect::<Vec<_>>();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn infer_session_type(label: &str) -> Option<String> {
    let normalized = label.trim().to_ascii_lowercase();
    if normalized.starts_with('p') || normalized.contains("practice") {
        Some("Practice".into())
    } else if normalized.starts_with('q') || normalized.contains("qual") {
        Some("Qualifying".into())
    } else if normalized.starts_with('r') || normalized.contains("race") {
        Some("Race".into())
    } else if normalized.starts_with('h') || normalized.contains("hotlap") {
        Some("Hotlap".into())
    } else {
        None
    }
}

fn identity_key(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn normalize_lap_positions(metadata: &LdImportMetadata, frames: &mut [TelemetryFrame]) {
    let mut starts = Vec::with_capacity(metadata.lap_boundaries.len() + 2);
    starts.push(0);
    starts.extend(metadata.lap_boundaries.iter().filter_map(|boundary| {
        usize::try_from(boundary_sample(
            boundary.elapsed_ns,
            metadata.output_rate_hz,
        ))
        .ok()
        .map(|sample| sample.min(frames.len()))
    }));
    starts.push(frames.len());
    starts.sort_unstable();
    starts.dedup();
    for range in starts.windows(2) {
        normalize_position_range(&mut frames[range[0]..range[1]]);
    }
}

fn normalize_position_range(frames: &mut [TelemetryFrame]) {
    if frames.len() < 2
        || frames
            .iter()
            .any(|frame| frame.lap.normalized_position.is_some())
    {
        return;
    }
    let mut cumulative = Vec::with_capacity(frames.len());
    cumulative.push(0.0);
    for pair in frames.windows(2) {
        let step = pair[0]
            .motion
            .position_m
            .zip(pair[1].motion.position_m)
            .map(|(left, right)| (right.x - left.x).hypot(right.z - left.z))
            .filter(|step| step.is_finite() && *step <= 100.0)
            .unwrap_or(0.0);
        cumulative.push(cumulative.last().copied().unwrap_or(0.0) + step);
    }
    let Some(total) = cumulative.last().copied().filter(|total| *total >= 100.0) else {
        return;
    };
    for (frame, distance) in frames.iter_mut().zip(cumulative) {
        #[allow(clippy::cast_possible_truncation)]
        let normalized = (distance / total) as f32;
        frame.lap.normalized_position = normalized.is_finite().then_some(normalized);
    }
}

fn build_laps(
    session_id: &str,
    metadata: &LdImportMetadata,
    frames: &[TelemetryFrame],
) -> Result<Vec<NewLap>, String> {
    if metadata.lap_boundaries.is_empty() {
        return Err("MoTeC lap boundaries are missing".to_owned());
    }
    let mut starts = Vec::with_capacity(metadata.lap_boundaries.len() + 2);
    starts.push((0_u64, 0_usize));
    for boundary in &metadata.lap_boundaries {
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
            let partial = index == 0 || index + 2 == starts.len();
            let duration_ns = end_ns.saturating_sub(start_ns);
            (sample_end > sample_start && (!partial || duration_ns >= 1_000_000_000)).then(|| {
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
                    duration_ns: (!partial).then_some(duration_ns),
                    validity: validity.into(),
                    validity_reason: Some(validity_reason.into()),
                    max_tyres_out,
                    sample_start: u64::try_from(sample_start).unwrap_or(u64::MAX),
                    sample_count: u64::try_from(sample_end - sample_start).unwrap_or(u64::MAX),
                    is_personal_best: false,
                    sectors: if partial {
                        Vec::new()
                    } else {
                        derive_sector_times(samples, end_ns.saturating_sub(start_ns))
                    },
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

fn derive_sector_times(samples: &[TelemetryFrame], lap_duration_ns: u64) -> Vec<NewSector> {
    let Some(mut previous) = samples
        .iter()
        .find_map(|frame| frame.lap.last_sector_time_ns)
    else {
        return Vec::new();
    };
    let mut durations = Vec::new();
    for duration in samples
        .iter()
        .skip(1)
        .filter_map(|frame| frame.lap.last_sector_time_ns)
    {
        if previous != duration {
            durations.push(duration);
        }
        previous = duration;
    }
    let completed_duration = durations.iter().copied().sum::<u64>();
    if completed_duration >= lap_duration_ns {
        return stable_sector_times(samples, lap_duration_ns);
    }
    durations.push(lap_duration_ns - completed_duration);
    durations
        .into_iter()
        .enumerate()
        .filter(|(_, duration_ns)| *duration_ns > 0)
        .map(|(index, duration_ns)| NewSector {
            index: u32::try_from(index + 1).unwrap_or(u32::MAX),
            duration_ns,
        })
        .collect()
}

fn stable_sector_times(samples: &[TelemetryFrame], lap_duration_ns: u64) -> Vec<NewSector> {
    const MINIMUM_STABLE_NS: u64 = 2_000_000_000;
    let mut runs = Vec::<(u64, u64, u64)>::new();
    for frame in samples {
        let Some(duration_ns) = frame.lap.last_sector_time_ns else {
            continue;
        };
        if let Some((value, _, end_ns)) = runs.last_mut()
            && *value == duration_ns
        {
            *end_ns = frame.elapsed.0;
        } else {
            runs.push((duration_ns, frame.elapsed.0, frame.elapsed.0));
        }
    }
    let durations = runs
        .into_iter()
        .filter(|(_, start_ns, end_ns)| end_ns.saturating_sub(*start_ns) >= MINIMUM_STABLE_NS)
        .map(|(duration_ns, _, _)| duration_ns)
        .skip(1)
        .collect::<Vec<_>>();
    let completed = durations.iter().copied().sum::<u64>();
    if durations.is_empty() || completed >= lap_duration_ns {
        return Vec::new();
    }
    durations
        .into_iter()
        .chain(std::iter::once(lap_duration_ns - completed))
        .enumerate()
        .map(|(index, duration_ns)| NewSector {
            index: u32::try_from(index + 1).unwrap_or(u32::MAX),
            duration_ns,
        })
        .collect()
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
    use trace_storage::ipc::{read_columns_range, read_lap_metrics};

    #[test]
    fn folder_scan_discovers_logs_recursively_and_reports_its_limit() {
        let directory = tempfile::tempdir().expect("temporary folder");
        let nested = directory.path().join("nested");
        fs::create_dir(&nested).expect("nested folder");
        fs::write(directory.path().join("first.ld"), b"invalid").expect("first log");
        fs::write(nested.join("second.LD"), b"invalid").expect("second log");
        fs::write(nested.join("ignored.txt"), b"invalid").expect("ignored file");

        let (all, limited) = find_motec_logs(directory.path(), 10).expect("folder discovery");
        assert_eq!(all.len(), 2);
        assert!(!limited);
        let (one, limited) = find_motec_logs(directory.path(), 1).expect("bounded discovery");
        assert_eq!(one.len(), 1);
        assert!(limited);
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../crates/trace-motec/tests/fixtures/acti-zandvoort")
            .join(name)
    }

    fn source_metadata() -> LdImportMetadata {
        LdImportMetadata {
            date: "09/10/2026".into(),
            time: "16:12:58".into(),
            driver: String::new(),
            vehicle_id: "LMP2_ELMS".into(),
            venue: String::new(),
            session_type: None,
            event_name: None,
            source_comment: None,
            channels: Vec::new(),
            ldx: None,
            lap_boundaries: Vec::new(),
            output_rate_hz: 1,
            frame_count: 0,
        }
    }

    #[test]
    fn infers_missing_lmu_identity_from_a_timestamped_filename() {
        let source = source_metadata();
        let options = inferred_options(
            Path::new("lmu/2026-10-09 - 16-12-58 - Algarve International Circuit - P1.ld"),
            &source,
        );
        assert_eq!(options.simulator_key, "le-mans-ultimate");
        assert_eq!(options.track, "Algarve International Circuit");
        assert_eq!(options.car, "LMP2_ELMS");
        assert_eq!(options.session_type, "Practice");
    }

    #[test]
    fn distinguishes_incomplete_logs_from_missing_sidecars() {
        let sidecar = Path::new("nordschleife.ldx");
        assert_eq!(
            missing_lap_boundaries_error(sidecar, true),
            "This MoTeC log contains no completed laps. TRACE read the matching nordschleife.ldx, but it contains no lap markers; the outing may have ended before the first lap was completed."
        );
        assert_eq!(
            missing_lap_boundaries_error(sidecar, false),
            "No completed lap markers were found. Add the matching nordschleife.ldx, or export a MoTeC log with a Lap Number channel."
        );
    }

    #[test]
    fn derives_lap_progress_independently_for_each_position_range() {
        let mut source = source_metadata();
        source.lap_boundaries = vec![trace_motec::LapBoundary {
            name: "Lap 2".into(),
            elapsed_ns: 3_000_000_000,
        }];
        let mut frames = [0.0, 50.0, 100.0, 0.0, 50.0, 100.0]
            .into_iter()
            .enumerate()
            .map(|(index, x)| TelemetryFrame {
                sequence: trace_domain::FrameSequence(u64::try_from(index).unwrap()),
                elapsed: trace_domain::ElapsedNanoseconds(
                    u64::try_from(index).unwrap() * 1_000_000_000,
                ),
                motion: trace_domain::MotionState {
                    position_m: Some(trace_domain::Vector3 {
                        x,
                        y: 0.0,
                        z: 0.0,
                        frame: trace_domain::CoordinateFrame::SourceWorld,
                    }),
                    ..trace_domain::MotionState::default()
                },
                ..TelemetryFrame::default()
            })
            .collect::<Vec<_>>();
        normalize_lap_positions(&source, &mut frames);
        assert_eq!(
            frames
                .iter()
                .map(|frame| frame.lap.normalized_position)
                .collect::<Vec<_>>(),
            vec![
                Some(0.0),
                Some(0.5),
                Some(1.0),
                Some(0.0),
                Some(0.5),
                Some(1.0)
            ]
        );
    }

    #[test]
    fn drops_a_subsecond_partial_lap_after_the_final_crossing() {
        let mut source = source_metadata();
        source.output_rate_hz = 2;
        source.lap_boundaries = vec![
            trace_motec::LapBoundary {
                name: "Lap 1".into(),
                elapsed_ns: 10_000_000_000,
            },
            trace_motec::LapBoundary {
                name: "Lap 2".into(),
                elapsed_ns: 20_000_000_000,
            },
        ];
        let frames = (0..42)
            .map(|sequence| TelemetryFrame {
                sequence: trace_domain::FrameSequence(sequence),
                elapsed: trace_domain::ElapsedNanoseconds(sequence * 500_000_000),
                ..TelemetryFrame::default()
            })
            .collect::<Vec<_>>();

        let laps = build_laps("long-track", &source, &frames).expect("laps");

        assert_eq!(laps.len(), 2);
        assert_eq!(laps[0].validity, "invalid");
        assert_eq!(laps[1].duration_ns, Some(10_000_000_000));
    }

    #[test]
    fn sector_import_ignores_short_logger_transients() {
        let values = [
            195_u64, 195, 195, 180, 170, 125, 125, 125, 125, 140, 167, 167, 167, 167,
        ];
        let samples = values
            .into_iter()
            .enumerate()
            .map(|(index, seconds)| TelemetryFrame {
                elapsed: trace_domain::ElapsedNanoseconds(
                    u64::try_from(index).unwrap() * 1_000_000_000,
                ),
                lap: trace_domain::LapObservation {
                    last_sector_time_ns: Some(seconds * 1_000_000_000),
                    ..trace_domain::LapObservation::default()
                },
                ..TelemetryFrame::default()
            })
            .collect::<Vec<_>>();

        assert_eq!(
            derive_sector_times(&samples, 486_000_000_000)
                .into_iter()
                .map(|sector| sector.duration_ns)
                .collect::<Vec<_>>(),
            vec![125_000_000_000, 167_000_000_000, 194_000_000_000]
        );
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
        assert_eq!(session.laps[1].sectors.len(), 3);
        assert_eq!(session.laps[1].sectors[0].duration_ns, 48_038_000_000);
        assert_eq!(session.laps[1].sectors[1].duration_ns, 28_319_000_000);
        assert_eq!(session.laps[1].sectors[2].duration_ns, 35_528_000_000);
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
        assert_eq!(columns.sector_index.first(), Some(&Some(0)));
        assert!(columns.sector_index.contains(&Some(2)));
        let metrics = read_lap_metrics(
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
        .expect("lap metrics");
        assert_eq!(metrics.fuel_capacity_litres, Some(45.0));
        assert!(
            metrics
                .fuel_start_litres
                .is_some_and(|fuel| (2.9..3.0).contains(&fuel))
        );
        assert!(
            metrics
                .fuel_end_litres
                .is_some_and(|fuel| (1.5..1.6).contains(&fuel))
        );
    }

    #[test]
    fn imports_longer_authorised_outing_with_all_sidecar_laps() {
        let directory = tempfile::tempdir().expect("temporary data directory");
        let summary = import_motec_session(
            directory.path(),
            &fixture("stint-7.ld"),
            "motec-long-fixture",
            64 * 1024 * 1024,
        )
        .expect("import longer ACTI fixture");
        assert_eq!(summary.sample_count, 11_394);
        assert_eq!(summary.lap_count, 5);

        let store =
            MetadataStore::open(&directory.path().join("trace.sqlite")).expect("imported metadata");
        let session = store
            .recent_sessions(10)
            .expect("sessions")
            .into_iter()
            .find(|session| session.id == "motec-long-fixture")
            .expect("imported session");
        assert_eq!(session.simulator_key, "assetto-corsa");
        assert_eq!(session.source_track_id.as_deref(), Some("zandvoort2023"));
        assert_eq!(session.source_car_id.as_deref(), Some("ks_mazda_mx5_cup"));
        assert_eq!(session.user_driver.as_deref(), Some("E. Cavalli"));
        assert_eq!(session.laps.len(), 5);
        assert_eq!(
            session.laps.first().map(|lap| lap.validity.as_str()),
            Some("invalid")
        );
        assert_eq!(
            session.laps.last().map(|lap| lap.validity.as_str()),
            Some("invalid")
        );
        assert_eq!(
            session
                .laps
                .iter()
                .filter_map(|lap| lap.duration_ns)
                .collect::<Vec<_>>(),
            vec![111_996_000_000, 115_396_000_000, 117_388_000_000]
        );
        assert!(session.laps[1..4].iter().all(|lap| lap.sectors.len() == 3));
        assert!(session.laps[1].is_personal_best);
    }
}
