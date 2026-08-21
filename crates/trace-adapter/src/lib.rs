//! Adapter lifecycle shared by live simulator and replay sources.

use std::collections::VecDeque;

use trace_domain::{ChannelCapabilities, SessionSeed, SourceDescriptor, TelemetryFrame};

/// Adapter-specific identity visible to the host for diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterIdentity {
    pub key: String,
    pub display_name: String,
    pub version: String,
}

/// Why an attached simulator or replay source disconnected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisconnectReason {
    SourceClosed,
    SessionEnded,
    DataUnavailable,
    Other(String),
}

/// Ordered output from a simulator or replay adapter.
#[derive(Clone, Debug, PartialEq)]
pub enum AdapterEvent {
    Detected(SourceDescriptor),
    Connected(SessionSeed),
    CapabilitiesChanged(ChannelCapabilities),
    SessionChanged(SessionSeed),
    Frame(TelemetryFrame),
    Paused,
    Resumed,
    Disconnected(DisconnectReason),
}

/// A recoverable or fatal acquisition problem.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdapterError {
    TemporarilyUnavailable(String),
    InvalidSource(String),
    Fatal(String),
}

/// Stateful producer of canonical telemetry and lifecycle events.
pub trait SimulatorAdapter {
    /// Returns stable adapter identity.
    fn identity(&self) -> &AdapterIdentity;

    /// Returns the next bounded batch of ordered events.
    ///
    /// # Errors
    ///
    /// Returns an acquisition error without panicking. Hosts decide retry policy.
    fn poll(&mut self) -> Result<Vec<AdapterEvent>, AdapterError>;
}

/// Deterministic adapter for fixtures, integration tests, and offline development.
#[derive(Clone, Debug)]
pub struct ReplayAdapter {
    identity: AdapterIdentity,
    events: VecDeque<AdapterEvent>,
    max_events_per_poll: usize,
}

impl ReplayAdapter {
    /// Creates a replay adapter that emits at most `max_events_per_poll` each poll.
    ///
    /// # Errors
    ///
    /// Returns [`AdapterError::InvalidSource`] for a zero batch limit.
    pub fn new(
        identity: AdapterIdentity,
        events: impl IntoIterator<Item = AdapterEvent>,
        max_events_per_poll: usize,
    ) -> Result<Self, AdapterError> {
        if max_events_per_poll == 0 {
            return Err(AdapterError::InvalidSource(
                "replay poll batch size must be greater than zero".into(),
            ));
        }

        Ok(Self {
            identity,
            events: events.into_iter().collect(),
            max_events_per_poll,
        })
    }

    /// Number of events that have not yet been emitted.
    pub fn remaining(&self) -> usize {
        self.events.len()
    }
}

impl SimulatorAdapter for ReplayAdapter {
    fn identity(&self) -> &AdapterIdentity {
        &self.identity
    }

    fn poll(&mut self) -> Result<Vec<AdapterEvent>, AdapterError> {
        let count = self.max_events_per_poll.min(self.events.len());
        Ok(self.events.drain(..count).collect())
    }
}

#[cfg(test)]
mod tests {
    use trace_domain::{ElapsedNanoseconds, FrameSequence};

    use super::*;

    fn identity() -> AdapterIdentity {
        AdapterIdentity {
            key: "replay".into(),
            display_name: "TRACE Replay".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    #[test]
    fn replay_preserves_order_and_batch_bounds() {
        let events = (0..3).map(|sequence| {
            AdapterEvent::Frame(TelemetryFrame {
                sequence: FrameSequence(sequence),
                elapsed: ElapsedNanoseconds(sequence * 10),
                ..TelemetryFrame::default()
            })
        });
        let mut replay = ReplayAdapter::new(identity(), events, 2).expect("valid replay");

        let first = replay.poll().expect("first batch");
        assert_eq!(first.len(), 2);
        assert!(matches!(
            &first[0],
            AdapterEvent::Frame(frame) if frame.sequence == FrameSequence(0)
        ));
        assert_eq!(replay.remaining(), 1);

        let second = replay.poll().expect("second batch");
        assert!(matches!(
            &second[0],
            AdapterEvent::Frame(frame) if frame.sequence == FrameSequence(2)
        ));
        assert!(replay.poll().expect("exhausted replay").is_empty());
    }

    #[test]
    fn replay_rejects_unbounded_zero_progress_configuration() {
        assert!(matches!(
            ReplayAdapter::new(identity(), [], 0),
            Err(AdapterError::InvalidSource(_))
        ));
    }
}
