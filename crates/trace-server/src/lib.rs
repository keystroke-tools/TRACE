//! Account-free live telemetry service for TRACE.
//!
//! The server accepts only versioned [`trace_protocol`] envelopes. Simulator-specific
//! data stays at the desktop adapter boundary.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::sync::broadcast;
use tokio::sync::oneshot;
use trace_protocol::{
    CreateLiveSessionRequest, CreateLiveSessionResponse, Envelope, InstallationCredentials,
    LiveSessionSummary, LiveStatus, PROTOCOL_VERSION, Payload, ProtocolLimits, SessionEnd,
    SessionState,
};
use uuid::Uuid;

const DEFAULT_BUFFER_CAPACITY: usize = 2_400;
const BROADCAST_CAPACITY: usize = 512;
const MAX_WEBSOCKET_MESSAGE_BYTES: usize = 512 * 1024;

/// Runtime settings supplied by the deployment environment.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    public_base_url: String,
    buffer_capacity: usize,
}

impl ServerConfig {
    /// Creates settings for the public service URL.
    #[must_use]
    pub fn new(public_base_url: impl Into<String>) -> Self {
        Self {
            public_base_url: public_base_url.into().trim_end_matches('/').to_owned(),
            buffer_capacity: DEFAULT_BUFFER_CAPACITY,
        }
    }

    #[cfg(test)]
    fn with_buffer_capacity(mut self, capacity: usize) -> Self {
        self.buffer_capacity = capacity;
        self
    }
}

/// Shared live-service state.
#[derive(Clone)]
pub struct LiveService {
    inner: Arc<RwLock<ServiceState>>,
    config: ServerConfig,
}

/// Handle for a server embedded in the desktop process.
pub struct LocalServer {
    base_url: String,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl LocalServer {
    /// Returns the loopback URL that can be opened by a browser on this machine.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Requests the embedded listener to stop and waits for its task to exit.
    pub async fn shutdown(mut self) {
        if let Some(sender) = self.shutdown.take() {
            let _ = sender.send(());
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

/// Starts a loopback service on an available local TCP port.
///
/// # Errors
///
/// Returns the operating-system error when the loopback listener cannot be bound
/// or its assigned address cannot be read.
pub async fn start_local_server(
    mut config: ServerConfig,
    port: Option<u16>,
) -> Result<LocalServer, std::io::Error> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port.unwrap_or(0))).await?;
    let port = listener.local_addr()?.port();
    config.public_base_url = format!("http://127.0.0.1:{port}");
    let (shutdown, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app(config))
            .with_graceful_shutdown(async {
                let _ = receiver.await;
            })
            .await;
    });
    Ok(LocalServer {
        base_url: format!("http://127.0.0.1:{port}"),
        shutdown: Some(shutdown),
        task: Some(task),
    })
}

#[derive(Default)]
struct ServiceState {
    installations: HashMap<String, Installation>,
    sessions: HashMap<String, LiveSession>,
}

struct Installation {
    token_hash: [u8; 32],
}

struct LiveSession {
    owner_id: String,
    state: SessionState,
    last_sequence: Option<u64>,
    buffer: VecDeque<Envelope>,
    broadcast: broadcast::Sender<Envelope>,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: &'static str,
}

#[derive(Clone, Copy, Debug)]
enum ServiceError {
    Unauthorized,
    NotFound,
    InvalidMessage,
    OutOfOrder,
    Ended,
    Unavailable,
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let (status, error) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "publisher authentication failed"),
            Self::NotFound => (StatusCode::NOT_FOUND, "live session not found"),
            Self::InvalidMessage => (StatusCode::BAD_REQUEST, "invalid live protocol message"),
            Self::OutOfOrder => (StatusCode::CONFLICT, "message sequence is not increasing"),
            Self::Ended => (StatusCode::GONE, "live session has ended"),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "live service state unavailable",
            ),
        };
        (status, Json(ErrorBody { error })).into_response()
    }
}

impl LiveService {
    /// Creates an empty service state.
    #[must_use]
    pub fn new(config: ServerConfig) -> Self {
        Self {
            inner: Arc::new(RwLock::new(ServiceState::default())),
            config,
        }
    }

    fn bootstrap_installation(&self) -> Result<InstallationCredentials, ServiceError> {
        let installation_id = opaque_identifier();
        let publishing_token = format!("{}{}", opaque_identifier(), opaque_identifier());
        let installation = Installation {
            token_hash: hash_token(&publishing_token),
        };
        self.inner
            .write()
            .map_err(|_| ServiceError::Unavailable)?
            .installations
            .insert(installation_id.clone(), installation);
        Ok(InstallationCredentials {
            installation_id,
            publishing_token,
        })
    }

    fn create_session(
        &self,
        installation_id: &str,
        token: &str,
        mut state: SessionState,
    ) -> Result<CreateLiveSessionResponse, ServiceError> {
        let mut service = self.inner.write().map_err(|_| ServiceError::Unavailable)?;
        authenticate(&service, installation_id, token)?;
        let session_id = opaque_identifier();
        state.status = LiveStatus::Paused;
        validate_session_state(&session_id, &state)?;
        let (broadcast, _) = broadcast::channel(BROADCAST_CAPACITY);
        service.sessions.insert(
            session_id.clone(),
            LiveSession {
                owner_id: installation_id.to_owned(),
                state,
                last_sequence: None,
                buffer: VecDeque::with_capacity(self.config.buffer_capacity),
                broadcast,
            },
        );

        let websocket_base = websocket_base_url(&self.config.public_base_url);
        Ok(CreateLiveSessionResponse {
            publish_websocket_url: format!(
                "{websocket_base}/api/v1/live-sessions/{session_id}/publish"
            ),
            spectator_url: format!("{}/live/{session_id}", self.config.public_base_url),
            session_id,
        })
    }

    fn summary(&self, session_id: &str) -> Result<LiveSessionSummary, ServiceError> {
        let service = self.inner.read().map_err(|_| ServiceError::Unavailable)?;
        let live = service
            .sessions
            .get(session_id)
            .ok_or(ServiceError::NotFound)?;
        Ok(LiveSessionSummary {
            session_id: session_id.to_owned(),
            session: live.state.clone(),
            oldest_sequence: live.buffer.front().map(|message| message.sequence),
            newest_sequence: live.buffer.back().map(|message| message.sequence),
        })
    }

    fn authorize_session(
        &self,
        session_id: &str,
        installation_id: &str,
        token: &str,
    ) -> Result<(), ServiceError> {
        let service = self.inner.read().map_err(|_| ServiceError::Unavailable)?;
        authenticate(&service, installation_id, token)?;
        let session = service
            .sessions
            .get(session_id)
            .ok_or(ServiceError::NotFound)?;
        if session.owner_id == installation_id {
            Ok(())
        } else {
            Err(ServiceError::Unauthorized)
        }
    }

    fn publish(&self, session_id: &str, message: Envelope) -> Result<(), ServiceError> {
        message
            .validate(ProtocolLimits::default())
            .map_err(|_| ServiceError::InvalidMessage)?;
        if message.session_id != session_id {
            return Err(ServiceError::InvalidMessage);
        }

        let mut service = self.inner.write().map_err(|_| ServiceError::Unavailable)?;
        let session = service
            .sessions
            .get_mut(session_id)
            .ok_or(ServiceError::NotFound)?;
        if session.state.status == LiveStatus::Ended {
            return Err(ServiceError::Ended);
        }
        if let Some(sequence) = session.last_sequence {
            if message.sequence == sequence
                && session
                    .buffer
                    .back()
                    .is_some_and(|accepted| accepted.message_id == message.message_id)
            {
                return Ok(());
            }
            if message.sequence <= sequence {
                return Err(ServiceError::OutOfOrder);
            }
        }

        if let Payload::SessionState(state) = &message.payload {
            session.state = state.clone();
        }
        if matches!(message.payload, Payload::End(_)) {
            session.state.status = LiveStatus::Ended;
        }
        session.last_sequence = Some(message.sequence);
        push_buffered(session, message, self.config.buffer_capacity);
        Ok(())
    }

    fn subscribe(
        &self,
        session_id: &str,
    ) -> Result<(Vec<Envelope>, broadcast::Receiver<Envelope>), ServiceError> {
        let service = self.inner.read().map_err(|_| ServiceError::Unavailable)?;
        let session = service
            .sessions
            .get(session_id)
            .ok_or(ServiceError::NotFound)?;
        Ok((
            session.buffer.iter().cloned().collect(),
            session.broadcast.subscribe(),
        ))
    }

    fn end_session(
        &self,
        session_id: &str,
        installation_id: &str,
        token: &str,
    ) -> Result<(), ServiceError> {
        self.authorize_session(session_id, installation_id, token)?;
        let mut service = self.inner.write().map_err(|_| ServiceError::Unavailable)?;
        let session = service
            .sessions
            .get_mut(session_id)
            .ok_or(ServiceError::NotFound)?;
        if session.state.status == LiveStatus::Ended {
            return Ok(());
        }
        session.state.status = LiveStatus::Ended;
        let sequence = session
            .last_sequence
            .map_or(0, |value| value.saturating_add(1));
        let message = Envelope {
            protocol_version: PROTOCOL_VERSION,
            message_id: format!("server_end_{sequence}"),
            session_id: session_id.to_owned(),
            sequence,
            sent_at_unix_ms: unix_timestamp_ms(),
            payload: Payload::End(SessionEnd {
                reason: "publisher ended the session".to_owned(),
            }),
        };
        session.last_sequence = Some(sequence);
        push_buffered(session, message, self.config.buffer_capacity);
        Ok(())
    }
}

/// Builds the HTTP and WebSocket application.
pub fn app(config: ServerConfig) -> Router {
    let state = LiveService::new(config);
    Router::new()
        .route("/health", get(health))
        .route("/", get(landing_page))
        .route("/favicon.svg", get(favicon))
        .route("/api/v1/installations", post(bootstrap_installation))
        .route("/api/v1/live-sessions", post(create_live_session))
        .route(
            "/api/v1/live-sessions/{session_id}",
            get(get_live_session).delete(end_live_session),
        )
        .route(
            "/api/v1/live-sessions/{session_id}/publish",
            get(publisher_websocket),
        )
        .route(
            "/api/v1/live-sessions/{session_id}/spectate",
            get(spectator_websocket),
        )
        .route("/live/{session_id}", get(spectator_page))
        .route("/live-assets/spectator.css", get(spectator_styles))
        .route("/live-assets/spectator.js", get(spectator_script))
        .route("/live-assets/site.css", get(site_styles))
        .route("/live-assets/trace-live-og.png", get(social_card))
        .fallback(not_found)
        .with_state(state)
}

async fn health() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn landing_page(State(service): State<LiveService>) -> Html<String> {
    Html(site_html(
        include_str!("../assets/site/index.html"),
        &service.config.public_base_url,
    ))
}

async fn bootstrap_installation(
    State(service): State<LiveService>,
) -> Result<Json<InstallationCredentials>, ServiceError> {
    service.bootstrap_installation().map(Json)
}

async fn create_live_session(
    State(service): State<LiveService>,
    headers: HeaderMap,
    Json(request): Json<CreateLiveSessionRequest>,
) -> Result<(StatusCode, Json<CreateLiveSessionResponse>), ServiceError> {
    let (installation_id, token) = publisher_credentials(&headers)?;
    let response = service.create_session(installation_id, token, request.session)?;
    Ok((StatusCode::CREATED, Json(response)))
}

async fn get_live_session(
    State(service): State<LiveService>,
    Path(session_id): Path<String>,
) -> Result<Json<LiveSessionSummary>, ServiceError> {
    service.summary(&session_id).map(Json)
}

async fn spectator_page(
    State(service): State<LiveService>,
    Path(session_id): Path<String>,
) -> Response {
    match service.summary(&session_id) {
        Ok(summary) => Html(spectator_html(
            &summary.session,
            &session_id,
            &service.config.public_base_url,
        ))
        .into_response(),
        Err(ServiceError::NotFound) => not_found_response(&service),
        Err(error) => error.into_response(),
    }
}

fn spectator_html(state: &SessionState, session_id: &str, public_base_url: &str) -> String {
    let driver = state.driver_name.as_deref().unwrap_or("Live driver");
    let track = state.track.as_deref().unwrap_or("an unknown track");
    let car = state.car.as_deref().unwrap_or("an unknown car");
    let session_type = state
        .session_type
        .as_deref()
        .map_or_else(|| "live session".to_owned(), format_session_type);
    let title = format!("{driver} at {track} — TRACE Pit Wall");
    let description =
        format!("Watch {driver}'s {session_type} in {car} at {track} with live TRACE telemetry.");
    let session_url = format!("{public_base_url}/live/{session_id}");
    render_page(
        include_str!("../assets/spectator/index.html"),
        &title,
        &description,
        &session_url,
        public_base_url,
    )
}

async fn spectator_styles() -> impl IntoResponse {
    (
        [("content-type", "text/css; charset=utf-8")],
        include_str!("../assets/spectator/styles.css"),
    )
}

async fn spectator_script() -> impl IntoResponse {
    (
        [("content-type", "text/javascript; charset=utf-8")],
        include_str!("../assets/spectator/app.js"),
    )
}

async fn site_styles() -> impl IntoResponse {
    (
        [
            ("content-type", "text/css; charset=utf-8"),
            ("cache-control", "public, max-age=3600"),
        ],
        include_str!("../assets/site/site.css"),
    )
}

async fn favicon() -> impl IntoResponse {
    (
        [
            ("content-type", "image/svg+xml"),
            ("cache-control", "public, max-age=86400"),
        ],
        include_str!("../assets/site/favicon.svg"),
    )
}

async fn social_card() -> impl IntoResponse {
    (
        [
            ("content-type", "image/png"),
            ("cache-control", "public, max-age=86400"),
        ],
        include_bytes!("../assets/site/trace-live-og.png").as_slice(),
    )
}

async fn not_found(State(service): State<LiveService>) -> impl IntoResponse {
    not_found_response(&service)
}

fn not_found_response(service: &LiveService) -> Response {
    (
        StatusCode::NOT_FOUND,
        Html(site_html(
            include_str!("../assets/site/not-found.html"),
            &service.config.public_base_url,
        )),
    )
        .into_response()
}

fn site_html(template: &str, public_base_url: &str) -> String {
    let base_url = escape_html(public_base_url);
    template.replace("{{TRACE_URL}}", &base_url).replace(
        "{{TRACE_OG_IMAGE}}",
        &format!("{base_url}/live-assets/trace-live-og.png"),
    )
}

fn render_page(
    template: &str,
    title: &str,
    description: &str,
    page_url: &str,
    public_base_url: &str,
) -> String {
    template
        .replace("{{TRACE_TITLE}}", &escape_html(title))
        .replace("{{TRACE_DESCRIPTION}}", &escape_html(description))
        .replace("{{TRACE_URL}}", &escape_html(page_url))
        .replace(
            "{{TRACE_OG_IMAGE}}",
            &format!(
                "{}/live-assets/trace-live-og.png",
                escape_html(public_base_url)
            ),
        )
}

fn format_session_type(value: &str) -> String {
    value
        .split(|character: char| character.is_whitespace() || matches!(character, '_' | '-'))
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            characters.next().map_or_else(String::new, |first| {
                first
                    .to_uppercase()
                    .chain(characters.flat_map(char::to_lowercase))
                    .collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

async fn end_live_session(
    State(service): State<LiveService>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ServiceError> {
    let (installation_id, token) = publisher_credentials(&headers)?;
    service.end_session(&session_id, installation_id, token)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn publisher_websocket(
    websocket: WebSocketUpgrade,
    State(service): State<LiveService>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ServiceError> {
    let (installation_id, token) = publisher_credentials(&headers)?;
    service.authorize_session(&session_id, installation_id, token)?;
    Ok(websocket
        .max_message_size(MAX_WEBSOCKET_MESSAGE_BYTES)
        .on_upgrade(move |socket| receive_publisher(socket, service, session_id)))
}

async fn spectator_websocket(
    websocket: WebSocketUpgrade,
    State(service): State<LiveService>,
    Path(session_id): Path<String>,
) -> Result<Response, ServiceError> {
    let subscription = service.subscribe(&session_id)?;
    Ok(websocket
        .max_message_size(MAX_WEBSOCKET_MESSAGE_BYTES)
        .on_upgrade(move |socket| send_to_spectator(socket, subscription)))
}

async fn receive_publisher(mut socket: WebSocket, service: LiveService, session_id: String) {
    while let Some(Ok(message)) = socket.next().await {
        match message {
            Message::Text(text) => {
                let parsed = serde_json::from_str::<Envelope>(&text)
                    .map_err(|_| ServiceError::InvalidMessage)
                    .and_then(|envelope| service.publish(&session_id, envelope));
                if let Err(error) = parsed {
                    let reason = serde_json::to_string(&ErrorBody {
                        error: error_message(error),
                    })
                    .unwrap_or_else(|_| "{\"error\":\"invalid live protocol message\"}".to_owned());
                    let _ = socket.send(Message::Text(reason.into())).await;
                    if matches!(error, ServiceError::Ended) {
                        break;
                    }
                }
            }
            Message::Close(_) => break,
            Message::Binary(_) => {
                let _ = socket
                    .send(Message::Text(
                        "{\"error\":\"binary messages are not supported by protocol v1\"}".into(),
                    ))
                    .await;
            }
            Message::Ping(_) | Message::Pong(_) => {}
        }
    }
}

async fn send_to_spectator(
    mut socket: WebSocket,
    (snapshot, mut receiver): (Vec<Envelope>, broadcast::Receiver<Envelope>),
) {
    for envelope in snapshot {
        if send_envelope(&mut socket, &envelope).await.is_err() {
            return;
        }
    }

    loop {
        match receiver.recv().await {
            Ok(envelope) => {
                let ended = matches!(envelope.payload, Payload::End(_));
                if send_envelope(&mut socket, &envelope).await.is_err() || ended {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

async fn send_envelope(socket: &mut WebSocket, envelope: &Envelope) -> Result<(), ()> {
    let encoded = serde_json::to_string(envelope).map_err(|_| ())?;
    socket
        .send(Message::Text(encoded.into()))
        .await
        .map_err(|_| ())
}

fn publisher_credentials(headers: &HeaderMap) -> Result<(&str, &str), ServiceError> {
    let installation_id = headers
        .get("x-trace-installation-id")
        .and_then(|value| value.to_str().ok())
        .ok_or(ServiceError::Unauthorized)?;
    let authorization = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .ok_or(ServiceError::Unauthorized)?;
    let token = authorization
        .strip_prefix("Bearer ")
        .filter(|value| !value.is_empty())
        .ok_or(ServiceError::Unauthorized)?;
    Ok((installation_id, token))
}

fn authenticate(
    service: &ServiceState,
    installation_id: &str,
    token: &str,
) -> Result<(), ServiceError> {
    let installation = service
        .installations
        .get(installation_id)
        .ok_or(ServiceError::Unauthorized)?;
    if bool::from(installation.token_hash.ct_eq(&hash_token(token))) {
        Ok(())
    } else {
        Err(ServiceError::Unauthorized)
    }
}

fn opaque_identifier() -> String {
    Uuid::new_v4().simple().to_string()
}

fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn validate_session_state(session_id: &str, state: &SessionState) -> Result<(), ServiceError> {
    Envelope {
        protocol_version: PROTOCOL_VERSION,
        message_id: "session_state".to_owned(),
        session_id: session_id.to_owned(),
        sequence: 0,
        sent_at_unix_ms: 0,
        payload: Payload::SessionState(state.clone()),
    }
    .validate(ProtocolLimits::default())
    .map_err(|_| ServiceError::InvalidMessage)
}

fn push_buffered(session: &mut LiveSession, message: Envelope, capacity: usize) {
    if session.buffer.len() == capacity {
        session.buffer.pop_front();
    }
    session.buffer.push_back(message.clone());
    let _ = session.broadcast.send(message);
}

fn unix_timestamp_ms() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    i64::try_from(millis).unwrap_or(i64::MAX)
}

fn websocket_base_url(public_base_url: &str) -> String {
    if let Some(rest) = public_base_url.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = public_base_url.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        public_base_url.to_owned()
    }
}

fn error_message(error: ServiceError) -> &'static str {
    match error {
        ServiceError::Unauthorized => "publisher authentication failed",
        ServiceError::NotFound => "live session not found",
        ServiceError::InvalidMessage => "invalid live protocol message",
        ServiceError::OutOfOrder => "message sequence is not increasing",
        ServiceError::Ended => "live session has ended",
        ServiceError::Unavailable => "live service state unavailable",
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use futures_util::SinkExt;
    use tokio_tungstenite::{
        MaybeTlsStream, WebSocketStream,
        tungstenite::{
            Message as TestMessage,
            client::IntoClientRequest,
            http::{HeaderValue, header::AUTHORIZATION},
        },
    };
    use tower::ServiceExt;
    use trace_live::{LiveTelemetrySample, encode_recorded_session};
    use trace_protocol::{Hello, PROTOCOL_VERSION};

    fn session_state() -> SessionState {
        SessionState {
            driver_name: Some("3X3".to_owned()),
            simulator: "assetto-corsa".to_owned(),
            simulator_name: Some("Assetto Corsa".to_owned()),
            simulator_mark: Some("AC".to_owned()),
            car: Some("ks_mazda_mx5_cup".to_owned()),
            track: Some("zandvoort".to_owned()),
            layout: None,
            session_type: Some("hotlap".to_owned()),
            status: LiveStatus::Live,
        }
    }

    fn message(session_id: &str, sequence: u64) -> Envelope {
        Envelope {
            protocol_version: PROTOCOL_VERSION,
            message_id: format!("message_{sequence}"),
            session_id: session_id.to_owned(),
            sequence,
            sent_at_unix_ms: 1_700_000_000_000,
            payload: Payload::Hello(Hello {
                publisher_version: "0.4.0".to_owned(),
                source: "recorded-session".to_owned(),
            }),
        }
    }

    #[test]
    fn credentials_create_an_unlisted_session() {
        let service = LiveService::new(ServerConfig::new("https://live.simtrace.run"));
        let credentials = service.bootstrap_installation().expect("credentials");
        let created = service
            .create_session(
                &credentials.installation_id,
                &credentials.publishing_token,
                session_state(),
            )
            .expect("session");

        assert_eq!(created.session_id.len(), 32);
        assert_eq!(
            created.spectator_url,
            format!("https://live.simtrace.run/live/{}", created.session_id)
        );
        assert!(
            created
                .publish_websocket_url
                .starts_with("wss://live.simtrace.run/")
        );
        assert_eq!(
            service
                .summary(&created.session_id)
                .expect("summary")
                .session
                .status,
            LiveStatus::Paused
        );
    }

    #[test]
    fn embedded_spectator_page_contains_the_pit_wall_workspace() {
        let page = spectator_html(
            &session_state(),
            "0123456789abcdef0123456789abcdef",
            "https://live.simtrace.run",
        );
        assert!(page.contains("TRACE <i>//</i> PIT WALL"));
        assert!(page.contains("TRACK MAP"));
        assert!(page.contains("INPUT HISTORY"));
        assert!(page.contains("Spectator timeline"));
        assert!(page.contains("/live-assets/spectator.css"));
        assert!(page.contains("/live-assets/spectator.js"));
        assert!(page.contains("3X3 at zandvoort — TRACE Pit Wall"));
        assert!(page.contains("3X3&#39;s Hotlap"));
        assert!(page.contains("/live-assets/trace-live-og.png"));
        assert!(page.contains("/favicon.svg"));
        assert!(!page.contains("{{TRACE_"));
        let script = include_str!("../assets/spectator/app.js");
        assert!(script.contains("steeringWheel"));
        assert!(script.contains("session.in_pit_lane"));
        assert!(script.contains("playbackTick"));
        assert!(script.contains("motion.position.x"));
        assert!(script.contains("setMapZoom"));
        assert!(script.contains("setPointerCapture"));
        assert!(script.contains("setTraceHeight"));
        assert!(include_str!("../assets/spectator/styles.css").contains(".lap-sectors"));

        let mut unsafe_state = session_state();
        unsafe_state.driver_name = Some("Driver <One> & 'Two'".to_owned());
        let escaped = spectator_html(
            &unsafe_state,
            "0123456789abcdef0123456789abcdef",
            "https://live.simtrace.run",
        );
        assert!(escaped.contains("Driver &lt;One&gt; &amp; &#39;Two&#39;"));
        assert!(!escaped.contains("Driver <One>"));
    }

    #[test]
    fn rejects_wrong_credentials_and_out_of_order_messages() {
        let service = LiveService::new(ServerConfig::new("http://localhost:8080"));
        let credentials = service.bootstrap_installation().expect("credentials");
        assert!(matches!(
            service.create_session(&credentials.installation_id, "wrong", session_state()),
            Err(ServiceError::Unauthorized)
        ));

        let created = service
            .create_session(
                &credentials.installation_id,
                &credentials.publishing_token,
                session_state(),
            )
            .expect("session");
        service
            .publish(&created.session_id, message(&created.session_id, 1))
            .expect("first message");
        service
            .publish(&created.session_id, message(&created.session_id, 1))
            .expect("idempotent retry");
        let mut conflicting = message(&created.session_id, 1);
        conflicting.message_id = "different_message".to_owned();
        assert!(matches!(
            service.publish(&created.session_id, conflicting),
            Err(ServiceError::OutOfOrder)
        ));
    }

    #[test]
    fn retains_only_the_configured_tail_and_fans_out_messages() {
        let service =
            LiveService::new(ServerConfig::new("http://localhost:8080").with_buffer_capacity(2));
        let credentials = service.bootstrap_installation().expect("credentials");
        let created = service
            .create_session(
                &credentials.installation_id,
                &credentials.publishing_token,
                session_state(),
            )
            .expect("session");
        let (_, mut receiver) = service
            .subscribe(&created.session_id)
            .expect("subscription");

        for sequence in 1..=3 {
            service
                .publish(&created.session_id, message(&created.session_id, sequence))
                .expect("publish");
        }
        assert_eq!(receiver.try_recv().expect("broadcast").sequence, 1);
        let summary = service.summary(&created.session_id).expect("summary");
        assert_eq!(summary.oldest_sequence, Some(2));
        assert_eq!(summary.newest_sequence, Some(3));
        let (snapshot, _) = service
            .subscribe(&created.session_id)
            .expect("subscription");
        assert_eq!(
            snapshot
                .iter()
                .map(|item| item.sequence)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );

        service
            .end_session(
                &created.session_id,
                &credentials.installation_id,
                &credentials.publishing_token,
            )
            .expect("end session");
        let (_, mut ended_receiver) = service
            .subscribe(&created.session_id)
            .expect("ended subscription");
        service
            .end_session(
                &created.session_id,
                &credentials.installation_id,
                &credentials.publishing_token,
            )
            .expect("idempotent end");
        assert!(matches!(
            service
                .summary(&created.session_id)
                .expect("summary")
                .session
                .status,
            LiveStatus::Ended
        ));
        assert!(ended_receiver.try_recv().is_err());
        let (ended_snapshot, _) = service
            .subscribe(&created.session_id)
            .expect("ended snapshot");
        assert!(matches!(
            ended_snapshot.last().map(|envelope| &envelope.payload),
            Some(Payload::End(_))
        ));
    }

    #[tokio::test]
    async fn http_api_bootstraps_credentials_and_creates_a_session() {
        let application = app(ServerConfig::new("https://live.simtrace.run"));
        for (path, content_type) in [
            ("/live-assets/spectator.css", "text/css; charset=utf-8"),
            (
                "/live-assets/spectator.js",
                "text/javascript; charset=utf-8",
            ),
            ("/live-assets/site.css", "text/css; charset=utf-8"),
            ("/favicon.svg", "image/svg+xml"),
            ("/live-assets/trace-live-og.png", "image/png"),
        ] {
            let response = application
                .clone()
                .oneshot(
                    Request::get(path)
                        .body(Body::empty())
                        .expect("asset request"),
                )
                .await
                .expect("asset response");
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()["content-type"], content_type);
        }
        let response = application
            .clone()
            .oneshot(Request::get("/").body(Body::empty()).expect("home request"))
            .await
            .expect("home response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 128 * 1024)
            .await
            .expect("home body");
        let body = String::from_utf8(body.to_vec()).expect("home HTML");
        assert!(body.contains("The pit wall,"));
        assert!(body.contains("TRACE Go Live — The pit wall, in your browser"));
        assert!(body.contains("https://live.simtrace.run/live-assets/trace-live-og.png"));

        for path in ["/missing", "/live/missing"] {
            let response = application
                .clone()
                .oneshot(
                    Request::get(path)
                        .body(Body::empty())
                        .expect("missing request"),
                )
                .await
                .expect("missing response");
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            let body = to_bytes(response.into_body(), 64 * 1024)
                .await
                .expect("missing body");
            assert!(String::from_utf8_lossy(&body).contains("This lap has ended."));
        }
        let response = application
            .clone()
            .oneshot(
                Request::post("/api/v1/installations")
                    .body(Body::empty())
                    .expect("bootstrap request"),
            )
            .await
            .expect("bootstrap response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("credential body");
        let credentials: InstallationCredentials =
            serde_json::from_slice(&body).expect("credentials JSON");

        let body = serde_json::to_vec(&CreateLiveSessionRequest {
            session: session_state(),
        })
        .expect("session request JSON");
        let response = application
            .oneshot(
                Request::post("/api/v1/live-sessions")
                    .header("content-type", "application/json")
                    .header("x-trace-installation-id", &credentials.installation_id)
                    .header(
                        "authorization",
                        format!("Bearer {}", credentials.publishing_token),
                    )
                    .body(Body::from(body))
                    .expect("create request"),
            )
            .await
            .expect("create response");
        assert_eq!(response.status(), StatusCode::CREATED);
        let body = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("session body");
        let created: CreateLiveSessionResponse =
            serde_json::from_slice(&body).expect("session JSON");
        assert!(created.spectator_url.ends_with(&created.session_id));
    }

    #[tokio::test]
    async fn publisher_reconnects_without_duplicating_the_ambiguous_message() {
        let server = start_local_server(ServerConfig::new("http://127.0.0.1:0"), None)
            .await
            .expect("local server");
        let client = reqwest::Client::new();
        let credentials = client
            .post(format!("{}/api/v1/installations", server.base_url()))
            .send()
            .await
            .expect("bootstrap request")
            .json::<InstallationCredentials>()
            .await
            .expect("credentials");
        let created = client
            .post(format!("{}/api/v1/live-sessions", server.base_url()))
            .header("x-trace-installation-id", &credentials.installation_id)
            .bearer_auth(&credentials.publishing_token)
            .json(&CreateLiveSessionRequest {
                session: session_state(),
            })
            .send()
            .await
            .expect("create request")
            .json::<CreateLiveSessionResponse>()
            .await
            .expect("created session");

        let spectator_url = created
            .publish_websocket_url
            .replace("/publish", "/spectate");
        let (mut spectator, _) = tokio_tungstenite::connect_async(&spectator_url)
            .await
            .expect("spectator websocket");
        let mut publisher = connect_test_publisher(&created, &credentials).await;
        let first = message(&created.session_id, 0);
        publisher
            .send(TestMessage::Text(
                serde_json::to_string(&first).expect("first JSON").into(),
            ))
            .await
            .expect("first publish");
        assert_eq!(next_test_envelope(&mut spectator).await.sequence, 0);
        publisher.close(None).await.expect("disconnect publisher");

        let mut reconnected = connect_test_publisher(&created, &credentials).await;
        for envelope in [first, message(&created.session_id, 1)] {
            reconnected
                .send(TestMessage::Text(
                    serde_json::to_string(&envelope)
                        .expect("reconnected JSON")
                        .into(),
                ))
                .await
                .expect("reconnected publish");
        }
        assert_eq!(next_test_envelope(&mut spectator).await.sequence, 1);
        server.shutdown().await;
    }

    async fn connect_test_publisher(
        created: &CreateLiveSessionResponse,
        credentials: &InstallationCredentials,
    ) -> WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>> {
        let mut request = created
            .publish_websocket_url
            .as_str()
            .into_client_request()
            .expect("publisher request");
        request.headers_mut().insert(
            "x-trace-installation-id",
            HeaderValue::from_str(&credentials.installation_id).expect("installation header"),
        );
        request.headers_mut().insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", credentials.publishing_token))
                .expect("authorization header"),
        );
        tokio_tungstenite::connect_async(request)
            .await
            .expect("publisher websocket")
            .0
    }

    async fn next_test_envelope(
        socket: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    ) -> Envelope {
        let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("spectator timeout")
            .expect("spectator closed")
            .expect("spectator message");
        let TestMessage::Text(text) = message else {
            panic!("expected text telemetry");
        };
        serde_json::from_str(&text).expect("telemetry envelope")
    }

    #[test]
    fn accepts_a_complete_recorded_replay_stream() {
        let service = LiveService::new(ServerConfig::new("http://localhost:8080"));
        let credentials = service.bootstrap_installation().expect("credentials");
        let created = service
            .create_session(
                &credentials.installation_id,
                &credentials.publishing_token,
                session_state(),
            )
            .expect("session");
        let samples = [
            LiveTelemetrySample {
                elapsed_ns: 1_000_000_000,
                speed_mps: Some(20.0),
                ..LiveTelemetrySample::default()
            },
            LiveTelemetrySample {
                elapsed_ns: 1_050_000_000,
                speed_mps: Some(21.0),
                ..LiveTelemetrySample::default()
            },
        ];
        let messages = encode_recorded_session(
            &created.session_id,
            session_state(),
            &samples,
            1_700_000_000_000,
        )
        .expect("encoded replay");
        for message in messages {
            service
                .publish(&created.session_id, message.envelope)
                .expect("accepted replay message");
        }

        let summary = service.summary(&created.session_id).expect("summary");
        assert_eq!(summary.session.status, LiveStatus::Ended);
        assert_eq!(summary.oldest_sequence, Some(0));
        assert_eq!(summary.newest_sequence, Some(4));
    }
}
