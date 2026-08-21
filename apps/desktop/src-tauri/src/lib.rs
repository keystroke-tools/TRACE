use serde::Serialize;
use tauri::Manager;
use trace_storage::metadata::MetadataStore;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChannelCapability {
    id: &'static str,
    label: &'static str,
    available: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FoundationStatus {
    connection: &'static str,
    source: &'static str,
    sample_rate_hz: u16,
    session: &'static str,
    channels: Vec<ChannelCapability>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordedLapSummary {
    index: u32,
    time: String,
    valid: bool,
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
    laps: Vec<RecordedLapSummary>,
}

#[tauri::command]
fn recent_sessions(app: tauri::AppHandle) -> Result<Vec<RecordedSessionSummary>, String> {
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

    Ok(sessions
        .into_iter()
        .map(|session| RecordedSessionSummary {
            id: session.id,
            track: session.track.unwrap_or_else(|| "UNKNOWN TRACK".into()),
            car: session.car.unwrap_or_else(|| "UNKNOWN CAR".into()),
            session_type: session
                .session_type
                .unwrap_or_else(|| "UNKNOWN SESSION".into())
                .to_uppercase(),
            started_at: session.started_at,
            source: session.source_kind.replace('_', " ").to_uppercase(),
            laps: session
                .laps
                .into_iter()
                .map(|lap| RecordedLapSummary {
                    index: lap.index,
                    time: lap.duration_ns.map_or_else(|| "—".into(), format_lap_time),
                    valid: lap.validity == "valid",
                })
                .collect(),
        })
        .collect())
}

fn format_lap_time(duration_ns: u64) -> String {
    let total_ms = duration_ns / 1_000_000;
    let minutes = total_ms / 60_000;
    let seconds = (total_ms / 1_000) % 60;
    let milliseconds = total_ms % 1_000;
    format!("{minutes}:{seconds:02}.{milliseconds:03}")
}

#[tauri::command]
fn foundation_status() -> FoundationStatus {
    FoundationStatus {
        connection: "replay",
        source: "TRACE REPLAY",
        sample_rate_hz: 100,
        session: "MUGELLO / TATUUS FA01",
        channels: vec![
            ChannelCapability {
                id: "vehicle.speed",
                label: "SPEED",
                available: true,
            },
            ChannelCapability {
                id: "inputs.throttle",
                label: "THROTTLE",
                available: true,
            },
            ChannelCapability {
                id: "inputs.brake",
                label: "BRAKE",
                available: true,
            },
            ChannelCapability {
                id: "inputs.steering",
                label: "STEERING",
                available: false,
            },
            ChannelCapability {
                id: "tyres.brake_temperature",
                label: "BRAKE TEMP",
                available: false,
            },
        ],
    }
}

/// Starts the TRACE desktop application.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or run the desktop event loop.
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![foundation_status, recent_sessions])
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
}
