use std::{
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
};

use serde::Serialize;
use tauri::Manager;
use trace_storage::{ipc::export_core_csv, metadata::MetadataStore};

mod capture;

use capture::{CaptureStatus, SharedCaptureStatus};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChannelCapability {
    id: &'static str,
    label: &'static str,
    category: &'static str,
    detail: &'static str,
    available: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FoundationStatus {
    connection: String,
    source: String,
    sample_rate_hz: u16,
    session: String,
    channels: Vec<ChannelCapability>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordedLapSummary {
    index: u32,
    time: String,
    validity: String,
    validity_reason: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordedSessionSummary {
    id: String,
    track: String,
    car: String,
    session_type: String,
    started_at: String,
    source: String,
    exportable: bool,
    deletable: bool,
    laps: Vec<RecordedLapSummary>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionExport {
    path: String,
    format: String,
    sample_count: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionDeletion {
    session_id: String,
    cleanup_warning: Option<String>,
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri injects AppHandle as an owned command argument.
fn recent_sessions(
    app: tauri::AppHandle,
    status: tauri::State<'_, SharedCaptureStatus>,
) -> Result<Vec<RecordedSessionSummary>, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let sessions = store
        .recent_sessions(100)
        .map_err(|error| format!("failed to query TRACE sessions: {error:?}"))?;
    let active_session_id = status
        .lock()
        .ok()
        .and_then(|value| value.active_session_id.clone());

    Ok(sessions
        .into_iter()
        .map(|session| {
            let replay = session.source_kind == "simulator_replay";
            let deletable = active_session_id.as_deref() != Some(session.id.as_str());
            RecordedSessionSummary {
                id: session.id,
                track: session.track.unwrap_or_else(|| "TRACK NOT REPORTED".into()),
                car: session.car.unwrap_or_else(|| "CAR NOT REPORTED".into()),
                session_type: session
                    .session_type
                    .unwrap_or_else(|| if replay { "REPLAY" } else { "AC SESSION" }.into())
                    .to_uppercase(),
                started_at: session.started_at,
                source: session.source_kind.replace('_', " ").to_uppercase(),
                exportable: session.exportable,
                deletable,
                laps: session
                    .laps
                    .into_iter()
                    .map(|lap| RecordedLapSummary {
                        index: lap.index,
                        time: lap.duration_ns.map_or_else(|| "—".into(), format_lap_time),
                        validity: lap.validity,
                        validity_reason: lap.validity_reason,
                    })
                    .collect(),
            }
        })
        .collect())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri deserializes command arguments by value.
fn export_session(
    app: tauri::AppHandle,
    session_id: String,
    export_format: String,
) -> Result<SessionExport, String> {
    let data_directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let store = MetadataStore::open(&data_directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let locator = store
        .session_telemetry(&session_id)
        .map_err(|error| format!("session is not ready for export or does not exist: {error:?}"))?;
    let source_path = data_directory
        .join("telemetry")
        .join(locator.blob_path.as_str());
    let source = File::open(&source_path)
        .map_err(|error| format!("failed to open session telemetry: {error}"))?;
    let downloads = app
        .path()
        .download_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&downloads)
        .map_err(|error| format!("failed to create exports directory: {error}"))?;

    let (label, extension) = match export_format.as_str() {
        "arrow" => ("Arrow IPC", "arrow"),
        "csv" => ("CSV", "csv"),
        _ => return Err("unsupported export format".into()),
    };
    let stem = format!("trace-{}", safe_export_stem(&session_id));
    let (destination_path, mut destination) = create_export_file(&downloads, &stem, extension)?;
    let export_result = if export_format == "arrow" {
        let mut source = source;
        io::copy(&mut source, &mut destination)
            .map(|_| locator.sample_count)
            .map_err(|error| format!("failed to export Arrow telemetry: {error}"))
    } else {
        export_core_csv(source, &mut destination)
            .map_err(|error| format!("failed to export CSV telemetry: {error:?}"))
    };
    let sample_count = match export_result.and_then(|sample_count| {
        destination
            .sync_all()
            .map_err(|error| format!("failed to flush exported telemetry: {error}"))?;
        Ok(sample_count)
    }) {
        Ok(sample_count) => sample_count,
        Err(error) => {
            drop(destination);
            let _ = fs::remove_file(&destination_path);
            return Err(error);
        }
    };

    Ok(SessionExport {
        path: destination_path.to_string_lossy().into_owned(),
        format: label.into(),
        sample_count,
    })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri deserializes command arguments by value.
fn delete_session(
    app: tauri::AppHandle,
    status: tauri::State<'_, SharedCaptureStatus>,
    session_id: String,
) -> Result<SessionDeletion, String> {
    if status
        .lock()
        .ok()
        .and_then(|value| value.active_session_id.clone())
        .is_some_and(|active| active == session_id)
    {
        return Err("This session is still recording and cannot be deleted yet.".into());
    }
    let data_directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let mut store = MetadataStore::open(&data_directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let blob_path = store
        .delete_session(&session_id)
        .map_err(|error| match error {
            trace_storage::metadata::MetadataError::RecordNotFound => {
                "This session no longer exists.".into()
            }
            other => format!("failed to delete session metadata: {other:?}"),
        })?;

    let cleanup_warning = blob_path.and_then(|path| {
        let file = data_directory.join("telemetry").join(path.as_str());
        match fs::remove_file(&file) {
            Ok(()) => None,
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => Some(format!(
                "The session was removed, but its telemetry file could not be cleaned up: {error}"
            )),
        }
    });

    Ok(SessionDeletion {
        session_id,
        cleanup_warning,
    })
}

fn safe_export_stem(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "session".into()
    } else {
        sanitized
    }
}

fn create_export_file(
    directory: &Path,
    stem: &str,
    extension: &str,
) -> Result<(PathBuf, File), String> {
    for suffix in 0..1_000_u16 {
        let filename = if suffix == 0 {
            format!("{stem}.{extension}")
        } else {
            format!("{stem}-{suffix}.{extension}")
        };
        let path = directory.join(filename);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("failed to create export file: {error}")),
        }
    }
    Err("too many exports already use this session name".into())
}

fn format_lap_time(duration_ns: u64) -> String {
    let total_ms = duration_ns / 1_000_000;
    let minutes = total_ms / 60_000;
    let seconds = (total_ms / 1_000) % 60;
    let milliseconds = total_ms % 1_000;
    format!("{minutes}:{seconds:02}.{milliseconds:03}")
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri injects State as a command argument.
fn foundation_status(status: tauri::State<'_, SharedCaptureStatus>) -> FoundationStatus {
    let snapshot = status
        .lock()
        .map_or_else(|_| CaptureStatus::default(), |value| value.clone());
    FoundationStatus {
        connection: snapshot.connection,
        source: snapshot.source,
        sample_rate_hz: snapshot.sample_rate_hz,
        session: snapshot.session,
        channels: ac_channel_capabilities(),
    }
}

type ChannelCapabilityDefinition = (&'static str, &'static str, &'static str, &'static str, bool);

const AC_CHANNEL_CAPABILITY_DEFINITIONS: [ChannelCapabilityDefinition; 17] = [
    (
        "inputs.throttle",
        "Throttle",
        "DRIVER INPUTS",
        "Pedal position",
        true,
    ),
    (
        "inputs.brake",
        "Brake",
        "DRIVER INPUTS",
        "Pedal position",
        true,
    ),
    (
        "vehicle.speed",
        "Speed",
        "VEHICLE",
        "Metres per second",
        true,
    ),
    (
        "vehicle.engine_rpm",
        "Engine RPM",
        "VEHICLE",
        "Revolutions per minute",
        true,
    ),
    (
        "vehicle.gear",
        "Gear",
        "VEHICLE",
        "Reverse, neutral, or forward gear",
        true,
    ),
    ("vehicle.fuel", "Fuel", "VEHICLE", "Litres remaining", true),
    (
        "lap.position",
        "Lap position",
        "LAP PROGRESS",
        "Normalized track position",
        true,
    ),
    (
        "lap.current_time",
        "Current lap time",
        "LAP PROGRESS",
        "Simulator timer",
        true,
    ),
    (
        "environment.air_temperature",
        "Air temperature",
        "CONDITIONS",
        "Degrees Celsius",
        true,
    ),
    (
        "environment.track_temperature",
        "Track temperature",
        "CONDITIONS",
        "Degrees Celsius",
        true,
    ),
    (
        "motion.position",
        "World position",
        "MOTION",
        "Three-axis source-world coordinates",
        true,
    ),
    (
        "motion.velocity",
        "Velocity",
        "MOTION",
        "Three-axis metres per second",
        true,
    ),
    (
        "motion.acceleration",
        "Acceleration",
        "MOTION",
        "Three-axis metres per second squared",
        true,
    ),
    (
        "wheels.tyre_core_temperature",
        "Tyre core temperature",
        "WHEELS",
        "Degrees Celsius at all four corners",
        true,
    ),
    (
        "wheels.suspension_travel",
        "Suspension travel",
        "WHEELS",
        "Metres at all four corners",
        true,
    ),
    (
        "inputs.steering",
        "Steering angle",
        "NEEDS VALIDATION",
        "AC does not document a reliable unit or sign",
        false,
    ),
    (
        "tyres.extended",
        "Extended tyre data",
        "NEEDS VALIDATION",
        "Pressure, slip, load, and wear need fixture validation",
        false,
    ),
];

fn ac_channel_capabilities() -> Vec<ChannelCapability> {
    AC_CHANNEL_CAPABILITY_DEFINITIONS
        .into_iter()
        .map(
            |(id, label, category, detail, available)| ChannelCapability {
                id,
                label,
                category,
                detail,
                available,
            },
        )
        .collect()
}

/// Starts the TRACE desktop application.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or run the desktop event loop.
pub fn run() {
    let capture_status = SharedCaptureStatus::default();
    tauri::Builder::default()
        .manage(capture_status)
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            let status = app.state::<SharedCaptureStatus>().inner().clone();
            capture::spawn(directory, status);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            foundation_status,
            recent_sessions,
            export_session,
            delete_session
        ])
        .run(tauri::generate_context!())
        .expect("TRACE desktop runtime failed");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lap_times_are_formatted_with_tabular_precision() {
        assert_eq!(format_lap_time(110_906_999_999), "1:50.906");
        assert_eq!(format_lap_time(59_042_000_000), "0:59.042");
    }

    #[test]
    fn export_stems_cannot_escape_the_download_directory() {
        assert_eq!(safe_export_stem("session-123"), "session-123");
        assert_eq!(safe_export_stem("../session\\bad"), "---session-bad");
    }
}
