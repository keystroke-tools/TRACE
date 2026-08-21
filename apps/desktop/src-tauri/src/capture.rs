use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use trace_ac::AcAdapter;
use trace_adapter::{AdapterError, AdapterEvent, DisconnectReason, SimulatorAdapter};
use trace_domain::SessionSeed;
use trace_recorder::{
    RecorderOutput, SessionRecorder,
    persistence::{CompletionDescriptor, persist_recording},
};
use trace_storage::{
    FileBlobStore, RelativeBlobPath,
    metadata::{MetadataStore, NewSession},
};

const POLL_INTERVAL: Duration = Duration::from_millis(16);
const MAX_SESSION_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct CaptureStatus {
    pub connection: String,
    pub source: String,
    pub sample_rate_hz: u16,
    pub session: String,
}

impl Default for CaptureStatus {
    fn default() -> Self {
        Self {
            connection: "waiting".into(),
            source: "ASSETTO CORSA".into(),
            sample_rate_hz: 0,
            session: "NO ACTIVE SESSION".into(),
        }
    }
}

pub type SharedCaptureStatus = Arc<Mutex<CaptureStatus>>;

struct ActivePersistence {
    descriptor: CompletionDescriptor,
}

pub fn spawn(data_directory: PathBuf, status: SharedCaptureStatus) {
    thread::Builder::new()
        .name("trace-ac-capture".into())
        .spawn(move || run(data_directory, &status))
        .expect("failed to start TRACE capture worker");
}

fn run(data_directory: PathBuf, status: &SharedCaptureStatus) {
    let result = run_capture(data_directory, status);
    if let Err(error) = result {
        update_status(status, "error", 0, &format!("CAPTURE ERROR: {error}"));
        eprintln!("TRACE capture worker stopped: {error}");
    }
}

fn run_capture(data_directory: PathBuf, status: &SharedCaptureStatus) -> Result<(), String> {
    std::fs::create_dir_all(&data_directory).map_err(|error| error.to_string())?;
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

    let mut adapter = AcAdapter::new();
    let mut recorder = SessionRecorder::new();
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
        RecorderOutput::SessionStarted(seed) => {
            let session_id = unique_session_id()?;
            metadata
                .create_session(&new_session(&session_id, &seed)?)
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
            });
            update_status(status, "recording", 60, &session_label(&seed));
        }
        RecorderOutput::SessionCompleted(recording) => {
            let Some(mut persistence) = active.take() else {
                return Err("completed recording has no persistence identity".into());
            };
            persistence.descriptor.ended_at = now_rfc3339()?;
            persist_recording(blobs, metadata, &recording, &persistence.descriptor)
                .map_err(|error| format!("recording persistence failed: {error:?}"))?;
            update_status(status, "waiting", 0, "NO ACTIVE SESSION");
        }
    }
    Ok(())
}

fn new_session(id: &str, seed: &SessionSeed) -> Result<NewSession, String> {
    let track = seed.track_id.as_ref().map(|value| {
        (
            format!("track-{}", hex_identity(value)),
            value.clone(),
            value.clone(),
        )
    });
    let car = seed.car_id.as_ref().map(|value| {
        (
            format!("car-{}", hex_identity(value)),
            value.clone(),
            value.clone(),
        )
    });
    Ok(NewSession {
        id: id.into(),
        simulator_id: "sim-assetto-corsa".into(),
        simulator_key: "assetto-corsa".into(),
        simulator_version: None,
        track_id: track.as_ref().map(|value| value.0.clone()),
        source_track_id: track.as_ref().map(|value| value.1.clone()),
        layout_id: seed.layout_id.clone(),
        track_display_name: track.map(|value| value.2),
        car_id: car.as_ref().map(|value| value.0.clone()),
        source_car_id: car.as_ref().map(|value| value.1.clone()),
        car_display_name: car.map(|value| value.2),
        started_at: now_rfc3339()?,
        session_type: seed.session_type.clone(),
        source_kind: "native_capture".into(),
    })
}

fn unique_session_id() -> Result<String, String> {
    let now = OffsetDateTime::now_utc().unix_timestamp_nanos();
    Ok(format!("session-{now}"))
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
        seed.track_id.as_deref().unwrap_or("UNKNOWN TRACK"),
        seed.car_id.as_deref().unwrap_or("UNKNOWN CAR")
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
