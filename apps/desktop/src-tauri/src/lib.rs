use std::{
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
};

use serde::Serialize;
use tauri::Manager;
use trace_ac::AcAdapter;
use trace_adapter::SimulatorAdapter;
use trace_storage::{
    ipc::{export_core_csv, read_lap_metrics},
    metadata::MetadataStore,
};

mod ac_content;
mod capture;

use ac_content::AcContentNames;
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
    simulator_id: String,
    simulator_name: String,
    simulator_short_name: String,
    simulators: Vec<SimulatorOption>,
    connection: String,
    source: String,
    sample_rate_hz: u16,
    session: String,
    channels: Vec<ChannelCapability>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct SimulatorOption {
    id: &'static str,
    name: &'static str,
    short_name: &'static str,
    available: bool,
}

const SIMULATORS: &[SimulatorOption] = &[SimulatorOption {
    id: "assetto-corsa",
    name: "Assetto Corsa",
    short_name: "AC",
    available: true,
}];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordedLapSummary {
    index: u32,
    time: String,
    duration_ns: Option<u64>,
    validity: String,
    validity_reason: Option<String>,
    max_tyres_out: Option<u8>,
    is_fastest: bool,
    sectors: Vec<RecordedSectorSummary>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordedSectorSummary {
    index: u32,
    time: String,
    duration_ns: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordedLapMetrics {
    lap_index: u32,
    fuel_start_litres: Option<f32>,
    fuel_end_litres: Option<f32>,
    fuel_used_litres: Option<f32>,
    fuel_capacity_litres: Option<f32>,
    max_speed_kmh: Option<f32>,
    tyre_wear_start: [Option<f32>; 4],
    tyre_wear_end: [Option<f32>; 4],
    tyre_wear_minimum: [Option<f32>; 4],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordedSessionSummary {
    id: String,
    simulator_id: String,
    simulator_name: String,
    title: Option<String>,
    driver: Option<String>,
    ownership: String,
    tags: Vec<String>,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GameInstallDirectory {
    simulator_id: &'static str,
    simulator_name: &'static str,
    path: Option<String>,
    source: &'static str,
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
    let configured_ac_path = store
        .simulator_install_path("assetto-corsa")
        .map_err(|error| format!("failed to read simulator settings: {error:?}"))?
        .map(PathBuf::from);
    let content_names = AcContentNames::discover(configured_ac_path.as_deref());

    Ok(sessions
        .into_iter()
        .map(|session| {
            let replay = session.source_kind == "simulator_replay";
            let deletable = active_session_id.as_deref() != Some(session.id.as_str());
            let track = session
                .source_track_id
                .as_deref()
                .map(|source_id| content_names.track(source_id, session.layout_id.as_deref()))
                .or(session.track)
                .unwrap_or_else(|| "TRACK NOT REPORTED".into());
            let car = session
                .source_car_id
                .as_deref()
                .map(|source_id| content_names.car(source_id))
                .or(session.car)
                .unwrap_or_else(|| "CAR NOT REPORTED".into());
            RecordedSessionSummary {
                id: session.id,
                simulator_name: simulator_name(&session.simulator_key),
                simulator_id: session.simulator_key,
                title: session.user_title,
                driver: session.user_driver,
                ownership: session.ownership,
                tags: session.tags,
                track,
                car,
                session_type: session
                    .session_type
                    .unwrap_or_else(|| if replay { "REPLAY" } else { "SESSION" }.into())
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
                        duration_ns: lap.duration_ns,
                        validity: lap.validity,
                        validity_reason: lap.validity_reason,
                        max_tyres_out: lap.max_tyres_out,
                        is_fastest: lap.is_personal_best,
                        sectors: lap
                            .sectors
                            .into_iter()
                            .map(|sector| RecordedSectorSummary {
                                index: sector.index,
                                time: format_lap_time(sector.duration_ns),
                                duration_ns: sector.duration_ns,
                            })
                            .collect(),
                    })
                    .collect(),
            }
        })
        .collect())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri injects AppHandle by value.
fn game_install_directories(app: tauri::AppHandle) -> Result<Vec<GameInstallDirectory>, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    Ok(vec![game_install_directory(&store)?])
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri deserializes command arguments by value.
fn set_game_install_directory(
    app: tauri::AppHandle,
    simulator_id: String,
    custom_path: Option<String>,
) -> Result<GameInstallDirectory, String> {
    if simulator_id != "assetto-corsa" {
        return Err("that simulator does not have configurable install metadata yet".into());
    }
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let mut store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let normalized = custom_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(path) = normalized {
        let path = Path::new(path);
        if !path.join("content/cars").is_dir() || !path.join("content/tracks").is_dir() {
            return Err(
                "choose the Assetto Corsa folder that contains content/cars and content/tracks"
                    .into(),
            );
        }
    }
    store
        .set_simulator_install_path(&simulator_id, normalized)
        .map_err(|error| format!("failed to save simulator settings: {error:?}"))?;
    game_install_directory(&store)
}

fn game_install_directory(store: &MetadataStore) -> Result<GameInstallDirectory, String> {
    let configured = store
        .simulator_install_path("assetto-corsa")
        .map_err(|error| format!("failed to read simulator settings: {error:?}"))?;
    let configured_path = configured.as_deref().map(Path::new);
    let detected = AcContentNames::discover(configured_path);
    Ok(GameInstallDirectory {
        simulator_id: "assetto-corsa",
        simulator_name: "Assetto Corsa",
        path: detected
            .root()
            .map(|path| path.to_string_lossy().into_owned()),
        source: if configured.is_some() {
            "manual"
        } else if detected.root().is_some() {
            "detected"
        } else {
            "missing"
        },
    })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri injects AppHandle and deserializes the id.
fn session_lap_metrics(
    app: tauri::AppHandle,
    session_id: String,
) -> Result<Vec<RecordedLapMetrics>, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let session = store
        .recent_sessions(1_000)
        .map_err(|error| format!("failed to query TRACE sessions: {error:?}"))?
        .into_iter()
        .find(|session| session.id == session_id)
        .ok_or_else(|| "recorded session was not found".to_owned())?;
    let mut result = Vec::with_capacity(session.laps.len());

    for lap in session.laps {
        let Ok(locator) = store.lap_telemetry(&lap.id) else {
            continue;
        };
        let path = directory.join("telemetry").join(locator.blob_path.as_str());
        let file =
            File::open(path).map_err(|error| format!("failed to open lap telemetry: {error}"))?;
        let metrics = read_lap_metrics(file, locator.sample_start, locator.sample_count)
            .map_err(|error| format!("failed to derive lap metrics: {error:?}"))?;
        let fuel_used_litres = metrics
            .fuel_start_litres
            .zip(metrics.fuel_end_litres)
            .and_then(|(start, end)| (start >= end).then_some(start - end));
        result.push(RecordedLapMetrics {
            lap_index: lap.index,
            fuel_start_litres: metrics.fuel_start_litres,
            fuel_end_litres: metrics.fuel_end_litres,
            fuel_used_litres,
            fuel_capacity_litres: metrics.fuel_capacity_litres,
            max_speed_kmh: metrics.max_speed_mps.map(|value| value * 3.6),
            tyre_wear_start: metrics.tyre_wear_start,
            tyre_wear_end: metrics.tyre_wear_end,
            tyre_wear_minimum: metrics.tyre_wear_minimum,
        });
    }
    Ok(result)
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

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri deserializes command arguments by value.
fn update_session_details(
    app: tauri::AppHandle,
    session_id: String,
    title: Option<String>,
    driver: Option<String>,
    ownership: String,
    tags: Vec<String>,
) -> Result<(), String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let mut store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let normalized_title = title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let normalized_driver = driver
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let normalized_tags = tags
        .into_iter()
        .map(|tag| tag.trim().to_owned())
        .filter(|tag| !tag.is_empty())
        .collect::<Vec<_>>();
    store
        .update_session_details(
            &session_id,
            normalized_title,
            normalized_driver,
            &ownership,
            &normalized_tags,
        )
        .map_err(|error| format!("failed to update session details: {error:?}"))
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
    let simulator = simulator_option(&snapshot.simulator_id);
    let channels = simulator_channel_capabilities(&snapshot.simulator_id);
    FoundationStatus {
        simulator_id: snapshot.simulator_id,
        simulator_name: simulator
            .map_or_else(|| snapshot.source.clone(), |value| value.name.into()),
        simulator_short_name: simulator
            .map_or_else(|| "SIM".into(), |value| value.short_name.into()),
        simulators: SIMULATORS.to_vec(),
        connection: snapshot.connection,
        source: snapshot.source,
        sample_rate_hz: snapshot.sample_rate_hz,
        session: snapshot.session,
        channels,
    }
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri deserializes command arguments by value.
fn select_simulator(
    status: tauri::State<'_, SharedCaptureStatus>,
    simulator_id: String,
) -> Result<(), String> {
    let current = status
        .lock()
        .map_err(|_| "capture status is unavailable".to_owned())?;
    if current.simulator_id == simulator_id {
        return Ok(());
    }
    if simulator_option(&simulator_id).is_none() {
        return Err("That simulator adapter is not installed.".into());
    }
    Err("Close the active adapter before selecting another simulator.".into())
}

fn simulator_option(id: &str) -> Option<SimulatorOption> {
    SIMULATORS
        .iter()
        .copied()
        .find(|simulator| simulator.id == id)
}

fn simulator_name(id: &str) -> String {
    simulator_option(id).map_or_else(
        || id.split('-').map(capitalize).collect::<Vec<_>>().join(" "),
        |simulator| simulator.name.into(),
    )
}

fn capitalize(value: &str) -> String {
    let mut characters = value.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(characters).collect()
    })
}

type ChannelCapabilityDefinition = (&'static str, &'static str, &'static str, &'static str, bool);

const AC_CHANNEL_CAPABILITY_DEFINITIONS: &[ChannelCapabilityDefinition] = &[
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
        "native.inputs",
        "Clutch & steering source values",
        "AC-NATIVE · INPUTS",
        "Exact AC clutch and steering fields; steering interpretation remains source-native",
        true,
    ),
    (
        "native.tyres.dynamics",
        "Slip, load, pressure & angular speed",
        "AC-NATIVE · TYRES & WHEELS",
        "All four corners in AC's native units and ordering",
        true,
    ),
    (
        "native.tyres.condition",
        "Wear, dirt, camber & core temperature",
        "AC-NATIVE · TYRES & WHEELS",
        "All four corners, including documented camber radians and core temperature",
        true,
    ),
    (
        "native.tyres.temperatures",
        "Inner, middle & outer temperatures",
        "AC-NATIVE · TYRES & WHEELS",
        "Three carcass temperature bands and brake temperature at every corner",
        true,
    ),
    (
        "native.tyres.contact",
        "Contact points, normals & headings",
        "AC-NATIVE · TYRES & WHEELS",
        "Twelve values per contact-vector family",
        true,
    ),
    (
        "native.powertrain.electronics",
        "TC, ABS, DRS, KERS & ERS",
        "AC-NATIVE · POWERTRAIN",
        "States, charge, energy use, recovery, and power settings",
        true,
    ),
    (
        "native.powertrain.engine",
        "Turbo, engine brake & air density",
        "AC-NATIVE · POWERTRAIN",
        "Dynamic source values plus static limits and controller counts",
        true,
    ),
    (
        "native.chassis.orientation",
        "Heading, pitch, roll & angular velocity",
        "AC-NATIVE · CHASSIS",
        "World orientation and vehicle-local angular motion",
        true,
    ),
    (
        "native.chassis.state",
        "Ride height, damage, ballast & brake bias",
        "AC-NATIVE · CHASSIS",
        "Front/rear ride height, five damage channels, ballast, CG height, and bias",
        true,
    ),
    (
        "native.chassis.controls",
        "Pit limiter, tyres out, auto shift & FFB",
        "AC-NATIVE · CHASSIS",
        "Driver-assistance and control state including the final force-feedback value",
        true,
    ),
    (
        "native.session.timing",
        "Last/best laps, splits & session time",
        "AC-NATIVE · SESSION",
        "Formatted and integer timing, configured laps, position, and distance travelled",
        true,
    ),
    (
        "native.session.race_control",
        "Flags, pits, penalties & mandatory stop",
        "AC-NATIVE · SESSION",
        "Race-control, ideal-line, pit-lane, and mandatory-stop state",
        true,
    ),
    (
        "native.session.conditions",
        "Grip, wind & replay speed",
        "AC-NATIVE · SESSION",
        "Surface grip, wind speed/direction, replay multiplier, and tyre compound",
        true,
    ),
    (
        "native.static.identities",
        "Car, track, layout & skin IDs",
        "AC-NATIVE · CAR & TRACK",
        "Complete static identity fields; exported Arrow also contains AC player-name fields",
        true,
    ),
    (
        "native.static.limits",
        "Car limits & track length",
        "AC-NATIVE · CAR & TRACK",
        "RPM, fuel, torque, power, boost, suspension, tyre radius, and spline length",
        true,
    ),
    (
        "native.static.configuration",
        "Assists, rates & pit window",
        "AC-NATIVE · CAR & TRACK",
        "Penalty, fuel, tyre, damage, blanket, clutch, blip, grid, and timed-race settings",
        true,
    ),
];

fn ac_channel_capabilities() -> Vec<ChannelCapability> {
    AC_CHANNEL_CAPABILITY_DEFINITIONS
        .iter()
        .copied()
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

fn simulator_channel_capabilities(simulator_id: &str) -> Vec<ChannelCapability> {
    match simulator_id {
        "assetto-corsa" => ac_channel_capabilities(),
        _ => Vec::new(),
    }
}

/// Starts the TRACE desktop application.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or run the desktop event loop.
pub fn run() {
    let adapter_identity = AcAdapter::new().identity().clone();
    let capture_status = SharedCaptureStatus::new(std::sync::Mutex::new(
        CaptureStatus::for_adapter(&adapter_identity),
    ));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(capture_status)
        .setup(move |app| {
            let directory = app.path().app_data_dir()?;
            let status = app.state::<SharedCaptureStatus>().inner().clone();
            capture::spawn(directory, status, &adapter_identity, AcAdapter::new);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            foundation_status,
            select_simulator,
            recent_sessions,
            session_lap_metrics,
            game_install_directories,
            set_game_install_directory,
            export_session,
            delete_session,
            update_session_details
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

    #[test]
    fn unknown_simulator_keys_have_a_readable_fallback_name() {
        assert_eq!(simulator_name("example-racing-sim"), "Example Racing Sim");
    }
}
