use std::{
    fs::File,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_util::SinkExt;
use serde::{Deserialize, Serialize};
use tauri::Manager;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        http::{HeaderValue, header::AUTHORIZATION},
    },
};
use trace_domain::{Gear, SessionSeed, SourceDescriptor, TelemetryFrame};
use trace_live::{LiveStreamEncoder, LiveTelemetrySample, encode_recorded_session_with_geometry};
use trace_protocol::{
    CreateLiveSessionRequest, CreateLiveSessionResponse, InstallationCredentials, LiveStatus,
    SessionState, TrackGeometry, TrackPoint,
};
use trace_server::{LocalServer, ServerConfig, start_local_server};
use trace_storage::{
    ipc::{TelemetryColumns, read_columns_range},
    metadata::{MetadataStore, SessionSummary},
};

use crate::ac_content::{AcContentNames, AcTrackGeometry};

const DEFAULT_LIVE_SERVICE_ENDPOINT: &str = "https://live.simtrace.run";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveBroadcastPhase {
    Idle,
    Connecting,
    Reconnecting,
    Live,
    Ending,
    Ended,
    Error,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveBroadcastStatus {
    pub phase: LiveBroadcastPhase,
    pub source_session_id: Option<String>,
    pub live_session_id: Option<String>,
    pub spectator_url: Option<String>,
    pub elapsed_ns: u64,
    pub duration_ns: u64,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveBroadcastOptions {
    mode: LiveBroadcastMode,
    local_port: Option<u16>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum LiveBroadcastMode {
    Hosted,
    Local,
}

impl Default for LiveBroadcastStatus {
    fn default() -> Self {
        Self {
            phase: LiveBroadcastPhase::Idle,
            source_session_id: None,
            live_session_id: None,
            spectator_url: None,
            elapsed_ns: 0,
            duration_ns: 0,
            error: None,
        }
    }
}

#[derive(Clone)]
pub struct SharedLiveBroadcast {
    status: Arc<Mutex<LiveBroadcastStatus>>,
    credentials: Arc<Mutex<Option<InstallationCredentials>>>,
    generation: Arc<AtomicU64>,
    local_server: Arc<Mutex<Option<LocalServer>>>,
    capture: Arc<Mutex<Option<ActiveCapture>>>,
    capture_events: tokio::sync::broadcast::Sender<CaptureLiveEvent>,
}

#[derive(Clone, Debug)]
struct ActiveCapture {
    source: SourceDescriptor,
    seed: SessionSeed,
}

#[derive(Clone, Debug)]
enum CaptureLiveEvent {
    Started,
    Frame(Box<TelemetryFrame>),
    Ended,
}

impl Default for SharedLiveBroadcast {
    fn default() -> Self {
        let (capture_events, _) = tokio::sync::broadcast::channel(128);
        Self {
            status: Arc::new(Mutex::new(LiveBroadcastStatus::default())),
            credentials: Arc::new(Mutex::new(None)),
            generation: Arc::new(AtomicU64::new(0)),
            local_server: Arc::new(Mutex::new(None)),
            capture: Arc::new(Mutex::new(None)),
            capture_events,
        }
    }
}

impl SharedLiveBroadcast {
    pub fn capture_started(&self, source: &SourceDescriptor, seed: &SessionSeed) {
        let capture = ActiveCapture {
            source: source.clone(),
            seed: seed.clone(),
        };
        if let Ok(mut current) = self.capture.lock() {
            *current = Some(capture.clone());
        }
        let _ = self.capture_events.send(CaptureLiveEvent::Started);
    }

    pub fn capture_frame(&self, frame: &TelemetryFrame) {
        if self.capture_events.receiver_count() > 0 {
            let _ = self
                .capture_events
                .send(CaptureLiveEvent::Frame(Box::new(frame.clone())));
        }
    }

    pub fn capture_ended(&self) {
        if let Ok(mut current) = self.capture.lock() {
            *current = None;
        }
        let _ = self.capture_events.send(CaptureLiveEvent::Ended);
    }

    fn subscribe_capture(&self) -> tokio::sync::broadcast::Receiver<CaptureLiveEvent> {
        self.capture_events.subscribe()
    }

    fn active_capture(&self) -> Result<Option<ActiveCapture>, String> {
        self.capture
            .lock()
            .map(|capture| capture.clone())
            .map_err(|_| "active capture state is unavailable".to_owned())
    }

    fn snapshot(&self) -> Result<LiveBroadcastStatus, String> {
        self.status
            .lock()
            .map(|status| status.clone())
            .map_err(|_| "live broadcast status is unavailable".to_owned())
    }

    fn begin(&self, source_session_id: String, duration_ns: u64) -> Result<u64, String> {
        let mut status = self
            .status
            .lock()
            .map_err(|_| "live broadcast status is unavailable".to_owned())?;
        if matches!(
            status.phase,
            LiveBroadcastPhase::Connecting
                | LiveBroadcastPhase::Reconnecting
                | LiveBroadcastPhase::Live
                | LiveBroadcastPhase::Ending
        ) {
            return Err("another live broadcast is already active".to_owned());
        }
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *status = LiveBroadcastStatus {
            phase: LiveBroadcastPhase::Connecting,
            source_session_id: Some(source_session_id),
            duration_ns,
            ..LiveBroadcastStatus::default()
        };
        Ok(generation)
    }

    fn update(&self, generation: u64, update: impl FnOnce(&mut LiveBroadcastStatus)) {
        if self.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        if let Ok(mut status) = self.status.lock() {
            update(&mut status);
        }
    }

    fn current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
    }

    fn stop(&self) -> Result<LiveBroadcastStatus, String> {
        let mut status = self
            .status
            .lock()
            .map_err(|_| "live broadcast status is unavailable".to_owned())?;
        if !matches!(
            status.phase,
            LiveBroadcastPhase::Connecting
                | LiveBroadcastPhase::Reconnecting
                | LiveBroadcastPhase::Live
        ) {
            return Ok(status.clone());
        }
        self.generation.fetch_add(1, Ordering::SeqCst);
        status.phase = LiveBroadcastPhase::Ending;
        Ok(status.clone())
    }
}

struct RecordedBroadcast {
    endpoint: String,
    state: SessionState,
    samples: Vec<LiveTelemetrySample>,
    duration_ns: u64,
    track_geometry: Option<TrackGeometry>,
}

struct ActiveBroadcast {
    endpoint: String,
    state: SessionState,
    events: tokio::sync::broadcast::Receiver<CaptureLiveEvent>,
    track_geometry: Option<TrackGeometry>,
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn live_broadcast_status(
    state: tauri::State<'_, SharedLiveBroadcast>,
) -> Result<LiveBroadcastStatus, String> {
    state.snapshot()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn start_recorded_live_broadcast(
    app: tauri::AppHandle,
    state: tauri::State<'_, SharedLiveBroadcast>,
    session_id: String,
    options: LiveBroadcastOptions,
) -> Result<LiveBroadcastStatus, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let source_session_id = session_id.clone();
    let broadcast =
        tokio::task::spawn_blocking(move || load_recorded_broadcast(&directory, &session_id))
            .await
            .map_err(|error| format!("recorded broadcast loader stopped: {error}"))??;
    let generation = state.begin(source_session_id, broadcast.duration_ns)?;
    let endpoint =
        prepare_endpoint(&state, options, broadcast.endpoint.clone(), generation).await?;
    let broadcast = RecordedBroadcast {
        endpoint,
        ..broadcast
    };
    let shared = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        run_recorded_broadcast(shared, generation, broadcast).await;
    });
    state.snapshot()
}

async fn prepare_endpoint(
    state: &SharedLiveBroadcast,
    options: LiveBroadcastOptions,
    hosted_endpoint: String,
    generation: u64,
) -> Result<String, String> {
    if options.mode == LiveBroadcastMode::Hosted {
        stop_local_server(state).await;
        return Ok(hosted_endpoint);
    }
    let server = start_local_server(ServerConfig::new("http://127.0.0.1:0"), options.local_port)
        .await
        .map_err(|error| {
            let message = format!("could not start the local spectator service: {error}");
            state.update(generation, |status| {
                status.phase = LiveBroadcastPhase::Error;
                status.error = Some(message.clone());
            });
            message
        })?;
    let endpoint = server.base_url().to_owned();
    replace_local_server(state, server).await;
    Ok(endpoint)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub async fn start_active_live_broadcast(
    app: tauri::AppHandle,
    state: tauri::State<'_, SharedLiveBroadcast>,
    options: LiveBroadcastOptions,
) -> Result<LiveBroadcastStatus, String> {
    let capture = state
        .active_capture()?
        .ok_or_else(|| "start a simulator session before going live".to_owned())?;
    let events = state.subscribe_capture();
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let driver_name = store
        .driver_profile_name()
        .map_err(|error| format!("failed to read the local driver profile: {error:?}"))?;
    let configured_endpoint = store
        .live_service_endpoint()
        .map_err(|error| format!("failed to read Go Live settings: {error:?}"))?
        .unwrap_or_else(|| DEFAULT_LIVE_SERVICE_ENDPOINT.to_owned());
    let simulator_key = capture.source.simulator.as_str();
    let track_geometry = if simulator_key == "assetto-corsa" {
        let configured_path = store
            .simulator_install_path(simulator_key)
            .map_err(|error| format!("failed to read simulator settings: {error:?}"))?
            .map(std::path::PathBuf::from);
        capture.seed.track_id.as_deref().and_then(|track| {
            AcContentNames::discover(configured_path.as_deref())
                .track_geometry(track, capture.seed.layout_id.as_deref())
                .map(protocol_track_geometry)
        })
    } else {
        None
    };
    let (simulator_name, simulator_mark) = simulator_identity(simulator_key);
    let generation = state.begin("active-capture".to_owned(), 0)?;
    let endpoint = prepare_endpoint(&state, options, configured_endpoint, generation).await?;
    let source = ActiveBroadcast {
        endpoint,
        state: SessionState {
            driver_name: protocol_optional_text(driver_name),
            simulator: simulator_key.to_owned(),
            simulator_name: Some(simulator_name),
            simulator_mark: Some(simulator_mark),
            car: protocol_optional_text(capture.seed.car_id),
            track: protocol_optional_text(capture.seed.track_id),
            layout: protocol_optional_text(capture.seed.layout_id),
            session_type: protocol_optional_text(capture.seed.session_type),
            status: LiveStatus::Live,
        },
        events,
        track_geometry,
    };
    let shared = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        run_active_broadcast(shared, generation, source).await;
    });
    state.snapshot()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn stop_live_broadcast(
    state: tauri::State<'_, SharedLiveBroadcast>,
) -> Result<LiveBroadcastStatus, String> {
    state.stop()
}

async fn run_recorded_broadcast(
    shared: SharedLiveBroadcast,
    generation: u64,
    source: RecordedBroadcast,
) {
    let result = publish_recorded_broadcast(&shared, generation, &source).await;
    if shared.current(generation) {
        let failed = result.is_err();
        shared.update(generation, |status| match result {
            Ok(()) => {
                status.phase = LiveBroadcastPhase::Ended;
                status.elapsed_ns = status.duration_ns;
                status.error = None;
            }
            Err(error) => {
                status.phase = LiveBroadcastPhase::Error;
                status.error = Some(error);
            }
        });
        if failed && let Ok(mut credentials) = shared.credentials.lock() {
            *credentials = None;
        }
    } else {
        finish_cancelled_broadcast(&shared);
    }
}

async fn run_active_broadcast(
    shared: SharedLiveBroadcast,
    generation: u64,
    mut source: ActiveBroadcast,
) {
    let result = publish_active_broadcast(&shared, generation, &mut source).await;
    if shared.current(generation) {
        let failed = result.is_err();
        shared.update(generation, |status| match result {
            Ok(()) => {
                status.phase = LiveBroadcastPhase::Ended;
                status.error = None;
            }
            Err(error) => {
                status.phase = LiveBroadcastPhase::Error;
                status.error = Some(error);
            }
        });
        if failed && let Ok(mut credentials) = shared.credentials.lock() {
            *credentials = None;
        }
    } else {
        finish_cancelled_broadcast(&shared);
    }
}

async fn publish_active_broadcast(
    shared: &SharedLiveBroadcast,
    generation: u64,
    source: &mut ActiveBroadcast,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| format!("could not prepare the live client: {error}"))?;
    let credentials = installation_credentials(shared, &client, &source.endpoint).await?;
    let created = create_live_session(
        &client,
        &source.endpoint,
        &credentials,
        source.state.clone(),
    )
    .await?;
    let mut websocket = connect_publisher(&created, &credentials).await?;
    let (mut encoder, introduction) = LiveStreamEncoder::start(
        created.session_id.clone(),
        source.state.clone(),
        unix_timestamp_ms(),
    )
    .map_err(|error| format!("active telemetry cannot be published: {error:?}"))?;
    for envelope in introduction {
        send_with_reconnect(
            &mut websocket,
            &created,
            &credentials,
            &envelope,
            shared,
            generation,
        )
        .await?;
    }
    if let Some(geometry) = source.track_geometry.take() {
        let envelope = encoder
            .track_geometry(geometry, unix_timestamp_ms())
            .map_err(|error| format!("track geometry cannot be published: {error:?}"))?;
        send_with_reconnect(
            &mut websocket,
            &created,
            &credentials,
            &envelope,
            shared,
            generation,
        )
        .await?;
    }
    shared.update(generation, |status| {
        status.phase = LiveBroadcastPhase::Live;
        status.live_session_id = Some(created.session_id.clone());
        status.spectator_url = Some(created.spectator_url.clone());
        status.error = None;
    });

    let mut cancellation_check = tokio::time::interval(Duration::from_millis(250));
    loop {
        tokio::select! {
            _ = cancellation_check.tick() => {
                if !shared.current(generation) {
                    break;
                }
            }
            event = source.events.recv() => match event {
                Ok(CaptureLiveEvent::Frame(frame)) => {
                    let sample = live_sample_from_frame(&frame);
                    if let Some(envelope) = encoder.sample(&sample, unix_timestamp_ms())
                        .map_err(|error| format!("active telemetry cannot be published: {error:?}"))?
                    {
                        send_with_reconnect(
                            &mut websocket,
                            &created,
                            &credentials,
                            &envelope,
                            shared,
                            generation,
                        )
                        .await?;
                        shared.update(generation, |status| status.elapsed_ns = sample.elapsed_ns);
                    }
                }
                Ok(CaptureLiveEvent::Ended | CaptureLiveEvent::Started)
                | Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            }
        }
    }
    let terminal = encoder
        .end("capture ended", unix_timestamp_ms())
        .map_err(|error| format!("active telemetry cannot be ended: {error:?}"))?;
    let _ = send_with_reconnect(
        &mut websocket,
        &created,
        &credentials,
        &terminal,
        shared,
        generation,
    )
    .await;
    let _ = websocket.close(None).await;
    end_live_session(&client, &source.endpoint, &credentials, &created.session_id).await
}

async fn publish_recorded_broadcast(
    shared: &SharedLiveBroadcast,
    generation: u64,
    source: &RecordedBroadcast,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| format!("could not prepare the live client: {error}"))?;
    let credentials = installation_credentials(shared, &client, &source.endpoint).await?;
    if !shared.current(generation) {
        return Ok(());
    }
    let created = create_live_session(
        &client,
        &source.endpoint,
        &credentials,
        source.state.clone(),
    )
    .await?;
    if !shared.current(generation) {
        end_live_session(&client, &source.endpoint, &credentials, &created.session_id).await?;
        return Ok(());
    }
    let mut websocket = connect_publisher(&created, &credentials).await?;
    let messages = encode_recorded_session_with_geometry(
        &created.session_id,
        source.state.clone(),
        source.track_geometry.clone(),
        &source.samples,
        unix_timestamp_ms(),
    )
    .map_err(|error| format!("recorded telemetry cannot be published: {error:?}"))?;
    shared.update(generation, |status| {
        status.phase = LiveBroadcastPhase::Live;
        status.live_session_id = Some(created.session_id.clone());
        status.spectator_url = Some(created.spectator_url.clone());
        status.error = None;
    });

    let started = tokio::time::Instant::now();
    for message in messages {
        tokio::time::sleep_until(started + Duration::from_nanos(message.due_ns)).await;
        if !shared.current(generation) {
            let _ = websocket.close(None).await;
            end_live_session(&client, &source.endpoint, &credentials, &created.session_id).await?;
            return Ok(());
        }
        send_with_reconnect(
            &mut websocket,
            &created,
            &credentials,
            &message.envelope,
            shared,
            generation,
        )
        .await?;
        shared.update(generation, |status| {
            status.elapsed_ns = message.due_ns.min(status.duration_ns);
        });
    }
    websocket
        .close(None)
        .await
        .map_err(|error| format!("could not close the live publisher cleanly: {error}"))?;
    Ok(())
}

type PublisherSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect_publisher(
    created: &CreateLiveSessionResponse,
    credentials: &InstallationCredentials,
) -> Result<PublisherSocket, String> {
    let mut request = created
        .publish_websocket_url
        .as_str()
        .into_client_request()
        .map_err(|error| format!("live publisher URL is invalid: {error}"))?;
    request.headers_mut().insert(
        "x-trace-installation-id",
        HeaderValue::from_str(&credentials.installation_id)
            .map_err(|_| "installation identifier is invalid".to_owned())?,
    );
    request.headers_mut().insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", credentials.publishing_token))
            .map_err(|_| "publishing credential is invalid".to_owned())?,
    );
    tokio::time::timeout(Duration::from_secs(15), connect_async(request))
        .await
        .map_err(|_| "live publisher connection timed out".to_owned())?
        .map(|(socket, _)| socket)
        .map_err(|error| format!("could not connect to the live publisher: {error}"))
}

async fn send_envelope(
    socket: &mut PublisherSocket,
    envelope: &trace_protocol::Envelope,
) -> Result<(), String> {
    let encoded = serde_json::to_string(envelope)
        .map_err(|error| format!("could not encode live telemetry: {error}"))?;
    socket
        .send(Message::Text(encoded.into()))
        .await
        .map_err(|error| format!("live publisher disconnected: {error}"))
}

async fn send_with_reconnect(
    socket: &mut PublisherSocket,
    created: &CreateLiveSessionResponse,
    credentials: &InstallationCredentials,
    envelope: &trace_protocol::Envelope,
    shared: &SharedLiveBroadcast,
    generation: u64,
) -> Result<(), String> {
    if send_envelope(socket, envelope).await.is_ok() {
        return Ok(());
    }
    shared.update(generation, |status| {
        status.phase = LiveBroadcastPhase::Reconnecting;
        status.error = None;
    });
    let mut backoff = Duration::from_millis(500);
    loop {
        if !shared.current(generation) {
            return Err("live broadcast was cancelled while reconnecting".to_owned());
        }
        tokio::time::sleep(backoff).await;
        match connect_publisher(created, credentials).await {
            Ok(mut reconnected) => {
                if send_envelope(&mut reconnected, envelope).await.is_ok() {
                    *socket = reconnected;
                    shared.update(generation, |status| {
                        status.phase = LiveBroadcastPhase::Live;
                        status.error = None;
                    });
                    return Ok(());
                }
            }
            Err(error) => {
                shared.update(generation, |status| {
                    status.error = Some(format!("reconnecting: {error}"));
                });
            }
        }
        backoff = backoff.saturating_mul(2).min(Duration::from_secs(10));
    }
}

fn live_sample_from_frame(frame: &TelemetryFrame) -> LiveTelemetrySample {
    let position = frame.motion.position_m;
    let environment = frame.environment;
    LiveTelemetrySample {
        elapsed_ns: frame.elapsed.0,
        throttle: frame.inputs.throttle,
        brake: frame.inputs.brake,
        clutch: frame.inputs.clutch,
        steering_angle_rad: frame.inputs.steering_angle_rad,
        speed_mps: frame.vehicle.speed_mps,
        engine_rpm: frame.vehicle.engine_rpm,
        gear: frame.vehicle.gear.map(|gear| match gear {
            Gear::Reverse => -1,
            Gear::Neutral => 0,
            Gear::Forward(value) => i16::from(value),
            Gear::Unknown(value) => value,
        }),
        fuel_litres: frame.vehicle.fuel_litres,
        completed_laps: frame.lap.completed_laps,
        lap_position: frame.lap.normalized_position,
        lap_time_s: frame
            .lap
            .current_lap_time_ns
            .map(|value| Duration::from_nanos(value).as_secs_f32()),
        sector_index: frame.lap.current_sector_index,
        position_x_m: position.map(|value| value.x),
        position_z_m: position.map(|value| value.z),
        ambient_temperature_c: environment.and_then(|value| value.ambient_temperature_c),
        track_temperature_c: environment.and_then(|value| value.track_temperature_c),
        in_pit: native_boolean(frame, "graphics.is_in_pit"),
        in_pit_lane: native_boolean(frame, "graphics.is_in_pit_lane"),
    }
}

fn native_boolean(frame: &TelemetryFrame, key: &str) -> Option<bool> {
    frame
        .native
        .as_deref()
        .and_then(|native| native.integer_fields.get(key))
        .map(|value| *value != 0)
}

async fn installation_credentials(
    shared: &SharedLiveBroadcast,
    client: &reqwest::Client,
    endpoint: &str,
) -> Result<InstallationCredentials, String> {
    if let Some(credentials) = shared
        .credentials
        .lock()
        .map_err(|_| "publishing credential state is unavailable".to_owned())?
        .clone()
    {
        return Ok(credentials);
    }
    let response = client
        .post(format!("{endpoint}/api/v1/installations"))
        .send()
        .await
        .map_err(|error| format!("could not reach the live service: {error}"))?;
    let response = require_success(response, "installation bootstrap").await?;
    let credentials = response
        .json::<InstallationCredentials>()
        .await
        .map_err(|error| format!("live service returned invalid credentials: {error}"))?;
    let mut stored = shared
        .credentials
        .lock()
        .map_err(|_| "publishing credential state is unavailable".to_owned())?;
    let selected = stored.get_or_insert_with(|| credentials.clone());
    Ok(selected.clone())
}

async fn create_live_session(
    client: &reqwest::Client,
    endpoint: &str,
    credentials: &InstallationCredentials,
    state: SessionState,
) -> Result<CreateLiveSessionResponse, String> {
    let response = client
        .post(format!("{endpoint}/api/v1/live-sessions"))
        .header("x-trace-installation-id", &credentials.installation_id)
        .bearer_auth(&credentials.publishing_token)
        .json(&CreateLiveSessionRequest { session: state })
        .send()
        .await
        .map_err(|error| format!("could not create a live session: {error}"))?;
    require_success(response, "live-session creation")
        .await?
        .json::<CreateLiveSessionResponse>()
        .await
        .map_err(|error| format!("live service returned an invalid session: {error}"))
}

async fn end_live_session(
    client: &reqwest::Client,
    endpoint: &str,
    credentials: &InstallationCredentials,
    session_id: &str,
) -> Result<(), String> {
    let response = client
        .delete(format!("{endpoint}/api/v1/live-sessions/{session_id}"))
        .header("x-trace-installation-id", &credentials.installation_id)
        .bearer_auth(&credentials.publishing_token)
        .send()
        .await
        .map_err(|error| format!("could not end the live session: {error}"))?;
    require_success(response, "live-session ending").await?;
    Ok(())
}

async fn require_success(
    response: reqwest::Response,
    operation: &str,
) -> Result<reqwest::Response, String> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let detail = response
        .text()
        .await
        .unwrap_or_else(|_| "response body unavailable".to_owned());
    Err(format!("{operation} failed with {status}: {detail}"))
}

fn finish_cancelled_broadcast(shared: &SharedLiveBroadcast) {
    if let Ok(mut status) = shared.status.lock() {
        *status = LiveBroadcastStatus::default();
    }
}

async fn replace_local_server(shared: &SharedLiveBroadcast, server: LocalServer) {
    let previous = shared
        .local_server
        .lock()
        .ok()
        .and_then(|mut value| value.take());
    if let Some(previous) = previous {
        previous.shutdown().await;
    }
    if let Ok(mut value) = shared.local_server.lock() {
        *value = Some(server);
    }
}

async fn stop_local_server(shared: &SharedLiveBroadcast) {
    let previous = shared
        .local_server
        .lock()
        .ok()
        .and_then(|mut value| value.take());
    if let Some(previous) = previous {
        previous.shutdown().await;
    }
}

fn load_recorded_broadcast(
    directory: &std::path::Path,
    session_id: &str,
) -> Result<RecordedBroadcast, String> {
    let store = MetadataStore::open(&directory.join("trace.sqlite"))
        .map_err(|error| format!("failed to open TRACE metadata: {error:?}"))?;
    let session = store
        .recent_sessions(10_000)
        .map_err(|error| format!("failed to query TRACE sessions: {error:?}"))?
        .into_iter()
        .find(|session| session.id == session_id)
        .ok_or_else(|| "recorded session was not found".to_owned())?;
    let locator = store
        .session_telemetry(session_id)
        .map_err(|error| format!("session telemetry was not found: {error:?}"))?;
    let file = File::open(directory.join("telemetry").join(locator.blob_path.as_str()))
        .map_err(|error| format!("failed to open recorded telemetry: {error}"))?;
    let columns = read_columns_range(file, 0, locator.sample_count)
        .map_err(|error| format!("failed to read recorded telemetry: {error:?}"))?;
    let samples = live_samples(&columns)?;
    let duration_ns = samples
        .first()
        .zip(samples.last())
        .map_or(0, |(first, last)| {
            last.elapsed_ns.saturating_sub(first.elapsed_ns)
        });
    if duration_ns == 0 {
        return Err("recorded session does not contain a playable telemetry timeline".to_owned());
    }
    let endpoint = store
        .live_service_endpoint()
        .map_err(|error| format!("failed to read Go Live settings: {error:?}"))?
        .unwrap_or_else(|| "https://live.simtrace.run".to_owned());
    let driver_name = session.user_driver.clone().or(store
        .driver_profile_name()
        .map_err(|error| format!("failed to read the local driver profile: {error:?}"))?);
    let track_geometry = if session.simulator_key == "assetto-corsa" {
        let configured_path = store
            .simulator_install_path(&session.simulator_key)
            .map_err(|error| format!("failed to read simulator settings: {error:?}"))?
            .map(std::path::PathBuf::from);
        session.source_track_id.as_deref().and_then(|track| {
            AcContentNames::discover(configured_path.as_deref())
                .track_geometry(track, session.layout_id.as_deref())
                .map(protocol_track_geometry)
        })
    } else {
        None
    };
    let state = session_state(&session, driver_name);
    Ok(RecordedBroadcast {
        endpoint,
        state,
        samples,
        duration_ns,
        track_geometry,
    })
}

fn session_state(session: &SessionSummary, driver_name: Option<String>) -> SessionState {
    let (simulator_name, simulator_mark) = simulator_identity(&session.simulator_key);
    SessionState {
        driver_name: protocol_optional_text(driver_name),
        simulator: session.simulator_key.clone(),
        simulator_name: Some(simulator_name),
        simulator_mark: Some(simulator_mark),
        car: protocol_optional_text(
            session
                .car
                .clone()
                .or_else(|| session.source_car_id.clone()),
        ),
        track: protocol_optional_text(
            session
                .track
                .clone()
                .or_else(|| session.source_track_id.clone()),
        ),
        layout: protocol_optional_text(session.layout_id.clone()),
        session_type: protocol_optional_text(session.session_type.clone()),
        status: LiveStatus::Live,
    }
}

fn protocol_track_geometry(geometry: AcTrackGeometry) -> TrackGeometry {
    let points = |values: Vec<crate::ac_content::AcTrackPoint>| {
        values
            .into_iter()
            .map(|point| TrackPoint {
                x_m: point.x_m,
                z_m: point.z_m,
            })
            .collect()
    };
    TrackGeometry {
        centre_line: points(geometry.centre_line),
        left_boundary: points(geometry.left_boundary),
        right_boundary: points(geometry.right_boundary),
    }
}

fn simulator_identity(key: &str) -> (String, String) {
    if key == "assetto-corsa" {
        return ("Assetto Corsa".to_owned(), "AC".to_owned());
    }
    let name = key
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(characters).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mark = name
        .split_whitespace()
        .filter_map(|part| part.chars().next())
        .take(3)
        .collect::<String>()
        .to_uppercase();
    (
        if name.is_empty() {
            "Unknown simulator".to_owned()
        } else {
            name
        },
        if mark.is_empty() {
            "SIM".to_owned()
        } else {
            mark
        },
    )
}

fn protocol_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let normalized = value
            .chars()
            .map(|character| {
                if character.is_control() {
                    ' '
                } else {
                    character
                }
            })
            .collect::<String>();
        let trimmed = normalized.trim();
        if trimmed.is_empty() {
            return None;
        }
        let end = trimmed
            .char_indices()
            .map(|(index, _)| index)
            .take_while(|index| *index <= 256)
            .last()
            .unwrap_or(0);
        let candidate = if trimmed.len() <= 256 {
            trimmed
        } else {
            &trimmed[..end]
        };
        (!candidate.is_empty()).then(|| candidate.to_owned())
    })
}

fn live_samples(columns: &TelemetryColumns) -> Result<Vec<LiveTelemetrySample>, String> {
    if columns.is_empty() {
        return Err("recorded session contains no telemetry samples".to_owned());
    }
    let mut samples = Vec::with_capacity(columns.len());
    for index in 0..columns.len() {
        samples.push(LiveTelemetrySample {
            elapsed_ns: columns.elapsed_ns[index],
            throttle: columns.throttle[index],
            brake: columns.brake[index],
            clutch: columns.clutch[index],
            steering_angle_rad: columns.steering_angle_rad[index],
            speed_mps: columns.speed_mps[index],
            engine_rpm: columns.engine_rpm[index],
            gear: columns.gear_value[index],
            fuel_litres: columns.fuel_litres[index],
            completed_laps: columns.completed_laps[index],
            lap_position: columns.lap_position[index],
            lap_time_s: columns.lap_time_ns[index].and_then(nanoseconds_to_seconds),
            sector_index: columns.sector_index[index],
            position_x_m: columns.position_x_m[index],
            position_z_m: columns.position_z_m[index],
            ambient_temperature_c: columns.ambient_temperature_c[index],
            track_temperature_c: columns.track_temperature_c[index],
            in_pit: columns.in_pit[index],
            in_pit_lane: columns.in_pit_lane[index],
        });
    }
    Ok(samples)
}

#[allow(clippy::cast_precision_loss)]
fn nanoseconds_to_seconds(value: u64) -> Option<f32> {
    let seconds = value as f32 / 1_000_000_000.0;
    seconds.is_finite().then_some(seconds)
}

fn unix_timestamp_ms() -> i64 {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    i64::try_from(milliseconds).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_defaults_to_idle() {
        let shared = SharedLiveBroadcast::default();
        assert_eq!(
            shared.snapshot().expect("status").phase,
            LiveBroadcastPhase::Idle
        );
    }

    #[test]
    fn only_one_broadcast_can_start_at_a_time() {
        let shared = SharedLiveBroadcast::default();
        shared
            .begin("one".to_owned(), 100)
            .expect("first broadcast");
        assert_eq!(
            shared.begin("two".to_owned(), 100),
            Err("another live broadcast is already active".to_owned())
        );
        assert_eq!(
            shared.stop().expect("stop").phase,
            LiveBroadcastPhase::Ending
        );
    }

    #[test]
    fn optional_protocol_metadata_drops_empty_values_and_stays_bounded() {
        assert_eq!(protocol_optional_text(Some(String::new())), None);
        assert_eq!(protocol_optional_text(Some(" \t\n ".to_owned())), None);
        assert_eq!(
            protocol_optional_text(Some(" Zandvoort\n2023 ".to_owned())),
            Some("Zandvoort 2023".to_owned())
        );
        let bounded = protocol_optional_text(Some("é".repeat(200))).expect("bounded text");
        assert!(bounded.len() <= 256);
        assert!(bounded.is_char_boundary(bounded.len()));
    }
}
