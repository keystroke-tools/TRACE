//! Account-free live telemetry service for TRACE.
//!
//! The server accepts only versioned [`trace_protocol`] envelopes. Simulator-specific
//! data stays at the desktop adapter boundary.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    convert::Infallible,
    fs,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{
        ConnectInfo, DefaultBodyLimit, FromRequestParts, Path, Query, State, WebSocketUpgrade,
        ws::{CloseFrame, Message, WebSocket, close_code},
    },
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::sync::broadcast;
use tokio::sync::oneshot;
use trace_protocol::{
    CreateLiveSessionRequest, CreateLiveSessionResponse, Envelope, InstallationCredentials,
    LiveSessionSummary, LiveStatus, PROTOCOL_VERSION, Payload, ProtocolLimits,
    RotatePublishingTokenResponse, SessionEnd, SessionState, SharedSessionSummary,
};
use uuid::Uuid;

mod accounts;
mod library;
mod live_store;
mod shares;

use accounts::{
    AccountStore, AccountStoreError, AccountSummary, AccountTokenSummary, CreatedAccountToken,
    StoredInstanceSettings,
};
use library::{
    LibraryCleanupSummary, LibraryLapTelemetry, LibrarySessionDetail, LibrarySessionSummary,
    LibraryStore, LibraryStoreError, StoredLibrarySession,
};
use live_store::LiveStore;
use shares::{ShareStore, ShareStoreError};

const DEFAULT_BUFFER_CAPACITY: usize = 12_000;
const DEFAULT_LIVE_RETENTION: Duration = Duration::from_secs(10 * 60);
const BROADCAST_CAPACITY: usize = 512;
const MAX_WEBSOCKET_MESSAGE_BYTES: usize = 512 * 1024;
const DEFAULT_MAX_SHARED_SESSION_BYTES: u64 = 128 * 1024 * 1024;
const DEFAULT_SHARED_SESSION_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const MAX_ACCOUNT_REQUEST_BYTES: usize = 8 * 1024;
const DEFAULT_LIBRARY_TELEMETRY_POINTS: usize = 1_800;
const MAX_LIBRARY_TELEMETRY_POINTS: usize = 4_000;

/// Configurable service-abuse limits. Spectator access remains unauthenticated.
#[derive(Clone, Debug)]
pub struct RateLimits {
    pub bootstrap_per_minute: u64,
    pub session_creations_per_minute: u64,
    pub publisher_upgrades_per_minute: u64,
    pub publisher_messages_per_second: u64,
    pub publisher_bytes_per_second: u64,
    pub live_sessions_per_installation: usize,
    pub spectators_per_session: usize,
    pub spectators_per_address: usize,
    pub installation_records: usize,
    pub live_session_records: usize,
}

impl Default for RateLimits {
    fn default() -> Self {
        Self {
            bootstrap_per_minute: 30,
            session_creations_per_minute: 20,
            publisher_upgrades_per_minute: 60,
            publisher_messages_per_second: 120,
            publisher_bytes_per_second: 8 * 1024 * 1024,
            live_sessions_per_installation: 4,
            spectators_per_session: 64,
            spectators_per_address: 16,
            installation_records: 100_000,
            live_session_records: 10_000,
        }
    }
}

/// Runtime settings supplied by the deployment environment.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    instance_name: String,
    public_base_url: String,
    registration_policy: RegistrationPolicy,
    buffer_capacity: usize,
    live_retention: Duration,
    share_directory: Option<PathBuf>,
    shared_session_ttl: Duration,
    max_shared_session_bytes: u64,
    rate_limits: RateLimits,
    trust_proxy_headers: bool,
}

impl ServerConfig {
    /// Creates settings for the public service URL.
    #[must_use]
    pub fn new(public_base_url: impl Into<String>) -> Self {
        Self {
            instance_name: "TRACE".to_owned(),
            public_base_url: public_base_url.into().trim_end_matches('/').to_owned(),
            registration_policy: RegistrationPolicy::Disabled,
            buffer_capacity: DEFAULT_BUFFER_CAPACITY,
            live_retention: DEFAULT_LIVE_RETENTION,
            share_directory: None,
            shared_session_ttl: DEFAULT_SHARED_SESSION_TTL,
            max_shared_session_bytes: DEFAULT_MAX_SHARED_SESSION_BYTES,
            rate_limits: RateLimits::default(),
            trust_proxy_headers: false,
        }
    }

    /// Changes the human-readable name advertised by this deployment.
    #[must_use]
    pub fn with_instance_name(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        let name = name.trim();
        if !name.is_empty() {
            self.instance_name = name.chars().take(80).collect();
        }
        self
    }

    /// Changes whether this instance accepts account registrations.
    #[must_use]
    pub fn with_registration_policy(mut self, policy: RegistrationPolicy) -> Self {
        self.registration_policy = policy;
        self
    }

    /// Enables durable completed-session sharing beneath this directory.
    #[must_use]
    pub fn with_share_directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.share_directory = Some(directory.into());
        self
    }

    /// Changes how long completed session packages remain downloadable.
    #[must_use]
    pub fn with_shared_session_ttl(mut self, ttl: Duration) -> Self {
        self.shared_session_ttl = ttl;
        self
    }

    /// Changes how much recent live telemetry survives for playback and restart recovery.
    #[must_use]
    pub fn with_live_retention(mut self, retention: Duration) -> Self {
        self.live_retention = retention;
        self
    }

    /// Replaces the public-service abuse limits.
    #[must_use]
    pub fn with_rate_limits(mut self, limits: RateLimits) -> Self {
        self.rate_limits = limits;
        self
    }

    /// Allows a trusted reverse proxy to supply the leftmost `X-Forwarded-For` address.
    #[must_use]
    pub fn with_trusted_proxy_headers(mut self, trust: bool) -> Self {
        self.trust_proxy_headers = trust;
        self
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
    shares: Option<ShareStore>,
    accounts: Option<AccountStore>,
    library: Option<LibraryStore>,
    live_store: Option<LiveStore>,
    abuse: Arc<Mutex<AbuseState>>,
}

#[derive(Default)]
struct AbuseState {
    windows: HashMap<String, WindowUsage>,
    spectators_by_session: HashMap<String, usize>,
    spectators_by_address: HashMap<String, usize>,
}

struct WindowUsage {
    started: Instant,
    used: u64,
}

struct PeerAddress(String);

impl<S> FromRequestParts<S> for PeerAddress
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map_or_else(|| "unknown".to_owned(), |peer| peer.0.ip().to_string()),
        ))
    }
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
        let _ = axum::serve(
            listener,
            app(config).into_make_service_with_connect_info::<SocketAddr>(),
        )
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
    publisher_token_hash: Option<[u8; 32]>,
    state: SessionState,
    last_sequence: Option<u64>,
    updated_at_unix_ms: i64,
    buffer: VecDeque<BufferedEnvelope>,
    broadcast: broadcast::Sender<Envelope>,
}

struct BufferedEnvelope {
    envelope: Envelope,
    received_at_unix_ms: i64,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: &'static str,
}

#[derive(Debug, Serialize)]
struct InstanceDescription {
    name: String,
    version: &'static str,
    public_base_url: String,
    registration_policy: RegistrationPolicy,
    initial_admin_required: bool,
    registration_enabled: bool,
    features: Vec<&'static str>,
    limits: InstanceLimits,
}

/// Controls account creation independently for each deployment.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationPolicy {
    Disabled,
    Open,
}

#[derive(Debug, Deserialize)]
struct CreateAccountRequest {
    username: String,
    display_name: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct InstanceLimits {
    maximum_upload_bytes: u64,
    shared_session_ttl_seconds: u64,
    live_retention_seconds: u64,
}

#[derive(Clone, Copy, Debug)]
enum ServiceError {
    Unauthorized,
    NotFound,
    InvalidMessage,
    OutOfOrder,
    Ended,
    Unavailable,
    SharingUnavailable,
    InvalidPackage,
    PackageTooLarge,
    PublisherCredentialRequired,
    RateLimited,
    CapacityExceeded,
    AccountsUnavailable,
    RegistrationDisabled,
    InvalidAccount,
    UsernameTaken,
    AccountUnauthorized,
    LibraryUnavailable,
    LibrarySessionNotFound,
    InvalidLibraryMetadata,
    InvalidAdminSettings,
    AdministratorRequired,
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
            Self::SharingUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "completed-session sharing is unavailable",
            ),
            Self::InvalidPackage => (StatusCode::BAD_REQUEST, "invalid TRACE session package"),
            Self::PackageTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "TRACE session package is too large",
            ),
            Self::PublisherCredentialRequired => (
                StatusCode::UPGRADE_REQUIRED,
                "live session requires a session-scoped publishing credential",
            ),
            Self::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "request rate limit exceeded"),
            Self::CapacityExceeded => (StatusCode::TOO_MANY_REQUESTS, "connection limit exceeded"),
            Self::AccountsUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "account service is unavailable",
            ),
            Self::RegistrationDisabled => (
                StatusCode::FORBIDDEN,
                "account registration is disabled on this instance",
            ),
            Self::InvalidAccount => (StatusCode::BAD_REQUEST, "invalid account details"),
            Self::UsernameTaken => (StatusCode::CONFLICT, "username is already registered"),
            Self::AccountUnauthorized => (StatusCode::UNAUTHORIZED, "authentication required"),
            Self::LibraryUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "web session library is unavailable",
            ),
            Self::LibrarySessionNotFound => (StatusCode::NOT_FOUND, "library session not found"),
            Self::InvalidLibraryMetadata => (StatusCode::BAD_REQUEST, "invalid session metadata"),
            Self::InvalidAdminSettings => (StatusCode::BAD_REQUEST, "invalid instance settings"),
            Self::AdministratorRequired => {
                (StatusCode::FORBIDDEN, "administrator access is required")
            }
        };
        let mut response = (status, Json(ErrorBody { error })).into_response();
        if matches!(self, Self::RateLimited | Self::CapacityExceeded) {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
        }
        response
    }
}

impl LiveService {
    /// Creates an empty service state.
    ///
    /// # Panics
    ///
    /// Panics when durable storage is configured but its `SQLite` database cannot be
    /// opened or contains invalid live-session data. The standalone service treats
    /// this as a fatal startup error instead of silently losing persistence.
    #[must_use]
    pub fn new(config: ServerConfig) -> Self {
        let accounts = config.share_directory.as_ref().map(|directory| {
            AccountStore::open(directory).expect("TRACE account persistence could not be opened")
        });
        let library = config.share_directory.as_ref().map(|directory| {
            LibraryStore::open(directory, config.max_shared_session_bytes)
                .expect("TRACE web library persistence could not be opened")
        });
        if let (Some(accounts), Some(library)) = (&accounts, &library) {
            let configured = matches!(config.registration_policy, RegistrationPolicy::Open);
            let settings = accounts
                .instance_settings(configured)
                .expect("TRACE instance settings could not be loaded");
            library
                .enforce_retention(
                    None,
                    unix_timestamp_ms(),
                    settings.library_retention_days,
                    settings.library_max_sessions_per_account,
                    None,
                )
                .expect("TRACE library retention could not be enforced");
        }
        let shares = config.share_directory.as_ref().map(|directory| {
            ShareStore::new(
                directory.join("shares"),
                config.public_base_url.clone(),
                config.shared_session_ttl,
                config.max_shared_session_bytes,
            )
        });
        let live_store = config.share_directory.as_ref().map(|directory| {
            LiveStore::open(directory, config.live_retention)
                .expect("TRACE live persistence could not be opened")
        });
        let mut state = ServiceState::default();
        if let Some(store) = &live_store {
            let (installations, sessions) = store
                .load(unix_timestamp_ms())
                .expect("TRACE live persistence could not be loaded");
            state.installations = installations
                .into_iter()
                .map(|installation| {
                    (
                        installation.installation_id,
                        Installation {
                            token_hash: installation.token_hash,
                        },
                    )
                })
                .collect();
            state.sessions = sessions
                .into_iter()
                .map(|session| {
                    let (broadcast, _) = broadcast::channel(BROADCAST_CAPACITY);
                    let mut buffer = session
                        .messages
                        .into_iter()
                        .map(|(envelope, received_at_unix_ms)| BufferedEnvelope {
                            envelope,
                            received_at_unix_ms,
                        })
                        .collect::<VecDeque<_>>();
                    while buffer.len() > config.buffer_capacity {
                        buffer.pop_front();
                    }
                    (
                        session.session_id,
                        LiveSession {
                            owner_id: session.owner_id,
                            publisher_token_hash: session.publisher_token_hash,
                            state: session.state,
                            last_sequence: session.last_sequence,
                            updated_at_unix_ms: session.updated_at_unix_ms,
                            buffer,
                            broadcast,
                        },
                    )
                })
                .collect();
        }
        Self {
            inner: Arc::new(RwLock::new(state)),
            config,
            shares,
            accounts,
            library,
            live_store,
            abuse: Arc::new(Mutex::new(AbuseState::default())),
        }
    }

    fn check_rate(
        &self,
        scope: &str,
        subject: &str,
        cost: u64,
        limit: u64,
        window: Duration,
    ) -> Result<(), ServiceError> {
        let now = Instant::now();
        let mut abuse = self.abuse.lock().map_err(|_| ServiceError::Unavailable)?;
        let key = format!("{scope}:{subject}");
        let usage = abuse.windows.entry(key).or_insert(WindowUsage {
            started: now,
            used: 0,
        });
        if now.duration_since(usage.started) >= window {
            usage.started = now;
            usage.used = 0;
        }
        if cost > limit.saturating_sub(usage.used) {
            return Err(ServiceError::RateLimited);
        }
        usage.used = usage.used.saturating_add(cost);
        if abuse.windows.len() > 10_000 {
            abuse
                .windows
                .retain(|_, usage| now.duration_since(usage.started) < Duration::from_secs(120));
        }
        Ok(())
    }

    fn authorize_installation(
        &self,
        installation_id: &str,
        token: &str,
    ) -> Result<(), ServiceError> {
        let service = self.inner.read().map_err(|_| ServiceError::Unavailable)?;
        authenticate(&service, installation_id, token)
    }

    fn bootstrap_installation(&self) -> Result<InstallationCredentials, ServiceError> {
        let mut service = self.inner.write().map_err(|_| ServiceError::Unavailable)?;
        self.reserve_installation_capacity(&mut service)?;
        let installation_id = opaque_identifier();
        let publishing_token = format!("{}{}", opaque_identifier(), opaque_identifier());
        let installation = Installation {
            token_hash: hash_token(&publishing_token),
        };
        if let Some(store) = &self.live_store {
            store
                .save_installation(
                    &installation_id,
                    &installation.token_hash,
                    unix_timestamp_ms(),
                )
                .map_err(|_| ServiceError::Unavailable)?;
        }
        service
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
        self.prune_expired_sessions(&mut service, unix_timestamp_ms())?;
        if service.sessions.len() >= self.config.rate_limits.live_session_records {
            return Err(ServiceError::CapacityExceeded);
        }
        let active_sessions = service
            .sessions
            .values()
            .filter(|session| {
                session.owner_id == installation_id && session.state.status != LiveStatus::Ended
            })
            .count();
        if active_sessions >= self.config.rate_limits.live_sessions_per_installation {
            return Err(ServiceError::CapacityExceeded);
        }
        let session_id = opaque_identifier();
        let publishing_token = format!("{}{}", opaque_identifier(), opaque_identifier());
        let publisher_token_hash = hash_token(&publishing_token);
        state.status = LiveStatus::Paused;
        validate_session_state(&session_id, &state)?;
        let (broadcast, _) = broadcast::channel(BROADCAST_CAPACITY);
        let now_unix_ms = unix_timestamp_ms();
        if let Some(store) = &self.live_store {
            store
                .save_session(
                    &session_id,
                    installation_id,
                    &state,
                    &publisher_token_hash,
                    now_unix_ms,
                )
                .map_err(|_| ServiceError::Unavailable)?;
        }
        service.sessions.insert(
            session_id.clone(),
            LiveSession {
                owner_id: installation_id.to_owned(),
                publisher_token_hash: Some(publisher_token_hash),
                state,
                last_sequence: None,
                updated_at_unix_ms: now_unix_ms,
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
            publishing_token,
        })
    }

    fn summary(&self, session_id: &str) -> Result<LiveSessionSummary, ServiceError> {
        let mut service = self.inner.write().map_err(|_| ServiceError::Unavailable)?;
        self.expire_session(&mut service, session_id, unix_timestamp_ms())?;
        let live = service
            .sessions
            .get(session_id)
            .ok_or(ServiceError::NotFound)?;
        Ok(LiveSessionSummary {
            session_id: session_id.to_owned(),
            session: live.state.clone(),
            oldest_sequence: live.buffer.front().map(|message| message.envelope.sequence),
            newest_sequence: live.buffer.back().map(|message| message.envelope.sequence),
        })
    }

    fn authorize_session(
        &self,
        session_id: &str,
        installation_id: &str,
        token: &str,
    ) -> Result<(), ServiceError> {
        let mut service = self.inner.write().map_err(|_| ServiceError::Unavailable)?;
        authenticate(&service, installation_id, token)?;
        self.expire_session(&mut service, session_id, unix_timestamp_ms())?;
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

    fn authorize_publisher(
        &self,
        session_id: &str,
        publishing_token: &str,
    ) -> Result<(), ServiceError> {
        let mut service = self.inner.write().map_err(|_| ServiceError::Unavailable)?;
        self.expire_session(&mut service, session_id, unix_timestamp_ms())?;
        let session = service
            .sessions
            .get(session_id)
            .ok_or(ServiceError::NotFound)?;
        authenticate_publisher(session, publishing_token)
    }

    fn rotate_publisher_token(
        &self,
        session_id: &str,
        installation_id: &str,
        installation_token: &str,
    ) -> Result<RotatePublishingTokenResponse, ServiceError> {
        self.authorize_session(session_id, installation_id, installation_token)?;
        let mut service = self.inner.write().map_err(|_| ServiceError::Unavailable)?;
        let session = service
            .sessions
            .get_mut(session_id)
            .ok_or(ServiceError::NotFound)?;
        if session.state.status == LiveStatus::Ended {
            return Err(ServiceError::Ended);
        }
        let publishing_token = format!("{}{}", opaque_identifier(), opaque_identifier());
        let publisher_token_hash = hash_token(&publishing_token);
        if let Some(store) = &self.live_store {
            store
                .set_publisher_token_hash(session_id, Some(&publisher_token_hash))
                .map_err(|_| ServiceError::Unavailable)?;
        }
        session.publisher_token_hash = Some(publisher_token_hash);
        Ok(RotatePublishingTokenResponse { publishing_token })
    }

    fn revoke_publisher_token(
        &self,
        session_id: &str,
        installation_id: &str,
        installation_token: &str,
    ) -> Result<(), ServiceError> {
        self.authorize_session(session_id, installation_id, installation_token)?;
        let mut service = self.inner.write().map_err(|_| ServiceError::Unavailable)?;
        let session = service
            .sessions
            .get_mut(session_id)
            .ok_or(ServiceError::NotFound)?;
        if let Some(store) = &self.live_store {
            store
                .set_publisher_token_hash(session_id, None)
                .map_err(|_| ServiceError::Unavailable)?;
        }
        session.publisher_token_hash = None;
        Ok(())
    }

    fn publish(
        &self,
        session_id: &str,
        publishing_token: &str,
        message: Envelope,
    ) -> Result<(), ServiceError> {
        message
            .validate(ProtocolLimits::default())
            .map_err(|_| ServiceError::InvalidMessage)?;
        if message.session_id != session_id {
            return Err(ServiceError::InvalidMessage);
        }

        let mut service = self.inner.write().map_err(|_| ServiceError::Unavailable)?;
        self.expire_session(&mut service, session_id, unix_timestamp_ms())?;
        let session = service
            .sessions
            .get_mut(session_id)
            .ok_or(ServiceError::NotFound)?;
        authenticate_publisher(session, publishing_token)?;
        if session.state.status == LiveStatus::Ended {
            return Err(ServiceError::Ended);
        }
        if let Some(sequence) = session.last_sequence {
            if message.sequence == sequence
                && session
                    .buffer
                    .back()
                    .is_some_and(|accepted| accepted.envelope.message_id == message.message_id)
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
        let received_at_unix_ms = unix_timestamp_ms();
        let ended = session.state.status == LiveStatus::Ended;
        if let Some(store) = &self.live_store {
            store
                .save_message(
                    session_id,
                    &session.state,
                    &message,
                    received_at_unix_ms,
                    ended,
                )
                .map_err(|_| ServiceError::Unavailable)?;
        }
        session.last_sequence = Some(message.sequence);
        session.updated_at_unix_ms = received_at_unix_ms;
        if ended {
            session.publisher_token_hash = None;
        }
        push_buffered(
            session,
            message,
            received_at_unix_ms,
            self.config.buffer_capacity,
            self.config.live_retention,
        );
        Ok(())
    }

    fn subscribe(
        &self,
        session_id: &str,
    ) -> Result<(Vec<Envelope>, broadcast::Receiver<Envelope>), ServiceError> {
        let mut service = self.inner.write().map_err(|_| ServiceError::Unavailable)?;
        self.expire_session(&mut service, session_id, unix_timestamp_ms())?;
        let session = service
            .sessions
            .get(session_id)
            .ok_or(ServiceError::NotFound)?;
        Ok((
            session
                .buffer
                .iter()
                .map(|message| message.envelope.clone())
                .collect(),
            session.broadcast.subscribe(),
        ))
    }

    fn acquire_spectator(
        &self,
        session_id: &str,
        source: &str,
    ) -> Result<SpectatorPermit, ServiceError> {
        let mut abuse = self.abuse.lock().map_err(|_| ServiceError::Unavailable)?;
        let session_count = abuse
            .spectators_by_session
            .get(session_id)
            .copied()
            .unwrap_or_default();
        let address_count = abuse
            .spectators_by_address
            .get(source)
            .copied()
            .unwrap_or_default();
        if session_count >= self.config.rate_limits.spectators_per_session
            || address_count >= self.config.rate_limits.spectators_per_address
        {
            return Err(ServiceError::CapacityExceeded);
        }
        *abuse
            .spectators_by_session
            .entry(session_id.to_owned())
            .or_default() += 1;
        *abuse
            .spectators_by_address
            .entry(source.to_owned())
            .or_default() += 1;
        Ok(SpectatorPermit {
            abuse: Arc::clone(&self.abuse),
            session_id: session_id.to_owned(),
            source: source.to_owned(),
        })
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
        let received_at_unix_ms = unix_timestamp_ms();
        if let Some(store) = &self.live_store {
            store
                .save_message(
                    session_id,
                    &session.state,
                    &message,
                    received_at_unix_ms,
                    true,
                )
                .map_err(|_| ServiceError::Unavailable)?;
        }
        session.last_sequence = Some(sequence);
        session.updated_at_unix_ms = received_at_unix_ms;
        session.publisher_token_hash = None;
        push_buffered(
            session,
            message,
            received_at_unix_ms,
            self.config.buffer_capacity,
            self.config.live_retention,
        );
        Ok(())
    }

    fn expire_session(
        &self,
        service: &mut ServiceState,
        session_id: &str,
        now_unix_ms: i64,
    ) -> Result<(), ServiceError> {
        let cutoff = cutoff_unix_ms(now_unix_ms, self.config.live_retention);
        let expired = service
            .sessions
            .get(session_id)
            .is_some_and(|session| session.updated_at_unix_ms < cutoff);
        if expired {
            if let Some(store) = &self.live_store {
                store
                    .delete_session(session_id)
                    .map_err(|_| ServiceError::Unavailable)?;
            }
            service.sessions.remove(session_id);
        }
        Ok(())
    }

    fn reserve_installation_capacity(
        &self,
        service: &mut ServiceState,
    ) -> Result<(), ServiceError> {
        let limit = self.config.rate_limits.installation_records;
        if service.installations.len() < limit {
            return Ok(());
        }
        let owners = service
            .sessions
            .values()
            .map(|session| session.owner_id.as_str())
            .collect::<HashSet<_>>();
        let remove_count = service
            .installations
            .len()
            .saturating_add(1)
            .saturating_sub(limit);
        let removable = service
            .installations
            .keys()
            .filter(|installation_id| !owners.contains(installation_id.as_str()))
            .take(remove_count)
            .cloned()
            .collect::<Vec<_>>();
        if removable.len() < remove_count {
            return Err(ServiceError::CapacityExceeded);
        }
        for installation_id in removable {
            if let Some(store) = &self.live_store {
                store
                    .delete_installation(&installation_id)
                    .map_err(|_| ServiceError::Unavailable)?;
            }
            service.installations.remove(&installation_id);
        }
        Ok(())
    }

    fn prune_expired_sessions(
        &self,
        service: &mut ServiceState,
        now_unix_ms: i64,
    ) -> Result<(), ServiceError> {
        let cutoff = cutoff_unix_ms(now_unix_ms, self.config.live_retention);
        let expired = service
            .sessions
            .iter()
            .filter(|(_, session)| session.updated_at_unix_ms < cutoff)
            .map(|(session_id, _)| session_id.clone())
            .collect::<Vec<_>>();
        for session_id in expired {
            if let Some(store) = &self.live_store {
                store
                    .delete_session(&session_id)
                    .map_err(|_| ServiceError::Unavailable)?;
            }
            service.sessions.remove(&session_id);
        }
        Ok(())
    }
}

struct SpectatorPermit {
    abuse: Arc<Mutex<AbuseState>>,
    session_id: String,
    source: String,
}

impl Drop for SpectatorPermit {
    fn drop(&mut self) {
        let Ok(mut abuse) = self.abuse.lock() else {
            return;
        };
        decrement_counter(&mut abuse.spectators_by_session, &self.session_id);
        decrement_counter(&mut abuse.spectators_by_address, &self.source);
    }
}

fn decrement_counter(counters: &mut HashMap<String, usize>, key: &str) {
    if let Some(count) = counters.get_mut(key) {
        *count = count.saturating_sub(1);
        if *count == 0 {
            counters.remove(key);
        }
    }
}

/// Builds the HTTP and WebSocket application.
pub fn app(config: ServerConfig) -> Router {
    let maximum_upload_bytes =
        usize::try_from(config.max_shared_session_bytes).unwrap_or(usize::MAX);
    let state = LiveService::new(config);
    Router::new()
        .route("/health", get(health))
        .route("/", get(landing_page))
        .route("/favicon.svg", get(favicon))
        .route("/api/v1/instance", get(instance_description))
        .route(
            "/api/v1/accounts",
            post(create_account).layer(DefaultBodyLimit::max(MAX_ACCOUNT_REQUEST_BYTES)),
        )
        .route(
            "/api/v1/auth/sessions",
            post(login).layer(DefaultBodyLimit::max(MAX_ACCOUNT_REQUEST_BYTES)),
        )
        .route("/api/v1/auth/session", get(current_account).delete(logout))
        .route(
            "/api/v1/admin/settings",
            get(admin_settings)
                .put(set_admin_settings)
                .layer(DefaultBodyLimit::max(MAX_ACCOUNT_REQUEST_BYTES)),
        )
        .route(
            "/api/v1/admin/library-retention-preview",
            post(preview_library_retention).layer(DefaultBodyLimit::max(MAX_ACCOUNT_REQUEST_BYTES)),
        )
        .route(
            "/api/v1/auth/tokens",
            get(list_account_tokens)
                .post(create_account_token)
                .layer(DefaultBodyLimit::max(MAX_ACCOUNT_REQUEST_BYTES)),
        )
        .route(
            "/api/v1/auth/tokens/{token_id}",
            axum::routing::delete(revoke_account_token),
        )
        .route(
            "/api/v1/library/sessions",
            get(list_library_sessions).post(upload_library_session),
        )
        .route(
            "/api/v1/library/sessions/{session_id}",
            get(get_library_session)
                .put(update_library_session)
                .delete(delete_library_session)
                .layer(DefaultBodyLimit::max(MAX_ACCOUNT_REQUEST_BYTES)),
        )
        .route(
            "/api/v1/library/sessions/{session_id}/download",
            get(download_library_session),
        )
        .route(
            "/api/v1/library/sessions/{session_id}/review",
            get(review_library_session),
        )
        .route(
            "/api/v1/library/sessions/{session_id}/laps/{lap_id}/telemetry",
            get(get_library_lap_telemetry),
        )
        .route(
            "/api/v1/library/sessions/{session_id}/share",
            post(share_library_session).delete(revoke_library_share),
        )
        .route(
            "/api/v1/public-sessions/{share_id}",
            get(get_public_library_session),
        )
        .route(
            "/api/v1/public-sessions/{share_id}/laps/{lap_id}/telemetry",
            get(get_public_library_lap_telemetry),
        )
        .route(
            "/api/v1/public-sessions/{share_id}/download",
            get(download_public_library_session),
        )
        .route("/api/v1/installations", post(bootstrap_installation))
        .route("/api/v1/live-sessions", post(create_live_session))
        .route("/api/v1/shared-sessions", post(create_shared_session))
        .route(
            "/api/v1/shared-sessions/{share_id}",
            get(get_shared_session),
        )
        .route(
            "/api/v1/shared-sessions/{share_id}/download",
            get(download_shared_session),
        )
        .route(
            "/api/v1/live-sessions/{session_id}",
            get(get_live_session).delete(end_live_session),
        )
        .route(
            "/api/v1/live-sessions/{session_id}/publish",
            get(publisher_websocket),
        )
        .route(
            "/api/v1/live-sessions/{session_id}/publisher-credential",
            post(rotate_publisher_credential).delete(revoke_publisher_credential),
        )
        .route(
            "/api/v1/live-sessions/{session_id}/spectate",
            get(spectator_websocket),
        )
        .route("/live/{session_id}", get(spectator_page))
        .route("/s/{share_id}", get(shared_session_page))
        .route("/app", get(library_page))
        .route("/app/sessions/{session_ref}", get(library_page))
        .route("/p/{share_id}", get(library_page))
        .route("/shared/{share_ref}", get(library_page))
        .route("/app-assets/library.css", get(library_styles))
        .route("/app-assets/library-admin.css", get(library_admin_styles))
        .route("/app-assets/library-admin.js", get(library_admin_script))
        .route("/trace-assets/theme.css", get(trace_theme))
        .route("/live-assets/spectator.css", get(spectator_styles))
        .route("/live-assets/spectator.js", get(spectator_script))
        .route("/live-assets/site.css", get(site_styles))
        .route("/live-assets/trace-live-og.png", get(social_card))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(maximum_upload_bytes))
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    (
        [("x-trace-server-version", env!("CARGO_PKG_VERSION"))],
        StatusCode::NO_CONTENT,
    )
}

async fn instance_description(State(service): State<LiveService>) -> Json<InstanceDescription> {
    let durable_storage = service.config.share_directory.is_some();
    let mut features = vec!["live_spectating"];
    if service.shares.is_some() {
        features.push("completed_session_sharing");
    }
    if durable_storage {
        features.push("durable_storage");
    }
    if service.accounts.is_some() {
        features.push("accounts");
    }
    if service.library.is_some() {
        features.push("web_library");
    }
    let configured_registration =
        matches!(service.config.registration_policy, RegistrationPolicy::Open);
    let initial_admin_required = service
        .accounts
        .as_ref()
        .is_some_and(|store| store.initial_admin_required().unwrap_or(false));
    let registration_enabled = service.accounts.as_ref().is_some_and(|store| {
        store
            .registration_enabled(configured_registration)
            .unwrap_or(false)
    });
    Json(InstanceDescription {
        name: service.config.instance_name.clone(),
        version: env!("CARGO_PKG_VERSION"),
        public_base_url: service.config.public_base_url.clone(),
        registration_policy: service.config.registration_policy,
        initial_admin_required,
        registration_enabled,
        features,
        limits: InstanceLimits {
            maximum_upload_bytes: service.config.max_shared_session_bytes,
            shared_session_ttl_seconds: service.config.shared_session_ttl.as_secs(),
            live_retention_seconds: service.config.live_retention.as_secs(),
        },
    })
}

async fn create_account(
    State(service): State<LiveService>,
    peer: PeerAddress,
    headers: HeaderMap,
    Json(request): Json<CreateAccountRequest>,
) -> Result<(StatusCode, Json<AccountSummary>), ServiceError> {
    let (username, display_name) = validate_account_details(&request)?;
    let source = client_address(&service.config, &headers, &peer);
    service.check_rate(
        "account-registration",
        &source,
        1,
        service.config.rate_limits.bootstrap_per_minute,
        Duration::from_secs(60),
    )?;
    let store = service
        .accounts
        .clone()
        .ok_or(ServiceError::AccountsUnavailable)?;
    let configured_registration =
        matches!(service.config.registration_policy, RegistrationPolicy::Open);
    let registration_open = store
        .registration_enabled(configured_registration)
        .map_err(map_account_error)?;
    let password = request.password;
    let account = tokio::task::spawn_blocking(move || {
        store.register(
            &username,
            &display_name,
            &password,
            unix_timestamp_ms(),
            registration_open,
        )
    })
    .await
    .map_err(|_| ServiceError::Unavailable)?
    .map_err(map_account_error)?;
    Ok((StatusCode::CREATED, Json(account)))
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
struct AdminSettings {
    registration_enabled: bool,
    library_retention_days: u32,
    library_max_sessions_per_account: u32,
}

#[derive(Serialize)]
struct AdminSettingsResponse {
    #[serde(flatten)]
    settings: AdminSettings,
    cleanup: LibraryCleanupSummary,
}

async fn admin_settings(
    State(service): State<LiveService>,
    headers: HeaderMap,
) -> Result<Json<AdminSettingsResponse>, ServiceError> {
    let account = authenticated_account(&service, &headers).await?;
    ensure_administrator(&account)?;
    let store = service
        .accounts
        .clone()
        .ok_or(ServiceError::AccountsUnavailable)?;
    let configured = matches!(service.config.registration_policy, RegistrationPolicy::Open);
    let library = service
        .library
        .clone()
        .ok_or(ServiceError::LibraryUnavailable)?;
    tokio::task::spawn_blocking(move || {
        let stored = store.instance_settings(configured)?;
        let cleanup = library
            .preview_retention(
                None,
                unix_timestamp_ms(),
                stored.library_retention_days,
                stored.library_max_sessions_per_account,
            )
            .map_err(|_| AccountStoreError::Database)?;
        Ok((stored, cleanup))
    })
    .await
    .map_err(|_| ServiceError::Unavailable)?
    .map(|(stored, cleanup)| {
        Json(AdminSettingsResponse {
            settings: AdminSettings {
                registration_enabled: stored.registration_enabled,
                library_retention_days: stored.library_retention_days,
                library_max_sessions_per_account: stored.library_max_sessions_per_account,
            },
            cleanup,
        })
    })
    .map_err(map_account_error)
}

async fn preview_library_retention(
    State(service): State<LiveService>,
    headers: HeaderMap,
    Json(settings): Json<AdminSettings>,
) -> Result<Json<LibraryCleanupSummary>, ServiceError> {
    let account = authenticated_account(&service, &headers).await?;
    ensure_administrator(&account)?;
    validate_admin_settings(&settings)?;
    let library = service
        .library
        .clone()
        .ok_or(ServiceError::LibraryUnavailable)?;
    tokio::task::spawn_blocking(move || {
        library.preview_retention(
            None,
            unix_timestamp_ms(),
            settings.library_retention_days,
            settings.library_max_sessions_per_account,
        )
    })
    .await
    .map_err(|_| ServiceError::Unavailable)?
    .map(Json)
    .map_err(map_library_error)
}

async fn set_admin_settings(
    State(service): State<LiveService>,
    headers: HeaderMap,
    Json(settings): Json<AdminSettings>,
) -> Result<Json<AdminSettingsResponse>, ServiceError> {
    let account = authenticated_account(&service, &headers).await?;
    ensure_administrator(&account)?;
    validate_admin_settings(&settings)?;
    let store = service
        .accounts
        .clone()
        .ok_or(ServiceError::AccountsUnavailable)?;
    let library = service
        .library
        .clone()
        .ok_or(ServiceError::LibraryUnavailable)?;
    let account_id = account.account_id;
    let stored = StoredInstanceSettings {
        registration_enabled: settings.registration_enabled,
        library_retention_days: settings.library_retention_days,
        library_max_sessions_per_account: settings.library_max_sessions_per_account,
    };
    tokio::task::spawn_blocking(move || {
        store.set_instance_settings(&account_id, stored)?;
        let cleanup = library
            .enforce_retention(
                None,
                unix_timestamp_ms(),
                stored.library_retention_days,
                stored.library_max_sessions_per_account,
                None,
            )
            .map_err(|_| AccountStoreError::Database)?;
        Ok(cleanup)
    })
    .await
    .map_err(|_| ServiceError::Unavailable)?
    .map(|cleanup| Json(AdminSettingsResponse { settings, cleanup }))
    .map_err(map_account_error)
}

fn validate_admin_settings(settings: &AdminSettings) -> Result<(), ServiceError> {
    if settings.library_retention_days > 3_650
        || settings.library_max_sessions_per_account > 100_000
    {
        return Err(ServiceError::InvalidAdminSettings);
    }
    Ok(())
}

fn ensure_administrator(account: &AccountSummary) -> Result<(), ServiceError> {
    account
        .administrator
        .then_some(())
        .ok_or(ServiceError::AdministratorRequired)
}

async fn login(
    State(service): State<LiveService>,
    peer: PeerAddress,
    headers: HeaderMap,
    Json(request): Json<LoginRequest>,
) -> Result<Response, ServiceError> {
    if !valid_username(&request.username) || request.password.len() > 256 {
        return Err(ServiceError::AccountUnauthorized);
    }
    let source = client_address(&service.config, &headers, &peer);
    service.check_rate(
        "account-login",
        &source,
        1,
        service.config.rate_limits.bootstrap_per_minute,
        Duration::from_secs(60),
    )?;
    let store = service
        .accounts
        .clone()
        .ok_or(ServiceError::AccountsUnavailable)?;
    let username = request.username;
    let password = request.password;
    let (account, token) =
        tokio::task::spawn_blocking(move || store.login(&username, &password, unix_timestamp_ms()))
            .await
            .map_err(|_| ServiceError::Unavailable)?
            .map_err(map_account_error)?;
    let cookie = session_cookie(&token, &service.config.public_base_url)?;
    Ok(([(header::SET_COOKIE, cookie)], Json(account)).into_response())
}

async fn current_account(
    State(service): State<LiveService>,
    headers: HeaderMap,
) -> Result<Json<AccountSummary>, ServiceError> {
    let token = account_session_token(&headers)
        .ok_or(ServiceError::AccountUnauthorized)?
        .to_owned();
    let store = service
        .accounts
        .clone()
        .ok_or(ServiceError::AccountsUnavailable)?;
    tokio::task::spawn_blocking(move || store.find_session(&token, unix_timestamp_ms()))
        .await
        .map_err(|_| ServiceError::Unavailable)?
        .map(Json)
        .map_err(map_account_error)
}

async fn logout(
    State(service): State<LiveService>,
    headers: HeaderMap,
) -> Result<Response, ServiceError> {
    if let Some(token) = account_session_token(&headers) {
        let token = token.to_owned();
        let store = service
            .accounts
            .clone()
            .ok_or(ServiceError::AccountsUnavailable)?;
        tokio::task::spawn_blocking(move || store.logout(&token))
            .await
            .map_err(|_| ServiceError::Unavailable)?
            .map_err(map_account_error)?;
    }
    let cookie = expired_session_cookie(&service.config.public_base_url)?;
    Ok((StatusCode::NO_CONTENT, [(header::SET_COOKIE, cookie)]).into_response())
}

#[derive(Deserialize)]
struct CreateAccountTokenRequest {
    label: String,
}

async fn create_account_token(
    State(service): State<LiveService>,
    headers: HeaderMap,
    Json(request): Json<CreateAccountTokenRequest>,
) -> Result<(StatusCode, Json<CreatedAccountToken>), ServiceError> {
    let account = authenticated_account(&service, &headers).await?;
    let label = request.label.trim();
    if label.is_empty() || label.chars().count() > 40 || label.chars().any(char::is_control) {
        return Err(ServiceError::InvalidAccount);
    }
    let store = service
        .accounts
        .clone()
        .ok_or(ServiceError::AccountsUnavailable)?;
    let account_id = account.account_id;
    let label = label.to_owned();
    let token = tokio::task::spawn_blocking(move || {
        store.create_api_token(&account_id, &label, unix_timestamp_ms())
    })
    .await
    .map_err(|_| ServiceError::Unavailable)?
    .map_err(map_account_error)?;
    Ok((StatusCode::CREATED, Json(token)))
}

async fn list_account_tokens(
    State(service): State<LiveService>,
    headers: HeaderMap,
) -> Result<Json<Vec<AccountTokenSummary>>, ServiceError> {
    let account = authenticated_account(&service, &headers).await?;
    let store = service
        .accounts
        .clone()
        .ok_or(ServiceError::AccountsUnavailable)?;
    tokio::task::spawn_blocking(move || store.list_api_tokens(&account.account_id))
        .await
        .map_err(|_| ServiceError::Unavailable)?
        .map(Json)
        .map_err(map_account_error)
}

async fn revoke_account_token(
    State(service): State<LiveService>,
    Path(token_id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ServiceError> {
    let account = authenticated_account(&service, &headers).await?;
    let store = service
        .accounts
        .clone()
        .ok_or(ServiceError::AccountsUnavailable)?;
    tokio::task::spawn_blocking(move || store.revoke_api_token(&account.account_id, &token_id))
        .await
        .map_err(|_| ServiceError::Unavailable)?
        .map_err(map_account_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn upload_library_session(
    State(service): State<LiveService>,
    headers: HeaderMap,
    contents: Bytes,
) -> Result<(StatusCode, Json<LibrarySessionSummary>), ServiceError> {
    let account = authenticated_upload_account(&service, &headers).await?;
    let store = service
        .library
        .clone()
        .ok_or(ServiceError::LibraryUnavailable)?;
    let accounts = service
        .accounts
        .clone()
        .ok_or(ServiceError::AccountsUnavailable)?;
    let configured_registration =
        matches!(service.config.registration_policy, RegistrationPolicy::Open);
    let account_id = account.account_id;
    let contents = contents.to_vec();
    let summary = tokio::task::spawn_blocking(move || {
        let settings = accounts
            .instance_settings(configured_registration)
            .map_err(|_| LibraryStoreError::Database)?;
        let now = unix_timestamp_ms();
        let summary = store.upload(&account_id, &contents, now)?;
        store.enforce_retention(
            Some(&account_id),
            now,
            settings.library_retention_days,
            settings.library_max_sessions_per_account,
            Some(&summary.session_id),
        )?;
        Ok(summary)
    })
    .await
    .map_err(|_| ServiceError::Unavailable)?
    .map_err(map_library_error)?;
    Ok((StatusCode::CREATED, Json(summary)))
}

async fn list_library_sessions(
    State(service): State<LiveService>,
    headers: HeaderMap,
) -> Result<Json<Vec<LibrarySessionSummary>>, ServiceError> {
    let account = authenticated_account(&service, &headers).await?;
    let store = service
        .library
        .clone()
        .ok_or(ServiceError::LibraryUnavailable)?;
    tokio::task::spawn_blocking(move || store.list(&account.account_id))
        .await
        .map_err(|_| ServiceError::Unavailable)?
        .map(Json)
        .map_err(map_library_error)
}

async fn get_library_session(
    State(service): State<LiveService>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<LibrarySessionSummary>, ServiceError> {
    let account = authenticated_account(&service, &headers).await?;
    let store = service
        .library
        .clone()
        .ok_or(ServiceError::LibraryUnavailable)?;
    tokio::task::spawn_blocking(move || store.find(&account.account_id, &session_id))
        .await
        .map_err(|_| ServiceError::Unavailable)?
        .map(|stored| Json(stored.summary))
        .map_err(map_library_error)
}

#[derive(Debug, Deserialize)]
struct UpdateLibrarySessionRequest {
    title: Option<String>,
    driver_name: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

async fn update_library_session(
    State(service): State<LiveService>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<UpdateLibrarySessionRequest>,
) -> Result<Json<LibrarySessionSummary>, ServiceError> {
    let account = authenticated_account(&service, &headers).await?;
    let store = service
        .library
        .clone()
        .ok_or(ServiceError::LibraryUnavailable)?;
    tokio::task::spawn_blocking(move || {
        store.update_metadata(
            &account.account_id,
            &session_id,
            request.title.as_deref(),
            request.driver_name.as_deref(),
            &request.tags,
        )
    })
    .await
    .map_err(|_| ServiceError::Unavailable)?
    .map(Json)
    .map_err(|error| match error {
        LibraryStoreError::Invalid => ServiceError::InvalidLibraryMetadata,
        other => map_library_error(other),
    })
}

async fn review_library_session(
    State(service): State<LiveService>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<LibrarySessionDetail>, ServiceError> {
    let account = authenticated_account(&service, &headers).await?;
    let store = service
        .library
        .clone()
        .ok_or(ServiceError::LibraryUnavailable)?;
    tokio::task::spawn_blocking(move || store.detail(&account.account_id, &session_id))
        .await
        .map_err(|_| ServiceError::Unavailable)?
        .map(Json)
        .map_err(map_library_error)
}

#[derive(Deserialize)]
struct LibraryTelemetryQuery {
    max_points: Option<usize>,
}

#[derive(Serialize)]
struct LibraryShareSummary {
    share_id: String,
    share_url: String,
    download_url: String,
    visibility: &'static str,
}

async fn get_library_lap_telemetry(
    State(service): State<LiveService>,
    Path((session_id, lap_id)): Path<(String, String)>,
    Query(query): Query<LibraryTelemetryQuery>,
    headers: HeaderMap,
) -> Result<Json<LibraryLapTelemetry>, ServiceError> {
    let account = authenticated_account(&service, &headers).await?;
    let store = service
        .library
        .clone()
        .ok_or(ServiceError::LibraryUnavailable)?;
    let maximum_points = query
        .max_points
        .unwrap_or(DEFAULT_LIBRARY_TELEMETRY_POINTS)
        .clamp(100, MAX_LIBRARY_TELEMETRY_POINTS);
    tokio::task::spawn_blocking(move || {
        store.lap_telemetry(&account.account_id, &session_id, &lap_id, maximum_points)
    })
    .await
    .map_err(|_| ServiceError::Unavailable)?
    .map(Json)
    .map_err(map_library_error)
}

async fn share_library_session(
    State(service): State<LiveService>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<LibraryShareSummary>), ServiceError> {
    let account = authenticated_account(&service, &headers).await?;
    let store = service
        .library
        .clone()
        .ok_or(ServiceError::LibraryUnavailable)?;
    let (share_id, summary) = tokio::task::spawn_blocking(move || {
        store.share(&account.account_id, &session_id, unix_timestamp_ms())
    })
    .await
    .map_err(|_| ServiceError::Unavailable)?
    .map_err(map_library_error)?;
    Ok((
        StatusCode::CREATED,
        Json(library_share_summary(
            &service.config.public_base_url,
            share_id,
            &summary,
        )),
    ))
}

async fn revoke_library_share(
    State(service): State<LiveService>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ServiceError> {
    let account = authenticated_account(&service, &headers).await?;
    let store = service
        .library
        .clone()
        .ok_or(ServiceError::LibraryUnavailable)?;
    tokio::task::spawn_blocking(move || store.revoke_share(&account.account_id, &session_id))
        .await
        .map_err(|_| ServiceError::Unavailable)?
        .map_err(map_library_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_public_library_session(
    State(service): State<LiveService>,
    Path(share_id): Path<String>,
) -> Result<Json<LibrarySessionDetail>, ServiceError> {
    let store = service
        .library
        .clone()
        .ok_or(ServiceError::LibraryUnavailable)?;
    tokio::task::spawn_blocking(move || store.public_detail(&share_id))
        .await
        .map_err(|_| ServiceError::Unavailable)?
        .map(Json)
        .map_err(map_library_error)
}

async fn get_public_library_lap_telemetry(
    State(service): State<LiveService>,
    Path((share_id, lap_id)): Path<(String, String)>,
    Query(query): Query<LibraryTelemetryQuery>,
) -> Result<Json<LibraryLapTelemetry>, ServiceError> {
    let store = service
        .library
        .clone()
        .ok_or(ServiceError::LibraryUnavailable)?;
    let maximum_points = query
        .max_points
        .unwrap_or(DEFAULT_LIBRARY_TELEMETRY_POINTS)
        .clamp(100, MAX_LIBRARY_TELEMETRY_POINTS);
    tokio::task::spawn_blocking(move || {
        store.public_lap_telemetry(&share_id, &lap_id, maximum_points)
    })
    .await
    .map_err(|_| ServiceError::Unavailable)?
    .map(Json)
    .map_err(map_library_error)
}

async fn download_public_library_session(
    State(service): State<LiveService>,
    Path(share_id): Path<String>,
) -> Result<Response, ServiceError> {
    let store = service
        .library
        .clone()
        .ok_or(ServiceError::LibraryUnavailable)?;
    let stored = tokio::task::spawn_blocking(move || store.public_session(&share_id))
        .await
        .map_err(|_| ServiceError::Unavailable)?
        .map_err(map_library_error)?;
    library_download_response(stored, "no-store").await
}

fn library_share_summary(
    public_base_url: &str,
    share_id: String,
    summary: &LibrarySessionSummary,
) -> LibraryShareSummary {
    let slug = session_url_slug(summary);
    LibraryShareSummary {
        share_url: format!("{public_base_url}/shared/{slug}--{share_id}"),
        download_url: format!("{public_base_url}/api/v1/public-sessions/{share_id}/download"),
        share_id,
        visibility: "unlisted",
    }
}

fn session_url_slug(summary: &LibrarySessionSummary) -> String {
    let identity = summary
        .driver_name
        .as_deref()
        .or(summary.title.as_deref())
        .unwrap_or("session");
    let context = summary.track.as_deref().unwrap_or("unknown-track");
    let raw = format!("{identity}-{context}");
    let mut slug = String::with_capacity(raw.len().min(80));
    let mut separator = false;
    for character in raw.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            if separator && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character);
            separator = false;
        } else {
            separator = true;
        }
        if slug.len() >= 80 {
            break;
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "session".to_owned()
    } else {
        slug.to_owned()
    }
}

async fn download_library_session(
    State(service): State<LiveService>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ServiceError> {
    let account = authenticated_account(&service, &headers).await?;
    let store = service
        .library
        .clone()
        .ok_or(ServiceError::LibraryUnavailable)?;
    let stored = tokio::task::spawn_blocking(move || store.find(&account.account_id, &session_id))
        .await
        .map_err(|_| ServiceError::Unavailable)?
        .map_err(map_library_error)?;
    library_download_response(stored, "private, no-store").await
}

async fn library_download_response(
    stored: StoredLibrarySession,
    cache_control: &'static str,
) -> Result<Response, ServiceError> {
    let path = stored.path;
    let contents = tokio::task::spawn_blocking(move || fs::read(path))
        .await
        .map_err(|_| ServiceError::Unavailable)?
        .map_err(|_| ServiceError::Unavailable)?;
    let disposition = HeaderValue::from_str(&format!(
        "attachment; filename=\"{}\"",
        stored.download_name
    ))
    .map_err(|_| ServiceError::Unavailable)?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/vnd.simtrace.session"),
            ),
            (header::CONTENT_DISPOSITION, disposition),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static(cache_control),
            ),
        ],
        contents,
    )
        .into_response())
}

async fn delete_library_session(
    State(service): State<LiveService>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ServiceError> {
    let account = authenticated_account(&service, &headers).await?;
    let store = service
        .library
        .clone()
        .ok_or(ServiceError::LibraryUnavailable)?;
    tokio::task::spawn_blocking(move || store.delete(&account.account_id, &session_id))
        .await
        .map_err(|_| ServiceError::Unavailable)?
        .map_err(map_library_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn authenticated_account(
    service: &LiveService,
    headers: &HeaderMap,
) -> Result<AccountSummary, ServiceError> {
    let token = account_session_token(headers)
        .ok_or(ServiceError::AccountUnauthorized)?
        .to_owned();
    let store = service
        .accounts
        .clone()
        .ok_or(ServiceError::AccountsUnavailable)?;
    tokio::task::spawn_blocking(move || store.find_session(&token, unix_timestamp_ms()))
        .await
        .map_err(|_| ServiceError::Unavailable)?
        .map_err(map_account_error)
}

async fn authenticated_upload_account(
    service: &LiveService,
    headers: &HeaderMap,
) -> Result<AccountSummary, ServiceError> {
    if account_session_token(headers).is_some() {
        return authenticated_account(service, headers).await;
    }
    let token = bearer_token(headers)
        .map_err(|_| ServiceError::AccountUnauthorized)?
        .to_owned();
    let store = service
        .accounts
        .clone()
        .ok_or(ServiceError::AccountsUnavailable)?;
    tokio::task::spawn_blocking(move || store.find_api_token(&token, unix_timestamp_ms()))
        .await
        .map_err(|_| ServiceError::Unavailable)?
        .map_err(map_account_error)
}

async fn landing_page(State(service): State<LiveService>) -> Html<String> {
    Html(site_html(
        include_str!("../assets/site/index.html"),
        &service.config.public_base_url,
    ))
}

async fn bootstrap_installation(
    State(service): State<LiveService>,
    peer: PeerAddress,
    headers: HeaderMap,
) -> Result<Json<InstallationCredentials>, ServiceError> {
    let source = client_address(&service.config, &headers, &peer);
    service.check_rate(
        "bootstrap",
        &source,
        1,
        service.config.rate_limits.bootstrap_per_minute,
        Duration::from_secs(60),
    )?;
    service.bootstrap_installation().map(Json)
}

async fn create_live_session(
    State(service): State<LiveService>,
    peer: PeerAddress,
    headers: HeaderMap,
    Json(request): Json<CreateLiveSessionRequest>,
) -> Result<(StatusCode, Json<CreateLiveSessionResponse>), ServiceError> {
    let (installation_id, token) = publisher_credentials(&headers)?;
    let source = client_address(&service.config, &headers, &peer);
    let limit = service.config.rate_limits.session_creations_per_minute;
    service.check_rate("create-address", &source, 1, limit, Duration::from_secs(60))?;
    service.check_rate(
        "create-installation",
        installation_id,
        1,
        limit,
        Duration::from_secs(60),
    )?;
    let response = service.create_session(installation_id, token, request.session)?;
    Ok((StatusCode::CREATED, Json(response)))
}

async fn create_shared_session(
    State(service): State<LiveService>,
    headers: HeaderMap,
    contents: Bytes,
) -> Result<(StatusCode, Json<SharedSessionSummary>), ServiceError> {
    let (installation_id, token) = publisher_credentials(&headers)?;
    service.authorize_installation(installation_id, token)?;
    let store = service
        .shares
        .as_ref()
        .ok_or(ServiceError::SharingUnavailable)?;
    let summary = store
        .publish(contents.to_vec())
        .await
        .map_err(map_share_error)?;
    Ok((StatusCode::CREATED, Json(summary)))
}

async fn get_shared_session(
    State(service): State<LiveService>,
    Path(share_id): Path<String>,
) -> Result<Json<SharedSessionSummary>, ServiceError> {
    let store = service
        .shares
        .as_ref()
        .ok_or(ServiceError::SharingUnavailable)?;
    store
        .find(share_id)
        .await
        .map(|share| Json(share.summary))
        .map_err(map_share_error)
}

async fn download_shared_session(
    State(service): State<LiveService>,
    Path(share_id): Path<String>,
) -> Result<Response, ServiceError> {
    let store = service
        .shares
        .as_ref()
        .ok_or(ServiceError::SharingUnavailable)?;
    let share = store.find(share_id).await.map_err(map_share_error)?;
    let path = share.path.clone();
    let contents = tokio::task::spawn_blocking(move || fs::read(path))
        .await
        .map_err(|_| ServiceError::Unavailable)?
        .map_err(|_| ServiceError::Unavailable)?;
    let disposition =
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", share.download_name))
            .map_err(|_| ServiceError::Unavailable)?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/vnd.simtrace.session"),
            ),
            (header::CONTENT_DISPOSITION, disposition),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, max-age=300"),
            ),
        ],
        contents,
    )
        .into_response())
}

async fn shared_session_page(
    State(service): State<LiveService>,
    Path(share_id): Path<String>,
) -> Response {
    let Some(store) = service.shares.as_ref() else {
        return not_found_response(&service);
    };
    match store.find(share_id).await {
        Ok(share) => Html(shared_session_html(
            &share.summary,
            &service.config.public_base_url,
        ))
        .into_response(),
        Err(ShareStoreError::NotFound) => not_found_response(&service),
        Err(error) => map_share_error(error).into_response(),
    }
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

fn shared_session_html(summary: &SharedSessionSummary, public_base_url: &str) -> String {
    let driver = summary.driver_name.as_deref().unwrap_or("Shared driver");
    let title = format!("{driver} at {} — TRACE session", summary.track);
    let description = format!(
        "Download {driver}'s {} session in {} at {} and open it in TRACE.",
        format_session_type(&summary.session_type),
        summary.car,
        summary.track
    );
    let open_url = format!(
        "trace://shared-session?url={}",
        percent_encode_component(&summary.download_url)
    );
    render_page(
        include_str!("../assets/site/shared.html"),
        &title,
        &description,
        &summary.share_url,
        public_base_url,
    )
    .replace("{{TRACE_DRIVER}}", &escape_html(driver))
    .replace("{{TRACE_TRACK}}", &escape_html(&summary.track))
    .replace("{{TRACE_CAR}}", &escape_html(&summary.car))
    .replace(
        "{{TRACE_SESSION_TYPE}}",
        &escape_html(&format_session_type(&summary.session_type)),
    )
    .replace("{{TRACE_LAPS}}", &summary.lap_count.to_string())
    .replace("{{TRACE_OPEN_URL}}", &escape_html(&open_url))
    .replace(
        "{{TRACE_DOWNLOAD_URL}}",
        &escape_html(&summary.download_url),
    )
}

fn percent_encode_component(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

async fn spectator_styles() -> impl IntoResponse {
    (
        [("content-type", "text/css; charset=utf-8")],
        include_str!("../assets/spectator/styles.css"),
    )
}

async fn library_page() -> impl IntoResponse {
    let page = include_str!("../assets/library/index.html")
        .replace("__TRACE_VERSION__", env!("CARGO_PKG_VERSION"));
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'self'; base-uri 'none'; connect-src 'self'; form-action 'self'; frame-ancestors 'none'; img-src 'self'; script-src 'self'; style-src 'self'",
            ),
            (header::CACHE_CONTROL, "no-store"),
        ],
        page,
    )
}

async fn library_styles() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../assets/library/styles.css"),
    )
}

async fn library_admin_script() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../assets/library/library-admin.js"),
    )
}

async fn library_admin_styles() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../assets/library/library-admin.css"),
    )
}

async fn trace_theme() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../../../packages/trace-ui/src/theme.css"),
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
    template
        .replace("{{TRACE_URL}}", &base_url)
        .replace(
            "{{TRACE_OG_IMAGE}}",
            &format!("{base_url}/live-assets/trace-live-og.png"),
        )
        .replace("{{TRACE_VERSION}}", env!("CARGO_PKG_VERSION"))
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
        .replace("{{TRACE_VERSION}}", env!("CARGO_PKG_VERSION"))
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

async fn rotate_publisher_credential(
    State(service): State<LiveService>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<RotatePublishingTokenResponse>, ServiceError> {
    let (installation_id, token) = publisher_credentials(&headers)?;
    service
        .rotate_publisher_token(&session_id, installation_id, token)
        .map(Json)
}

async fn revoke_publisher_credential(
    State(service): State<LiveService>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ServiceError> {
    let (installation_id, token) = publisher_credentials(&headers)?;
    service.revoke_publisher_token(&session_id, installation_id, token)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn publisher_websocket(
    websocket: WebSocketUpgrade,
    State(service): State<LiveService>,
    Path(session_id): Path<String>,
    peer: PeerAddress,
    headers: HeaderMap,
) -> Result<Response, ServiceError> {
    let source = client_address(&service.config, &headers, &peer);
    service.check_rate(
        "publisher-upgrade",
        &source,
        1,
        service.config.rate_limits.publisher_upgrades_per_minute,
        Duration::from_secs(60),
    )?;
    let publishing_token = bearer_token(&headers)?.to_owned();
    service.authorize_publisher(&session_id, &publishing_token)?;
    Ok(websocket
        .max_message_size(MAX_WEBSOCKET_MESSAGE_BYTES)
        .on_upgrade(move |socket| receive_publisher(socket, service, session_id, publishing_token)))
}

async fn spectator_websocket(
    websocket: WebSocketUpgrade,
    State(service): State<LiveService>,
    Path(session_id): Path<String>,
    peer: PeerAddress,
    headers: HeaderMap,
) -> Result<Response, ServiceError> {
    let subscription = service.subscribe(&session_id)?;
    let source = client_address(&service.config, &headers, &peer);
    let permit = service.acquire_spectator(&session_id, &source)?;
    Ok(websocket
        .max_message_size(MAX_WEBSOCKET_MESSAGE_BYTES)
        .on_upgrade(move |socket| send_to_spectator(socket, subscription, permit)))
}

async fn receive_publisher(
    mut socket: WebSocket,
    service: LiveService,
    session_id: String,
    publishing_token: String,
) {
    while let Some(Ok(message)) = socket.next().await {
        match message {
            Message::Text(text) => {
                let rate_check = service
                    .check_rate(
                        "publisher-messages",
                        &session_id,
                        1,
                        service.config.rate_limits.publisher_messages_per_second,
                        Duration::from_secs(1),
                    )
                    .and_then(|()| {
                        service.check_rate(
                            "publisher-bytes",
                            &session_id,
                            u64::try_from(text.len()).unwrap_or(u64::MAX),
                            service.config.rate_limits.publisher_bytes_per_second,
                            Duration::from_secs(1),
                        )
                    });
                let parsed = rate_check.and_then(|()| {
                    serde_json::from_str::<Envelope>(&text)
                        .map_err(|_| ServiceError::InvalidMessage)
                        .and_then(|envelope| {
                            service.publish(&session_id, &publishing_token, envelope)
                        })
                });
                if let Err(error) = parsed {
                    let reason = serde_json::to_string(&ErrorBody {
                        error: error_message(error),
                    })
                    .unwrap_or_else(|_| "{\"error\":\"invalid live protocol message\"}".to_owned());
                    let _ = socket.send(Message::Text(reason.into())).await;
                    if matches!(error, ServiceError::RateLimited) {
                        let _ = socket
                            .send(Message::Close(Some(CloseFrame {
                                code: close_code::POLICY,
                                reason: "publisher rate limit exceeded".into(),
                            })))
                            .await;
                    }
                    if matches!(
                        error,
                        ServiceError::Ended
                            | ServiceError::Unauthorized
                            | ServiceError::PublisherCredentialRequired
                            | ServiceError::RateLimited
                    ) {
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
    _permit: SpectatorPermit,
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
    Ok((installation_id, bearer_token(headers)?))
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, ServiceError> {
    let authorization = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .ok_or(ServiceError::Unauthorized)?;
    let token = authorization
        .strip_prefix("Bearer ")
        .filter(|value| !value.is_empty())
        .ok_or(ServiceError::Unauthorized)?;
    Ok(token)
}

fn client_address(config: &ServerConfig, headers: &HeaderMap, peer: &PeerAddress) -> String {
    if config.trust_proxy_headers
        && let Some(forwarded) = headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    {
        return forwarded.to_owned();
    }
    peer.0.clone()
}

fn authenticate_publisher(
    session: &LiveSession,
    publishing_token: &str,
) -> Result<(), ServiceError> {
    let token_hash = session
        .publisher_token_hash
        .as_ref()
        .ok_or(ServiceError::PublisherCredentialRequired)?;
    if bool::from(token_hash.ct_eq(&hash_token(publishing_token))) {
        Ok(())
    } else {
        Err(ServiceError::Unauthorized)
    }
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

fn validate_account_details(
    request: &CreateAccountRequest,
) -> Result<(String, String), ServiceError> {
    let username = request.username.trim().to_ascii_lowercase();
    let display_name = request.display_name.trim().to_owned();
    if !valid_username(&username)
        || display_name.is_empty()
        || display_name.chars().count() > 80
        || display_name.chars().any(char::is_control)
        || !(12..=256).contains(&request.password.len())
    {
        return Err(ServiceError::InvalidAccount);
    }
    Ok((username, display_name))
}

fn valid_username(username: &str) -> bool {
    let bytes = username.as_bytes();
    (3..=32).contains(&bytes.len())
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn account_session_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|cookie| cookie.trim().split_once('='))
        .find_map(|(name, value)| (name == "trace_session").then_some(value))
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn session_cookie(token: &str, public_base_url: &str) -> Result<HeaderValue, ServiceError> {
    let secure = if public_base_url.starts_with("https://") {
        "; Secure"
    } else {
        ""
    };
    HeaderValue::from_str(&format!(
        "trace_session={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000{secure}"
    ))
    .map_err(|_| ServiceError::Unavailable)
}

fn expired_session_cookie(public_base_url: &str) -> Result<HeaderValue, ServiceError> {
    let secure = if public_base_url.starts_with("https://") {
        "; Secure"
    } else {
        ""
    };
    HeaderValue::from_str(&format!(
        "trace_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}"
    ))
    .map_err(|_| ServiceError::Unavailable)
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

fn push_buffered(
    session: &mut LiveSession,
    message: Envelope,
    received_at_unix_ms: i64,
    capacity: usize,
    retention: Duration,
) {
    let retention_ms = i64::try_from(retention.as_millis()).unwrap_or(i64::MAX);
    let cutoff = received_at_unix_ms.saturating_sub(retention_ms);
    while session
        .buffer
        .front()
        .is_some_and(|buffered| buffered.received_at_unix_ms < cutoff)
    {
        session.buffer.pop_front();
    }
    if session.buffer.len() == capacity {
        session.buffer.pop_front();
    }
    session.buffer.push_back(BufferedEnvelope {
        envelope: message.clone(),
        received_at_unix_ms,
    });
    let _ = session.broadcast.send(message);
}

fn unix_timestamp_ms() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    i64::try_from(millis).unwrap_or(i64::MAX)
}

fn cutoff_unix_ms(now_unix_ms: i64, retention: Duration) -> i64 {
    let retention_ms = i64::try_from(retention.as_millis()).unwrap_or(i64::MAX);
    now_unix_ms.saturating_sub(retention_ms)
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
        ServiceError::SharingUnavailable => "completed-session sharing is unavailable",
        ServiceError::InvalidPackage => "invalid TRACE session package",
        ServiceError::PackageTooLarge => "TRACE session package is too large",
        ServiceError::PublisherCredentialRequired => {
            "live session requires a session-scoped publishing credential"
        }
        ServiceError::RateLimited => "request rate limit exceeded",
        ServiceError::CapacityExceeded => "connection limit exceeded",
        ServiceError::AccountsUnavailable => "account service is unavailable",
        ServiceError::RegistrationDisabled => "account registration is disabled on this instance",
        ServiceError::InvalidAccount => "invalid account details",
        ServiceError::UsernameTaken => "username is already registered",
        ServiceError::AccountUnauthorized => "authentication required",
        ServiceError::LibraryUnavailable => "web session library is unavailable",
        ServiceError::LibrarySessionNotFound => "library session not found",
        ServiceError::InvalidLibraryMetadata => "invalid session metadata",
        ServiceError::InvalidAdminSettings => "invalid instance settings",
        ServiceError::AdministratorRequired => "administrator access is required",
    }
}

fn map_account_error(error: AccountStoreError) -> ServiceError {
    match error {
        AccountStoreError::Database => ServiceError::Unavailable,
        AccountStoreError::InvalidCredentials => ServiceError::AccountUnauthorized,
        AccountStoreError::RegistrationDisabled => ServiceError::RegistrationDisabled,
        AccountStoreError::AdministratorRequired => ServiceError::AdministratorRequired,
        AccountStoreError::UsernameTaken => ServiceError::UsernameTaken,
    }
}

fn map_library_error(error: LibraryStoreError) -> ServiceError {
    match error {
        LibraryStoreError::Database | LibraryStoreError::Unavailable => ServiceError::Unavailable,
        LibraryStoreError::Invalid => ServiceError::InvalidPackage,
        LibraryStoreError::NotFound => ServiceError::LibrarySessionNotFound,
        LibraryStoreError::TooLarge => ServiceError::PackageTooLarge,
    }
}

fn map_share_error(error: ShareStoreError) -> ServiceError {
    match error {
        ShareStoreError::Invalid => ServiceError::InvalidPackage,
        ShareStoreError::NotFound => ServiceError::NotFound,
        ShareStoreError::TooLarge => ServiceError::PackageTooLarge,
        ShareStoreError::Unavailable => ServiceError::Unavailable,
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
    use trace_domain::{ElapsedNanoseconds, FrameSequence, TelemetryFrame};
    use trace_live::{LiveTelemetrySample, encode_recorded_session};
    use trace_protocol::{Hello, PROTOCOL_VERSION};
    use trace_storage::{
        ipc::{TELEMETRY_SCHEMA_VERSION, encode_frames},
        metadata::{LapSummary, SessionConditions, SessionSummary},
        package::{PACKAGE_VERSION, SessionPackageLap, SessionPackageManifest, write_package},
    };

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

    #[tokio::test]
    async fn rate_limits_return_retry_after_and_bound_service_capacity() {
        let limits = RateLimits {
            bootstrap_per_minute: 1,
            publisher_messages_per_second: 1,
            publisher_bytes_per_second: 8,
            live_sessions_per_installation: 1,
            spectators_per_session: 1,
            spectators_per_address: 1,
            ..RateLimits::default()
        };
        let application =
            app(ServerConfig::new("http://localhost:8080").with_rate_limits(limits.clone()));
        let first = application
            .clone()
            .oneshot(
                Request::post("/api/v1/installations")
                    .body(Body::empty())
                    .expect("bootstrap request"),
            )
            .await
            .expect("bootstrap response");
        assert_eq!(first.status(), StatusCode::OK);
        let second = application
            .oneshot(
                Request::post("/api/v1/installations")
                    .body(Body::empty())
                    .expect("limited bootstrap request"),
            )
            .await
            .expect("limited bootstrap response");
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(second.headers()[header::RETRY_AFTER], "60");

        let service =
            LiveService::new(ServerConfig::new("http://localhost:8080").with_rate_limits(limits));
        service
            .check_rate(
                "publisher-messages",
                "session",
                1,
                1,
                Duration::from_secs(1),
            )
            .expect("first message");
        assert!(matches!(
            service.check_rate(
                "publisher-messages",
                "session",
                1,
                1,
                Duration::from_secs(1),
            ),
            Err(ServiceError::RateLimited)
        ));
        assert!(matches!(
            service.check_rate("publisher-bytes", "session", 9, 8, Duration::from_secs(1),),
            Err(ServiceError::RateLimited)
        ));

        let credentials = service.bootstrap_installation().expect("credentials");
        let created = service
            .create_session(
                &credentials.installation_id,
                &credentials.publishing_token,
                session_state(),
            )
            .expect("session");
        assert!(matches!(
            service.create_session(
                &credentials.installation_id,
                &credentials.publishing_token,
                session_state(),
            ),
            Err(ServiceError::CapacityExceeded)
        ));
        let permit = service
            .acquire_spectator(&created.session_id, "127.0.0.1")
            .expect("spectator permit");
        assert!(matches!(
            service.acquire_spectator(&created.session_id, "127.0.0.1"),
            Err(ServiceError::CapacityExceeded)
        ));
        drop(permit);
        service
            .acquire_spectator(&created.session_id, "127.0.0.1")
            .expect("released spectator permit");
    }

    #[test]
    fn session_creation_limits_are_scoped_by_address_and_installation() {
        let service = LiveService::new(ServerConfig::new("http://localhost:8080"));
        for scope in ["create-address", "create-installation"] {
            service
                .check_rate(scope, "subject", 1, 1, Duration::from_secs(60))
                .expect("first creation request");
            assert!(matches!(
                service.check_rate(scope, "subject", 1, 1, Duration::from_secs(60)),
                Err(ServiceError::RateLimited)
            ));
        }
    }

    #[test]
    fn durable_record_quotas_evict_only_unused_installations() {
        let limits = RateLimits {
            installation_records: 1,
            live_session_records: 1,
            live_sessions_per_installation: 2,
            ..RateLimits::default()
        };
        let directory = tempfile::tempdir().expect("temporary directory");
        let service = LiveService::new(
            ServerConfig::new("http://localhost:8080")
                .with_share_directory(directory.path())
                .with_rate_limits(limits),
        );
        let evicted = service
            .bootstrap_installation()
            .expect("first installation");
        let credentials = service
            .bootstrap_installation()
            .expect("replacement installation");
        assert!(matches!(
            service.authorize_installation(&evicted.installation_id, &evicted.publishing_token,),
            Err(ServiceError::Unauthorized)
        ));
        let created = service
            .create_session(
                &credentials.installation_id,
                &credentials.publishing_token,
                session_state(),
            )
            .expect("bounded session record");
        assert!(matches!(
            service.create_session(
                &credentials.installation_id,
                &credentials.publishing_token,
                session_state(),
            ),
            Err(ServiceError::CapacityExceeded)
        ));
        assert!(matches!(
            service.bootstrap_installation(),
            Err(ServiceError::CapacityExceeded)
        ));
        service
            .end_session(
                &created.session_id,
                &credentials.installation_id,
                &credentials.publishing_token,
            )
            .expect("end bounded session");
        let connection = rusqlite::Connection::open(directory.path().join("live.sqlite"))
            .expect("durable database");
        let installation_count = connection
            .query_row("SELECT COUNT(*) FROM live_installations", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("installation count");
        let session_count = connection
            .query_row("SELECT COUNT(*) FROM live_sessions", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("session count");
        assert_eq!(installation_count, 1);
        assert_eq!(session_count, 1);
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

    fn shared_package() -> Vec<u8> {
        let telemetry = encode_frames(&[TelemetryFrame {
            sequence: FrameSequence(1),
            elapsed: ElapsedNanoseconds(1),
            ..TelemetryFrame::default()
        }])
        .expect("telemetry");
        let lap = LapSummary {
            id: "lap-1".into(),
            index: 1,
            duration_ns: Some(90_000_000_000),
            validity: "valid".into(),
            validity_reason: None,
            max_tyres_out: Some(0),
            is_personal_best: true,
            sectors: Vec::new(),
        };
        let manifest = SessionPackageManifest {
            format_version: PACKAGE_VERSION,
            telemetry_schema_version: TELEMETRY_SCHEMA_VERSION,
            session: SessionSummary {
                id: "shared-session".into(),
                simulator_key: "assetto-corsa".into(),
                source_track_id: Some("zandvoort".into()),
                layout_id: None,
                source_car_id: Some("ks_mazda_mx5_cup".into()),
                user_title: None,
                user_driver: Some("Alex".into()),
                ownership: "mine".into(),
                tags: Vec::new(),
                track: Some("Zandvoort".into()),
                car: Some("Mazda MX-5 Cup".into()),
                session_type: Some("hotlap".into()),
                started_at: "2026-10-18T12:00:00Z".into(),
                source_kind: "native_capture".into(),
                conditions: SessionConditions::default(),
                exportable: true,
                laps: vec![lap.clone()],
            },
            laps: vec![SessionPackageLap {
                summary: lap,
                sample_start: 0,
                sample_count: 1,
            }],
            sample_count: 1,
            confirmed_setup: None,
        };
        let mut package = Vec::new();
        write_package(&mut package, telemetry.as_slice(), &manifest).expect("package");
        package
    }

    async fn registered_account_cookie(application: &Router, username: &str) -> String {
        let registration = serde_json::json!({
            "username": username,
            "display_name": username,
            "password": "correct horse battery"
        });
        let response = application
            .clone()
            .oneshot(
                Request::post("/api/v1/accounts")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(registration.to_string()))
                    .expect("registration request"),
            )
            .await
            .expect("registration response");
        assert_eq!(response.status(), StatusCode::CREATED);
        let login = serde_json::json!({
            "username": username,
            "password": "correct horse battery"
        });
        let response = application
            .clone()
            .oneshot(
                Request::post("/api/v1/auth/sessions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(login.to_string()))
                    .expect("login request"),
            )
            .await
            .expect("login response");
        assert_eq!(response.status(), StatusCode::OK);
        response.headers()[header::SET_COOKIE]
            .to_str()
            .expect("session cookie")
            .split(';')
            .next()
            .expect("cookie value")
            .to_owned()
    }

    async fn assert_private_library_review(
        application: &Router,
        session_id: &str,
        owner_cookie: &str,
        other_cookie: &str,
    ) {
        let review = application
            .clone()
            .oneshot(
                Request::get(format!("/api/v1/library/sessions/{session_id}/review"))
                    .header(header::COOKIE, owner_cookie)
                    .body(Body::empty())
                    .expect("library review request"),
            )
            .await
            .expect("library review response");
        assert_eq!(review.status(), StatusCode::OK);
        let review: serde_json::Value = serde_json::from_slice(
            &to_bytes(review.into_body(), 64 * 1024)
                .await
                .expect("library review body"),
        )
        .expect("library review");
        assert_eq!(review["summary"]["title"], "League race");
        assert_eq!(review["summary"]["driver_name"], "Alex R.");
        assert_eq!(review["source_track_id"], "zandvoort");
        assert_eq!(review["laps"][0]["id"], "lap-1");
        assert_eq!(review["laps"][0]["duration_ns"], 90_000_000_000_u64);

        let telemetry = application
            .clone()
            .oneshot(
                Request::get(format!(
                    "/api/v1/library/sessions/{session_id}/laps/lap-1/telemetry?max_points=1"
                ))
                .header(header::COOKIE, owner_cookie)
                .body(Body::empty())
                .expect("lap telemetry request"),
            )
            .await
            .expect("lap telemetry response");
        assert_eq!(telemetry.status(), StatusCode::OK);
        let telemetry: serde_json::Value = serde_json::from_slice(
            &to_bytes(telemetry.into_body(), 256 * 1024)
                .await
                .expect("lap telemetry body"),
        )
        .expect("lap telemetry");
        assert_eq!(telemetry["lap_id"], "lap-1");
        assert_eq!(telemetry["source_samples"], 1);
        assert_eq!(telemetry["returned_samples"], 1);
        assert_eq!(telemetry["points"].as_array().map(Vec::len), Some(1));

        let hidden = application
            .clone()
            .oneshot(
                Request::get(format!(
                    "/api/v1/library/sessions/{session_id}/laps/lap-1/telemetry"
                ))
                .header(header::COOKIE, other_cookie)
                .body(Body::empty())
                .expect("other account telemetry request"),
            )
            .await
            .expect("other account telemetry response");
        assert_eq!(hidden.status(), StatusCode::NOT_FOUND);
    }

    async fn assert_revocable_library_share(
        application: &Router,
        session_id: &str,
        owner_cookie: &str,
        other_cookie: &str,
        package: &[u8],
    ) {
        let hidden = application
            .clone()
            .oneshot(
                Request::post(format!("/api/v1/library/sessions/{session_id}/share"))
                    .header(header::COOKIE, other_cookie)
                    .body(Body::empty())
                    .expect("other account share request"),
            )
            .await
            .expect("other account share response");
        assert_eq!(hidden.status(), StatusCode::NOT_FOUND);

        let created = application
            .clone()
            .oneshot(
                Request::post(format!("/api/v1/library/sessions/{session_id}/share"))
                    .header(header::COOKIE, owner_cookie)
                    .body(Body::empty())
                    .expect("create library share request"),
            )
            .await
            .expect("create library share response");
        assert_eq!(created.status(), StatusCode::CREATED);
        let created: serde_json::Value = serde_json::from_slice(
            &to_bytes(created.into_body(), 64 * 1024)
                .await
                .expect("library share body"),
        )
        .expect("library share");
        let share_id = created["share_id"].as_str().expect("share ID");
        assert_eq!(created["visibility"], "unlisted");
        assert!(
            created["share_url"]
                .as_str()
                .is_some_and(|url| url.contains("/shared/") && url.contains("--"))
        );
        assert!(
            created["share_url"]
                .as_str()
                .is_some_and(|url| url.ends_with(share_id))
        );

        let review = application
            .clone()
            .oneshot(
                Request::get(format!("/api/v1/public-sessions/{share_id}"))
                    .body(Body::empty())
                    .expect("public review request"),
            )
            .await
            .expect("public review response");
        assert_eq!(review.status(), StatusCode::OK);
        let review: serde_json::Value = serde_json::from_slice(
            &to_bytes(review.into_body(), 64 * 1024)
                .await
                .expect("public review body"),
        )
        .expect("public review");
        assert_eq!(review["summary"]["title"], "League race");
        let telemetry = application
            .clone()
            .oneshot(
                Request::get(format!(
                    "/api/v1/public-sessions/{share_id}/laps/lap-1/telemetry"
                ))
                .body(Body::empty())
                .expect("public telemetry request"),
            )
            .await
            .expect("public telemetry response");
        assert_eq!(telemetry.status(), StatusCode::OK);
        let download = application
            .clone()
            .oneshot(
                Request::get(format!("/api/v1/public-sessions/{share_id}/download"))
                    .body(Body::empty())
                    .expect("public download request"),
            )
            .await
            .expect("public download response");
        assert_eq!(download.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(download.into_body(), package.len())
                .await
                .expect("public download body"),
            package
        );

        let revoked = application
            .clone()
            .oneshot(
                Request::delete(format!("/api/v1/library/sessions/{session_id}/share"))
                    .header(header::COOKIE, owner_cookie)
                    .body(Body::empty())
                    .expect("revoke library share request"),
            )
            .await
            .expect("revoke library share response");
        assert_eq!(revoked.status(), StatusCode::NO_CONTENT);
        let missing = application
            .clone()
            .oneshot(
                Request::get(format!("/api/v1/public-sessions/{share_id}"))
                    .body(Body::empty())
                    .expect("revoked public review request"),
            )
            .await
            .expect("revoked public review response");
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
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
        assert_eq!(created.publishing_token.len(), 64);
        assert_ne!(created.publishing_token, credentials.publishing_token);
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
        assert!(matches!(
            service.publish(
                &created.session_id,
                &credentials.publishing_token,
                message(&created.session_id, 1),
            ),
            Err(ServiceError::Unauthorized)
        ));
        let other = service
            .create_session(
                &credentials.installation_id,
                &credentials.publishing_token,
                session_state(),
            )
            .expect("other session");
        assert!(matches!(
            service.publish(
                &other.session_id,
                &created.publishing_token,
                message(&other.session_id, 1),
            ),
            Err(ServiceError::Unauthorized)
        ));
        service
            .publish(
                &created.session_id,
                &created.publishing_token,
                message(&created.session_id, 1),
            )
            .expect("first message");
        service
            .publish(
                &created.session_id,
                &created.publishing_token,
                message(&created.session_id, 1),
            )
            .expect("idempotent retry");
        let mut conflicting = message(&created.session_id, 1);
        conflicting.message_id = "different_message".to_owned();
        assert!(matches!(
            service.publish(&created.session_id, &created.publishing_token, conflicting),
            Err(ServiceError::OutOfOrder)
        ));
    }

    #[test]
    fn publisher_credentials_can_be_rotated_and_revoked() {
        let service = LiveService::new(ServerConfig::new("http://localhost:8080"));
        let credentials = service.bootstrap_installation().expect("credentials");
        let created = service
            .create_session(
                &credentials.installation_id,
                &credentials.publishing_token,
                session_state(),
            )
            .expect("session");
        let rotated = service
            .rotate_publisher_token(
                &created.session_id,
                &credentials.installation_id,
                &credentials.publishing_token,
            )
            .expect("rotated credential");
        assert_ne!(rotated.publishing_token, created.publishing_token);
        assert!(matches!(
            service.publish(
                &created.session_id,
                &created.publishing_token,
                message(&created.session_id, 1),
            ),
            Err(ServiceError::Unauthorized)
        ));
        service
            .publish(
                &created.session_id,
                &rotated.publishing_token,
                message(&created.session_id, 1),
            )
            .expect("rotated token publishes");
        service
            .revoke_publisher_token(
                &created.session_id,
                &credentials.installation_id,
                &credentials.publishing_token,
            )
            .expect("revoked credential");
        assert!(matches!(
            service.publish(
                &created.session_id,
                &rotated.publishing_token,
                message(&created.session_id, 2),
            ),
            Err(ServiceError::PublisherCredentialRequired)
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
                .publish(
                    &created.session_id,
                    &created.publishing_token,
                    message(&created.session_id, sequence),
                )
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
        assert!(matches!(
            service.publish(
                &created.session_id,
                &created.publishing_token,
                message(&created.session_id, 4),
            ),
            Err(ServiceError::PublisherCredentialRequired)
        ));
    }

    #[test]
    fn durable_live_state_survives_a_service_restart() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let config = ServerConfig::new("https://live.simtrace.run")
            .with_share_directory(directory.path())
            .with_live_retention(Duration::from_secs(600));
        let service = LiveService::new(config.clone());
        let credentials = service.bootstrap_installation().expect("credentials");
        let created = service
            .create_session(
                &credentials.installation_id,
                &credentials.publishing_token,
                session_state(),
            )
            .expect("session");
        service
            .publish(
                &created.session_id,
                &created.publishing_token,
                message(&created.session_id, 7),
            )
            .expect("publish");
        let rotated = service
            .rotate_publisher_token(
                &created.session_id,
                &credentials.installation_id,
                &credentials.publishing_token,
            )
            .expect("rotate persisted credential");
        drop(service);

        let restarted = LiveService::new(config);
        restarted
            .authorize_session(
                &created.session_id,
                &credentials.installation_id,
                &credentials.publishing_token,
            )
            .expect("persisted credentials and ownership");
        restarted
            .publish(
                &created.session_id,
                &rotated.publishing_token,
                message(&created.session_id, 8),
            )
            .expect("persisted rotated publishing credential");
        let summary = restarted.summary(&created.session_id).expect("summary");
        assert_eq!(summary.oldest_sequence, Some(7));
        assert_eq!(summary.newest_sequence, Some(8));
        let (snapshot, _) = restarted
            .subscribe(&created.session_id)
            .expect("subscription");
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].message_id, "message_7");
    }

    #[tokio::test]
    async fn instance_endpoint_describes_the_independent_deployment() {
        let application =
            app(ServerConfig::new("https://live.simtrace.run").with_instance_name("Club pit wall"));
        let instance = application
            .oneshot(
                Request::get("/api/v1/instance")
                    .body(Body::empty())
                    .expect("instance request"),
            )
            .await
            .expect("instance response");
        assert_eq!(instance.status(), StatusCode::OK);
        let body = to_bytes(instance.into_body(), 64 * 1024)
            .await
            .expect("instance body");
        let instance: serde_json::Value = serde_json::from_slice(&body).expect("instance JSON");
        assert_eq!(instance["name"], "Club pit wall");
        assert_eq!(instance["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(instance["public_base_url"], "https://live.simtrace.run");
        assert_eq!(instance["features"], serde_json::json!(["live_spectating"]));
        assert_eq!(instance["limits"]["maximum_upload_bytes"], 134_217_728);
        assert_eq!(instance["limits"]["shared_session_ttl_seconds"], 604_800);
        assert_eq!(instance["limits"]["live_retention_seconds"], 600);
    }

    #[tokio::test]
    async fn instance_local_accounts_register_login_and_logout() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let application = app(ServerConfig::new("https://telemetry.example.test")
            .with_share_directory(directory.path())
            .with_registration_policy(RegistrationPolicy::Open));
        let registration = serde_json::json!({
            "username": "Local.Driver",
            "display_name": "Local Driver",
            "password": "correct horse battery"
        });
        let response = application
            .clone()
            .oneshot(
                Request::post("/api/v1/accounts")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(registration.to_string()))
                    .expect("registration request"),
            )
            .await
            .expect("registration response");
        assert_eq!(response.status(), StatusCode::CREATED);
        let account: serde_json::Value = serde_json::from_slice(
            &to_bytes(response.into_body(), 64 * 1024)
                .await
                .expect("registration body"),
        )
        .expect("account JSON");
        assert_eq!(account["username"], "local.driver");
        assert_eq!(account["administrator"], true);

        let login = serde_json::json!({
            "username": "LOCAL.DRIVER",
            "password": "correct horse battery"
        });
        let response = application
            .clone()
            .oneshot(
                Request::post("/api/v1/auth/sessions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(login.to_string()))
                    .expect("login request"),
            )
            .await
            .expect("login response");
        assert_eq!(response.status(), StatusCode::OK);
        let set_cookie = response.headers()[header::SET_COOKIE]
            .to_str()
            .expect("session cookie");
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Lax"));
        assert!(set_cookie.contains("Secure"));
        let cookie = set_cookie
            .split(';')
            .next()
            .expect("cookie value")
            .to_owned();

        let response = application
            .clone()
            .oneshot(
                Request::get("/api/v1/auth/session")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .expect("current account request"),
            )
            .await
            .expect("current account response");
        assert_eq!(response.status(), StatusCode::OK);

        let response = application
            .clone()
            .oneshot(
                Request::delete("/api/v1/auth/session")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .expect("logout request"),
            )
            .await
            .expect("logout response");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(
            response.headers()[header::SET_COOKIE]
                .to_str()
                .expect("expired cookie")
                .contains("Max-Age=0")
        );

        let response = application
            .oneshot(
                Request::get("/api/v1/auth/session")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .expect("revoked account request"),
            )
            .await
            .expect("revoked account response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn first_account_bootstraps_admin_then_registration_closes() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let application =
            app(ServerConfig::new("http://localhost:8080").with_share_directory(directory.path()));
        let instance = application
            .clone()
            .oneshot(
                Request::get("/api/v1/instance")
                    .body(Body::empty())
                    .expect("instance request"),
            )
            .await
            .expect("instance response");
        let instance: serde_json::Value = serde_json::from_slice(
            &to_bytes(instance.into_body(), 64 * 1024)
                .await
                .expect("instance body"),
        )
        .expect("instance JSON");
        assert_eq!(instance["initial_admin_required"], true);
        assert_eq!(instance["registration_enabled"], false);

        let registration = serde_json::json!({
            "username": "driver",
            "display_name": "Driver",
            "password": "correct horse battery"
        });
        let response = application
            .clone()
            .oneshot(
                Request::post("/api/v1/accounts")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(registration.to_string()))
                    .expect("registration request"),
            )
            .await
            .expect("registration response");
        assert_eq!(response.status(), StatusCode::CREATED);

        let second = application
            .oneshot(
                Request::post("/api/v1/accounts")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "username": "other",
                            "display_name": "Other Driver",
                            "password": "another safe password"
                        })
                        .to_string(),
                    ))
                    .expect("second registration request"),
            )
            .await
            .expect("second registration response");
        assert_eq!(second.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn http_api_bootstraps_credentials_and_creates_a_session() {
        let application = app(ServerConfig::new("https://live.simtrace.run"));
        let health = application
            .clone()
            .oneshot(
                Request::get("/health")
                    .body(Body::empty())
                    .expect("health request"),
            )
            .await
            .expect("health response");
        assert_eq!(health.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            health.headers()["x-trace-server-version"],
            env!("CARGO_PKG_VERSION")
        );
        for (path, content_type) in [
            ("/live-assets/spectator.css", "text/css; charset=utf-8"),
            (
                "/live-assets/spectator.js",
                "text/javascript; charset=utf-8",
            ),
            ("/live-assets/site.css", "text/css; charset=utf-8"),
            ("/app-assets/library.css", "text/css; charset=utf-8"),
            ("/app-assets/library-admin.css", "text/css; charset=utf-8"),
            (
                "/app-assets/library-admin.js",
                "text/javascript; charset=utf-8",
            ),
            ("/trace-assets/theme.css", "text/css; charset=utf-8"),
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
        assert!(body.contains(&format!("v{}", env!("CARGO_PKG_VERSION"))));
        assert!(!body.contains("{{TRACE_"));

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
    async fn embedded_library_page_uses_external_assets_and_a_strict_policy() {
        let application = app(ServerConfig::new("https://telemetry.example.test"));
        let response = application
            .clone()
            .oneshot(
                Request::get("/app")
                    .body(Body::empty())
                    .expect("library page request"),
            )
            .await
            .expect("library page response");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .contains_key(header::CONTENT_SECURITY_POLICY)
        );
        let body = to_bytes(response.into_body(), 128 * 1024)
            .await
            .expect("library page body");
        let body = String::from_utf8(body.to_vec()).expect("library HTML");
        assert!(body.contains("Your private, self-hosted TRACE telemetry library."));
        assert!(body.contains("id=\"compare-picker-root\""));
        assert!(
            include_str!("../assets/library/library-admin.js")
                .contains("Compare against your library")
        );
        assert!(body.contains("id=\"comparison-summary-root\""));
        assert!(body.contains("id=\"telemetry-workspace-root\""));
        assert!(
            include_str!("../assets/library/library-admin.js")
                .contains("TIME DELTA · ANALYSED VS REFERENCE")
        );
        assert!(body.contains(&format!(
            "/app-assets/library-admin.js?v={}",
            env!("CARGO_PKG_VERSION")
        )));
        assert!(body.contains(&format!(
            "/app-assets/library.css?v={}",
            env!("CARGO_PKG_VERSION")
        )));
        assert!(body.contains(&format!(
            "/app-assets/library-admin.css?v={}",
            env!("CARGO_PKG_VERSION")
        )));
        assert!(body.contains(&format!(
            "/trace-assets/theme.css?v={}",
            env!("CARGO_PKG_VERSION")
        )));
        assert!(!body.contains("__TRACE_VERSION__"));
        assert!(!body.contains("<script>"));

        for path in [
            "/app/sessions/driver-zandvoort--0123456789abcdef0123456789abcdef",
            "/shared/driver-zandvoort--0123456789abcdef0123456789abcdef",
        ] {
            let response = application
                .clone()
                .oneshot(
                    Request::get(path)
                        .body(Body::empty())
                        .expect("navigable library route request"),
                )
                .await
                .expect("navigable library route response");
            assert_eq!(response.status(), StatusCode::OK);
        }
    }

    #[test]
    fn embedded_library_mounts_the_react_account_shell() {
        let html = include_str!("../assets/library/index.html");
        assert!(html.contains("id=\"auth-root\""));
        assert!(html.contains("id=\"account-shell-root\""));

        let bundle = include_str!("../assets/library/library-admin.js");
        assert!(bundle.contains("/api/v1/auth/sessions"));
        assert!(bundle.contains("/api/v1/auth/tokens"));
        assert!(include_str!("../assets/library/styles.css").contains("[hidden]"));
    }

    #[test]
    fn browser_library_uses_the_trace_theme_and_copyable_links() {
        let styles = include_str!("../assets/library/styles.css");
        let theme = include_str!("../../../packages/trace-ui/src/theme.css");
        assert!(theme.contains("--trace-accent: #c9ff00"));
        assert!(styles.contains("button {"));
        assert!(styles.contains("border-radius: 0"));
        assert!(!styles.contains("#68e997"));

        let script = include_str!("../assets/library/library-admin.js");
        assert!(script.contains("execCommand("));
        assert!(script.contains("trace://shared-session?url="));
        assert!(script.contains("trace:copy-app-link"));
        assert!(include_str!("../assets/library/index.html").contains("review-header-root"));

        let landing = site_html(
            include_str!("../assets/site/index.html"),
            "https://live.simtrace.run",
        );
        assert!(landing.contains("class=\"button primary\" href=\"/app\""));
        assert!(landing.contains("Open web app"));
        assert!(landing.contains(&format!(
            "/live-assets/site.css?v={}",
            env!("CARGO_PKG_VERSION")
        )));
    }

    #[tokio::test]
    async fn shared_packages_are_validated_downloadable_and_durable() {
        let directory = tempfile::tempdir().expect("share directory");
        let config =
            ServerConfig::new("https://live.simtrace.run").with_share_directory(directory.path());
        let application = app(config.clone());
        let credentials_response = application
            .clone()
            .oneshot(
                Request::post("/api/v1/installations")
                    .body(Body::empty())
                    .expect("credentials request"),
            )
            .await
            .expect("credentials response");
        let credentials = serde_json::from_slice::<InstallationCredentials>(
            &to_bytes(credentials_response.into_body(), 64 * 1024)
                .await
                .expect("credentials body"),
        )
        .expect("credentials");

        let unauthorized = application
            .clone()
            .oneshot(
                Request::post("/api/v1/shared-sessions")
                    .body(Body::from(shared_package()))
                    .expect("unauthorized request"),
            )
            .await
            .expect("unauthorized response");
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let invalid = application
            .clone()
            .oneshot(
                Request::post("/api/v1/shared-sessions")
                    .header("x-trace-installation-id", &credentials.installation_id)
                    .header(
                        "authorization",
                        format!("Bearer {}", credentials.publishing_token),
                    )
                    .body(Body::from("not a package"))
                    .expect("invalid request"),
            )
            .await
            .expect("invalid response");
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

        let package = shared_package();
        let created_response = application
            .oneshot(
                Request::post("/api/v1/shared-sessions")
                    .header("x-trace-installation-id", &credentials.installation_id)
                    .header(
                        "authorization",
                        format!("Bearer {}", credentials.publishing_token),
                    )
                    .body(Body::from(package.clone()))
                    .expect("upload request"),
            )
            .await
            .expect("upload response");
        assert_eq!(created_response.status(), StatusCode::CREATED);
        let created = serde_json::from_slice::<SharedSessionSummary>(
            &to_bytes(created_response.into_body(), 64 * 1024)
                .await
                .expect("upload body"),
        )
        .expect("share summary");
        assert_eq!(created.driver_name.as_deref(), Some("Alex"));
        assert_eq!(created.track, "Zandvoort");
        assert_eq!(created.lap_count, 1);

        let restarted = app(config);
        let page = restarted
            .clone()
            .oneshot(
                Request::get(format!("/s/{}", created.share_id))
                    .body(Body::empty())
                    .expect("share page request"),
            )
            .await
            .expect("share page response");
        assert_eq!(page.status(), StatusCode::OK);
        let page = to_bytes(page.into_body(), 128 * 1024)
            .await
            .expect("share page body");
        let page = String::from_utf8(page.to_vec()).expect("share page HTML");
        assert!(page.contains("Alex at Zandvoort"));
        assert!(page.contains("Open in TRACE"));
        let open_url_prefix = concat!(
            "trace://shared-session?url=https%3A%2F%2Flive.simtrace.run%2Fapi%2Fv1%",
            "2Fshared-sessions%2F"
        );
        assert!(page.contains(open_url_prefix));
        assert!(!page.contains("{{TRACE_"));

        let download = restarted
            .oneshot(
                Request::get(format!(
                    "/api/v1/shared-sessions/{}/download",
                    created.share_id
                ))
                .body(Body::empty())
                .expect("download request"),
            )
            .await
            .expect("download response");
        assert_eq!(download.status(), StatusCode::OK);
        assert_eq!(
            download.headers()["content-type"],
            "application/vnd.simtrace.session"
        );
        assert_eq!(
            to_bytes(download.into_body(), package.len())
                .await
                .expect("download body"),
            package
        );
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn account_library_uploads_are_private_durable_and_deletable() {
        let directory = tempfile::tempdir().expect("library directory");
        let config = ServerConfig::new("https://telemetry.example.test")
            .with_share_directory(directory.path())
            .with_registration_policy(RegistrationPolicy::Open);
        let application = app(config.clone());
        let owner_cookie = registered_account_cookie(&application, "owner").await;
        let other_cookie = registered_account_cookie(&application, "other").await;
        let package = shared_package();

        let token_response = application
            .clone()
            .oneshot(
                Request::post("/api/v1/auth/tokens")
                    .header(header::COOKIE, &owner_cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"label":"Test desktop"}"#))
                    .expect("token request"),
            )
            .await
            .expect("token response");
        assert_eq!(token_response.status(), StatusCode::CREATED);
        let token: serde_json::Value = serde_json::from_slice(
            &to_bytes(token_response.into_body(), 64 * 1024)
                .await
                .expect("token body"),
        )
        .expect("token JSON");
        let token = token["token"].as_str().expect("raw API token");

        let response = application
            .clone()
            .oneshot(
                Request::post("/api/v1/library/sessions")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::from(package.clone()))
                    .expect("library upload request"),
            )
            .await
            .expect("library upload response");
        assert_eq!(response.status(), StatusCode::CREATED);
        let created: serde_json::Value = serde_json::from_slice(
            &to_bytes(response.into_body(), 64 * 1024)
                .await
                .expect("library upload body"),
        )
        .expect("library summary");
        assert_eq!(created["driver_name"], "Alex");
        assert_eq!(created["track"], "Zandvoort");
        assert_eq!(created["lap_count"], 1);
        let session_id = created["session_id"]
            .as_str()
            .expect("library session ID")
            .to_owned();

        let hidden = application
            .clone()
            .oneshot(
                Request::get(format!("/api/v1/library/sessions/{session_id}"))
                    .header(header::COOKIE, &other_cookie)
                    .body(Body::empty())
                    .expect("other account request"),
            )
            .await
            .expect("other account response");
        assert_eq!(hidden.status(), StatusCode::NOT_FOUND);

        let restarted = app(config);
        let listed = restarted
            .clone()
            .oneshot(
                Request::get("/api/v1/library/sessions")
                    .header(header::COOKIE, &owner_cookie)
                    .body(Body::empty())
                    .expect("library list request"),
            )
            .await
            .expect("library list response");
        assert_eq!(listed.status(), StatusCode::OK);
        let listed: serde_json::Value = serde_json::from_slice(
            &to_bytes(listed.into_body(), 64 * 1024)
                .await
                .expect("library list body"),
        )
        .expect("library list");
        assert_eq!(listed.as_array().map(Vec::len), Some(1));

        let updated = restarted
            .clone()
            .oneshot(
                Request::put(format!("/api/v1/library/sessions/{session_id}"))
                    .header(header::COOKIE, &owner_cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"title":"League race","driver_name":"Alex R.","tags":["league","wet","league"]}"#,
                    ))
                    .expect("metadata update request"),
            )
            .await
            .expect("metadata update response");
        assert_eq!(updated.status(), StatusCode::OK);
        let updated: serde_json::Value = serde_json::from_slice(
            &to_bytes(updated.into_body(), 64 * 1024)
                .await
                .expect("metadata update body"),
        )
        .expect("updated library summary");
        assert_eq!(updated["title"], "League race");
        assert_eq!(updated["driver_name"], "Alex R.");
        assert_eq!(updated["tags"], serde_json::json!(["league", "wet"]));

        let forbidden_update = restarted
            .clone()
            .oneshot(
                Request::put(format!("/api/v1/library/sessions/{session_id}"))
                    .header(header::COOKIE, &other_cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"title":"Not mine","driver_name":null,"tags":[]}"#,
                    ))
                    .expect("other metadata update request"),
            )
            .await
            .expect("other metadata update response");
        assert_eq!(forbidden_update.status(), StatusCode::NOT_FOUND);

        assert_private_library_review(&restarted, &session_id, &owner_cookie, &other_cookie).await;
        assert_revocable_library_share(
            &restarted,
            &session_id,
            &owner_cookie,
            &other_cookie,
            &package,
        )
        .await;

        let downloaded = restarted
            .clone()
            .oneshot(
                Request::get(format!("/api/v1/library/sessions/{session_id}/download"))
                    .header(header::COOKIE, &owner_cookie)
                    .body(Body::empty())
                    .expect("library download request"),
            )
            .await
            .expect("library download response");
        assert_eq!(downloaded.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(downloaded.into_body(), 128 * 1024)
                .await
                .expect("library download body"),
            package
        );

        let deleted = restarted
            .clone()
            .oneshot(
                Request::delete(format!("/api/v1/library/sessions/{session_id}"))
                    .header(header::COOKIE, &owner_cookie)
                    .body(Body::empty())
                    .expect("library delete request"),
            )
            .await
            .expect("library delete response");
        assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
        let missing = restarted
            .oneshot(
                Request::get(format!("/api/v1/library/sessions/{session_id}"))
                    .header(header::COOKIE, owner_cookie)
                    .body(Body::empty())
                    .expect("missing library request"),
            )
            .await
            .expect("missing library response");
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn administrator_library_retention_is_previewed_persisted_and_enforced() {
        let directory = tempfile::tempdir().expect("library directory");
        let config = ServerConfig::new("https://telemetry.example.test")
            .with_share_directory(directory.path())
            .with_registration_policy(RegistrationPolicy::Open);
        let application = app(config.clone());
        let owner_cookie = registered_account_cookie(&application, "retention-owner").await;
        let package = shared_package();

        let mut session_ids = Vec::new();
        for _ in 0..2 {
            let response = application
                .clone()
                .oneshot(
                    Request::post("/api/v1/library/sessions")
                        .header(header::COOKIE, &owner_cookie)
                        .body(Body::from(package.clone()))
                        .expect("library upload request"),
                )
                .await
                .expect("library upload response");
            assert_eq!(response.status(), StatusCode::CREATED);
            let uploaded: serde_json::Value = serde_json::from_slice(
                &to_bytes(response.into_body(), 64 * 1024)
                    .await
                    .expect("uploaded session body"),
            )
            .expect("uploaded session");
            session_ids.push(
                uploaded["session_id"]
                    .as_str()
                    .expect("uploaded session ID")
                    .to_owned(),
            );
            tokio::time::sleep(Duration::from_millis(2)).await;
        }

        rusqlite::Connection::open(directory.path().join("live.sqlite"))
            .expect("retention database")
            .execute(
                "UPDATE web_library_sessions SET created_at_unix_ms = 0 WHERE session_id = ?1",
                [&session_ids[0]],
            )
            .expect("age first session");

        let age_preview = serde_json::json!({
            "registration_enabled": false,
            "library_retention_days": 1,
            "library_max_sessions_per_account": 0
        });
        let age_preview = application
            .clone()
            .oneshot(
                Request::post("/api/v1/admin/library-retention-preview")
                    .header(header::COOKIE, &owner_cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(age_preview.to_string()))
                    .expect("age retention preview request"),
            )
            .await
            .expect("age retention preview response");
        let age_preview: serde_json::Value = serde_json::from_slice(
            &to_bytes(age_preview.into_body(), 64 * 1024)
                .await
                .expect("age retention preview body"),
        )
        .expect("age retention preview");
        assert_eq!(age_preview["session_count"], 1);

        let settings = serde_json::json!({
            "registration_enabled": false,
            "library_retention_days": 0,
            "library_max_sessions_per_account": 1
        });
        let preview = application
            .clone()
            .oneshot(
                Request::post("/api/v1/admin/library-retention-preview")
                    .header(header::COOKIE, &owner_cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(settings.to_string()))
                    .expect("retention preview request"),
            )
            .await
            .expect("retention preview response");
        assert_eq!(preview.status(), StatusCode::OK);
        let preview: serde_json::Value = serde_json::from_slice(
            &to_bytes(preview.into_body(), 64 * 1024)
                .await
                .expect("retention preview body"),
        )
        .expect("retention preview");
        assert_eq!(preview["session_count"], 1);

        let saved = application
            .clone()
            .oneshot(
                Request::put("/api/v1/admin/settings")
                    .header(header::COOKIE, &owner_cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(settings.to_string()))
                    .expect("save retention request"),
            )
            .await
            .expect("save retention response");
        assert_eq!(saved.status(), StatusCode::OK);
        let saved: serde_json::Value = serde_json::from_slice(
            &to_bytes(saved.into_body(), 64 * 1024)
                .await
                .expect("save retention body"),
        )
        .expect("saved settings");
        assert_eq!(saved["cleanup"]["session_count"], 1);

        let restarted = app(config);
        let persisted = restarted
            .clone()
            .oneshot(
                Request::get("/api/v1/admin/settings")
                    .header(header::COOKIE, &owner_cookie)
                    .body(Body::empty())
                    .expect("persisted settings request"),
            )
            .await
            .expect("persisted settings response");
        let persisted: serde_json::Value = serde_json::from_slice(
            &to_bytes(persisted.into_body(), 64 * 1024)
                .await
                .expect("persisted settings body"),
        )
        .expect("persisted settings");
        assert_eq!(persisted["library_max_sessions_per_account"], 1);

        let uploaded = restarted
            .clone()
            .oneshot(
                Request::post("/api/v1/library/sessions")
                    .header(header::COOKIE, &owner_cookie)
                    .body(Body::from(package))
                    .expect("retained upload request"),
            )
            .await
            .expect("retained upload response");
        assert_eq!(uploaded.status(), StatusCode::CREATED);
        let listed = restarted
            .oneshot(
                Request::get("/api/v1/library/sessions")
                    .header(header::COOKIE, owner_cookie)
                    .body(Body::empty())
                    .expect("retained library request"),
            )
            .await
            .expect("retained library response");
        let listed: serde_json::Value = serde_json::from_slice(
            &to_bytes(listed.into_body(), 64 * 1024)
                .await
                .expect("retained library body"),
        )
        .expect("retained library");
        assert_eq!(listed.as_array().map(Vec::len), Some(1));
    }

    #[tokio::test]
    async fn publisher_reconnects_without_duplicating_the_ambiguous_message() {
        let limits = RateLimits {
            publisher_upgrades_per_minute: 3,
            ..RateLimits::default()
        };
        let server = start_local_server(
            ServerConfig::new("http://127.0.0.1:0").with_rate_limits(limits),
            None,
        )
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

        let mut installation_token_request = created
            .publish_websocket_url
            .as_str()
            .into_client_request()
            .expect("publisher request");
        installation_token_request.headers_mut().insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", credentials.publishing_token))
                .expect("authorization header"),
        );
        let rejected = tokio_tungstenite::connect_async(installation_token_request)
            .await
            .expect_err("installation token must not open the publisher websocket");
        assert!(rejected.to_string().contains("401"));

        let spectator_url = created
            .publish_websocket_url
            .replace("/publish", "/spectate");
        let (mut spectator, _) = tokio_tungstenite::connect_async(&spectator_url)
            .await
            .expect("spectator websocket");
        let mut publisher = connect_test_publisher(&created).await;
        let first = message(&created.session_id, 0);
        publisher
            .send(TestMessage::Text(
                serde_json::to_string(&first).expect("first JSON").into(),
            ))
            .await
            .expect("first publish");
        assert_eq!(next_test_envelope(&mut spectator).await.sequence, 0);
        publisher.close(None).await.expect("disconnect publisher");

        let mut reconnected = connect_test_publisher(&created).await;
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
        let limited = tokio_tungstenite::connect_async(&created.publish_websocket_url)
            .await
            .expect_err("publisher upgrade attempts must be limited");
        assert!(limited.to_string().contains("429"));
        server.shutdown().await;
    }

    async fn connect_test_publisher(
        created: &CreateLiveSessionResponse,
    ) -> WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>> {
        let mut request = created
            .publish_websocket_url
            .as_str()
            .into_client_request()
            .expect("publisher request");
        request.headers_mut().insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", created.publishing_token))
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
                .publish(
                    &created.session_id,
                    &created.publishing_token,
                    message.envelope,
                )
                .expect("accepted replay message");
        }

        let summary = service.summary(&created.session_id).expect("summary");
        assert_eq!(summary.session.status, LiveStatus::Ended);
        assert_eq!(summary.oldest_sequence, Some(0));
        assert_eq!(summary.newest_sequence, Some(4));
    }
}
