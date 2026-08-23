use std::time::{Duration, Instant};

use trace_adapter::{
    AdapterError, AdapterEvent, AdapterIdentity, DisconnectReason, SimulatorAdapter,
};
use trace_domain::{
    ChannelAvailability, ChannelCapabilities, ChannelDescriptor, ChannelId, ElapsedNanoseconds,
    FrameSequence, SessionSeed, SimulatorId, SourceDescriptor, SourceKind, Unit, ValueProvenance,
};

use crate::{AcAvailability, AcCaptureError, AcSharedMemory, AcSnapshot};

const STATUS_OFF: i32 = 0;
const STATUS_REPLAY: i32 = 1;
const STATUS_LIVE: i32 = 2;
const STATUS_PAUSE: i32 = 3;
const DEFAULT_STALE_PACKET_TIMEOUT: Duration = Duration::from_secs(5);
const SUPPORTED_SHARED_MEMORY_VERSION: &str = "1.7";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AcRuntimeStatus {
    Off,
    Live,
    Replay,
    Paused,
}

#[derive(Clone, Debug)]
struct StalePacketTracker {
    timeout: Duration,
    observation: Option<((i32, i32), Instant)>,
}

impl StalePacketTracker {
    fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            observation: None,
        }
    }

    fn observe(&mut self, signature: (i32, i32), paused: bool, now: Instant) -> bool {
        if paused {
            self.observation = None;
            return false;
        }
        match self.observation {
            Some((previous, since)) if previous == signature => {
                now.saturating_duration_since(since) >= self.timeout
            }
            _ => {
                self.observation = Some((signature, now));
                false
            }
        }
    }

    fn reset(&mut self) {
        self.observation = None;
    }
}

/// Injectable acquisition seam used by the production Windows source and fixtures.
pub trait AcSource {
    /// Reports whether the simulator can currently be connected.
    ///
    /// # Errors
    ///
    /// Returns a capture error when detection itself fails.
    fn detect(&mut self) -> Result<AcAvailability, AcCaptureError>;

    /// Opens all required pages.
    ///
    /// # Errors
    ///
    /// Returns a capture error when a required page cannot be opened.
    fn connect(&mut self) -> Result<(), AcCaptureError>;

    /// Returns one owned packet-stable snapshot.
    ///
    /// # Errors
    ///
    /// Returns a capture error for unavailable, malformed, or unstable pages.
    fn snapshot(&mut self) -> Result<AcSnapshot, AcCaptureError>;

    /// Releases the current connection. This operation must be idempotent.
    fn disconnect(&mut self);
}

/// Production source backed by vanilla AC's three Windows mappings.
#[derive(Default)]
pub struct SystemAcSource {
    connection: Option<AcSharedMemory>,
}

impl AcSource for SystemAcSource {
    fn detect(&mut self) -> Result<AcAvailability, AcCaptureError> {
        AcSharedMemory::detect()
    }

    fn connect(&mut self) -> Result<(), AcCaptureError> {
        self.connection = Some(AcSharedMemory::open()?);
        Ok(())
    }

    fn snapshot(&mut self) -> Result<AcSnapshot, AcCaptureError> {
        self.connection
            .as_mut()
            .ok_or(AcCaptureError::Mapping(
                trace_windows_shmem::MappingError::NotFound,
            ))?
            .snapshot()
    }

    fn disconnect(&mut self) {
        self.connection = None;
    }
}

#[derive(Clone, Debug)]
enum ConnectionState {
    Disconnected,
    Running { session: SessionSeed, paused: bool },
}

/// Assetto Corsa implementation of the canonical simulator adapter lifecycle.
pub struct AcAdapter<S = SystemAcSource> {
    identity: AdapterIdentity,
    source: S,
    state: ConnectionState,
    next_sequence: u64,
    stream_started: Option<Instant>,
    stale_packets: StalePacketTracker,
}

impl AcAdapter<SystemAcSource> {
    /// Creates the production Assetto Corsa adapter.
    pub fn new() -> Self {
        Self::with_source(SystemAcSource::default())
    }
}

impl Default for AcAdapter<SystemAcSource> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> AcAdapter<S> {
    /// Creates an adapter around an acquisition source.
    pub fn with_source(source: S) -> Self {
        Self {
            identity: AdapterIdentity {
                key: "assetto-corsa".into(),
                display_name: "Assetto Corsa".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            source,
            state: ConnectionState::Disconnected,
            next_sequence: 0,
            stream_started: None,
            stale_packets: StalePacketTracker::new(DEFAULT_STALE_PACKET_TIMEOUT),
        }
    }
}

impl<S: AcSource> SimulatorAdapter for AcAdapter<S> {
    fn identity(&self) -> &AdapterIdentity {
        &self.identity
    }

    fn poll(&mut self) -> Result<Vec<AdapterEvent>, AdapterError> {
        match &self.state {
            ConnectionState::Disconnected => self.poll_disconnected(),
            ConnectionState::Running { .. } => self.poll_running(),
        }
    }
}

impl<S: AcSource> AcAdapter<S> {
    fn poll_disconnected(&mut self) -> Result<Vec<AdapterEvent>, AdapterError> {
        match self.source.detect().map_err(adapter_error)? {
            AcAvailability::NotRunning | AcAvailability::UnsupportedPlatform => Ok(Vec::new()),
            AcAvailability::Available => {
                self.source.connect().map_err(adapter_error)?;
                let snapshot = self.source.snapshot().map_err(adapter_error)?;
                let (shared_memory_version, assetto_corsa_version) =
                    snapshot.versions().map_err(adapter_error)?;
                if shared_memory_version.as_deref() != Some(SUPPORTED_SHARED_MEMORY_VERSION) {
                    self.source.disconnect();
                    return Err(AdapterError::InvalidSource(format!(
                        "unsupported Assetto Corsa shared-memory version {}; expected {SUPPORTED_SHARED_MEMORY_VERSION}",
                        shared_memory_version.as_deref().unwrap_or("missing")
                    )));
                }
                let (session, environment) = snapshot.map_session().map_err(adapter_error)?;
                let status = runtime_status(snapshot.status())?;
                if status == AcRuntimeStatus::Off {
                    self.source.disconnect();
                    return Ok(Vec::new());
                }

                self.next_sequence = 0;
                let now = Instant::now();
                self.stream_started = Some(now);
                let paused = status == AcRuntimeStatus::Paused;
                self.stale_packets
                    .observe(snapshot.packet_signature(), paused, now);
                self.state = ConnectionState::Running {
                    session: session.clone(),
                    paused,
                };
                let mut frame = snapshot
                    .map_frame(FrameSequence(0), ElapsedNanoseconds(0))
                    .map_err(adapter_error)?;
                frame.environment = environment;
                self.next_sequence = 1;

                let mut events = vec![
                    AdapterEvent::Detected(source_descriptor(
                        assetto_corsa_version,
                        status == AcRuntimeStatus::Replay,
                    )),
                    AdapterEvent::Connected(session),
                    AdapterEvent::CapabilitiesChanged(capabilities()),
                ];
                if paused {
                    events.push(AdapterEvent::Paused);
                }
                events.push(AdapterEvent::Frame(frame));
                Ok(events)
            }
        }
    }

    fn poll_running(&mut self) -> Result<Vec<AdapterEvent>, AdapterError> {
        let snapshot = match self.source.snapshot() {
            Ok(snapshot) => snapshot,
            Err(error @ AcCaptureError::UnstablePacket { .. }) => {
                return Err(adapter_error(error));
            }
            Err(error) => {
                self.disconnect();
                return Err(AdapterError::ConnectionLost(format!(
                    "Assetto Corsa shared memory connection was lost: {error:?}"
                )));
            }
        };
        let status = runtime_status(snapshot.status())?;
        if status == AcRuntimeStatus::Off {
            self.disconnect();
            return Ok(vec![AdapterEvent::Disconnected(
                DisconnectReason::SourceClosed,
            )]);
        }

        if self.stale_packets.observe(
            snapshot.packet_signature(),
            status == AcRuntimeStatus::Paused,
            Instant::now(),
        ) {
            self.disconnect();
            return Err(AdapterError::ConnectionLost(
                "Assetto Corsa shared-memory packets stopped advancing".into(),
            ));
        }

        let (session, environment) = snapshot.map_session().map_err(adapter_error)?;
        let mut events = Vec::new();
        let ConnectionState::Running {
            session: previous,
            paused,
        } = &mut self.state
        else {
            unreachable!("running poll requires running state")
        };
        if session_identity_changed(previous, &session) {
            *previous = session.clone();
            events.push(AdapterEvent::SessionChanged(session));
        }
        let now_paused = status == AcRuntimeStatus::Paused;
        if *paused != now_paused {
            events.push(if now_paused {
                AdapterEvent::Paused
            } else {
                AdapterEvent::Resumed
            });
            *paused = now_paused;
        }

        let elapsed = self.stream_started.map_or(0, |started| {
            u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
        });
        let mut frame = snapshot
            .map_frame(
                FrameSequence(self.next_sequence),
                ElapsedNanoseconds(elapsed),
            )
            .map_err(adapter_error)?;
        frame.environment = environment;
        self.next_sequence = self.next_sequence.saturating_add(1);
        events.push(AdapterEvent::Frame(frame));
        Ok(events)
    }

    fn disconnect(&mut self) {
        self.source.disconnect();
        self.state = ConnectionState::Disconnected;
        self.stream_started = None;
        self.stale_packets.reset();
    }
}

fn source_descriptor(simulator_version: Option<String>, replay: bool) -> SourceDescriptor {
    SourceDescriptor {
        simulator: SimulatorId::parse("assetto-corsa").expect("static simulator identifier"),
        adapter_version: env!("CARGO_PKG_VERSION").into(),
        simulator_version,
        kind: if replay {
            SourceKind::SimulatorReplay
        } else {
            SourceKind::NativeCapture
        },
    }
}

fn session_identity_changed(previous: &SessionSeed, current: &SessionSeed) -> bool {
    // AC briefly clears static-page strings and individual graphics fields while
    // loading or resetting a hotlap. Treat those snapshots as incomplete metadata,
    // not dozens of real session boundaries.
    if current.car_id.is_none() || current.track_id.is_none() {
        return false;
    }
    if previous.car_id != current.car_id || previous.track_id != current.track_id {
        return true;
    }
    if matches!(
        (&previous.source_session_id, &current.source_session_id),
        (Some(previous), Some(current)) if previous != current
    ) {
        return true;
    }
    if matches!(
        (&previous.layout_id, &current.layout_id),
        (Some(previous), Some(current)) if previous != current
    ) {
        return true;
    }
    matches!(
        (&previous.session_type, &current.session_type),
        (Some(previous), Some(current)) if previous != current
    )
}

fn runtime_status(status: i32) -> Result<AcRuntimeStatus, AdapterError> {
    match status {
        STATUS_OFF => Ok(AcRuntimeStatus::Off),
        STATUS_REPLAY => Ok(AcRuntimeStatus::Replay),
        STATUS_LIVE => Ok(AcRuntimeStatus::Live),
        STATUS_PAUSE => Ok(AcRuntimeStatus::Paused),
        value => Err(AdapterError::InvalidSource(format!(
            "Assetto Corsa reported unknown graphics status {value}"
        ))),
    }
}

fn capabilities() -> ChannelCapabilities {
    let mut capabilities = ChannelCapabilities::default();
    for (id, unit, source_field) in [
        ("inputs.throttle", Unit::Ratio, "gas"),
        ("inputs.brake", Unit::Ratio, "brake"),
        ("vehicle.speed", Unit::MetresPerSecond, "speedKmh"),
        ("vehicle.engine_rpm", Unit::RevolutionsPerMinute, "rpms"),
        ("vehicle.gear", Unit::Unitless, "gear"),
        ("vehicle.fuel", Unit::Litre, "fuel"),
        ("lap.position", Unit::Ratio, "normalizedCarPosition"),
        ("lap.current_time", Unit::Second, "iCurrentTime"),
        (
            "environment.air_temperature",
            Unit::DegreeCelsius,
            "airTemp",
        ),
        (
            "environment.track_temperature",
            Unit::DegreeCelsius,
            "roadTemp",
        ),
    ] {
        let id = ChannelId::parse(id).expect("static channel identifier");
        capabilities.insert(ChannelDescriptor {
            id,
            unit,
            availability: ChannelAvailability::Available,
            provenance: ValueProvenance::Measured,
            source_field: Some(source_field.into()),
        });
    }
    capabilities
}

fn adapter_error(error: AcCaptureError) -> AdapterError {
    match error {
        AcCaptureError::UnstablePacket { .. } => AdapterError::TemporarilyUnavailable(format!(
            "Assetto Corsa packet was unstable: {error:?}"
        )),
        AcCaptureError::InvalidPage(_) => {
            AdapterError::InvalidSource(format!("Assetto Corsa page was invalid: {error:?}"))
        }
        AcCaptureError::InvalidNativePayload => {
            AdapterError::InvalidSource("Assetto Corsa native payload was invalid".into())
        }
        AcCaptureError::Mapping(_) => AdapterError::TemporarilyUnavailable(format!(
            "Assetto Corsa shared memory is unavailable: {error:?}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::pages;

    struct ScriptedSource {
        availability: VecDeque<AcAvailability>,
        snapshots: VecDeque<Result<AcSnapshot, AcCaptureError>>,
        connects: usize,
        disconnects: usize,
    }

    impl AcSource for ScriptedSource {
        fn detect(&mut self) -> Result<AcAvailability, AcCaptureError> {
            Ok(self
                .availability
                .pop_front()
                .unwrap_or(AcAvailability::NotRunning))
        }

        fn connect(&mut self) -> Result<(), AcCaptureError> {
            self.connects += 1;
            Ok(())
        }

        fn snapshot(&mut self) -> Result<AcSnapshot, AcCaptureError> {
            self.snapshots.pop_front().expect("scripted snapshot")
        }

        fn disconnect(&mut self) {
            self.disconnects += 1;
        }
    }

    fn put_i32(bytes: &mut [u8], offset: usize, value: i32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_utf16(bytes: &mut [u8], offset: usize, slots: usize, value: &str) {
        for (index, unit) in value.encode_utf16().take(slots - 1).enumerate() {
            let start = offset + index * 2;
            bytes[start..start + 2].copy_from_slice(&unit.to_le_bytes());
        }
    }

    fn snapshot(status: i32, car: &str, track: &str) -> AcSnapshot {
        snapshot_with_version(status, car, track, SUPPORTED_SHARED_MEMORY_VERSION)
    }

    fn snapshot_with_version(status: i32, car: &str, track: &str, version: &str) -> AcSnapshot {
        let physics = vec![0; pages::PHYSICS_PREFIX_LENGTH];
        let mut graphics = vec![0; pages::GRAPHICS_PREFIX_LENGTH];
        put_i32(&mut graphics, 4, status);
        let mut static_page = vec![0; pages::STATIC_PREFIX_LENGTH];
        put_utf16(&mut static_page, 0, 15, version);
        put_utf16(&mut static_page, 30, 15, "fixture");
        put_utf16(&mut static_page, 68, 33, car);
        put_utf16(&mut static_page, 134, 33, track);
        AcSnapshot::from_pages(physics, graphics, static_page).expect("valid snapshot")
    }

    fn source(snapshots: impl IntoIterator<Item = AcSnapshot>) -> ScriptedSource {
        ScriptedSource {
            availability: VecDeque::from([AcAvailability::Available]),
            snapshots: snapshots.into_iter().map(Ok).collect(),
            connects: 0,
            disconnects: 0,
        }
    }

    #[test]
    fn emits_ordered_connect_capability_and_frame_events() {
        let mut adapter =
            AcAdapter::with_source(source([snapshot(STATUS_LIVE, "tatuusfa1", "mugello")]));
        let events = adapter.poll().expect("connect events");

        assert!(matches!(
            &events[0],
            AdapterEvent::Detected(source) if source.simulator_version.as_deref() == Some("fixture")
        ));
        assert!(matches!(events[1], AdapterEvent::Connected(_)));
        assert!(matches!(events[2], AdapterEvent::CapabilitiesChanged(_)));
        assert!(matches!(
            &events[3],
            AdapterEvent::Frame(frame) if frame.sequence == FrameSequence(0)
        ));
    }

    #[test]
    fn identifies_replay_as_a_distinct_source_kind() {
        let mut adapter =
            AcAdapter::with_source(source([snapshot(STATUS_REPLAY, "car-a", "track-a")]));
        let events = adapter.poll().expect("replay connect");

        assert!(matches!(
            &events[0],
            AdapterEvent::Detected(source) if source.kind == SourceKind::SimulatorReplay
        ));
    }

    #[test]
    fn reports_session_pause_resume_and_disconnect_transitions() {
        let mut adapter = AcAdapter::with_source(source([
            snapshot(STATUS_LIVE, "car-a", "track-a"),
            snapshot(STATUS_PAUSE, "car-b", "track-a"),
            snapshot(STATUS_REPLAY, "car-b", "track-a"),
            snapshot(STATUS_OFF, "car-b", "track-a"),
        ]));
        adapter.poll().expect("connect");

        let paused = adapter.poll().expect("pause and session change");
        assert!(matches!(paused[0], AdapterEvent::SessionChanged(_)));
        assert_eq!(paused[1], AdapterEvent::Paused);
        assert!(matches!(paused[2], AdapterEvent::Frame(_)));

        let resumed = adapter.poll().expect("resume");
        assert_eq!(resumed[0], AdapterEvent::Resumed);
        assert!(matches!(resumed[1], AdapterEvent::Frame(_)));

        assert_eq!(
            adapter.poll().expect("disconnect"),
            vec![AdapterEvent::Disconnected(DisconnectReason::SourceClosed)]
        );
    }

    #[test]
    fn ignores_transient_missing_ac_session_identity() {
        let mut adapter = AcAdapter::with_source(source([
            snapshot(STATUS_LIVE, "car-a", "track-a"),
            snapshot(STATUS_LIVE, "", ""),
            snapshot(STATUS_LIVE, "car-a", "track-a"),
        ]));
        adapter.poll().expect("connect");

        let missing = adapter.poll().expect("transient missing identity");
        assert!(matches!(missing.as_slice(), [AdapterEvent::Frame(_)]));

        let recovered = adapter.poll().expect("recovered identity");
        assert!(matches!(recovered.as_slice(), [AdapterEvent::Frame(_)]));
    }

    #[test]
    fn retries_detection_after_a_normal_disconnect() {
        let mut scripted = source([
            snapshot(STATUS_LIVE, "car-a", "track-a"),
            snapshot(STATUS_OFF, "car-a", "track-a"),
            snapshot(STATUS_LIVE, "car-a", "track-a"),
        ]);
        scripted.availability.push_back(AcAvailability::Available);
        let mut adapter = AcAdapter::with_source(scripted);

        adapter.poll().expect("first connect");
        adapter.poll().expect("disconnect");
        let reconnected = adapter.poll().expect("reconnect");
        assert!(matches!(reconnected[0], AdapterEvent::Detected(_)));
        assert!(matches!(reconnected[1], AdapterEvent::Connected(_)));
    }

    #[test]
    fn unstable_packets_are_temporary_without_disconnect() {
        let mut scripted = source([snapshot(STATUS_LIVE, "car-a", "track-a")]);
        scripted
            .snapshots
            .push_back(Err(AcCaptureError::UnstablePacket {
                page: "acpmf_physics",
                attempts: 3,
            }));
        scripted
            .snapshots
            .push_back(Ok(snapshot(STATUS_LIVE, "car-a", "track-a")));
        let mut adapter = AcAdapter::with_source(scripted);
        adapter.poll().expect("connect");

        assert!(matches!(
            adapter.poll(),
            Err(AdapterError::TemporarilyUnavailable(_))
        ));
        assert!(matches!(
            adapter.poll().expect("recovered frame")[0],
            AdapterEvent::Frame(_)
        ));
    }

    #[test]
    fn rejects_an_unverified_shared_memory_version() {
        let unsupported = snapshot_with_version(STATUS_LIVE, "car-a", "track-a", "9.9");
        let mut adapter = AcAdapter::with_source(source([unsupported]));

        assert!(matches!(
            adapter.poll(),
            Err(AdapterError::InvalidSource(message)) if message.contains("version 9.9")
        ));
        assert_eq!(adapter.source.disconnects, 1);
    }

    #[test]
    fn stale_packets_timeout_only_while_the_simulator_is_running() {
        let started = Instant::now();
        let timeout = Duration::from_secs(5);
        let mut tracker = StalePacketTracker::new(timeout);

        assert!(!tracker.observe((10, 20), false, started));
        assert!(!tracker.observe(
            (10, 20),
            false,
            started + timeout.saturating_sub(Duration::from_millis(1))
        ));
        assert!(tracker.observe((10, 20), false, started + timeout));

        assert!(!tracker.observe((11, 20), false, started + timeout));
        assert!(!tracker.observe((11, 20), true, started + timeout * 10));
        assert!(!tracker.observe((11, 20), false, started + timeout * 20));
    }
}
