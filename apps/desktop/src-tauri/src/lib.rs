use serde::Serialize;

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

#[tauri::command]
fn recent_sessions() -> Vec<()> {
    Vec::new()
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
