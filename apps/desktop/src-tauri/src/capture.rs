use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use trace_adapter::{
    AdapterError, AdapterEvent, AdapterIdentity, DisconnectReason, SimulatorAdapter,
};
use trace_domain::{SessionSeed, SourceDescriptor, SourceKind};
use trace_recorder::{
    RecorderOutput, SessionRecorder,
    persistence::{CompletionDescriptor, persist_streamed_recording},
};
use trace_storage::{
    FileBlobStore, FileBlobWriter, RelativeBlobPath, TelemetryBlobStore,
    ipc::TelemetryIpcWriter,
    metadata::{MetadataStore, NewSession, SessionConditions},
};

use crate::{ac_content::AcContentNames, live_broadcast::SharedLiveBroadcast};

const POLL_INTERVAL: Duration = Duration::from_millis(16);
const MAX_SESSION_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const ARROW_BATCH_FRAMES: usize = 240;

#[derive(Clone, Debug)]
pub struct CaptureStatus {
    pub simulator_id: String,
    pub connection: String,
    pub source: String,
    pub sample_rate_hz: u16,
    pub session: String,
    pub active_session_id: Option<String>,
    pub completed_session_id: Option<String>,
    pub presence_session: Option<PresenceSession>,
    pub live_inputs: LiveInputs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresenceSession {
    pub simulator: String,
    pub session_type: String,
    pub track: String,
    pub car: String,
    pub started_at_unix: i64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LiveInputs {
    pub sequence: u64,
    pub throttle: Option<f32>,
    pub brake: Option<f32>,
    pub clutch: Option<f32>,
    pub steering_angle_rad: Option<f32>,
}

impl Default for CaptureStatus {
    fn default() -> Self {
        Self::for_adapter(&AdapterIdentity {
            key: "unconfigured".into(),
            display_name: "No simulator".into(),
            version: String::new(),
        })
    }
}

impl CaptureStatus {
    pub fn for_adapter(adapter: &AdapterIdentity) -> Self {
        Self {
            simulator_id: adapter.key.clone(),
            connection: "waiting".into(),
            source: adapter.display_name.to_uppercase(),
            sample_rate_hz: 0,
            session: "NO ACTIVE SESSION".into(),
            active_session_id: None,
            completed_session_id: None,
            presence_session: None,
            live_inputs: LiveInputs::default(),
        }
    }
}

pub type SharedCaptureStatus = Arc<Mutex<CaptureStatus>>;

enum ActivePersistence {
    Pending {
        source: SourceDescriptor,
        seed: SessionSeed,
    },
    Recording {
        descriptor: CompletionDescriptor,
        writer: Option<Box<TelemetryIpcWriter<FileBlobWriter>>>,
    },
}

struct CaptureOutputContext<'a> {
    data_directory: &'a std::path::Path,
    ac_race_config: Option<&'a std::path::Path>,
    status: &'a SharedCaptureStatus,
    live_broadcast: &'a SharedLiveBroadcast,
}

pub fn spawn<A, F>(
    data_directory: PathBuf,
    ac_race_config: Option<PathBuf>,
    status: SharedCaptureStatus,
    live_broadcast: SharedLiveBroadcast,
    identity: &AdapterIdentity,
    adapter_factory: F,
) where
    A: SimulatorAdapter + 'static,
    F: FnOnce() -> A + Send + 'static,
{
    let worker_name = format!("trace-{}-capture", identity.key);
    thread::Builder::new()
        .name(worker_name)
        .spawn(move || {
            let mut adapter = adapter_factory();
            run(
                &data_directory,
                ac_race_config.as_deref(),
                &status,
                &live_broadcast,
                &mut adapter,
            );
        })
        .expect("failed to start TRACE capture worker");
}

fn run(
    data_directory: &std::path::Path,
    ac_race_config: Option<&std::path::Path>,
    status: &SharedCaptureStatus,
    live_broadcast: &SharedLiveBroadcast,
    adapter: &mut dyn SimulatorAdapter,
) {
    let result = run_capture(
        data_directory,
        ac_race_config,
        status,
        live_broadcast,
        adapter,
    );
    if let Err(error) = result {
        update_status(status, "error", 0, &format!("CAPTURE ERROR: {error}"));
        eprintln!("TRACE capture worker stopped: {error}");
    }
}

fn run_capture(
    data_directory: &std::path::Path,
    ac_race_config: Option<&std::path::Path>,
    status: &SharedCaptureStatus,
    live_broadcast: &SharedLiveBroadcast,
    adapter: &mut dyn SimulatorAdapter,
) -> Result<(), String> {
    std::fs::create_dir_all(data_directory).map_err(|error| error.to_string())?;
    let mut metadata = MetadataStore::open(&data_directory.join("trace.sqlite"))
        .map_err(|error| format!("metadata initialization failed: {error:?}"))?;
    let discarded = metadata
        .discard_empty_sessions()
        .map_err(|error| format!("empty session cleanup failed: {error:?}"))?;
    if !discarded.is_empty() {
        eprintln!("TRACE discarded {} empty session records", discarded.len());
    }
    let mut blobs = FileBlobStore::open(&data_directory.join("telemetry"), MAX_SESSION_BYTES)
        .map_err(|error| format!("blob initialization failed: {error:?}"))?;
    let referenced = metadata
        .referenced_blob_paths()
        .map_err(|error| format!("blob reference query failed: {error:?}"))?;
    let reconciliation = blobs
        .reconcile(&referenced)
        .map_err(|error| format!("blob reconciliation failed: {error:?}"))?;
    if !reconciliation.committed.is_empty() || !reconciliation.pending.is_empty() {
        eprintln!(
            "TRACE quarantined {} unreferenced and {} interrupted telemetry files",
            reconciliation.committed.len(),
            reconciliation.pending.len()
        );
    }

    let mut recorder = SessionRecorder::streaming();
    let mut active = None;
    let context = CaptureOutputContext {
        data_directory,
        ac_race_config,
        status,
        live_broadcast,
    };
    loop {
        match adapter.poll() {
            Ok(events) => {
                for event in events {
                    for output in recorder
                        .consume(event)
                        .map_err(|error| format!("recording state failed: {error:?}"))?
                    {
                        if let Err(error) =
                            handle_output(output, &mut active, &mut metadata, &mut blobs, &context)
                        {
                            eprintln!("TRACE could not persist capture output: {error}");
                            update_status(status, "error", 0, "PERSISTENCE ERROR");
                        }
                    }
                }
            }
            Err(AdapterError::TemporarilyUnavailable(_)) => {}
            Err(AdapterError::ConnectionLost(message)) => {
                eprintln!("TRACE capture connection lost: {message}");
                for output in recorder
                    .consume(AdapterEvent::Disconnected(
                        DisconnectReason::DataUnavailable,
                    ))
                    .map_err(|error| format!("disconnect recording failed: {error:?}"))?
                {
                    if let Err(error) =
                        handle_output(output, &mut active, &mut metadata, &mut blobs, &context)
                    {
                        eprintln!("TRACE could not finalize disconnected capture: {error}");
                        update_status(status, "error", 0, "PERSISTENCE ERROR");
                    }
                }
            }
            Err(error) => eprintln!("TRACE adapter rejected telemetry: {error:?}"),
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn handle_output(
    output: RecorderOutput,
    active: &mut Option<ActivePersistence>,
    metadata: &mut MetadataStore,
    blobs: &mut FileBlobStore,
    context: &CaptureOutputContext<'_>,
) -> Result<(), String> {
    match output {
        RecorderOutput::SessionStarted { source, seed } => {
            set_completed_session(context.status, None);
            context.live_broadcast.capture_started(&source, &seed);
            context.live_broadcast.start_automatically_if_configured(
                context.data_directory,
                &source,
                &seed,
            );
            let label = session_label(metadata, &source, &seed);
            set_presence_session(context.status, metadata, &source, &seed);
            *active = Some(ActivePersistence::Pending {
                source,
                seed: seed.clone(),
            });
            update_status(context.status, "recording", 60, &label);
            clear_live_inputs(context.status);
        }
        RecorderOutput::FrameAccepted(frame) => {
            context.live_broadcast.capture_frame(&frame);
            update_live_inputs(context.status, &frame);
            if matches!(active, Some(ActivePersistence::Pending { .. })) {
                let Some(ActivePersistence::Pending { source, seed }) = active.take() else {
                    unreachable!("pending persistence was checked above")
                };
                *active = Some(begin_recording(
                    metadata,
                    blobs,
                    context.ac_race_config,
                    context.status,
                    &source,
                    &seed,
                )?);
            }
            let persistence = active
                .as_mut()
                .ok_or_else(|| "accepted frame has no persistence identity".to_owned())?;
            let ActivePersistence::Recording { writer, .. } = persistence else {
                return Err("accepted frame did not initialize persistence".into());
            };
            writer
                .as_mut()
                .ok_or_else(|| "accepted frame has no Arrow writer".to_owned())?
                .push(frame)
                .map_err(|error| format!("Arrow batch write failed: {error:?}"))?;
        }
        RecorderOutput::SessionCompleted(recording) => {
            context.live_broadcast.capture_ended();
            clear_presence_session(context.status);
            let Some(persistence) = active.take() else {
                return Err("completed recording has no persistence identity".into());
            };
            let ActivePersistence::Recording {
                mut descriptor,
                mut writer,
            } = persistence
            else {
                set_active_session(context.status, None);
                update_status(context.status, "waiting", 0, "NO ACTIVE SESSION");
                return Ok(());
            };
            let writer = writer
                .take()
                .ok_or_else(|| "completed recording has no Arrow writer".to_owned())?;
            let valid_lap_count = recording
                .laps
                .iter()
                .filter(|lap| lap_is_valid_for_session(lap))
                .count();
            if valid_lap_count <= 1 {
                let (writer, _) = (*writer)
                    .finish()
                    .map_err(|error| format!("Arrow stream discard failed: {error:?}"))?;
                blobs
                    .abort(&writer.into_pending())
                    .map_err(|error| format!("empty telemetry cleanup failed: {error:?}"))?;
                metadata
                    .delete_session(&descriptor.session_id)
                    .map_err(|error| format!("discarded session cleanup failed: {error:?}"))?;
                eprintln!(
                    "TRACE discarded capture with {valid_lap_count} valid lap(s); at least 2 are required"
                );
                set_active_session(context.status, None);
                update_status(context.status, "waiting", 0, "NO ACTIVE SESSION");
                return Ok(());
            }
            descriptor.ended_at = now_rfc3339()?;
            let completed_session_id = descriptor.session_id.clone();
            let result =
                persist_streamed_recording(blobs, metadata, &recording, &descriptor, *writer);
            set_active_session(context.status, None);
            result.map_err(|error| format!("recording persistence failed: {error:?}"))?;
            set_completed_session(context.status, Some(completed_session_id));
            update_status(context.status, "waiting", 0, "NO ACTIVE SESSION");
        }
    }
    Ok(())
}

fn lap_is_valid_for_session(lap: &trace_recorder::RecordedLap) -> bool {
    !lap.partial
        && lap.duration_ns.is_some()
        && lap.max_tyres_out.is_none_or(|tyres_out| tyres_out < 3)
}

fn begin_recording(
    metadata: &mut MetadataStore,
    blobs: &mut FileBlobStore,
    ac_race_config: Option<&std::path::Path>,
    status: &SharedCaptureStatus,
    source: &SourceDescriptor,
    seed: &SessionSeed,
) -> Result<ActivePersistence, String> {
    let session_id = unique_session_id();
    let configured_path = metadata
        .simulator_install_path(source.simulator.as_str())
        .map_err(|error| format!("simulator settings query failed: {error:?}"))?
        .map(PathBuf::from);
    let driver_profile = metadata
        .driver_profile_name()
        .map_err(|error| format!("driver profile query failed: {error:?}"))?;
    metadata
        .create_session(&new_session(
            &session_id,
            source,
            seed,
            configured_path.as_deref(),
            ac_race_config,
        )?)
        .map_err(|error| format!("session creation failed: {error:?}"))?;
    if let Some(driver) = driver_profile.as_deref() {
        metadata
            .update_session_details(&session_id, None, Some(driver), "mine", &[])
            .map_err(|error| format!("driver attribution failed: {error:?}"))?;
    }
    let path = RelativeBlobPath::parse(format!("sessions/{session_id}.arrow"))
        .map_err(|error| format!("session blob path failed: {error:?}"))?;
    let writer = TelemetryIpcWriter::new(
        blobs
            .begin_writer()
            .map_err(|error| format!("telemetry staging failed: {error:?}"))?,
        ARROW_BATCH_FRAMES,
    )
    .map_err(|error| format!("Arrow stream start failed: {error:?}"))?;
    set_active_session(status, Some(session_id.clone()));
    Ok(ActivePersistence::Recording {
        descriptor: CompletionDescriptor {
            session_id: session_id.clone(),
            ended_at: String::new(),
            blob_path: path,
            lap_id_prefix: format!("{session_id}-lap"),
        },
        writer: Some(Box::new(writer)),
    })
}

fn new_session(
    id: &str,
    source: &SourceDescriptor,
    seed: &SessionSeed,
    configured_path: Option<&std::path::Path>,
    ac_race_config: Option<&std::path::Path>,
) -> Result<NewSession, String> {
    let names = AcContentNames::discover(configured_path);
    let track = seed.track_id.as_ref().map(|value| {
        (
            format!("track-{}", hex_identity(value)),
            value.clone(),
            names.track(value, seed.layout_id.as_deref()),
        )
    });
    let car = seed.car_id.as_ref().map(|value| {
        (
            format!("car-{}", hex_identity(value)),
            value.clone(),
            names.car(value),
        )
    });
    let simulator_key = source.simulator.as_str();
    Ok(NewSession {
        id: id.into(),
        simulator_id: format!("sim-{simulator_key}"),
        simulator_key: simulator_key.into(),
        simulator_version: source.simulator_version.clone(),
        track_id: track.as_ref().map(|value| value.0.clone()),
        source_track_id: track.as_ref().map(|value| value.1.clone()),
        layout_id: seed.layout_id.clone(),
        track_display_name: track.map(|value| value.2),
        car_id: car.as_ref().map(|value| value.0.clone()),
        source_car_id: car.as_ref().map(|value| value.1.clone()),
        car_display_name: car.map(|value| value.2),
        started_at: now_rfc3339()?,
        session_type: seed.session_type.clone(),
        source_kind: match source.kind {
            SourceKind::NativeCapture => "native_capture",
            SourceKind::SimulatorReplay => "simulator_replay",
            SourceKind::Imported => "imported",
        }
        .into(),
        conditions: if simulator_key == "assetto-corsa" {
            ac_race_config.map_or_else(SessionConditions::default, read_ac_session_conditions)
        } else {
            SessionConditions::default()
        },
    })
}

fn read_ac_session_conditions(path: &std::path::Path) -> SessionConditions {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return SessionConditions::default();
    };
    SessionConditions {
        ambient_temperature_c: ini_temperature(&contents, "AMBIENT"),
        road_temperature_c: ini_temperature(&contents, "ROAD"),
        weather_name: ini_value(&contents, "WEATHER", "NAME"),
        track_grip_percent: ini_value(&contents, "DYNAMIC_TRACK", "SESSION_START")
            .and_then(|value| value.parse().ok())
            .filter(|value| *value <= 100),
    }
}

fn ini_temperature(contents: &str, key: &str) -> Option<String> {
    ini_value(contents, "TEMPERATURE", key).and_then(|value| {
        value
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite() && (-50.0..=100.0).contains(value))
            .map(|_| value)
    })
}

fn ini_value(contents: &str, wanted_section: &str, wanted_key: &str) -> Option<String> {
    let mut section = "";
    for line in contents.lines().map(str::trim) {
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            section = name;
            continue;
        }
        if section.eq_ignore_ascii_case(wanted_section)
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case(wanted_key)
        {
            let value = value.trim();
            if !value.is_empty()
                && value.chars().count() <= 80
                && !value.chars().any(char::is_control)
            {
                return Some(value.to_owned());
            }
        }
    }
    None
}

pub(crate) fn assetto_corsa_steering_lock_degrees(controls_config: Option<&Path>) -> Option<f32> {
    let contents = std::fs::read_to_string(controls_config?).ok()?;
    ini_value(&contents, "STEER", "LOCK")?
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite() && (90.0..=2_160.0).contains(value))
}

fn unique_session_id() -> String {
    let now = OffsetDateTime::now_utc().unix_timestamp_nanos();
    format!("session-{now}")
}

fn now_rfc3339() -> Result<String, String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| error.to_string())
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

fn session_label(
    metadata: &MetadataStore,
    source: &SourceDescriptor,
    seed: &SessionSeed,
) -> String {
    let configured_path = metadata
        .simulator_install_path(source.simulator.as_str())
        .ok()
        .flatten()
        .map(PathBuf::from);
    let content_names = (source.simulator.as_str() == "assetto-corsa")
        .then(|| AcContentNames::discover(configured_path.as_deref()));
    let track = seed.track_id.as_deref().map_or_else(
        || "TRACK NOT REPORTED".into(),
        |source_id| {
            content_names.as_ref().map_or_else(
                || source_id.to_owned(),
                |names| names.track_label(source_id, seed.layout_id.as_deref(), None),
            )
        },
    );
    let car = seed.car_id.as_deref().map_or_else(
        || "CAR NOT REPORTED".into(),
        |source_id| {
            content_names.as_ref().map_or_else(
                || source_id.to_owned(),
                |names| names.car_label(source_id, None),
            )
        },
    );
    format!("{track} / {car}").to_uppercase()
}

fn update_status(
    status: &SharedCaptureStatus,
    connection: &str,
    sample_rate_hz: u16,
    session: &str,
) {
    if let Ok(mut value) = status.lock() {
        value.connection = connection.into();
        value.sample_rate_hz = sample_rate_hz;
        value.session = session.into();
        if !matches!(connection, "recording" | "replay") {
            value.live_inputs = LiveInputs::default();
        }
    }
}

fn set_active_session(status: &SharedCaptureStatus, session_id: Option<String>) {
    if let Ok(mut value) = status.lock() {
        value.active_session_id = session_id;
    }
}

fn set_completed_session(status: &SharedCaptureStatus, session_id: Option<String>) {
    if let Ok(mut value) = status.lock() {
        value.completed_session_id = session_id;
    }
}

fn set_presence_session(
    status: &SharedCaptureStatus,
    metadata: &MetadataStore,
    source: &SourceDescriptor,
    seed: &SessionSeed,
) {
    let configured_path = metadata
        .simulator_install_path(source.simulator.as_str())
        .ok()
        .flatten()
        .map(PathBuf::from);
    let names = (source.simulator.as_str() == "assetto-corsa")
        .then(|| AcContentNames::discover(configured_path.as_deref()));
    let track = seed.track_id.as_deref().map_or_else(
        || "Unknown track".to_owned(),
        |id| {
            names.as_ref().map_or_else(
                || id.to_owned(),
                |names| names.track(id, seed.layout_id.as_deref()),
            )
        },
    );
    let car = seed.car_id.as_deref().map_or_else(
        || "Unknown car".to_owned(),
        |id| {
            names
                .as_ref()
                .map_or_else(|| id.to_owned(), |names| names.car(id))
        },
    );
    if let Ok(mut value) = status.lock() {
        value.presence_session = Some(PresenceSession {
            simulator: source.simulator.as_str().to_owned(),
            session_type: seed
                .session_type
                .clone()
                .unwrap_or_else(|| "Session".to_owned()),
            track,
            car,
            started_at_unix: OffsetDateTime::now_utc().unix_timestamp(),
        });
    }
}

fn clear_presence_session(status: &SharedCaptureStatus) {
    if let Ok(mut value) = status.lock() {
        value.presence_session = None;
    }
}

fn update_live_inputs(status: &SharedCaptureStatus, frame: &trace_domain::TelemetryFrame) {
    if let Ok(mut value) = status.lock() {
        value.live_inputs = LiveInputs {
            sequence: frame.sequence.0,
            throttle: frame.inputs.throttle,
            brake: frame.inputs.brake,
            clutch: frame.inputs.clutch,
            steering_angle_rad: frame.inputs.steering_angle_rad,
        };
    }
}

fn clear_live_inputs(status: &SharedCaptureStatus) {
    if let Ok(mut value) = status.lock() {
        value.live_inputs = LiveInputs::default();
    }
}

#[cfg(test)]
mod tests {
    use trace_domain::SimulatorId;
    use trace_recorder::RecordedLap;

    use super::*;

    fn recorded_lap(
        partial: bool,
        duration_ns: Option<u64>,
        max_tyres_out: Option<u8>,
    ) -> RecordedLap {
        RecordedLap {
            lap_index: 1,
            started_offset_ns: 0,
            duration_ns,
            sample_start: 0,
            sample_count: 60,
            partial,
            max_tyres_out,
            sectors: Vec::new(),
        }
    }

    #[test]
    fn only_complete_timed_laps_without_track_limit_evidence_are_valid_for_sessions() {
        assert!(lap_is_valid_for_session(&recorded_lap(
            false,
            Some(90),
            None
        )));
        assert!(lap_is_valid_for_session(&recorded_lap(
            false,
            Some(90),
            Some(2)
        )));
        assert!(!lap_is_valid_for_session(&recorded_lap(
            true,
            Some(90),
            None
        )));
        assert!(!lap_is_valid_for_session(&recorded_lap(false, None, None)));
        assert!(!lap_is_valid_for_session(&recorded_lap(
            false,
            Some(90),
            Some(3)
        )));
    }

    #[test]
    fn replay_provenance_and_simulator_version_reach_session_metadata() {
        let source = SourceDescriptor {
            simulator: SimulatorId::parse("assetto-corsa").expect("simulator"),
            adapter_version: "1".into(),
            simulator_version: Some("1.16.4".into()),
            kind: SourceKind::SimulatorReplay,
        };
        let session = new_session("session-1", &source, &SessionSeed::default(), None, None)
            .expect("session");

        assert_eq!(session.simulator_version.as_deref(), Some("1.16.4"));
        assert_eq!(session.simulator_id, "sim-assetto-corsa");
        assert_eq!(session.simulator_key, "assetto-corsa");
        assert_eq!(session.source_kind, "simulator_replay");
    }

    #[test]
    fn session_identity_comes_from_the_selected_adapter_source() {
        let source = SourceDescriptor {
            simulator: SimulatorId::parse("future-sim").expect("simulator"),
            adapter_version: "1".into(),
            simulator_version: None,
            kind: SourceKind::NativeCapture,
        };

        let session = new_session("session-2", &source, &SessionSeed::default(), None, None)
            .expect("session");
        assert_eq!(session.simulator_id, "sim-future-sim");
        assert_eq!(session.simulator_key, "future-sim");
    }

    #[test]
    fn parses_assetto_corsa_session_conditions() {
        let contents = "[TEMPERATURE]\nAMBIENT=15\nROAD=12\n\n[WEATHER]\nNAME=2_light_fog\n\n[DYNAMIC_TRACK]\nSESSION_START=95\n";
        assert_eq!(ini_temperature(contents, "AMBIENT").as_deref(), Some("15"));
        assert_eq!(ini_temperature(contents, "ROAD").as_deref(), Some("12"));
        assert_eq!(
            ini_value(contents, "WEATHER", "NAME").as_deref(),
            Some("2_light_fog")
        );
        assert_eq!(
            ini_value(contents, "DYNAMIC_TRACK", "SESSION_START").as_deref(),
            Some("95")
        );
    }

    #[test]
    fn parses_assetto_corsa_total_steering_rotation() {
        let directory = tempfile::tempdir().expect("temporary controls directory");
        let controls = directory.path().join("controls.ini");
        std::fs::write(&controls, "[STEER]\nLOCK=900\n").expect("controls fixture");
        assert_eq!(
            assetto_corsa_steering_lock_degrees(Some(&controls)),
            Some(900.0)
        );
    }
}
