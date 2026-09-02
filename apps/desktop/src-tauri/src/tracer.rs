use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tauri::Manager;
use trace_storage::metadata::MetadataStore;

use super::{LapTrace, ac_content::AcContentNames};

const APP_DIRECTORY: &str = "TRACE_Tracer";
const PROFILE_VERSION: u8 = 1;
const BRAKE_THRESHOLD_PERCENT: f64 = 5.0;
const MINIMUM_BRAKE_PEAK_PERCENT: f64 = 20.0;
const MINIMUM_BRAKE_ZONE_METRES: f64 = 10.0;
const MERGE_BRAKE_GAP_METRES: f64 = 15.0;
const THROTTLE_CUE_PERCENT: f64 = 30.0;

const MANIFEST: &str = include_str!("../tracer-ac/manifest.ini");
const LUA_APP: &str = include_str!("../tracer-ac/TRACE_Tracer.lua");
const ICON: &[u8] = include_bytes!("../icons/icon.png");

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TracerReferenceStatus {
    installed: bool,
    install_path: String,
    reference_path: String,
    session_id: String,
    lap_index: u32,
    lap_time: String,
    brake_zone_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TracerInstallStatus {
    installed: bool,
    install_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_field_names)]
pub(super) struct TracerSessionQuery {
    track_id: String,
    #[serde(default)]
    layout_id: Option<String>,
    car_id: String,
    #[serde(default)]
    include_other_tracks: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_field_names)]
pub(super) struct TracerReferenceRequest {
    session_id: String,
    lap_index: u32,
    track_id: String,
    #[serde(default)]
    layout_id: Option<String>,
    car_id: String,
    #[serde(default)]
    allow_track_mismatch: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TracerSession {
    id: String,
    title: Option<String>,
    driver: Option<String>,
    started_at: String,
    session_type: Option<String>,
    track: String,
    layout_id: Option<String>,
    exact_match: bool,
    best_lap_time: Option<String>,
    laps: Vec<TracerLap>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TracerLap {
    index: u32,
    time: String,
    validity: String,
    is_fastest: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReferenceProfile<'a> {
    schema_version: u8,
    simulator_id: &'a str,
    track_id: &'a str,
    layout_id: Option<&'a str>,
    car_id: &'a str,
    track_length_m: f64,
    sample_spacing_m: f64,
    source: ReferenceSource<'a>,
    samples: Vec<ReferenceSample>,
    brake_zones: Vec<BrakeZone>,
    throttle_cues: Vec<ThrottleCue>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReferenceSource<'a> {
    session_id: &'a str,
    lap_index: u32,
    lap_time: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    driver: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct ReferenceSample {
    #[serde(rename = "d")]
    distance_m: f64,
    #[serde(rename = "s", skip_serializing_if = "Option::is_none")]
    speed_kmh: Option<f64>,
    #[serde(rename = "t", skip_serializing_if = "Option::is_none")]
    throttle_percent: Option<f64>,
    #[serde(rename = "b", skip_serializing_if = "Option::is_none")]
    brake_percent: Option<f64>,
    #[serde(rename = "g", skip_serializing_if = "Option::is_none")]
    gear: Option<i16>,
    #[serde(rename = "e", skip_serializing_if = "Option::is_none")]
    elapsed_seconds: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrakeZone {
    start_m: f64,
    end_m: f64,
    peak_percent: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThrottleCue {
    start_m: f64,
}

pub(super) fn prepare_reference(
    app: &tauri::AppHandle,
    trace: &LapTrace,
) -> Result<TracerReferenceStatus, String> {
    if trace.simulator_id != "assetto-corsa" {
        return Err("Tracer currently supports Assetto Corsa sessions only".into());
    }
    let track_id = trace.source_track_id.as_deref().ok_or_else(|| {
        "the session does not contain an Assetto Corsa track identifier".to_owned()
    })?;
    let car_id = trace
        .source_car_id
        .as_deref()
        .ok_or_else(|| "the session does not contain an Assetto Corsa car identifier".to_owned())?;

    let documents = app
        .path()
        .document_dir()
        .map_err(|error| format!("could not locate the Documents directory: {error}"))?;
    let reference_directory = documents
        .join("Assetto Corsa")
        .join("cfg")
        .join("extension")
        .join("state")
        .join("lua")
        .join("app")
        .join(APP_DIRECTORY);
    fs::create_dir_all(&reference_directory)
        .map_err(|error| format!("failed to create Tracer configuration directory: {error}"))?;

    let samples = trace
        .samples
        .iter()
        .map(|sample| ReferenceSample {
            distance_m: sample.distance_m,
            speed_kmh: finite(sample.speed_kmh),
            throttle_percent: finite(sample.throttle_percent),
            brake_percent: finite(sample.brake_percent),
            gear: sample.gear,
            elapsed_seconds: finite(sample.elapsed_seconds),
        })
        .collect::<Vec<_>>();
    let brake_zones = detect_brake_zones(&samples);
    let throttle_cues = detect_throttle_cues(&samples, &brake_zones);
    let sample_spacing_m = samples
        .windows(2)
        .map(|pair| pair[1].distance_m - pair[0].distance_m)
        .find(|spacing| *spacing > 0.0)
        .unwrap_or(5.0);
    let profile = ReferenceProfile {
        schema_version: PROFILE_VERSION,
        simulator_id: &trace.simulator_id,
        track_id,
        layout_id: trace.layout_id.as_deref(),
        car_id,
        track_length_m: trace.lap_length_m,
        sample_spacing_m,
        source: ReferenceSource {
            session_id: &trace.session_id,
            lap_index: trace.lap_index,
            lap_time: &trace.lap_time,
            driver: trace.driver_name.as_deref(),
            title: trace.session_title.as_deref(),
        },
        samples,
        brake_zones,
        throttle_cues,
    };
    let encoded = serde_json::to_vec(&profile)
        .map_err(|error| format!("failed to encode the Tracer reference: {error}"))?;
    let reference_path = reference_directory.join("reference.json");
    fs::write(&reference_path, encoded)
        .map_err(|error| format!("failed to write the Tracer reference: {error}"))?;

    Ok(TracerReferenceStatus {
        installed: true,
        install_path: tracer_install_path(app)?.to_string_lossy().into_owned(),
        reference_path: reference_path.to_string_lossy().into_owned(),
        session_id: trace.session_id.clone(),
        lap_index: trace.lap_index,
        lap_time: trace.lap_time.clone(),
        brake_zone_count: profile.brake_zones.len(),
    })
}

pub(super) fn install(app: &tauri::AppHandle) -> Result<TracerInstallStatus, String> {
    let install_path = install_app(&assetto_corsa_root(app)?)?;
    Ok(TracerInstallStatus {
        installed: true,
        install_path: install_path.to_string_lossy().into_owned(),
    })
}

pub(super) fn install_status(app: &tauri::AppHandle) -> Result<TracerInstallStatus, String> {
    let install_path = tracer_install_path(app)?;
    let installed = install_path.join("TRACE_Tracer.lua").is_file()
        && install_path.join("manifest.ini").is_file();
    Ok(TracerInstallStatus {
        installed,
        install_path: install_path.to_string_lossy().into_owned(),
    })
}

pub(super) fn refresh_if_installed(app: &tauri::AppHandle) {
    let Ok(ac_root) = assetto_corsa_root(app) else {
        return;
    };
    if ac_root
        .join("apps")
        .join("lua")
        .join(APP_DIRECTORY)
        .is_dir()
    {
        let _ = install_app(&ac_root);
    }
}

pub(super) fn matching_sessions(
    app: &tauri::AppHandle,
    query: &TracerSessionQuery,
) -> Result<Vec<TracerSession>, String> {
    validate_identity(&query.track_id, query.layout_id.as_deref(), &query.car_id)?;
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let sessions = store
        .recent_sessions(1_000)
        .map_err(|error| format!("failed to query TRACE sessions: {error:?}"))?;
    Ok(matching_session_summaries(sessions, query))
}

fn matching_session_summaries(
    sessions: Vec<trace_storage::metadata::SessionSummary>,
    query: &TracerSessionQuery,
) -> Vec<TracerSession> {
    let mut sessions = sessions
        .into_iter()
        .filter(|session| session_is_candidate(session, query))
        .filter_map(|session| {
            let fastest_valid_ns = session
                .laps
                .iter()
                .filter(|lap| {
                    lap.duration_ns.is_some()
                        && lap.validity != "invalid"
                        && lap.max_tyres_out.is_none_or(|value| value < 3)
                })
                .filter_map(|lap| lap.duration_ns)
                .min();
            let laps = session
                .laps
                .iter()
                .filter_map(|lap| {
                    let duration_ns = lap.duration_ns?;
                    Some(TracerLap {
                        index: lap.index,
                        time: super::format_lap_time(duration_ns),
                        validity: lap.validity.clone(),
                        is_fastest: Some(duration_ns) == fastest_valid_ns,
                    })
                })
                .collect::<Vec<_>>();
            if laps.is_empty() {
                return None;
            }
            let exact_match = session_track_matches(&session, query);
            Some(TracerSession {
                id: session.id,
                title: session.user_title,
                driver: session.user_driver,
                started_at: session.started_at,
                session_type: session.session_type,
                track: session
                    .track
                    .or(session.source_track_id)
                    .unwrap_or_else(|| "Unknown track".into()),
                layout_id: session.layout_id,
                exact_match,
                best_lap_time: fastest_valid_ns.map(super::format_lap_time),
                laps,
            })
        })
        .collect::<Vec<_>>();
    sessions.sort_by_key(|session| !session.exact_match);
    sessions
}

pub(super) fn activate_reference(
    app: &tauri::AppHandle,
    request: &TracerReferenceRequest,
) -> Result<TracerReferenceStatus, String> {
    let query = TracerSessionQuery {
        track_id: request.track_id.clone(),
        layout_id: request.layout_id.clone(),
        car_id: request.car_id.clone(),
        include_other_tracks: true,
    };
    let selected = matching_sessions(app, &query)?
        .into_iter()
        .find(|session| session.id == request.session_id)
        .ok_or_else(|| "the selected session does not match the current car".to_owned())?;
    if !selected.exact_match && !request.allow_track_mismatch {
        return Err("selecting a different track requires explicit confirmation".into());
    }
    if !selected
        .laps
        .iter()
        .any(|lap| lap.index == request.lap_index)
    {
        return Err("the selected timed lap was not found".into());
    }
    let trace = super::visualize_session_lap(app.clone(), selected.id, request.lap_index)?;
    prepare_reference(app, &trace)
}

fn session_is_candidate(
    session: &trace_storage::metadata::SessionSummary,
    query: &TracerSessionQuery,
) -> bool {
    session.simulator_key == "assetto-corsa"
        && session.source_car_id.as_deref() == Some(query.car_id.as_str())
        && (query.include_other_tracks || session_track_matches(session, query))
}

fn session_track_matches(
    session: &trace_storage::metadata::SessionSummary,
    query: &TracerSessionQuery,
) -> bool {
    session.source_track_id.as_deref() == Some(query.track_id.as_str())
        && session.layout_id.as_deref().unwrap_or("") == query.layout_id.as_deref().unwrap_or("")
}

fn validate_identity(track_id: &str, layout_id: Option<&str>, car_id: &str) -> Result<(), String> {
    let valid = |value: &str| {
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    };
    if !valid(track_id) || !valid(car_id) || layout_id.is_some_and(|value| !valid(value)) {
        return Err("invalid Assetto Corsa content identity".into());
    }
    Ok(())
}

fn assetto_corsa_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let data_directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let store = MetadataStore::open(&data_directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let configured = store
        .simulator_install_path("assetto-corsa")
        .map_err(|error| format!("failed to read Assetto Corsa settings: {error:?}"))?;
    AcContentNames::discover(configured.as_deref().map(Path::new))
        .root()
        .map(Path::to_path_buf)
        .ok_or_else(|| "set the Assetto Corsa installation directory in Settings first".to_owned())
}

fn tracer_install_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(assetto_corsa_root(app)?
        .join("apps")
        .join("lua")
        .join(APP_DIRECTORY))
}

fn finite(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite())
}

fn install_app(ac_root: &Path) -> Result<PathBuf, String> {
    let lua_apps = ac_root.join("apps").join("lua");
    if !lua_apps.is_dir() {
        return Err(format!(
            "{} does not look like an Assetto Corsa installation with CSP Lua apps",
            ac_root.display()
        ));
    }
    let install_path = lua_apps.join(APP_DIRECTORY);
    fs::create_dir_all(&install_path)
        .map_err(|error| format!("failed to create the Tracer app directory: {error}"))?;
    fs::write(install_path.join("manifest.ini"), MANIFEST)
        .map_err(|error| format!("failed to install the Tracer manifest: {error}"))?;
    fs::write(install_path.join("TRACE_Tracer.lua"), LUA_APP)
        .map_err(|error| format!("failed to install the Tracer Lua app: {error}"))?;
    fs::write(install_path.join("icon.png"), ICON)
        .map_err(|error| format!("failed to install the Tracer icon: {error}"))?;
    Ok(install_path)
}

fn detect_brake_zones(samples: &[ReferenceSample]) -> Vec<BrakeZone> {
    let mut raw = Vec::new();
    let mut start = None;
    let mut peak = 0.0_f64;

    for sample in samples {
        let brake = sample.brake_percent.unwrap_or_default().max(0.0);
        if brake >= BRAKE_THRESHOLD_PERCENT {
            start.get_or_insert(sample.distance_m);
            peak = peak.max(brake);
        } else if let Some(start_m) = start.take() {
            raw.push(BrakeZone {
                start_m,
                end_m: sample.distance_m,
                peak_percent: peak,
            });
            peak = 0.0;
        }
    }
    if let (Some(start_m), Some(last)) = (start, samples.last()) {
        raw.push(BrakeZone {
            start_m,
            end_m: last.distance_m,
            peak_percent: peak,
        });
    }

    let mut merged: Vec<BrakeZone> = Vec::new();
    for zone in raw {
        if let Some(previous) = merged
            .last_mut()
            .filter(|previous| zone.start_m - previous.end_m <= MERGE_BRAKE_GAP_METRES)
        {
            previous.end_m = zone.end_m;
            previous.peak_percent = previous.peak_percent.max(zone.peak_percent);
        } else {
            merged.push(zone);
        }
    }
    merged.retain(|zone| {
        zone.end_m - zone.start_m >= MINIMUM_BRAKE_ZONE_METRES
            && zone.peak_percent >= MINIMUM_BRAKE_PEAK_PERCENT
    });
    merged
}

fn detect_throttle_cues(
    samples: &[ReferenceSample],
    brake_zones: &[BrakeZone],
) -> Vec<ThrottleCue> {
    brake_zones
        .iter()
        .enumerate()
        .filter_map(|(index, zone)| {
            let next_brake_start = brake_zones.get(index + 1).map(|next| next.start_m);
            samples
                .iter()
                .find(|sample| {
                    sample.distance_m >= zone.end_m
                        && next_brake_start.is_none_or(|start_m| sample.distance_m < start_m)
                        && sample.throttle_percent.unwrap_or_default() >= THROTTLE_CUE_PERCENT
                })
                .map(|sample| ThrottleCue {
                    start_m: sample.distance_m,
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        BrakeZone, PROFILE_VERSION, ReferenceProfile, ReferenceSample, ReferenceSource,
        ThrottleCue, TracerSessionQuery, detect_brake_zones, detect_throttle_cues, install_app,
        matching_session_summaries,
    };
    use trace_storage::metadata::{LapSummary, SessionConditions, SessionSummary};

    fn sample(distance_m: f64, brake_percent: f64) -> ReferenceSample {
        ReferenceSample {
            distance_m,
            speed_kmh: None,
            throttle_percent: None,
            brake_percent: Some(brake_percent),
            gear: None,
            elapsed_seconds: None,
        }
    }

    #[test]
    fn merges_short_release_inside_a_braking_zone() {
        let zones = detect_brake_zones(&[
            sample(0.0, 0.0),
            sample(5.0, 60.0),
            sample(20.0, 40.0),
            sample(25.0, 0.0),
            sample(35.0, 30.0),
            sample(50.0, 0.0),
        ]);
        assert_eq!(
            zones,
            vec![BrakeZone {
                start_m: 5.0,
                end_m: 50.0,
                peak_percent: 60.0
            }]
        );
    }

    #[test]
    fn ignores_noise_and_tiny_brake_taps() {
        let zones = detect_brake_zones(&[
            sample(0.0, 0.0),
            sample(5.0, 6.0),
            sample(10.0, 0.0),
            sample(20.0, 18.0),
            sample(40.0, 0.0),
        ]);
        assert!(zones.is_empty());
    }

    #[test]
    fn finds_meaningful_throttle_application_after_braking() {
        let mut samples = vec![
            sample(0.0, 0.0),
            sample(10.0, 50.0),
            sample(30.0, 0.0),
            sample(40.0, 0.0),
            sample(50.0, 0.0),
        ];
        samples[3].throttle_percent = Some(20.0);
        samples[4].throttle_percent = Some(45.0);
        let zones = vec![BrakeZone {
            start_m: 10.0,
            end_m: 30.0,
            peak_percent: 50.0,
        }];
        assert_eq!(
            detect_throttle_cues(&samples, &zones),
            vec![ThrottleCue { start_m: 50.0 }]
        );
    }

    #[test]
    fn installs_the_csp_app_assets() {
        let directory = tempfile::tempdir().expect("temp directory");
        std::fs::create_dir_all(directory.path().join("apps").join("lua"))
            .expect("CSP apps directory");
        let installed = install_app(directory.path()).expect("install succeeds");
        assert!(installed.join("manifest.ini").is_file());
        assert!(installed.join("TRACE_Tracer.lua").is_file());
        assert!(installed.join("icon.png").is_file());
        let manifest = std::fs::read_to_string(installed.join("manifest.ini"))
            .expect("installed manifest is readable");
        assert!(manifest.contains("FUNCTION_MAIN = windowSettings"));
        assert!(manifest.contains("FUNCTION_MAIN = windowBrake"));
        assert!(manifest.contains("FUNCTION_MAIN = windowProgress"));
        assert!(manifest.contains("FUNCTION_MAIN = windowGear"));
        assert!(manifest.contains("FUNCTION_MAIN = windowCoach"));
    }

    #[test]
    fn rejects_a_directory_without_csp_lua_apps() {
        let directory = tempfile::tempdir().expect("temp directory");
        let error = install_app(directory.path()).expect_err("invalid install is rejected");
        assert!(error.contains("does not look like an Assetto Corsa installation"));
    }

    #[test]
    fn reference_contract_is_versioned_and_uses_compact_sample_keys() {
        let profile = ReferenceProfile {
            schema_version: PROFILE_VERSION,
            simulator_id: "assetto-corsa",
            track_id: "ks_zandvoort",
            layout_id: None,
            car_id: "ks_mazda_mx5_cup",
            track_length_m: 4_307.0,
            sample_spacing_m: 5.0,
            source: ReferenceSource {
                session_id: "session-1",
                lap_index: 2,
                lap_time: "1:52.000",
                driver: Some("Driver One"),
                title: Some("Evening practice"),
            },
            samples: vec![sample(10.0, 75.0)],
            brake_zones: vec![BrakeZone {
                start_m: 10.0,
                end_m: 35.0,
                peak_percent: 75.0,
            }],
            throttle_cues: vec![ThrottleCue { start_m: 40.0 }],
        };
        let value = serde_json::to_value(profile).expect("profile serializes");
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["source"]["sessionId"], "session-1");
        assert_eq!(value["source"]["lapIndex"], 2);
        assert_eq!(value["source"]["driver"], "Driver One");
        assert_eq!(value["samples"][0]["d"], 10.0);
        assert_eq!(value["samples"][0]["b"], 75.0);
        assert!(value["samples"][0].get("distanceM").is_none());
        assert_eq!(value["brakeZones"][0]["startM"], 10.0);
        assert_eq!(value["throttleCues"][0]["startM"], 40.0);
    }

    #[test]
    fn matching_sessions_default_to_exact_content_and_expose_timed_laps() {
        let query = TracerSessionQuery {
            track_id: "zandvoort2023".into(),
            layout_id: None,
            car_id: "ks_mazda_mx5_cup".into(),
            include_other_tracks: false,
        };
        let mut matching = session("matching", "zandvoort2023", None);
        matching.laps = vec![
            lap(1, 110_000_000_000, "invalid"),
            lap(2, 112_000_000_000, "valid"),
            lap(3, 111_000_000_000, "valid"),
        ];
        let wrong_layout = session("wrong-layout", "zandvoort2023", Some("club"));
        let result =
            matching_session_summaries(vec![matching.clone(), wrong_layout.clone()], &query);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "matching");
        assert_eq!(result[0].best_lap_time.as_deref(), Some("1:51.000"));
        assert_eq!(result[0].laps.len(), 3);
        assert!(
            result[0]
                .laps
                .iter()
                .any(|lap| lap.index == 1 && lap.validity == "invalid")
        );
        assert!(
            result[0]
                .laps
                .iter()
                .any(|lap| lap.index == 3 && lap.is_fastest)
        );

        let other_tracks = matching_session_summaries(
            vec![wrong_layout, matching],
            &TracerSessionQuery {
                include_other_tracks: true,
                ..query
            },
        );
        assert_eq!(other_tracks.len(), 2);
        assert_eq!(other_tracks[0].id, "matching");
        assert!(other_tracks[0].exact_match);
        assert!(!other_tracks[1].exact_match);
    }

    fn session(id: &str, track_id: &str, layout_id: Option<&str>) -> SessionSummary {
        SessionSummary {
            id: id.into(),
            simulator_key: "assetto-corsa".into(),
            source_track_id: Some(track_id.into()),
            layout_id: layout_id.map(str::to_owned),
            source_car_id: Some("ks_mazda_mx5_cup".into()),
            user_title: None,
            user_driver: Some("Driver".into()),
            ownership: "self".into(),
            tags: Vec::new(),
            track: Some("Zandvoort".into()),
            car: Some("Mazda MX-5 Cup".into()),
            session_type: Some("hotlap".into()),
            started_at: "2026-09-02T12:00:00Z".into(),
            source_kind: "simulator_live".into(),
            conditions: SessionConditions::default(),
            exportable: true,
            laps: vec![lap(1, 112_000_000_000, "valid")],
        }
    }

    fn lap(index: u32, duration_ns: u64, validity: &str) -> LapSummary {
        LapSummary {
            id: format!("lap-{index}"),
            index,
            duration_ns: Some(duration_ns),
            validity: validity.into(),
            validity_reason: None,
            max_tyres_out: None,
            is_personal_best: false,
            sectors: Vec::new(),
        }
    }
}
