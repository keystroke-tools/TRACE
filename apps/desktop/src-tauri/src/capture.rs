use std::{
    path::PathBuf,
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
    FileBlobStore, FileBlobWriter, RelativeBlobPath,
    ipc::TelemetryIpcWriter,
    metadata::{MetadataStore, NewSession},
};

use crate::ac_content::AcContentNames;

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
        }
    }
}

pub type SharedCaptureStatus = Arc<Mutex<CaptureStatus>>;

struct ActivePersistence {
    descriptor: CompletionDescriptor,
    writer: Option<TelemetryIpcWriter<FileBlobWriter>>,
}

pub fn spawn<A, F>(
    data_directory: PathBuf,
    status: SharedCaptureStatus,
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
            run(&data_directory, &status, &mut adapter);
        })
        .expect("failed to start TRACE capture worker");
}

fn run(
    data_directory: &std::path::Path,
    status: &SharedCaptureStatus,
    adapter: &mut dyn SimulatorAdapter,
) {
    let result = run_capture(data_directory, status, adapter);
    if let Err(error) = result {
        update_status(status, "error", 0, &format!("CAPTURE ERROR: {error}"));
        eprintln!("TRACE capture worker stopped: {error}");
    }
}

fn run_capture(
    data_directory: &std::path::Path,
    status: &SharedCaptureStatus,
    adapter: &mut dyn SimulatorAdapter,
) -> Result<(), String> {
    std::fs::create_dir_all(data_directory).map_err(|error| error.to_string())?;
    let mut metadata = MetadataStore::open(&data_directory.join("trace.sqlite"))
        .map_err(|error| format!("metadata initialization failed: {error:?}"))?;
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
    loop {
        match adapter.poll() {
            Ok(events) => {
                for event in events {
                    for output in recorder
                        .consume(event)
                        .map_err(|error| format!("recording state failed: {error:?}"))?
                    {
                        if let Err(error) =
                            handle_output(output, &mut active, &mut metadata, &mut blobs, status)
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
                        handle_output(output, &mut active, &mut metadata, &mut blobs, status)
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
    status: &SharedCaptureStatus,
) -> Result<(), String> {
    match output {
        RecorderOutput::SessionStarted { source, seed } => {
            let session_id = unique_session_id();
            let configured_path = metadata
                .simulator_install_path(source.simulator.as_str())
                .map_err(|error| format!("simulator settings query failed: {error:?}"))?
                .map(PathBuf::from);
            metadata
                .create_session(&new_session(
                    &session_id,
                    &source,
                    &seed,
                    configured_path.as_deref(),
                )?)
                .map_err(|error| format!("session creation failed: {error:?}"))?;
            let path = RelativeBlobPath::parse(format!("sessions/{session_id}.arrow"))
                .map_err(|error| format!("session blob path failed: {error:?}"))?;
            *active = Some(ActivePersistence {
                descriptor: CompletionDescriptor {
                    session_id: session_id.clone(),
                    ended_at: String::new(),
                    blob_path: path,
                    lap_id_prefix: format!("{session_id}-lap"),
                },
                writer: Some(
                    TelemetryIpcWriter::new(
                        blobs
                            .begin_writer()
                            .map_err(|error| format!("telemetry staging failed: {error:?}"))?,
                        ARROW_BATCH_FRAMES,
                    )
                    .map_err(|error| format!("Arrow stream start failed: {error:?}"))?,
                ),
            });
            set_active_session(status, Some(session_id));
            update_status(status, "recording", 60, &session_label(&seed));
        }
        RecorderOutput::FrameAccepted(frame) => {
            let persistence = active
                .as_mut()
                .ok_or_else(|| "accepted frame has no persistence identity".to_owned())?;
            persistence
                .writer
                .as_mut()
                .ok_or_else(|| "accepted frame has no Arrow writer".to_owned())?
                .push(frame)
                .map_err(|error| format!("Arrow batch write failed: {error:?}"))?;
        }
        RecorderOutput::SessionCompleted(recording) => {
            let Some(mut persistence) = active.take() else {
                return Err("completed recording has no persistence identity".into());
            };
            persistence.descriptor.ended_at = now_rfc3339()?;
            let writer = persistence
                .writer
                .take()
                .ok_or_else(|| "completed recording has no Arrow writer".to_owned())?;
            let result = persist_streamed_recording(
                blobs,
                metadata,
                &recording,
                &persistence.descriptor,
                writer,
            );
            set_active_session(status, None);
            result.map_err(|error| format!("recording persistence failed: {error:?}"))?;
            update_status(status, "waiting", 0, "NO ACTIVE SESSION");
        }
    }
    Ok(())
}

fn new_session(
    id: &str,
    source: &SourceDescriptor,
    seed: &SessionSeed,
    configured_path: Option<&std::path::Path>,
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
    })
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

fn session_label(seed: &SessionSeed) -> String {
    format!(
        "{} / {}",
        seed.track_id.as_deref().unwrap_or("TRACK NOT REPORTED"),
        seed.car_id.as_deref().unwrap_or("CAR NOT REPORTED")
    )
    .to_uppercase()
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
    }
}

fn set_active_session(status: &SharedCaptureStatus, session_id: Option<String>) {
    if let Ok(mut value) = status.lock() {
        value.active_session_id = session_id;
    }
}

#[cfg(test)]
mod tests {
    use trace_domain::SimulatorId;

    use super::*;

    #[test]
    fn replay_provenance_and_simulator_version_reach_session_metadata() {
        let source = SourceDescriptor {
            simulator: SimulatorId::parse("assetto-corsa").expect("simulator"),
            adapter_version: "1".into(),
            simulator_version: Some("1.16.4".into()),
            kind: SourceKind::SimulatorReplay,
        };
        let session =
            new_session("session-1", &source, &SessionSeed::default(), None).expect("session");

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

        let session =
            new_session("session-2", &source, &SessionSeed::default(), None).expect("session");
        assert_eq!(session.simulator_id, "sim-future-sim");
        assert_eq!(session.simulator_key, "future-sim");
    }
}
