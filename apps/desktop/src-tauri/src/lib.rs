use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::Manager;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use trace_ac::AcAdapter;
use trace_adapter::SimulatorAdapter;
use trace_core::{
    analysis::{AnalysisResult, ComparisonContext},
    corners::{CornerComparison, CornerComparisonSample, analyze_corner_comparison},
    delta::{ElapsedTimeSeries, calculate_delta},
    distance::{DistanceSample, DistanceSeries, InterpolationMethod, uniform_grid},
    driving::{DrivingAnalysis, analyze_driving_comparison},
};
use trace_storage::{
    BlobCommit, BlobFormat, FileBlobStore, RelativeBlobPath, TelemetryBlobStore,
    ipc::{
        TELEMETRY_SCHEMA_VERSION, TelemetryColumns, export_core_csv, read_columns_range,
        read_lap_metrics, sample_count,
    },
    metadata::{CompatibleSetup, MetadataStore, NewSetupImport, SavedComparison, SessionSummary},
    package::{
        MAX_SETUP_PAYLOAD_BYTES, PACKAGE_VERSION, SessionPackageLap, SessionPackageManifest,
        SessionPackageSetup, decode_setup_payload, encode_setup_payload, imported_records,
        read_package, write_compact_package,
    },
};

mod ac_content;
mod capture;
mod discord_activity;
mod live_broadcast;
mod obs_overlay;
mod setup_analysis;
mod setup_editor;
mod setup_import;
mod startup;

use ac_content::{AcContentNames, AcTrackGeometry};
use capture::{CaptureStatus, SharedCaptureStatus};
use discord_activity::SharedDiscordActivity;
use live_broadcast::{
    LiveAutomationSettings, SharedLiveBroadcast, live_broadcast_status,
    load_live_automation_settings, save_live_automation_settings, start_active_live_broadcast,
    start_recorded_live_broadcast, stop_live_broadcast,
};
use setup_analysis::compare_setups;
use setup_editor::{save_setup_copy, setup_document};
use setup_import::{
    attach_session_setup, detect_setup_folder, import_setup_archives, import_setup_files,
    index_existing_setups, setup_importers, setup_library,
};
use startup::{set_launch_on_startup, startup_settings};

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
struct DriverProfile {
    name: Option<String>,
}

const DEFAULT_LIVE_SERVICE_ENDPOINT: &str = "https://live.simtrace.run";
const LEGACY_SERVICE_ENDPOINT: &str = "https://simtrace.run";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LiveSettings {
    endpoint: String,
    auto_stream: LiveAutomationSettings,
    discord_activity_enabled: bool,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LivePedalTelemetry {
    connection: String,
    simulator_name: String,
    session: String,
    sequence: u64,
    throttle_percent: Option<f32>,
    brake_percent: Option<f32>,
    clutch_percent: Option<f32>,
    steering_degrees: Option<f32>,
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
struct LapComparison {
    reference_session_id: String,
    reference_session_title: Option<String>,
    reference_track: String,
    reference_car: String,
    comparison_session_id: String,
    comparison_session_title: Option<String>,
    comparison_track: String,
    comparison_car: String,
    track_map: Option<AcTrackGeometry>,
    reference_lap_index: u32,
    reference_lap_time: String,
    comparison_lap_index: u32,
    comparison_lap_time: String,
    lap_length_m: f64,
    corner_analysis: AnalysisResult<CornerComparison>,
    driving_analysis: AnalysisResult<DrivingAnalysis>,
    samples: Vec<LapComparisonSample>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LapComparisonSample {
    distance_m: f64,
    delta_seconds: Option<f64>,
    reference_elapsed_seconds: Option<f64>,
    comparison_elapsed_seconds: Option<f64>,
    reference_speed_kmh: Option<f64>,
    comparison_speed_kmh: Option<f64>,
    reference_throttle_percent: Option<f64>,
    comparison_throttle_percent: Option<f64>,
    reference_brake_percent: Option<f64>,
    comparison_brake_percent: Option<f64>,
    reference_steering_percent: Option<f64>,
    comparison_steering_percent: Option<f64>,
    reference_rpm: Option<f64>,
    comparison_rpm: Option<f64>,
    sector_index: Option<u32>,
    reference_gear: Option<i16>,
    comparison_gear: Option<i16>,
    reference_position_x_m: Option<f64>,
    reference_position_z_m: Option<f64>,
    comparison_position_x_m: Option<f64>,
    comparison_position_z_m: Option<f64>,
    reference_air_temperature_c: Option<f64>,
    reference_track_temperature_c: Option<f64>,
    comparison_air_temperature_c: Option<f64>,
    comparison_track_temperature_c: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LapTrace {
    session_id: String,
    lap_index: u32,
    lap_time: String,
    track: String,
    car: String,
    lap_length_m: f64,
    track_map: Option<AcTrackGeometry>,
    samples: Vec<LapTraceSample>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LapTraceSample {
    distance_m: f64,
    elapsed_seconds: Option<f64>,
    sector_index: Option<u32>,
    speed_kmh: Option<f64>,
    throttle_percent: Option<f64>,
    brake_percent: Option<f64>,
    clutch_percent: Option<f64>,
    steering_percent: Option<f64>,
    steering_degrees: Option<f64>,
    rpm: Option<f64>,
    gear: Option<i16>,
    position_x_m: Option<f64>,
    position_z_m: Option<f64>,
    air_temperature_c: Option<f64>,
    track_temperature_c: Option<f64>,
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
    ambient_temperature_c: Option<String>,
    road_temperature_c: Option<String>,
    weather_name: Option<String>,
    track_grip_percent: Option<u8>,
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
struct SessionImport {
    session_id: String,
    lap_count: usize,
    sample_count: u64,
    setup_name: Option<String>,
}

const MAX_SESSION_BYTES: u64 = 2 * 1024 * 1024 * 1024;

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
    let profile_name = store
        .driver_profile_name()
        .map_err(|error| format!("failed to read driver profile: {error:?}"))?;
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
        .map(|mut session| {
            if session.user_driver.is_none()
                && session.source_kind != "imported"
                && session.ownership != "other"
            {
                session.user_driver.clone_from(&profile_name);
                if profile_name.is_some() {
                    session.ownership = "mine".into();
                }
            }
            let replay = session.source_kind == "simulator_replay";
            let deletable = active_session_id.as_deref() != Some(session.id.as_str());
            let (track, car) = resolved_session_content_names(&session, Some(&content_names));
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
                ambient_temperature_c: session.conditions.ambient_temperature_c,
                road_temperature_c: session.conditions.road_temperature_c,
                weather_name: session.conditions.weather_name,
                track_grip_percent: session.conditions.track_grip_percent,
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
#[allow(clippy::needless_pass_by_value)]
fn compatible_setups(
    app: tauri::AppHandle,
    session_id: String,
) -> Result<Vec<CompatibleSetup>, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    store
        .compatible_setups(&session_id, 12)
        .map_err(|error| format!("failed to find compatible setups: {error:?}"))
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn confirm_session_setup(
    app: tauri::AppHandle,
    session_id: String,
    setup_id: String,
) -> Result<Vec<CompatibleSetup>, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let mut store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let confirmed_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| error.to_string())?;
    store
        .confirm_session_setup(&session_id, &setup_id, &confirmed_at)
        .map_err(|error| format!("failed to confirm session setup: {error:?}"))?;
    store
        .compatible_setups(&session_id, 12)
        .map_err(|error| format!("failed to refresh compatible setups: {error:?}"))
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn clear_session_setup(
    app: tauri::AppHandle,
    session_id: String,
) -> Result<Vec<CompatibleSetup>, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let mut store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    store
        .clear_session_setup(&session_id)
        .map_err(|error| format!("failed to clear session setup: {error:?}"))?;
    store
        .compatible_setups(&session_id, 12)
        .map_err(|error| format!("failed to refresh compatible setups: {error:?}"))
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
#[allow(clippy::needless_pass_by_value)]
fn driver_profile(app: tauri::AppHandle) -> Result<DriverProfile, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    Ok(DriverProfile {
        name: store
            .driver_profile_name()
            .map_err(|error| format!("failed to read driver profile: {error:?}"))?,
    })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn set_driver_profile(
    app: tauri::AppHandle,
    name: Option<String>,
) -> Result<DriverProfile, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let mut store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let normalized = name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    store
        .set_driver_profile_name(normalized)
        .map_err(|error| format!("failed to save driver profile: {error:?}"))?;
    Ok(DriverProfile {
        name: normalized.map(str::to_owned),
    })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn live_settings(app: tauri::AppHandle) -> Result<LiveSettings, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let configured_live_endpoint = store
        .live_service_endpoint()
        .map_err(|error| format!("failed to read Go Live settings: {error:?}"))?;
    let discord_activity_enabled = store
        .discord_activity_enabled()
        .map_err(|error| format!("failed to read Discord activity setting: {error:?}"))?;
    Ok(LiveSettings {
        endpoint: configured_live_endpoint
            .filter(|endpoint| endpoint != LEGACY_SERVICE_ENDPOINT)
            .unwrap_or_else(|| DEFAULT_LIVE_SERVICE_ENDPOINT.to_owned()),
        auto_stream: load_live_automation_settings(&directory)?,
        discord_activity_enabled,
    })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn set_live_settings(
    app: tauri::AppHandle,
    endpoint: String,
    auto_stream: LiveAutomationSettings,
    discord_activity_enabled: bool,
) -> Result<LiveSettings, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let mut store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let endpoint = endpoint.trim();
    store
        .set_live_service_endpoint(endpoint)
        .map_err(|error| format!("failed to save Go Live endpoint: {error:?}"))?;
    let auto_stream = save_live_automation_settings(&mut store, auto_stream)?;
    store
        .set_discord_activity_enabled(discord_activity_enabled)
        .map_err(|error| format!("failed to save Discord activity setting: {error:?}"))?;
    app.state::<SharedDiscordActivity>()
        .set_enabled(discord_activity_enabled);
    Ok(LiveSettings {
        endpoint: endpoint.to_owned(),
        auto_stream,
        discord_activity_enabled,
    })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn saved_comparisons(app: tauri::AppHandle) -> Result<Vec<SavedComparison>, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let comparisons = store
        .saved_comparisons()
        .map_err(|error| format!("failed to read saved comparisons: {error:?}"))?;
    resolve_saved_comparison_names(&store, comparisons)
}

#[tauri::command]
#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
fn save_comparison(
    app: tauri::AppHandle,
    name: String,
    reference_session_id: String,
    reference_lap_index: u32,
    analysed_session_id: String,
    analysed_lap_index: u32,
) -> Result<Vec<SavedComparison>, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let mut store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let created_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| error.to_string())?;
    store
        .save_comparison(
            &format!("comparison-{nonce}"),
            &name,
            &reference_session_id,
            reference_lap_index,
            &analysed_session_id,
            analysed_lap_index,
            &created_at,
        )
        .map_err(|error| format!("failed to save comparison: {error:?}"))?;
    let comparisons = store
        .saved_comparisons()
        .map_err(|error| format!("failed to read saved comparisons: {error:?}"))?;
    resolve_saved_comparison_names(&store, comparisons)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn delete_saved_comparison(
    app: tauri::AppHandle,
    comparison_id: String,
) -> Result<Vec<SavedComparison>, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let mut store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    store
        .delete_saved_comparison(&comparison_id)
        .map_err(|error| format!("failed to delete saved comparison: {error:?}"))?;
    let comparisons = store
        .saved_comparisons()
        .map_err(|error| format!("failed to read saved comparisons: {error:?}"))?;
    resolve_saved_comparison_names(&store, comparisons)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn rename_saved_comparison(
    app: tauri::AppHandle,
    comparison_id: String,
    name: String,
) -> Result<Vec<SavedComparison>, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let mut store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    store
        .rename_saved_comparison(&comparison_id, &name)
        .map_err(|error| format!("failed to rename saved comparison: {error:?}"))?;
    let comparisons = store
        .saved_comparisons()
        .map_err(|error| format!("failed to read saved comparisons: {error:?}"))?;
    resolve_saved_comparison_names(&store, comparisons)
}

fn resolve_saved_comparison_names(
    store: &MetadataStore,
    mut comparisons: Vec<SavedComparison>,
) -> Result<Vec<SavedComparison>, String> {
    let names = ac_content_names(store)?;
    for comparison in &mut comparisons {
        if comparison.simulator_key == "assetto-corsa" {
            comparison.track = names.track_label(&comparison.track, None, Some(&comparison.track));
            comparison.car = names.car_label(&comparison.car, Some(&comparison.car));
        }
    }
    Ok(comparisons)
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
#[allow(
    clippy::needless_pass_by_value, // Tauri deserializes command arguments by value.
    clippy::too_many_lines // The command is linear channel-loading orchestration.
)]
fn compare_session_laps(
    app: tauri::AppHandle,
    reference_session_id: String,
    reference_lap_index: u32,
    comparison_session_id: String,
    comparison_lap_index: u32,
) -> Result<LapComparison, String> {
    if reference_session_id == comparison_session_id && reference_lap_index == comparison_lap_index
    {
        return Err("choose two different laps to compare".into());
    }
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let sessions = store
        .recent_sessions(1_000)
        .map_err(|error| format!("failed to query TRACE sessions: {error:?}"))?;
    let reference_session = sessions
        .iter()
        .find(|session| session.id == reference_session_id)
        .cloned()
        .ok_or_else(|| "reference session was not found".to_owned())?;
    let comparison_session = sessions
        .into_iter()
        .find(|session| session.id == comparison_session_id)
        .ok_or_else(|| "comparison session was not found".to_owned())?;
    if !sessions_share_track(&reference_session, &comparison_session) {
        return Err("laps must be recorded on the same simulator, track, and layout".into());
    }
    let reference = reference_session
        .laps
        .iter()
        .find(|lap| lap.index == reference_lap_index)
        .ok_or_else(|| "reference lap was not found".to_owned())?;
    let comparison = comparison_session
        .laps
        .iter()
        .find(|lap| lap.index == comparison_lap_index)
        .ok_or_else(|| "comparison lap was not found".to_owned())?;
    if lap_is_invalid_for_comparison(reference.validity.as_str(), reference.max_tyres_out)
        || lap_is_invalid_for_comparison(comparison.validity.as_str(), comparison.max_tyres_out)
    {
        return Err("invalid or incomplete laps cannot be compared".into());
    }

    let reference_columns = read_recorded_lap(&directory, &store, &reference.id)?;
    let comparison_columns = read_recorded_lap(&directory, &store, &comparison.id)?;
    let lap_length_m = shared_track_length(&reference_columns, &comparison_columns)?;
    let reference_channels = AlignedLapChannels::new(&reference_columns, lap_length_m)?;
    let comparison_channels = AlignedLapChannels::new(&comparison_columns, lap_length_m)?;
    let common_end_m = reference_channels
        .elapsed
        .samples()
        .last()
        .zip(comparison_channels.elapsed.samples().last())
        .map(|(reference, comparison)| reference.distance_m.min(comparison.distance_m))
        .ok_or_else(|| "laps do not contain a common distance range".to_owned())?;
    let grid = uniform_grid(common_end_m, 5.0)
        .map_err(|error| format!("comparison grid is unavailable: {error:?}"))?;
    let delta = calculate_delta(
        &reference_channels.elapsed,
        &comparison_channels.elapsed,
        &grid,
        30.0,
    )
    .map_err(|error| format!("lap delta is unavailable: {error:?}"))?;
    let reference_elapsed_series =
        DistanceSeries::new(reference_channels.elapsed.samples().to_vec())
            .map_err(|error| format!("reference lap clock is unavailable: {error:?}"))?;
    let comparison_elapsed_series =
        DistanceSeries::new(comparison_channels.elapsed.samples().to_vec())
            .map_err(|error| format!("comparison lap clock is unavailable: {error:?}"))?;
    let reference_elapsed = interpolate_channel(Some(&reference_elapsed_series), &grid, 1.0)?;
    let comparison_elapsed = interpolate_channel(Some(&comparison_elapsed_series), &grid, 1.0)?
        .into_iter()
        .map(|value| value.map(|seconds| seconds - delta.baseline_s))
        .collect::<Vec<_>>();

    let reference_speed = interpolate_channel(reference_channels.speed.as_ref(), &grid, 3.6)?;
    let comparison_speed = interpolate_channel(comparison_channels.speed.as_ref(), &grid, 3.6)?;
    let reference_throttle =
        interpolate_channel(reference_channels.throttle.as_ref(), &grid, 100.0)?;
    let comparison_throttle =
        interpolate_channel(comparison_channels.throttle.as_ref(), &grid, 100.0)?;
    let reference_brake = interpolate_channel(reference_channels.brake.as_ref(), &grid, 100.0)?;
    let comparison_brake = interpolate_channel(comparison_channels.brake.as_ref(), &grid, 100.0)?;
    let reference_steering =
        interpolate_channel(reference_channels.steering.as_ref(), &grid, 100.0)?;
    let comparison_steering =
        interpolate_channel(comparison_channels.steering.as_ref(), &grid, 100.0)?;
    let reference_rpm = interpolate_channel(reference_channels.rpm.as_ref(), &grid, 1.0)?;
    let comparison_rpm = interpolate_channel(comparison_channels.rpm.as_ref(), &grid, 1.0)?;
    let sectors = interpolate_discrete(reference_channels.sector.as_ref(), &grid)?;
    let reference_gear = interpolate_discrete(reference_channels.gear.as_ref(), &grid)?;
    let comparison_gear = interpolate_discrete(comparison_channels.gear.as_ref(), &grid)?;
    let reference_position_x =
        interpolate_channel(reference_channels.position_x.as_ref(), &grid, 1.0)?;
    let reference_position_z =
        interpolate_channel(reference_channels.position_z.as_ref(), &grid, 1.0)?;
    let comparison_position_x =
        interpolate_channel(comparison_channels.position_x.as_ref(), &grid, 1.0)?;
    let comparison_position_z =
        interpolate_channel(comparison_channels.position_z.as_ref(), &grid, 1.0)?;
    let reference_air_temperature =
        interpolate_channel(reference_channels.air_temperature.as_ref(), &grid, 1.0)?;
    let reference_track_temperature =
        interpolate_channel(reference_channels.track_temperature.as_ref(), &grid, 1.0)?;
    let comparison_air_temperature =
        interpolate_channel(comparison_channels.air_temperature.as_ref(), &grid, 1.0)?;
    let comparison_track_temperature =
        interpolate_channel(comparison_channels.track_temperature.as_ref(), &grid, 1.0)?;

    let corner_samples = grid
        .iter()
        .enumerate()
        .map(|(index, distance_m)| CornerComparisonSample {
            distance_m: *distance_m,
            delta_s: delta.samples[index].delta_s,
            reference_speed_kmh: reference_speed[index],
            comparison_speed_kmh: comparison_speed[index],
            reference_throttle_percent: reference_throttle[index],
            comparison_throttle_percent: comparison_throttle[index],
            reference_brake_percent: reference_brake[index],
            comparison_brake_percent: comparison_brake[index],
            reference_steering_percent: reference_steering[index],
            comparison_steering_percent: comparison_steering[index],
            reference_position_x_m: reference_position_x[index],
            reference_position_z_m: reference_position_z[index],
            comparison_position_x_m: comparison_position_x[index],
            comparison_position_z_m: comparison_position_z[index],
        })
        .collect::<Vec<_>>();
    let same_car = match (
        reference_session.source_car_id.as_deref(),
        comparison_session.source_car_id.as_deref(),
    ) {
        (Some(reference), Some(comparison)) => reference == comparison,
        _ => reference_session.car == comparison_session.car,
    };
    let comparison_context = ComparisonContext {
        same_simulator: true,
        same_car,
        same_track_layout: true,
        setup_differs: None,
        conditions_differ: None,
    };
    let corner_analysis = analyze_corner_comparison(&corner_samples, comparison_context.clone());
    let driving_analysis = analyze_driving_comparison(
        corner_analysis.value.as_ref(),
        &corner_samples,
        comparison_context,
    );

    let samples = grid
        .into_iter()
        .enumerate()
        .map(|(index, distance_m)| LapComparisonSample {
            distance_m,
            delta_seconds: delta.samples[index].delta_s,
            reference_elapsed_seconds: reference_elapsed[index],
            comparison_elapsed_seconds: comparison_elapsed[index],
            reference_speed_kmh: reference_speed[index],
            comparison_speed_kmh: comparison_speed[index],
            reference_throttle_percent: reference_throttle[index],
            comparison_throttle_percent: comparison_throttle[index],
            reference_brake_percent: reference_brake[index],
            comparison_brake_percent: comparison_brake[index],
            reference_steering_percent: reference_steering[index],
            comparison_steering_percent: comparison_steering[index],
            reference_rpm: reference_rpm[index],
            comparison_rpm: comparison_rpm[index],
            sector_index: sectors[index].and_then(rounded_u32),
            reference_gear: reference_gear[index].and_then(rounded_i16),
            comparison_gear: comparison_gear[index].and_then(rounded_i16),
            reference_position_x_m: reference_position_x[index],
            reference_position_z_m: reference_position_z[index],
            comparison_position_x_m: comparison_position_x[index],
            comparison_position_z_m: comparison_position_z[index],
            reference_air_temperature_c: reference_air_temperature[index],
            reference_track_temperature_c: reference_track_temperature[index],
            comparison_air_temperature_c: comparison_air_temperature[index],
            comparison_track_temperature_c: comparison_track_temperature[index],
        })
        .collect();

    let reference_lap_time = format_optional_lap_time(reference.duration_ns);
    let comparison_lap_time = format_optional_lap_time(comparison.duration_ns);
    let (reference_track, reference_car) = session_content_names(&store, &reference_session)?;
    let (comparison_track, comparison_car) = session_content_names(&store, &comparison_session)?;
    let track_map = track_map_for_session(
        &store,
        &reference_session,
        reference_columns.track_configuration.as_deref(),
    )?;
    Ok(LapComparison {
        reference_session_id,
        reference_session_title: reference_session.user_title,
        reference_track,
        reference_car,
        comparison_session_id,
        comparison_session_title: comparison_session.user_title,
        comparison_track,
        comparison_car,
        track_map,
        reference_lap_index,
        reference_lap_time,
        comparison_lap_index,
        comparison_lap_time,
        lap_length_m,
        corner_analysis,
        driving_analysis,
        samples,
    })
}

fn sessions_share_track(
    reference: &trace_storage::metadata::SessionSummary,
    comparison: &trace_storage::metadata::SessionSummary,
) -> bool {
    if reference.simulator_key != comparison.simulator_key {
        return false;
    }
    match (&reference.source_track_id, &comparison.source_track_id) {
        (Some(reference_track), Some(comparison_track)) => {
            reference_track == comparison_track && reference.layout_id == comparison.layout_id
        }
        _ => reference.track == comparison.track && reference.layout_id == comparison.layout_id,
    }
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn visualize_session_lap(
    app: tauri::AppHandle,
    session_id: String,
    lap_index: u32,
) -> Result<LapTrace, String> {
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
    let lap = session
        .laps
        .iter()
        .find(|lap| lap.index == lap_index)
        .ok_or_else(|| "lap was not found".to_owned())?;
    let columns = read_recorded_lap(&directory, &store, &lap.id)?;
    let lap_length_m = columns
        .track_length_m
        .filter(|value| value.is_finite() && (100.0..=100_000.0).contains(value))
        .ok_or_else(|| "the simulator did not report a usable track length".to_owned())?;
    let channels = AlignedLapChannels::new(&columns, lap_length_m)?;
    let elapsed_series = DistanceSeries::new(channels.elapsed.samples().to_vec())
        .map_err(|error| format!("lap time cannot be interpolated: {error:?}"))?;
    let end_m = channels
        .elapsed
        .samples()
        .last()
        .map(|sample| sample.distance_m)
        .ok_or_else(|| "lap does not contain a usable distance range".to_owned())?;
    let grid =
        uniform_grid(end_m, 5.0).map_err(|error| format!("lap grid is unavailable: {error:?}"))?;
    let speed = interpolate_channel(channels.speed.as_ref(), &grid, 3.6)?;
    let throttle = interpolate_channel(channels.throttle.as_ref(), &grid, 100.0)?;
    let brake = interpolate_channel(channels.brake.as_ref(), &grid, 100.0)?;
    let clutch = interpolate_channel(channels.clutch.as_ref(), &grid, 100.0)?;
    let elapsed = interpolate_channel(Some(&elapsed_series), &grid, 1.0)?;
    let steering = interpolate_channel(channels.steering.as_ref(), &grid, 100.0)?;
    let steering_degrees = interpolate_channel(
        channels.steering.as_ref(),
        &grid,
        180.0 / std::f64::consts::PI,
    )?;
    let rpm = interpolate_channel(channels.rpm.as_ref(), &grid, 1.0)?;
    let sector = interpolate_discrete(channels.sector.as_ref(), &grid)?;
    let gear = interpolate_discrete(channels.gear.as_ref(), &grid)?;
    let position_x = interpolate_channel(channels.position_x.as_ref(), &grid, 1.0)?;
    let position_z = interpolate_channel(channels.position_z.as_ref(), &grid, 1.0)?;
    let air_temperature = interpolate_channel(channels.air_temperature.as_ref(), &grid, 1.0)?;
    let track_temperature = interpolate_channel(channels.track_temperature.as_ref(), &grid, 1.0)?;
    let samples = grid
        .into_iter()
        .enumerate()
        .map(|(index, distance_m)| LapTraceSample {
            distance_m,
            elapsed_seconds: elapsed[index],
            sector_index: sector[index].and_then(rounded_u32),
            speed_kmh: speed[index],
            throttle_percent: throttle[index],
            brake_percent: brake[index],
            clutch_percent: clutch[index],
            steering_percent: steering[index],
            steering_degrees: steering_degrees[index],
            rpm: rpm[index],
            gear: gear[index].and_then(rounded_i16),
            position_x_m: position_x[index],
            position_z_m: position_z[index],
            air_temperature_c: air_temperature[index],
            track_temperature_c: track_temperature[index],
        })
        .collect();
    let lap_time = format_optional_lap_time(lap.duration_ns);
    let track_map =
        track_map_for_session(&store, &session, columns.track_configuration.as_deref())?;
    let (track, car) = session_content_names(&store, &session)?;
    Ok(LapTrace {
        session_id,
        lap_index,
        lap_time,
        track,
        car,
        lap_length_m,
        track_map,
        samples,
    })
}

fn track_map_for_session(
    store: &MetadataStore,
    session: &SessionSummary,
    recorded_layout: Option<&str>,
) -> Result<Option<AcTrackGeometry>, String> {
    if session.simulator_key != "assetto-corsa" {
        return Ok(None);
    }
    let Some(source_track_id) = session.source_track_id.as_deref() else {
        return Ok(None);
    };
    let configured_path = store
        .simulator_install_path("assetto-corsa")
        .map_err(|error| format!("failed to read simulator settings: {error:?}"))?
        .map(PathBuf::from);
    Ok(
        AcContentNames::discover(configured_path.as_deref()).track_geometry(
            source_track_id,
            recorded_layout.or(session.layout_id.as_deref()),
        ),
    )
}

fn ac_content_names(store: &MetadataStore) -> Result<AcContentNames, String> {
    let configured_path = store
        .simulator_install_path("assetto-corsa")
        .map_err(|error| format!("failed to read simulator settings: {error:?}"))?
        .map(PathBuf::from);
    Ok(AcContentNames::discover(configured_path.as_deref()))
}

fn session_content_names(
    store: &MetadataStore,
    session: &SessionSummary,
) -> Result<(String, String), String> {
    let names = if session.simulator_key == "assetto-corsa" {
        Some(ac_content_names(store)?)
    } else {
        None
    };
    Ok(resolved_session_content_names(session, names.as_ref()))
}

fn resolved_session_content_names(
    session: &SessionSummary,
    names: Option<&AcContentNames>,
) -> (String, String) {
    let track = session
        .track
        .clone()
        .or_else(|| session.source_track_id.clone())
        .unwrap_or_else(|| "TRACK NOT REPORTED".into());
    let car = session
        .car
        .clone()
        .or_else(|| session.source_car_id.clone())
        .unwrap_or_else(|| "CAR NOT REPORTED".into());
    let Some(names) = names.filter(|_| session.simulator_key == "assetto-corsa") else {
        return (track, car);
    };
    (
        session
            .source_track_id
            .as_deref()
            .map_or(track, |source_id| {
                names.track_label(
                    source_id,
                    session.layout_id.as_deref(),
                    session.track.as_deref(),
                )
            }),
        session.source_car_id.as_deref().map_or(car, |source_id| {
            names.car_label(source_id, session.car.as_deref())
        }),
    )
}

fn lap_is_invalid_for_comparison(validity: &str, max_tyres_out: Option<u8>) -> bool {
    validity == "invalid" || max_tyres_out.is_some_and(|value| value >= 3)
}

fn format_optional_lap_time(duration_ns: Option<u64>) -> String {
    duration_ns.map_or_else(|| "—".into(), format_lap_time)
}

fn read_recorded_lap(
    directory: &Path,
    store: &MetadataStore,
    lap_id: &str,
) -> Result<TelemetryColumns, String> {
    let locator = store
        .lap_telemetry(lap_id)
        .map_err(|error| format!("lap telemetry was not found: {error:?}"))?;
    let path = directory.join("telemetry").join(locator.blob_path.as_str());
    let file =
        File::open(path).map_err(|error| format!("failed to open lap telemetry: {error}"))?;
    read_columns_range(file, locator.sample_start, locator.sample_count)
        .map_err(|error| format!("failed to read lap telemetry: {error:?}"))
}

fn shared_track_length(
    reference: &TelemetryColumns,
    comparison: &TelemetryColumns,
) -> Result<f64, String> {
    let reference_length = reference
        .track_length_m
        .filter(|value| value.is_finite() && (100.0..=100_000.0).contains(value));
    let comparison_length = comparison
        .track_length_m
        .filter(|value| value.is_finite() && (100.0..=100_000.0).contains(value));
    match (reference_length, comparison_length) {
        (Some(reference), Some(comparison)) if (reference - comparison).abs() <= 5.0 => {
            Ok(reference.midpoint(comparison))
        }
        (Some(value), None) | (None, Some(value)) => Ok(value),
        (Some(_), Some(_)) => Err("laps report incompatible track lengths".into()),
        (None, None) => Err("the simulator did not report a usable track length".into()),
    }
}

struct AlignedLapChannels {
    elapsed: ElapsedTimeSeries,
    speed: Option<DistanceSeries>,
    throttle: Option<DistanceSeries>,
    brake: Option<DistanceSeries>,
    clutch: Option<DistanceSeries>,
    steering: Option<DistanceSeries>,
    rpm: Option<DistanceSeries>,
    sector: Option<DistanceSeries>,
    gear: Option<DistanceSeries>,
    position_x: Option<DistanceSeries>,
    position_z: Option<DistanceSeries>,
    air_temperature: Option<DistanceSeries>,
    track_temperature: Option<DistanceSeries>,
}

impl AlignedLapChannels {
    fn new(columns: &TelemetryColumns, lap_length_m: f64) -> Result<Self, String> {
        let lap_positions = normalized_lap_positions(&columns.lap_position);
        let elapsed_origin = columns.elapsed_ns.first().copied().unwrap_or_default();
        let elapsed_values = columns
            .elapsed_ns
            .iter()
            .map(|elapsed| {
                Some(
                    std::time::Duration::from_nanos(elapsed.saturating_sub(elapsed_origin))
                        .as_secs_f64(),
                )
            })
            .collect::<Vec<_>>();
        let elapsed_samples =
            distance_samples(&lap_positions, elapsed_values.into_iter(), lap_length_m);
        let elapsed = ElapsedTimeSeries::new(elapsed_samples)
            .map_err(|error| format!("lap time cannot be aligned by distance: {error:?}"))?;
        Ok(Self {
            elapsed,
            speed: continuous_series(&lap_positions, &columns.speed_mps, lap_length_m),
            throttle: continuous_series(&lap_positions, &columns.throttle, lap_length_m),
            brake: continuous_series(&lap_positions, &columns.brake, lap_length_m),
            clutch: continuous_series(&lap_positions, &columns.clutch, lap_length_m),
            steering: continuous_series(&lap_positions, &columns.steering_angle_rad, lap_length_m),
            rpm: continuous_series(&lap_positions, &columns.engine_rpm, lap_length_m),
            sector: numeric_series(
                &lap_positions,
                columns
                    .sector_index
                    .iter()
                    .map(|value| value.map(|value| f64::from(value + 1))),
                lap_length_m,
            ),
            gear: numeric_series(
                &lap_positions,
                columns
                    .gear_kind
                    .iter()
                    .zip(&columns.gear_value)
                    .map(|(kind, value)| resolved_gear(*kind, *value).map(f64::from)),
                lap_length_m,
            ),
            position_x: numeric_series(
                &lap_positions,
                columns.position_x_m.iter().copied(),
                lap_length_m,
            ),
            position_z: numeric_series(
                &lap_positions,
                columns.position_z_m.iter().copied(),
                lap_length_m,
            ),
            air_temperature: numeric_series(
                &lap_positions,
                columns
                    .ambient_temperature_c
                    .iter()
                    .map(|value| value.filter(|value| *value != 0.0).map(f64::from)),
                lap_length_m,
            ),
            track_temperature: numeric_series(
                &lap_positions,
                columns
                    .track_temperature_c
                    .iter()
                    .map(|value| value.filter(|value| *value != 0.0).map(f64::from)),
                lap_length_m,
            ),
        })
    }
}

fn normalized_lap_positions(positions: &[Option<f32>]) -> Vec<Option<f32>> {
    let mut forward_travel = 0.0_f32;
    let mut reverse_travel = 0.0_f32;
    let mut previous: Option<f32> = None;
    for position in positions.iter().filter_map(|position| {
        position.filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
    }) {
        if let Some(previous) = previous {
            let delta = position - previous;
            if delta.abs() <= 0.5 {
                if delta > 0.0 {
                    forward_travel += delta;
                } else {
                    reverse_travel -= delta;
                }
            }
        }
        previous = Some(position);
    }
    let reversed = reverse_travel > forward_travel;
    let origin = positions
        .iter()
        .flatten()
        .copied()
        .find(|position| position.is_finite() && (0.0..=1.0).contains(position))
        .map(|position| if reversed { 1.0 - position } else { position });
    positions
        .iter()
        .map(|position| {
            position.map(|position| {
                let position = if reversed { 1.0 - position } else { position };
                origin.map_or(position, |origin| (position - origin).rem_euclid(1.0))
            })
        })
        .collect()
}

fn continuous_series(
    positions: &[Option<f32>],
    values: &[Option<f32>],
    lap_length_m: f64,
) -> Option<DistanceSeries> {
    let samples = distance_samples(
        positions,
        values.iter().map(|value| value.map(f64::from)),
        lap_length_m,
    );
    (samples.len() >= 2)
        .then(|| DistanceSeries::new(samples).ok())
        .flatten()
}

fn numeric_series(
    positions: &[Option<f32>],
    values: impl Iterator<Item = Option<f64>>,
    lap_length_m: f64,
) -> Option<DistanceSeries> {
    let samples = distance_samples(positions, values, lap_length_m);
    (samples.len() >= 2)
        .then(|| DistanceSeries::new(samples).ok())
        .flatten()
}

fn resolved_gear(kind: Option<i8>, value: Option<i16>) -> Option<i16> {
    match kind? {
        -1 => Some(-1),
        0 => Some(0),
        1 => value,
        _ => None,
    }
}

fn rounded_u32(value: f64) -> Option<u32> {
    let rounded = value.round();
    if !rounded.is_finite() || !(0.0..=f64::from(u32::MAX)).contains(&rounded) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(rounded as u32)
}

fn rounded_i16(value: f64) -> Option<i16> {
    let rounded = value.round();
    if !rounded.is_finite() || !(f64::from(i16::MIN)..=f64::from(i16::MAX)).contains(&rounded) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    Some(rounded as i16)
}

fn distance_samples(
    positions: &[Option<f32>],
    values: impl Iterator<Item = Option<f64>>,
    lap_length_m: f64,
) -> Vec<DistanceSample> {
    let mut last_distance = None;
    positions
        .iter()
        .zip(values)
        .filter_map(|(position, value)| {
            let position = f64::from((*position)?);
            let value = value?;
            if !position.is_finite() || !(0.0..=1.0).contains(&position) || !value.is_finite() {
                return None;
            }
            let distance_m = position * lap_length_m;
            if last_distance.is_some_and(|last| distance_m <= last) {
                return None;
            }
            last_distance = Some(distance_m);
            Some(DistanceSample { distance_m, value })
        })
        .collect()
}

fn interpolate_channel(
    series: Option<&DistanceSeries>,
    grid: &[f64],
    scale: f64,
) -> Result<Vec<Option<f64>>, String> {
    series.map_or_else(
        || Ok(vec![None; grid.len()]),
        |series| {
            series
                .interpolate(grid, InterpolationMethod::Linear, 30.0)
                .map(|values| {
                    values
                        .into_iter()
                        .map(|value| value.map(|value| value * scale))
                        .collect()
                })
                .map_err(|error| format!("telemetry interpolation failed: {error:?}"))
        },
    )
}

fn interpolate_discrete(
    series: Option<&DistanceSeries>,
    grid: &[f64],
) -> Result<Vec<Option<f64>>, String> {
    series.map_or_else(
        || Ok(vec![None; grid.len()]),
        |series| {
            series
                .interpolate(grid, InterpolationMethod::HoldPrevious, 30.0)
                .map_err(|error| format!("telemetry interpolation failed: {error:?}"))
        },
    )
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
    let mut source = File::open(&source_path)
        .map_err(|error| format!("failed to open session telemetry: {error}"))?;
    let downloads = app
        .path()
        .download_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&downloads)
        .map_err(|error| format!("failed to create exports directory: {error}"))?;

    let (label, extension) = match export_format.as_str() {
        "trace" => ("TRACE session", "trace"),
        "arrow" => ("Arrow IPC", "arrow"),
        "csv" => ("CSV", "csv"),
        _ => return Err("unsupported export format".into()),
    };
    let mut session = store
        .recent_sessions(10_000)
        .map_err(|error| format!("failed to read session metadata: {error:?}"))?
        .into_iter()
        .find(|session| session.id == session_id)
        .ok_or_else(|| "session metadata no longer exists".to_owned())?;
    if session.user_driver.is_none() && session.ownership != "other" {
        session.user_driver = store
            .driver_profile_name()
            .map_err(|error| format!("failed to read driver profile: {error:?}"))?;
        if session.user_driver.is_some() {
            session.ownership = "mine".into();
        }
    }
    let (display_track, display_car) = session_content_names(&store, &session)?;
    let stem = session_export_stem(&session, &display_track, &display_car);
    let (destination_path, mut destination) = create_export_file(&downloads, &stem, extension)?;
    let export_result = if export_format == "trace" {
        let manifest = trace_package_manifest(&store, &session_id, locator.sample_count)?;
        write_compact_package(&mut destination, &mut source, &manifest)
            .map(|_| locator.sample_count)
            .map_err(|error| format!("failed to export TRACE session: {error:?}"))
    } else if export_format == "arrow" {
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

fn trace_package_manifest(
    store: &MetadataStore,
    session_id: &str,
    sample_count: u64,
) -> Result<SessionPackageManifest, String> {
    let mut session = store
        .recent_sessions(10_000)
        .map_err(|error| format!("failed to read session metadata: {error:?}"))?
        .into_iter()
        .find(|session| session.id == session_id)
        .ok_or_else(|| "session metadata no longer exists".to_owned())?;
    if session.user_driver.is_none() && session.ownership != "other" {
        session.user_driver = store
            .driver_profile_name()
            .map_err(|error| format!("failed to read driver profile: {error:?}"))?;
        if session.user_driver.is_some() {
            session.ownership = "mine".into();
        }
    }
    let (track, car) = session_content_names(store, &session)?;
    session.track = Some(track);
    session.car = Some(car);
    let laps = session
        .laps
        .iter()
        .map(|lap| {
            let locator = store
                .lap_telemetry(&lap.id)
                .map_err(|error| format!("failed to read lap metadata: {error:?}"))?;
            Ok(SessionPackageLap {
                summary: lap.clone(),
                sample_start: locator.sample_start,
                sample_count: locator.sample_count,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let confirmed_setup = package_confirmed_setup(store, session_id)?;
    Ok(SessionPackageManifest {
        format_version: PACKAGE_VERSION,
        telemetry_schema_version: TELEMETRY_SCHEMA_VERSION,
        session,
        laps,
        sample_count,
        confirmed_setup,
    })
}

fn package_confirmed_setup(
    store: &MetadataStore,
    session_id: &str,
) -> Result<Option<SessionPackageSetup>, String> {
    let Some(setup) = store
        .confirmed_session_setup(session_id)
        .map_err(|error| format!("failed to read confirmed setup: {error:?}"))?
    else {
        return Ok(None);
    };
    let file = File::open(&setup.installed_path)
        .map_err(|error| format!("failed to open confirmed setup {}: {error}", setup.name))?;
    let mut contents = Vec::new();
    file.take(
        u64::try_from(MAX_SETUP_PAYLOAD_BYTES)
            .unwrap_or(u64::MAX)
            .saturating_add(1),
    )
    .read_to_end(&mut contents)
    .map_err(|error| format!("failed to read confirmed setup {}: {error}", setup.name))?;
    let payload = encode_setup_payload(
        SessionPackageSetup {
            simulator_key: setup.simulator_key,
            source_car_id: setup.source_car_id,
            source_track_id: setup.source_track_id,
            layout_id: setup.layout_id,
            name: setup.name,
            source_archive: setup.source_archive,
            content_sha256: setup.content_sha256,
            content_base64: String::new(),
            confirmed_at: setup.confirmed_at,
        },
        &contents,
    )
    .map_err(|error| format!("confirmed setup is no longer exportable: {error:?}"))?;
    Ok(Some(payload))
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn import_session(app: tauri::AppHandle, path: String) -> Result<SessionImport, String> {
    let source_path = PathBuf::from(path);
    let source = File::open(&source_path)
        .map_err(|error| format!("failed to open TRACE session: {error}"))?;
    let file_length = source
        .metadata()
        .map_err(|error| format!("failed to inspect TRACE session: {error}"))?
        .len();
    let package = read_package(source, file_length, MAX_SESSION_BYTES)
        .map_err(|error| format!("invalid TRACE session package: {error:?}"))?;
    let manifest = package.manifest;
    let mut telemetry = package.telemetry;
    if manifest.telemetry_schema_version > TELEMETRY_SCHEMA_VERSION {
        return Err(format!(
            "this session uses telemetry schema {} but this TRACE build supports up to {}",
            manifest.telemetry_schema_version, TELEMETRY_SCHEMA_VERSION
        ));
    }
    let actual_sample_count = sample_count(&mut telemetry)
        .map_err(|error| format!("invalid TRACE telemetry payload: {error:?}"))?;
    if actual_sample_count != manifest.sample_count {
        return Err("TRACE session sample count does not match its telemetry".into());
    }
    telemetry
        .seek(SeekFrom::Start(0))
        .map_err(|error| format!("failed to rewind TRACE telemetry: {error}"))?;

    let data_directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&data_directory)
        .map_err(|error| format!("failed to prepare TRACE storage: {error}"))?;
    let mut metadata = MetadataStore::open(&data_directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let mut blobs = FileBlobStore::open(&data_directory.join("telemetry"), MAX_SESSION_BYTES)
        .map_err(|error| format!("failed to open TRACE telemetry storage: {error:?}"))?;
    let session_id = unique_import_session_id();
    let (new_session, laps) = imported_records(&session_id, &manifest);
    metadata
        .create_session(&new_session)
        .map_err(|error| format!("failed to create imported session: {error:?}"))?;

    let relative_path = RelativeBlobPath::parse(format!("sessions/{session_id}.arrow"))
        .map_err(|error| format!("failed to create imported telemetry path: {error:?}"))?;
    let import_result = (|| {
        let mut writer = blobs
            .begin_writer()
            .map_err(|error| format!("failed to stage imported telemetry: {error:?}"))?;
        io::copy(&mut telemetry, &mut writer)
            .map_err(|error| format!("failed to copy imported telemetry: {error}"))?;
        writer
            .flush()
            .map_err(|error| format!("failed to flush imported telemetry: {error}"))?;
        let blob = blobs
            .commit(
                &writer.into_pending(),
                BlobCommit {
                    path: relative_path.clone(),
                    format: BlobFormat::ArrowIpc,
                    schema_version: manifest.telemetry_schema_version,
                    sample_count: manifest.sample_count,
                    expected_sha256: None,
                },
            )
            .map_err(|error| format!("failed to commit imported telemetry: {error:?}"))?;
        metadata
            .complete_session(&session_id, &manifest.session.started_at, &blob, &laps)
            .map_err(|error| format!("failed to index imported laps: {error:?}"))?;
        metadata
            .update_session_details(
                &session_id,
                manifest.session.user_title.as_deref(),
                manifest.session.user_driver.as_deref(),
                "other",
                &manifest.session.tags,
            )
            .map_err(|error| format!("failed to restore session details: {error:?}"))?;
        if let Some(setup) = &manifest.confirmed_setup {
            restore_package_setup(&data_directory, &mut metadata, &session_id, setup)?;
        }
        Ok::<(), String>(())
    })();
    if let Err(error) = import_result {
        let _ = metadata.delete_session(&session_id);
        let _ = fs::remove_file(
            data_directory
                .join("telemetry")
                .join(relative_path.as_str()),
        );
        return Err(error);
    }

    Ok(SessionImport {
        session_id,
        lap_count: manifest.laps.len(),
        sample_count: manifest.sample_count,
        setup_name: manifest.confirmed_setup.map(|setup| setup.name),
    })
}

fn restore_package_setup(
    data_directory: &Path,
    metadata: &mut MetadataStore,
    session_id: &str,
    setup: &SessionPackageSetup,
) -> Result<(), String> {
    let contents = decode_setup_payload(setup)
        .map_err(|error| format!("invalid packaged setup: {error:?}"))?;
    let digest = setup.content_sha256.iter().fold(
        String::with_capacity(setup.content_sha256.len() * 2),
        |mut output, byte| {
            use std::fmt::Write as _;
            write!(output, "{byte:02x}").expect("writing to String cannot fail");
            output
        },
    );
    let setup_directory = data_directory.join("setup-library").join(&digest);
    fs::create_dir_all(&setup_directory)
        .map_err(|error| format!("failed to prepare imported setup storage: {error}"))?;
    let setup_path = setup_directory.join(&setup.name);
    let created = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&setup_path)
    {
        Ok(mut file) => {
            if let Err(error) = file.write_all(&contents).and_then(|()| file.sync_all()) {
                drop(file);
                let _ = fs::remove_file(&setup_path);
                return Err(format!("failed to restore packaged setup: {error}"));
            }
            true
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let existing = fs::read(&setup_path)
                .map_err(|read_error| format!("failed to verify restored setup: {read_error}"))?;
            if existing != contents {
                return Err("restored setup path contains different data".into());
            }
            false
        }
        Err(error) => return Err(format!("failed to create restored setup: {error}")),
    };
    let imported_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| error.to_string())?;
    let mut identity = Sha256::new();
    identity.update(setup.simulator_key.as_bytes());
    identity.update([0]);
    identity.update(setup_path.to_string_lossy().as_bytes());
    let record = NewSetupImport {
        id: format!("setup-{:x}", identity.finalize()),
        simulator_key: setup.simulator_key.clone(),
        source_car_id: setup.source_car_id.clone(),
        source_track_id: setup.source_track_id.clone(),
        layout_id: setup.layout_id.clone(),
        name: setup.name.clone(),
        installed_path: setup_path.to_string_lossy().into_owned(),
        source_archive: setup.source_archive.clone(),
        content_sha256: setup.content_sha256,
        imported_at,
    };
    if let Err(error) =
        metadata.restore_package_session_setup(session_id, &record, &setup.confirmed_at)
    {
        if created {
            let _ = fs::remove_file(&setup_path);
        }
        return Err(format!("failed to index packaged setup: {error:?}"));
    }
    Ok(())
}

fn unique_import_session_id() -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("imported-{nonce}")
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

fn safe_export_component(value: &str) -> String {
    let mut previous_was_separator = false;
    let sanitized: String = value
        .trim()
        .chars()
        .filter_map(|character| {
            if character.is_ascii_alphanumeric() {
                previous_was_separator = false;
                Some(character)
            } else if previous_was_separator {
                None
            } else {
                previous_was_separator = true;
                Some('-')
            }
        })
        .take(48)
        .collect();
    sanitized.trim_matches('-').to_owned()
}

fn session_export_stem(session: &SessionSummary, track: &str, car: &str) -> String {
    let date = session.started_at.get(..10).filter(|value| {
        value.len() == 10
            && value.as_bytes()[4] == b'-'
            && value.as_bytes()[7] == b'-'
            && value
                .bytes()
                .enumerate()
                .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
    });
    let values = [
        Some("trace"),
        date,
        session.user_title.as_deref(),
        session.user_driver.as_deref(),
        Some(track),
        Some(car),
    ];
    let stem = values
        .into_iter()
        .flatten()
        .map(safe_export_component)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if stem == "trace" {
        format!("trace-{}", safe_export_component(&session.id))
    } else {
        stem
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
#[allow(clippy::needless_pass_by_value)] // Tauri injects State as a command argument.
fn live_pedal_telemetry(status: tauri::State<'_, SharedCaptureStatus>) -> LivePedalTelemetry {
    let snapshot = status
        .lock()
        .map_or_else(|_| CaptureStatus::default(), |value| value.clone());
    let simulator_name = simulator_name(&snapshot.simulator_id);
    LivePedalTelemetry {
        connection: snapshot.connection,
        simulator_name,
        session: snapshot.session,
        sequence: snapshot.live_inputs.sequence,
        throttle_percent: percentage(snapshot.live_inputs.throttle),
        brake_percent: percentage(snapshot.live_inputs.brake),
        clutch_percent: percentage(snapshot.live_inputs.clutch),
        steering_degrees: snapshot
            .live_inputs
            .steering_angle_rad
            .filter(|value| value.is_finite())
            .map(f32::to_degrees),
    }
}

fn percentage(value: Option<f32>) -> Option<f32> {
    value
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0.0, 1.0) * 100.0)
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
        "inputs.clutch",
        "Clutch",
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
        "Physics °C when published; session configuration fallback otherwise",
        true,
    ),
    (
        "environment.track_temperature",
        "Track temperature",
        "CONDITIONS",
        "Physics °C when published; session configuration fallback otherwise",
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
        "Signed steering source value",
        "AC-NATIVE · INPUTS",
        "Exact AC signed source steering value",
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
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(capture_status)
        .manage(SharedLiveBroadcast::default())
        .manage(SharedDiscordActivity::default())
        .setup(move |app| {
            let directory = app.path().app_data_dir()?;
            let ac_race_config = app
                .path()
                .document_dir()
                .ok()
                .map(|documents| documents.join("Assetto Corsa").join("cfg").join("race.ini"));
            let status = app.state::<SharedCaptureStatus>().inner().clone();
            let live_broadcast = app.state::<SharedLiveBroadcast>().inner().clone();
            let discord_activity = app.state::<SharedDiscordActivity>().inner().clone();
            let discord_enabled = MetadataStore::open(&directory.join("trace.sqlite"))
                .and_then(|store| store.discord_activity_enabled())
                .unwrap_or(false);
            discord_activity.set_enabled(discord_enabled);
            discord_activity::spawn(discord_activity, status.clone(), live_broadcast.clone());
            obs_overlay::spawn(status.clone());
            capture::spawn(
                directory,
                ac_race_config,
                status,
                live_broadcast,
                &adapter_identity,
                AcAdapter::new,
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            foundation_status,
            live_pedal_telemetry,
            select_simulator,
            recent_sessions,
            compatible_setups,
            confirm_session_setup,
            clear_session_setup,
            compare_setups,
            session_lap_metrics,
            visualize_session_lap,
            compare_session_laps,
            game_install_directories,
            set_game_install_directory,
            detect_setup_folder,
            import_setup_archives,
            import_setup_files,
            attach_session_setup,
            index_existing_setups,
            setup_library,
            setup_document,
            save_setup_copy,
            setup_importers,
            startup_settings,
            set_launch_on_startup,
            driver_profile,
            set_driver_profile,
            live_settings,
            set_live_settings,
            live_broadcast_status,
            start_active_live_broadcast,
            start_recorded_live_broadcast,
            stop_live_broadcast,
            saved_comparisons,
            save_comparison,
            delete_saved_comparison,
            rename_saved_comparison,
            export_session,
            import_session,
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
    fn export_filename_components_are_safe_and_readable() {
        assert_eq!(safe_export_component("session-123"), "session-123");
        assert_eq!(safe_export_component("../session\\bad"), "session-bad");
        assert_eq!(
            safe_export_component("  Mazda MX-5 Cup  "),
            "Mazda-MX-5-Cup"
        );
    }

    #[test]
    fn unknown_simulator_keys_have_a_readable_fallback_name() {
        assert_eq!(simulator_name("example-racing-sim"), "Example Racing Sim");
    }

    #[test]
    fn forward_lap_positions_are_unchanged() {
        let positions = [Some(0.0), Some(0.25), None, Some(0.75), Some(0.99)];
        assert_eq!(normalized_lap_positions(&positions), positions);
    }

    #[test]
    fn reverse_lap_positions_are_normalized_to_increasing_distance() {
        let positions = [Some(1.0), Some(0.75), None, Some(0.25), Some(0.01)];
        assert_eq!(
            normalized_lap_positions(&positions),
            [Some(0.0), Some(0.25), None, Some(0.75), Some(0.99)]
        );
    }

    #[test]
    fn lap_positions_are_unwrapped_relative_to_the_recorded_boundary() {
        let positions = [
            Some(0.992),
            Some(0.998),
            Some(0.002),
            Some(0.5),
            Some(0.991),
        ];
        let normalized = normalized_lap_positions(&positions);
        let expected = [0.0, 0.006, 0.01, 0.508, 0.999];
        for (actual, expected) in normalized.into_iter().flatten().zip(expected) {
            assert!((actual - expected).abs() < 0.000_01);
        }
    }

    #[test]
    fn packaged_setup_is_restored_and_linked_to_imported_session() {
        use trace_storage::metadata::{NewSession, SessionConditions};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("trace-package-setup-{nonce}"));
        fs::create_dir_all(&directory).expect("test directory");
        let mut metadata = MetadataStore::open(&directory.join("trace.sqlite")).expect("metadata");
        metadata
            .create_session(&NewSession {
                id: "imported-session".into(),
                simulator_id: "sim-assetto-corsa".into(),
                simulator_key: "assetto-corsa".into(),
                simulator_version: None,
                track_id: Some("track-mugello".into()),
                source_track_id: Some("mugello".into()),
                layout_id: None,
                track_display_name: Some("Mugello".into()),
                car_id: Some("car-tatuusfa1".into()),
                source_car_id: Some("tatuusfa1".into()),
                car_display_name: Some("Tatuus FA01".into()),
                started_at: "2026-08-23T10:00:00Z".into(),
                session_type: Some("practice".into()),
                source_kind: "imported".into(),
                conditions: SessionConditions::default(),
            })
            .expect("session");
        let contents = b"[CAR]\nMODEL=tatuusfa1\n[ARB]\nFRONT=3\n";
        let digest: [u8; 32] = Sha256::digest(contents).into();
        let setup = encode_setup_payload(
            SessionPackageSetup {
                simulator_key: "assetto-corsa".into(),
                source_car_id: "tatuusfa1".into(),
                source_track_id: "mugello".into(),
                layout_id: None,
                name: "race.ini".into(),
                source_archive: Some("friend-setups.zip".into()),
                content_sha256: digest,
                content_base64: String::new(),
                confirmed_at: "2026-08-23T10:05:00Z".into(),
            },
            contents,
        )
        .expect("payload");

        restore_package_setup(&directory, &mut metadata, "imported-session", &setup)
            .expect("restore setup");
        let confirmed = metadata
            .confirmed_session_setup("imported-session")
            .expect("query")
            .expect("confirmed setup");
        assert_eq!(confirmed.name, "race.ini");
        assert_eq!(confirmed.confirmation_source, "package_confirmed");
        assert_eq!(
            fs::read(confirmed.installed_path).expect("setup file"),
            contents
        );
        drop(metadata);
        fs::remove_dir_all(directory).expect("cleanup");
    }
}
