//! Simulator-agnostic session and lap recording from ordered adapter events.

use trace_adapter::{AdapterEvent, DisconnectReason};
use trace_domain::{SessionSeed, SourceDescriptor, TelemetryFrame};

/// A completed lap's location within a recorded canonical frame stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordedLap {
    pub lap_index: u32,
    pub started_offset_ns: u64,
    pub duration_ns: Option<u64>,
    pub sample_start: u64,
    pub sample_count: u64,
}

/// An immutable in-memory recording ready for encoding and durable persistence.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedSession {
    pub source: SourceDescriptor,
    pub seed: SessionSeed,
    pub frames: Vec<TelemetryFrame>,
    pub laps: Vec<RecordedLap>,
    pub end_reason: RecordingEndReason,
}

/// Why a recorder finalized a session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecordingEndReason {
    SessionChanged,
    Disconnected(DisconnectReason),
}

/// State transition emitted after consuming one adapter event.
#[derive(Clone, Debug, PartialEq)]
pub enum RecorderOutput {
    SessionStarted(SessionSeed),
    SessionCompleted(RecordedSession),
}

/// Rejected lifecycle or frame ordering that would make lap ranges unreliable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecorderError {
    ConnectedBeforeDetection,
    SessionAlreadyActive,
    FrameOutsideSession,
    NonIncreasingSequence,
    NonIncreasingElapsedTime,
    CompletedLapCounterRegressed,
    CompletedLapCounterJumped,
}

#[derive(Clone, Debug)]
struct ActiveSession {
    source: SourceDescriptor,
    seed: SessionSeed,
    frames: Vec<TelemetryFrame>,
    laps: Vec<RecordedLap>,
    current_lap: Option<OpenLap>,
}

#[derive(Clone, Copy, Debug)]
struct OpenLap {
    index: u32,
    sample_start: usize,
    started_offset_ns: u64,
    started_at_boundary: bool,
}

/// Converts adapter lifecycle events into bounded session recordings.
#[derive(Clone, Debug, Default)]
pub struct SessionRecorder {
    detected_source: Option<SourceDescriptor>,
    active: Option<ActiveSession>,
}

impl SessionRecorder {
    /// Creates an idle recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies one ordered adapter event.
    ///
    /// # Errors
    ///
    /// Rejects invalid lifecycle ordering, non-monotonic frames, and ambiguous lap
    /// counter changes instead of persisting misleading lap ranges.
    pub fn consume(&mut self, event: AdapterEvent) -> Result<Vec<RecorderOutput>, RecorderError> {
        match event {
            AdapterEvent::Detected(source) => {
                self.detected_source = Some(source);
                Ok(Vec::new())
            }
            AdapterEvent::Connected(seed) => {
                if self.active.is_some() {
                    return Err(RecorderError::SessionAlreadyActive);
                }
                let source = self
                    .detected_source
                    .clone()
                    .ok_or(RecorderError::ConnectedBeforeDetection)?;
                self.active = Some(ActiveSession::new(source, seed.clone()));
                Ok(vec![RecorderOutput::SessionStarted(seed)])
            }
            AdapterEvent::SessionChanged(seed) => {
                let source = self
                    .detected_source
                    .clone()
                    .ok_or(RecorderError::ConnectedBeforeDetection)?;
                let mut output = Vec::new();
                if let Some(active) = self.active.take() {
                    output.push(RecorderOutput::SessionCompleted(
                        active.finish(RecordingEndReason::SessionChanged),
                    ));
                }
                self.active = Some(ActiveSession::new(source, seed.clone()));
                output.push(RecorderOutput::SessionStarted(seed));
                Ok(output)
            }
            AdapterEvent::Frame(frame) => {
                self.active
                    .as_mut()
                    .ok_or(RecorderError::FrameOutsideSession)?
                    .push_frame(frame)?;
                Ok(Vec::new())
            }
            AdapterEvent::Disconnected(reason) => Ok(self
                .active
                .take()
                .map(|active| {
                    vec![RecorderOutput::SessionCompleted(
                        active.finish(RecordingEndReason::Disconnected(reason)),
                    )]
                })
                .unwrap_or_default()),
            AdapterEvent::CapabilitiesChanged(_) | AdapterEvent::Paused | AdapterEvent::Resumed => {
                Ok(Vec::new())
            }
        }
    }
}

impl ActiveSession {
    fn new(source: SourceDescriptor, seed: SessionSeed) -> Self {
        Self {
            source,
            seed,
            frames: Vec::new(),
            laps: Vec::new(),
            current_lap: None,
        }
    }

    fn push_frame(&mut self, frame: TelemetryFrame) -> Result<(), RecorderError> {
        if let Some(previous) = self.frames.last() {
            if frame.sequence <= previous.sequence {
                return Err(RecorderError::NonIncreasingSequence);
            }
            if frame.elapsed <= previous.elapsed {
                return Err(RecorderError::NonIncreasingElapsedTime);
            }
        }

        let sample_index = self.frames.len();
        if let Some(completed_laps) = frame.lap.completed_laps {
            match self.current_lap {
                None => {
                    self.current_lap = Some(OpenLap {
                        index: completed_laps,
                        sample_start: sample_index,
                        started_offset_ns: frame.elapsed.0,
                        started_at_boundary: false,
                    });
                }
                Some(open) if completed_laps == open.index => {}
                Some(open) if completed_laps == open.index + 1 => {
                    let previous = self.frames.last().expect("open lap has a frame");
                    if open.started_at_boundary {
                        self.laps.push(RecordedLap {
                            lap_index: open.index,
                            started_offset_ns: open.started_offset_ns,
                            duration_ns: previous.lap.current_lap_time_ns,
                            sample_start: open.sample_start as u64,
                            sample_count: (sample_index - open.sample_start) as u64,
                        });
                    }
                    self.current_lap = Some(OpenLap {
                        index: completed_laps,
                        sample_start: sample_index,
                        started_offset_ns: frame.elapsed.0,
                        started_at_boundary: true,
                    });
                }
                Some(open) if completed_laps < open.index => {
                    return Err(RecorderError::CompletedLapCounterRegressed);
                }
                Some(_) => return Err(RecorderError::CompletedLapCounterJumped),
            }
        }
        self.frames.push(frame);
        Ok(())
    }

    fn finish(self, end_reason: RecordingEndReason) -> RecordedSession {
        RecordedSession {
            source: self.source,
            seed: self.seed,
            frames: self.frames,
            laps: self.laps,
            end_reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use trace_adapter::AdapterEvent;
    use trace_domain::{
        ElapsedNanoseconds, FrameSequence, LapObservation, SimulatorId, SourceDescriptor,
    };

    use super::*;

    fn source() -> SourceDescriptor {
        SourceDescriptor {
            simulator: SimulatorId::parse("fixture").expect("valid simulator"),
            adapter_version: "1".into(),
            simulator_version: None,
        }
    }

    fn frame(sequence: u64, elapsed: u64, completed: u32, lap_time: u64) -> TelemetryFrame {
        TelemetryFrame {
            sequence: FrameSequence(sequence),
            elapsed: ElapsedNanoseconds(elapsed),
            lap: LapObservation {
                completed_laps: Some(completed),
                current_lap_time_ns: Some(lap_time),
                ..LapObservation::default()
            },
            ..TelemetryFrame::default()
        }
    }

    #[test]
    fn records_only_completed_laps_with_exact_sample_ranges() {
        let mut recorder = SessionRecorder::new();
        recorder
            .consume(AdapterEvent::Detected(source()))
            .expect("detected");
        recorder
            .consume(AdapterEvent::Connected(SessionSeed::default()))
            .expect("connected");
        for value in [
            frame(1, 100, 0, 0),
            frame(2, 200, 0, 100),
            frame(3, 300, 0, 200),
            frame(4, 400, 1, 0),
            frame(5, 500, 1, 100),
            frame(6, 600, 1, 200),
            frame(7, 700, 2, 0),
        ] {
            assert!(recorder.consume(AdapterEvent::Frame(value)).is_ok());
        }

        let output = recorder
            .consume(AdapterEvent::Disconnected(DisconnectReason::SessionEnded))
            .expect("disconnected");
        let RecorderOutput::SessionCompleted(session) = &output[0] else {
            panic!("expected completed session");
        };
        assert_eq!(session.frames.len(), 7);
        assert_eq!(
            session.laps,
            vec![RecordedLap {
                lap_index: 1,
                started_offset_ns: 400,
                duration_ns: Some(200),
                sample_start: 3,
                sample_count: 3,
            }]
        );
    }

    #[test]
    fn session_change_finalizes_before_starting_the_replacement() {
        let mut recorder = SessionRecorder::new();
        recorder
            .consume(AdapterEvent::Detected(source()))
            .expect("detected");
        recorder
            .consume(AdapterEvent::Connected(SessionSeed::default()))
            .expect("connected");
        recorder
            .consume(AdapterEvent::Frame(frame(1, 100, 0, 0)))
            .expect("frame");

        let replacement = SessionSeed {
            source_session_id: Some("replacement".into()),
            ..SessionSeed::default()
        };
        let output = recorder
            .consume(AdapterEvent::SessionChanged(replacement.clone()))
            .expect("changed");
        assert!(matches!(
            &output[..],
            [RecorderOutput::SessionCompleted(_), RecorderOutput::SessionStarted(seed)]
                if seed == &replacement
        ));
    }

    #[test]
    fn rejects_ambiguous_ordering_without_appending_the_frame() {
        let mut recorder = SessionRecorder::new();
        recorder
            .consume(AdapterEvent::Detected(source()))
            .expect("detected");
        recorder
            .consume(AdapterEvent::Connected(SessionSeed::default()))
            .expect("connected");
        recorder
            .consume(AdapterEvent::Frame(frame(2, 200, 0, 100)))
            .expect("first frame");

        assert_eq!(
            recorder.consume(AdapterEvent::Frame(frame(1, 300, 0, 200))),
            Err(RecorderError::NonIncreasingSequence)
        );
        assert_eq!(
            recorder.consume(AdapterEvent::Frame(frame(3, 300, 2, 200))),
            Err(RecorderError::CompletedLapCounterJumped)
        );
    }

    #[test]
    fn rejects_a_second_connection_that_would_drop_an_active_recording() {
        let mut recorder = SessionRecorder::new();
        recorder
            .consume(AdapterEvent::Detected(source()))
            .expect("detected");
        recorder
            .consume(AdapterEvent::Connected(SessionSeed::default()))
            .expect("connected");

        assert_eq!(
            recorder.consume(AdapterEvent::Connected(SessionSeed::default())),
            Err(RecorderError::SessionAlreadyActive)
        );
    }
}
