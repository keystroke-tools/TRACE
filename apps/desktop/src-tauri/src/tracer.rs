use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;
use tauri::Manager;
use trace_storage::metadata::MetadataStore;

use super::LapTrace;

const APP_DIRECTORY: &str = "TRACE_Tracer";
const PROFILE_VERSION: u8 = 1;
const BRAKE_THRESHOLD_PERCENT: f64 = 5.0;
const MINIMUM_BRAKE_PEAK_PERCENT: f64 = 20.0;
const MINIMUM_BRAKE_ZONE_METRES: f64 = 10.0;
const MERGE_BRAKE_GAP_METRES: f64 = 15.0;

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
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReferenceSource<'a> {
    session_id: &'a str,
    lap_index: u32,
    lap_time: &'a str,
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

    let data_directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let store = MetadataStore::open(&data_directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let ac_root = store
        .simulator_install_path("assetto-corsa")
        .map_err(|error| format!("failed to read Assetto Corsa settings: {error:?}"))?
        .map(PathBuf::from)
        .ok_or_else(|| {
            "set the Assetto Corsa installation directory in Settings first".to_owned()
        })?;
    let install_path = install_app(&ac_root)?;

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
        },
        samples,
        brake_zones,
    };
    let encoded = serde_json::to_vec(&profile)
        .map_err(|error| format!("failed to encode the Tracer reference: {error}"))?;
    let reference_path = reference_directory.join("reference.json");
    fs::write(&reference_path, encoded)
        .map_err(|error| format!("failed to write the Tracer reference: {error}"))?;

    Ok(TracerReferenceStatus {
        installed: true,
        install_path: install_path.to_string_lossy().into_owned(),
        reference_path: reference_path.to_string_lossy().into_owned(),
        session_id: trace.session_id.clone(),
        lap_index: trace.lap_index,
        lap_time: trace.lap_time.clone(),
        brake_zone_count: profile.brake_zones.len(),
    })
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

#[cfg(test)]
mod tests {
    use super::{
        BrakeZone, PROFILE_VERSION, ReferenceProfile, ReferenceSample, ReferenceSource,
        detect_brake_zones, install_app,
    };

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
    fn installs_the_csp_app_assets() {
        let directory = tempfile::tempdir().expect("temp directory");
        std::fs::create_dir_all(directory.path().join("apps").join("lua"))
            .expect("CSP apps directory");
        let installed = install_app(directory.path()).expect("install succeeds");
        assert!(installed.join("manifest.ini").is_file());
        assert!(installed.join("TRACE_Tracer.lua").is_file());
        assert!(installed.join("icon.png").is_file());
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
            },
            samples: vec![sample(10.0, 75.0)],
            brake_zones: vec![BrakeZone {
                start_m: 10.0,
                end_m: 35.0,
                peak_percent: 75.0,
            }],
        };
        let value = serde_json::to_value(profile).expect("profile serializes");
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["samples"][0]["d"], 10.0);
        assert_eq!(value["samples"][0]["b"], 75.0);
        assert!(value["samples"][0].get("distanceM").is_none());
        assert_eq!(value["brakeZones"][0]["startM"], 10.0);
    }
}
