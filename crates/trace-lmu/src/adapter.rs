use std::time::Instant;

use trace_adapter::{
    AdapterError, AdapterEvent, AdapterIdentity, DisconnectReason, SimulatorAdapter,
};
use trace_domain::{
    ChannelAvailability, ChannelCapabilities, ChannelDescriptor, ChannelId, ElapsedNanoseconds,
    FrameSequence, SessionSeed, SimulatorId, SourceDescriptor, SourceKind, Unit, ValueProvenance,
};
use trace_windows_shmem::MappingError;

use crate::{LmuAvailability, LmuCaptureError, LmuSharedMemory, LmuSnapshot};

const PHASE_BEFORE_SESSION: u8 = 0;
const PHASE_SESSION_OVER: u8 = 8;
const PHASE_PAUSED: u8 = 9;

/// Injectable acquisition seam for production shared memory and regression fixtures.
pub trait LmuSource {
    /// Reports whether LMU can currently be connected.
    ///
    /// # Errors
    ///
    /// Returns a capture error when detection itself fails.
    fn detect(&mut self) -> Result<LmuAvailability, LmuCaptureError>;

    /// Opens the official shared-memory feed.
    ///
    /// # Errors
    ///
    /// Returns a capture error when required Windows objects are unavailable.
    fn connect(&mut self) -> Result<(), LmuCaptureError>;

    /// Returns one synchronized, owned snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error for temporary lock contention, lost mappings, or bad data.
    fn snapshot(&mut self) -> Result<LmuSnapshot, LmuCaptureError>;

    /// Releases the current connection. This operation must be idempotent.
    fn disconnect(&mut self);
}

/// Production LMU source backed by the simulator's built-in Windows mapping.
#[derive(Default)]
pub struct SystemLmuSource {
    connection: Option<LmuSharedMemory>,
}

impl LmuSource for SystemLmuSource {
    fn detect(&mut self) -> Result<LmuAvailability, LmuCaptureError> {
        LmuSharedMemory::detect()
    }

    fn connect(&mut self) -> Result<(), LmuCaptureError> {
        self.connection = Some(LmuSharedMemory::open()?);
        Ok(())
    }

    fn snapshot(&mut self) -> Result<LmuSnapshot, LmuCaptureError> {
        self.connection
            .as_ref()
            .ok_or(LmuCaptureError::Mapping(MappingError::NotFound))?
            .snapshot()
    }

    fn disconnect(&mut self) {
        self.connection = None;
    }
}

#[derive(Clone, Debug)]
enum ConnectionState {
    Disconnected,
    Running {
        session: SessionSeed,
        paused: bool,
        replay: bool,
    },
}

/// Le Mans Ultimate implementation of TRACE's simulator adapter lifecycle.
pub struct LmuAdapter<S = SystemLmuSource> {
    identity: AdapterIdentity,
    source: S,
    state: ConnectionState,
    next_sequence: u64,
    stream_started: Option<Instant>,
}

impl LmuAdapter<SystemLmuSource> {
    /// Creates the production LMU adapter.
    pub fn new() -> Self {
        Self::with_source(SystemLmuSource::default())
    }
}

impl Default for LmuAdapter<SystemLmuSource> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> LmuAdapter<S> {
    /// Creates an adapter around an alternate acquisition source.
    pub fn with_source(source: S) -> Self {
        Self {
            identity: AdapterIdentity {
                key: "le-mans-ultimate".into(),
                display_name: "Le Mans Ultimate".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            source,
            state: ConnectionState::Disconnected,
            next_sequence: 0,
            stream_started: None,
        }
    }
}

impl<S: LmuSource> SimulatorAdapter for LmuAdapter<S> {
    fn identity(&self) -> &AdapterIdentity {
        &self.identity
    }

    fn poll(&mut self) -> Result<Vec<AdapterEvent>, AdapterError> {
        match self.state {
            ConnectionState::Disconnected => self.poll_disconnected(),
            ConnectionState::Running { .. } => self.poll_running(),
        }
    }
}

impl<S: LmuSource> LmuAdapter<S> {
    fn poll_disconnected(&mut self) -> Result<Vec<AdapterEvent>, AdapterError> {
        match self.source.detect().map_err(adapter_error)? {
            LmuAvailability::NotRunning | LmuAvailability::UnsupportedPlatform => Ok(Vec::new()),
            LmuAvailability::Available => {
                self.source.connect().map_err(adapter_error)?;
                let snapshot = self.source.snapshot().map_err(adapter_error)?;
                let phase = snapshot.game_phase().map_err(page_error)?;
                let in_realtime = snapshot.in_realtime().map_err(page_error)?;
                let replay = snapshot.player_control().map_err(page_error)? == 3;
                let player_finished = snapshot.player_finish_status().map_err(page_error)? != 0;
                let session = snapshot.map_session().map_err(page_error)?;
                if phase == PHASE_BEFORE_SESSION
                    || (phase == PHASE_SESSION_OVER
                        && (player_finished || (!replay && !in_realtime)))
                    || !complete_identity(&session)
                {
                    self.disconnect();
                    return Ok(Vec::new());
                }
                self.next_sequence = 0;
                self.stream_started = Some(Instant::now());
                let paused = phase == PHASE_PAUSED;
                self.state = ConnectionState::Running {
                    session: session.clone(),
                    paused,
                    replay,
                };
                let mut events = vec![
                    AdapterEvent::Detected(source_descriptor(
                        snapshot.simulator_version().map_err(page_error)?,
                        replay,
                    )),
                    AdapterEvent::Connected(session),
                    AdapterEvent::CapabilitiesChanged(capabilities()),
                ];
                if paused {
                    events.push(AdapterEvent::Paused);
                }
                events.push(AdapterEvent::Frame(self.map_next_frame(&snapshot)?));
                Ok(events)
            }
        }
    }

    fn poll_running(&mut self) -> Result<Vec<AdapterEvent>, AdapterError> {
        let snapshot = match self.source.snapshot() {
            Ok(snapshot) => snapshot,
            Err(LmuCaptureError::Mapping(MappingError::SynchronizationBusy)) => {
                return Ok(Vec::new());
            }
            Err(error) => {
                self.disconnect();
                return Err(AdapterError::ConnectionLost(format!(
                    "Le Mans Ultimate shared-memory connection was lost: {error:?}"
                )));
            }
        };
        let phase = snapshot.game_phase().map_err(page_error)?;
        let in_realtime = snapshot.in_realtime().map_err(page_error)?;
        let player_finished = snapshot.player_finish_status().map_err(page_error)? != 0;
        let replay = matches!(&self.state, ConnectionState::Running { replay: true, .. });
        if phase == PHASE_BEFORE_SESSION
            || (phase == PHASE_SESSION_OVER && (player_finished || (!replay && !in_realtime)))
        {
            let frame = self.map_next_frame(&snapshot)?;
            self.disconnect();
            return Ok(vec![
                AdapterEvent::Frame(frame),
                AdapterEvent::Disconnected(DisconnectReason::SessionEnded),
            ]);
        }
        let now_replay = snapshot.player_control().map_err(page_error)? == 3;
        if replay != now_replay {
            self.disconnect();
            return Ok(vec![AdapterEvent::Disconnected(
                DisconnectReason::SourceClosed,
            )]);
        }
        let session = snapshot.map_session().map_err(page_error)?;
        let mut events = Vec::new();
        let ConnectionState::Running {
            session: previous,
            paused,
            ..
        } = &mut self.state
        else {
            unreachable!("running poll requires running state")
        };
        if session_identity_changed(previous, &session) {
            *previous = session.clone();
            events.push(AdapterEvent::SessionChanged(session));
        }
        let now_paused = phase == PHASE_PAUSED || (!replay && !in_realtime);
        if *paused != now_paused {
            events.push(if now_paused {
                AdapterEvent::Paused
            } else {
                AdapterEvent::Resumed
            });
            *paused = now_paused;
        }
        events.push(AdapterEvent::Frame(self.map_next_frame(&snapshot)?));
        Ok(events)
    }

    fn map_next_frame(
        &mut self,
        snapshot: &LmuSnapshot,
    ) -> Result<trace_domain::TelemetryFrame, AdapterError> {
        let elapsed = self.stream_started.map_or(0, |started| {
            u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
        });
        let frame = snapshot
            .map_frame(
                FrameSequence(self.next_sequence),
                ElapsedNanoseconds(elapsed),
            )
            .map_err(page_error)?;
        self.next_sequence = self.next_sequence.saturating_add(1);
        Ok(frame)
    }

    fn disconnect(&mut self) {
        self.source.disconnect();
        self.state = ConnectionState::Disconnected;
        self.stream_started = None;
    }
}

fn complete_identity(session: &SessionSeed) -> bool {
    session.car_id.is_some() && session.track_id.is_some()
}

fn session_identity_changed(previous: &SessionSeed, current: &SessionSeed) -> bool {
    complete_identity(current)
        && (previous.car_id != current.car_id
            || previous.track_id != current.track_id
            || previous.session_type != current.session_type)
}

fn source_descriptor(simulator_version: Option<String>, replay: bool) -> SourceDescriptor {
    SourceDescriptor {
        simulator: SimulatorId::parse("le-mans-ultimate")
            .expect("static simulator identifier is valid"),
        adapter_version: env!("CARGO_PKG_VERSION").into(),
        simulator_version,
        kind: if replay {
            SourceKind::SimulatorReplay
        } else {
            SourceKind::NativeCapture
        },
    }
}

fn capabilities() -> ChannelCapabilities {
    let mut capabilities = ChannelCapabilities::default();
    for (id, unit, source_field, provenance) in [
        (
            "inputs.throttle",
            Unit::Ratio,
            "mUnfilteredThrottle",
            ValueProvenance::Measured,
        ),
        (
            "inputs.brake",
            Unit::Ratio,
            "mUnfilteredBrake",
            ValueProvenance::Measured,
        ),
        (
            "inputs.clutch",
            Unit::Ratio,
            "mUnfilteredClutch",
            ValueProvenance::Measured,
        ),
        (
            "inputs.steering",
            Unit::Radian,
            "mUnfilteredSteering × physical/visual steering wheel range",
            ValueProvenance::SimulatorDerived,
        ),
        (
            "vehicle.speed",
            Unit::MetresPerSecond,
            "mLocalVel",
            ValueProvenance::SimulatorDerived,
        ),
        (
            "vehicle.engine_rpm",
            Unit::RevolutionsPerMinute,
            "mEngineRPM",
            ValueProvenance::Measured,
        ),
        (
            "vehicle.gear",
            Unit::Unitless,
            "mGear",
            ValueProvenance::Measured,
        ),
        (
            "vehicle.fuel",
            Unit::Litre,
            "mFuel",
            ValueProvenance::Measured,
        ),
        (
            "lap.position",
            Unit::Ratio,
            "mLapDist / mLapDist",
            ValueProvenance::SimulatorDerived,
        ),
        (
            "lap.current_time",
            Unit::Second,
            "mElapsedTime - mLapStartET",
            ValueProvenance::SimulatorDerived,
        ),
        (
            "environment.air_temperature",
            Unit::DegreeCelsius,
            "mAmbientTemp",
            ValueProvenance::Measured,
        ),
        (
            "environment.track_temperature",
            Unit::DegreeCelsius,
            "mTrackTemp",
            ValueProvenance::Measured,
        ),
    ] {
        capabilities.insert(ChannelDescriptor {
            id: ChannelId::parse(id).expect("static channel identifier"),
            unit,
            availability: ChannelAvailability::Available,
            provenance,
            source_field: Some(source_field.into()),
        });
    }
    capabilities
}

fn adapter_error(error: LmuCaptureError) -> AdapterError {
    match error {
        LmuCaptureError::Mapping(_) => AdapterError::TemporarilyUnavailable(format!(
            "Le Mans Ultimate shared memory is unavailable: {error:?}"
        )),
        LmuCaptureError::InvalidPage(_) => AdapterError::InvalidSource(format!(
            "Le Mans Ultimate shared memory was invalid: {error:?}"
        )),
    }
}

fn page_error(error: crate::LmuPageError) -> AdapterError {
    AdapterError::InvalidSource(format!(
        "Le Mans Ultimate shared memory was invalid: {error:?}"
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::{
        SCORING_OFFSET, SCORING_VEHICLES_OFFSET, SHARED_MEMORY_SIZE, TELEMETRY_OFFSET,
        TELEMETRY_VEHICLES_OFFSET,
    };

    struct ScriptedSource {
        availability: VecDeque<LmuAvailability>,
        snapshots: VecDeque<Result<LmuSnapshot, LmuCaptureError>>,
        connects: usize,
        disconnects: usize,
    }

    impl LmuSource for ScriptedSource {
        fn detect(&mut self) -> Result<LmuAvailability, LmuCaptureError> {
            Ok(self
                .availability
                .pop_front()
                .unwrap_or(LmuAvailability::NotRunning))
        }

        fn connect(&mut self) -> Result<(), LmuCaptureError> {
            self.connects += 1;
            Ok(())
        }

        fn snapshot(&mut self) -> Result<LmuSnapshot, LmuCaptureError> {
            self.snapshots.pop_front().expect("scripted LMU snapshot")
        }

        fn disconnect(&mut self) {
            self.disconnects += 1;
        }
    }

    fn put_i32(bytes: &mut [u8], offset: usize, value: i32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_f64(bytes: &mut [u8], offset: usize, value: f64) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn put_text(bytes: &mut [u8], offset: usize, value: &str) {
        bytes[offset..offset + value.len()].copy_from_slice(value.as_bytes());
    }

    fn snapshot(phase: u8, realtime: bool) -> LmuSnapshot {
        snapshot_with_finish_and_control(phase, realtime, 0, 0)
    }

    fn snapshot_with_finish(phase: u8, realtime: bool, finish_status: u8) -> LmuSnapshot {
        snapshot_with_finish_and_control(phase, realtime, finish_status, 0)
    }

    fn snapshot_with_finish_and_control(
        phase: u8,
        realtime: bool,
        finish_status: u8,
        control: i8,
    ) -> LmuSnapshot {
        let mut bytes = vec![0; SHARED_MEMORY_SIZE];
        put_i32(&mut bytes, 60, 202_610);
        bytes[TELEMETRY_OFFSET] = 1;
        bytes[TELEMETRY_OFFSET + 2] = 1;
        put_i32(&mut bytes, SCORING_OFFSET + 64, 10);
        put_f64(&mut bytes, SCORING_OFFSET + 88, 5_000.0);
        put_i32(&mut bytes, SCORING_OFFSET + 104, 1);
        bytes[SCORING_OFFSET + 108] = phase;
        bytes[SCORING_OFFSET + 115] = u8::from(realtime);
        put_text(&mut bytes, SCORING_OFFSET, "Sebring International");
        put_i32(&mut bytes, TELEMETRY_VEHICLES_OFFSET, 7);
        put_text(
            &mut bytes,
            TELEMETRY_VEHICLES_OFFSET + 796,
            "BMW M Hybrid V8",
        );
        put_i32(&mut bytes, SCORING_VEHICLES_OFFSET, 7);
        bytes[SCORING_VEHICLES_OFFSET + 103] = finish_status;
        bytes[SCORING_VEHICLES_OFFSET + 196] = 1;
        bytes[SCORING_VEHICLES_OFFSET + 197] = control.to_le_bytes()[0];
        LmuSnapshot::from_bytes(bytes).expect("valid fixture snapshot")
    }

    #[test]
    fn keeps_recording_after_global_session_end_until_the_player_finishes() {
        let source = ScriptedSource {
            availability: VecDeque::from([LmuAvailability::Available]),
            snapshots: VecDeque::from([
                Ok(snapshot(5, true)),
                Ok(snapshot_with_finish(PHASE_SESSION_OVER, true, 0)),
                Ok(snapshot_with_finish(PHASE_SESSION_OVER, true, 1)),
            ]),
            connects: 0,
            disconnects: 0,
        };
        let mut adapter = LmuAdapter::with_source(source);

        adapter.poll().expect("connect poll");
        let still_running = adapter.poll().expect("global finish poll");
        assert!(matches!(still_running.as_slice(), [AdapterEvent::Frame(_)]));
        let player_finished = adapter.poll().expect("player finish poll");
        assert!(matches!(
            player_finished.first(),
            Some(AdapterEvent::Frame(_))
        ));
        assert!(matches!(
            player_finished.last(),
            Some(AdapterEvent::Disconnected(DisconnectReason::SessionEnded))
        ));
    }

    #[test]
    fn identifies_replay_control_as_a_distinct_source_kind() {
        let source = ScriptedSource {
            availability: VecDeque::from([LmuAvailability::Available]),
            snapshots: VecDeque::from([Ok(snapshot_with_finish_and_control(5, false, 0, 3))]),
            connects: 0,
            disconnects: 0,
        };
        let mut adapter = LmuAdapter::with_source(source);

        let events = adapter.poll().expect("replay connect");
        assert!(matches!(
            events.first(),
            Some(AdapterEvent::Detected(source)) if source.kind == SourceKind::SimulatorReplay
        ));
    }

    #[test]
    fn reconnects_as_a_native_capture_when_leaving_the_monitor_for_the_car() {
        let source = ScriptedSource {
            availability: VecDeque::from([LmuAvailability::Available, LmuAvailability::Available]),
            snapshots: VecDeque::from([
                Ok(snapshot_with_finish_and_control(5, false, 0, 3)),
                Ok(snapshot(5, true)),
                Ok(snapshot(5, true)),
            ]),
            connects: 0,
            disconnects: 0,
        };
        let mut adapter = LmuAdapter::with_source(source);

        adapter.poll().expect("monitor connect");
        assert_eq!(
            adapter.poll().expect("monitor exit"),
            vec![AdapterEvent::Disconnected(DisconnectReason::SourceClosed)]
        );
        let driving = adapter.poll().expect("driving reconnect");
        assert!(matches!(
            driving.first(),
            Some(AdapterEvent::Detected(source)) if source.kind == SourceKind::NativeCapture
        ));
    }

    #[test]
    fn emits_connection_pause_resume_and_session_end_lifecycle() {
        let source = ScriptedSource {
            availability: VecDeque::from([LmuAvailability::Available]),
            snapshots: VecDeque::from([
                Ok(snapshot(5, true)),
                Ok(snapshot(PHASE_PAUSED, false)),
                Ok(snapshot(5, true)),
                Ok(snapshot(PHASE_SESSION_OVER, false)),
            ]),
            connects: 0,
            disconnects: 0,
        };
        let mut adapter = LmuAdapter::with_source(source);

        let connected = adapter.poll().expect("connect poll");
        assert!(matches!(connected.first(), Some(AdapterEvent::Detected(_))));
        assert!(
            connected
                .iter()
                .any(|event| matches!(event, AdapterEvent::Connected(_)))
        );
        assert!(matches!(connected.last(), Some(AdapterEvent::Frame(_))));

        let paused = adapter.poll().expect("pause poll");
        assert!(
            paused
                .iter()
                .any(|event| matches!(event, AdapterEvent::Paused))
        );
        let resumed = adapter.poll().expect("resume poll");
        assert!(
            resumed
                .iter()
                .any(|event| matches!(event, AdapterEvent::Resumed))
        );
        let ended = adapter.poll().expect("session end poll");
        assert!(matches!(ended.first(), Some(AdapterEvent::Frame(_))));
        assert!(matches!(
            ended.last(),
            Some(AdapterEvent::Disconnected(DisconnectReason::SessionEnded))
        ));
    }

    #[test]
    fn ignores_pre_session_pages_instead_of_creating_blank_sessions() {
        let source = ScriptedSource {
            availability: VecDeque::from([LmuAvailability::Available]),
            snapshots: VecDeque::from([Ok(snapshot(PHASE_BEFORE_SESSION, false))]),
            connects: 0,
            disconnects: 0,
        };
        let mut adapter = LmuAdapter::with_source(source);
        assert!(adapter.poll().expect("pre-session poll").is_empty());
        assert_eq!(adapter.source.connects, 1);
        assert_eq!(adapter.source.disconnects, 1);
    }
}
