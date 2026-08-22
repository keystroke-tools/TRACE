//! Simulator-agnostic session and lap recording from ordered adapter events.

use trace_adapter::{AdapterEvent, DisconnectReason};
use trace_domain::{SessionSeed, SourceDescriptor, TelemetryFrame};

pub mod persistence;

/// A completed lap's location within a recorded canonical frame stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordedLap {
    pub lap_index: u32,
    pub started_offset_ns: u64,
    pub duration_ns: Option<u64>,
    pub sample_start: u64,
    pub sample_count: u64,
    pub partial: bool,
    pub max_tyres_out: Option<u8>,
    pub sectors: Vec<RecordedSector>,
}

/// One simulator-observed sector completed within a recorded lap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordedSector {
    pub index: u32,
    pub duration_ns: u64,
}

/// An immutable in-memory recording ready for encoding and durable persistence.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedSession {
    pub source: SourceDescriptor,
    pub seed: SessionSeed,
    pub frames: Vec<TelemetryFrame>,
    pub sample_count: u64,
    pub laps: Vec<RecordedLap>,
    pub end_reason: RecordingEndReason,
}

/// Why a recorder finalized a session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecordingEndReason {
    SessionChanged,
    CounterDiscontinuity,
    Disconnected(DisconnectReason),
}

/// State transition emitted after consuming one adapter event.
#[derive(Clone, Debug, PartialEq)]
pub enum RecorderOutput {
    SessionStarted {
        source: SourceDescriptor,
        seed: SessionSeed,
    },
    FrameAccepted(TelemetryFrame),
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
    SampleCountOverflow,
}

#[derive(Clone, Debug)]
struct ActiveSession {
    source: SourceDescriptor,
    seed: SessionSeed,
    frames: Vec<TelemetryFrame>,
    sample_count: u64,
    last_frame: Option<TelemetryFrame>,
    retain_frames: bool,
    laps: Vec<RecordedLap>,
    current_lap: Option<OpenLap>,
}

#[derive(Clone, Debug)]
struct OpenLap {
    index: u32,
    sample_start: u64,
    started_offset_ns: u64,
    started_at_boundary: bool,
    max_tyres_out: Option<u8>,
    current_sector_index: Option<u32>,
    sectors: Vec<RecordedSector>,
}

/// Converts adapter lifecycle events into bounded session recordings.
#[derive(Clone, Debug, Default)]
pub struct SessionRecorder {
    detected_source: Option<SourceDescriptor>,
    active: Option<ActiveSession>,
    retain_frames: bool,
}

impl SessionRecorder {
    /// Creates an idle recorder.
    pub fn new() -> Self {
        Self {
            retain_frames: true,
            ..Self::default()
        }
    }

    /// Creates a recorder that emits accepted frames without retaining session data.
    pub fn streaming() -> Self {
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
                self.active = Some(ActiveSession::new(
                    source.clone(),
                    seed.clone(),
                    self.retain_frames,
                ));
                Ok(vec![RecorderOutput::SessionStarted { source, seed }])
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
                self.active = Some(ActiveSession::new(
                    source.clone(),
                    seed.clone(),
                    self.retain_frames,
                ));
                output.push(RecorderOutput::SessionStarted { source, seed });
                Ok(output)
            }
            AdapterEvent::Frame(frame) => {
                let result = self
                    .active
                    .as_mut()
                    .ok_or(RecorderError::FrameOutsideSession)?
                    .push_frame(&frame);
                match result {
                    Ok(()) => Ok(vec![RecorderOutput::FrameAccepted(frame)]),
                    Err(
                        RecorderError::CompletedLapCounterRegressed
                        | RecorderError::CompletedLapCounterJumped,
                    ) => {
                        let active = self
                            .active
                            .take()
                            .ok_or(RecorderError::FrameOutsideSession)?;
                        let source = active.source.clone();
                        let seed = active.seed.clone();
                        let completed = active.finish(RecordingEndReason::CounterDiscontinuity);
                        let mut replacement =
                            ActiveSession::new(source.clone(), seed.clone(), self.retain_frames);
                        replacement.push_frame(&frame)?;
                        self.active = Some(replacement);
                        Ok(vec![
                            RecorderOutput::SessionCompleted(completed),
                            RecorderOutput::SessionStarted { source, seed },
                            RecorderOutput::FrameAccepted(frame),
                        ])
                    }
                    Err(error) => Err(error),
                }
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
    fn new(source: SourceDescriptor, seed: SessionSeed, retain_frames: bool) -> Self {
        Self {
            source,
            seed,
            frames: Vec::new(),
            sample_count: 0,
            last_frame: None,
            retain_frames,
            laps: Vec::new(),
            current_lap: None,
        }
    }

    fn push_frame(&mut self, frame: &TelemetryFrame) -> Result<(), RecorderError> {
        if let Some(previous) = &self.last_frame {
            if frame.sequence <= previous.sequence {
                return Err(RecorderError::NonIncreasingSequence);
            }
            if frame.elapsed <= previous.elapsed {
                return Err(RecorderError::NonIncreasingElapsedTime);
            }
        }

        let sample_index = self.sample_count;
        self.observe_sector(frame);
        if let Some(completed_laps) = frame.lap.completed_laps {
            match self.current_lap.as_ref() {
                None => {
                    self.current_lap = Some(OpenLap {
                        index: completed_laps,
                        sample_start: sample_index,
                        started_offset_ns: frame.elapsed.0,
                        started_at_boundary: false,
                        max_tyres_out: None,
                        current_sector_index: frame.lap.current_sector_index,
                        sectors: Vec::new(),
                    });
                }
                Some(open) if completed_laps == open.index => {}
                Some(open) if completed_laps == open.index + 1 => {
                    let open = self.current_lap.take().expect("open lap exists");
                    self.laps.push(RecordedLap {
                        lap_index: open.index.saturating_add(1),
                        started_offset_ns: open.started_offset_ns,
                        duration_ns: open
                            .started_at_boundary
                            .then_some(frame.elapsed.0 - open.started_offset_ns),
                        sample_start: open.sample_start,
                        sample_count: sample_index - open.sample_start,
                        partial: !open.started_at_boundary,
                        max_tyres_out: open.max_tyres_out,
                        sectors: open.sectors,
                    });
                    self.current_lap = Some(OpenLap {
                        index: completed_laps,
                        sample_start: sample_index,
                        started_offset_ns: frame.elapsed.0,
                        started_at_boundary: true,
                        max_tyres_out: None,
                        current_sector_index: frame.lap.current_sector_index,
                        sectors: Vec::new(),
                    });
                }
                Some(open) if completed_laps < open.index => {
                    return Err(RecorderError::CompletedLapCounterRegressed);
                }
                Some(_) => return Err(RecorderError::CompletedLapCounterJumped),
            }
        }
        if let (Some(open), Some(tyres_out)) = (&mut self.current_lap, frame.lap.tyres_out) {
            open.max_tyres_out = Some(
                open.max_tyres_out
                    .map_or(tyres_out, |value| value.max(tyres_out)),
            );
        }
        self.sample_count = self
            .sample_count
            .checked_add(1)
            .ok_or(RecorderError::SampleCountOverflow)?;
        self.last_frame = Some(frame.clone());
        if self.retain_frames {
            self.frames.push(frame.clone());
        }
        Ok(())
    }

    fn observe_sector(&mut self, frame: &TelemetryFrame) {
        let Some(open) = self.current_lap.as_mut() else {
            return;
        };
        let Some(current) = frame.lap.current_sector_index else {
            return;
        };
        let Some(previous) = open.current_sector_index.replace(current) else {
            return;
        };
        if current == previous {
            return;
        }
        if let Some(duration_ns) = frame.lap.last_sector_time_ns.filter(|value| *value > 0) {
            open.sectors.push(RecordedSector {
                index: previous.saturating_add(1),
                duration_ns,
            });
        }
    }

    fn finish(self, end_reason: RecordingEndReason) -> RecordedSession {
        RecordedSession {
            source: self.source,
            seed: self.seed,
            frames: self.frames,
            sample_count: self.sample_count,
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
        SourceKind,
    };

    use super::*;

    fn source() -> SourceDescriptor {
        SourceDescriptor {
            simulator: SimulatorId::parse("fixture").expect("valid simulator"),
            adapter_version: "1".into(),
            simulator_version: None,
            kind: SourceKind::SimulatorReplay,
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
    fn records_partial_and_completed_laps_with_exact_sample_ranges() {
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
        assert_eq!(session.sample_count, 7);
        assert_eq!(
            session.laps,
            vec![
                RecordedLap {
                    lap_index: 1,
                    started_offset_ns: 100,
                    duration_ns: None,
                    sample_start: 0,
                    sample_count: 3,
                    partial: true,
                    max_tyres_out: None,
                    sectors: Vec::new(),
                },
                RecordedLap {
                    lap_index: 2,
                    started_offset_ns: 400,
                    duration_ns: Some(300),
                    sample_start: 3,
                    sample_count: 3,
                    partial: false,
                    max_tyres_out: None,
                    sectors: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn records_maximum_tyres_out_as_lap_evidence() {
        let mut recorder = SessionRecorder::new();
        recorder
            .consume(AdapterEvent::Detected(source()))
            .expect("detected");
        recorder
            .consume(AdapterEvent::Connected(SessionSeed::default()))
            .expect("connected");
        for (sequence, completed, tyres_out) in
            [(1, 0, 0), (2, 1, 0), (3, 1, 1), (4, 1, 3), (5, 2, 0)]
        {
            let mut sample = frame(sequence, sequence * 100, completed, 0);
            sample.lap.tyres_out = Some(tyres_out);
            recorder
                .consume(AdapterEvent::Frame(sample))
                .expect("frame");
        }
        let output = recorder
            .consume(AdapterEvent::Disconnected(DisconnectReason::SessionEnded))
            .expect("disconnected");
        let RecorderOutput::SessionCompleted(session) = &output[0] else {
            panic!("expected completed session");
        };

        assert_eq!(session.laps[1].max_tyres_out, Some(3));
    }

    #[test]
    fn records_real_sector_crossings_with_the_completed_lap() {
        let mut recorder = SessionRecorder::new();
        recorder
            .consume(AdapterEvent::Detected(source()))
            .expect("detected");
        recorder
            .consume(AdapterEvent::Connected(SessionSeed::default()))
            .expect("connected");
        for (sequence, elapsed, completed, sector, last_sector_time) in [
            (1, 100, 0, 0, None),
            (2, 200, 0, 1, Some(30)),
            (3, 300, 0, 2, Some(40)),
            (4, 400, 1, 0, Some(50)),
            (5, 500, 1, 1, Some(29)),
            (6, 600, 1, 2, Some(39)),
            (7, 700, 2, 0, Some(49)),
        ] {
            let mut value = frame(sequence, elapsed, completed, 0);
            value.lap.current_sector_index = Some(sector);
            value.lap.last_sector_time_ns = last_sector_time;
            recorder.consume(AdapterEvent::Frame(value)).expect("frame");
        }

        let output = recorder
            .consume(AdapterEvent::Disconnected(DisconnectReason::SessionEnded))
            .expect("disconnected");
        let RecorderOutput::SessionCompleted(session) = &output[0] else {
            panic!("expected completed session");
        };
        assert_eq!(
            session.laps[1].sectors,
            vec![
                RecordedSector {
                    index: 1,
                    duration_ns: 29
                },
                RecordedSector {
                    index: 2,
                    duration_ns: 39
                },
                RecordedSector {
                    index: 3,
                    duration_ns: 49
                },
            ]
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
            [
                RecorderOutput::SessionCompleted(_),
                RecorderOutput::SessionStarted { seed, .. }
            ] if seed == &replacement
        ));
    }

    #[test]
    fn rejects_non_increasing_frame_order_without_appending_the_frame() {
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
    }

    #[test]
    fn splits_and_resynchronizes_after_a_lap_counter_discontinuity() {
        let mut recorder = SessionRecorder::new();
        recorder
            .consume(AdapterEvent::Detected(source()))
            .expect("detected");
        recorder
            .consume(AdapterEvent::Connected(SessionSeed::default()))
            .expect("connected");
        recorder
            .consume(AdapterEvent::Frame(frame(1, 100, 0, 100)))
            .expect("first frame");

        let output = recorder
            .consume(AdapterEvent::Frame(frame(2, 200, 4, 200)))
            .expect("counter jump resynchronizes");
        assert!(matches!(
            &output[..],
            [
                RecorderOutput::SessionCompleted(RecordedSession {
                    sample_count: 1,
                    end_reason: RecordingEndReason::CounterDiscontinuity,
                    ..
                }),
                RecorderOutput::SessionStarted { .. },
                RecorderOutput::FrameAccepted(frame),
            ] if frame.lap.completed_laps == Some(4)
        ));
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

    #[test]
    fn streaming_recorder_emits_frames_without_retaining_them() {
        let mut recorder = SessionRecorder::streaming();
        recorder
            .consume(AdapterEvent::Detected(source()))
            .expect("detected");
        recorder
            .consume(AdapterEvent::Connected(SessionSeed::default()))
            .expect("connected");
        let output = recorder
            .consume(AdapterEvent::Frame(frame(1, 100, 0, 0)))
            .expect("frame");
        assert!(matches!(&output[..], [RecorderOutput::FrameAccepted(_)]));
        let output = recorder
            .consume(AdapterEvent::Disconnected(DisconnectReason::SessionEnded))
            .expect("disconnect");
        let RecorderOutput::SessionCompleted(session) = &output[0] else {
            panic!("completed session");
        };
        assert!(session.frames.is_empty());
        assert_eq!(session.sample_count, 1);
    }
}
