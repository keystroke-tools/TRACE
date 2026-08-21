use std::time::Duration;

use trace_adapter::{AdapterEvent, AdapterIdentity, ReplayAdapter, SimulatorAdapter};
use trace_core::delta::{ElapsedTimeSeries, calculate_delta};
use trace_core::distance::{DistanceSample, uniform_grid};
use trace_domain::TelemetryFrame;

fn elapsed_time_series(frames: &[TelemetryFrame], lap: u32) -> ElapsedTimeSeries {
    let samples = frames
        .iter()
        .filter(|frame| frame.lap.completed_laps == Some(lap))
        .map(|frame| DistanceSample {
            distance_m: frame.lap.simulator_distance_m.expect("fixture distance"),
            value: Duration::from_nanos(frame.lap.current_lap_time_ns.expect("fixture lap time"))
                .as_secs_f64(),
        })
        .collect();

    ElapsedTimeSeries::new(samples).expect("valid fixture lap")
}

#[test]
fn recorded_replay_produces_distance_aligned_lap_delta() {
    let frames: Vec<TelemetryFrame> =
        serde_json::from_str(include_str!("fixtures/two_laps.json")).expect("valid fixture");
    let events = frames.iter().cloned().map(AdapterEvent::Frame);
    let mut replay = ReplayAdapter::new(
        AdapterIdentity {
            key: "fixture".into(),
            display_name: "Recorded fixture".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
        events,
        2,
    )
    .expect("valid replay");

    let mut emitted = Vec::new();
    while replay.remaining() > 0 {
        emitted.extend(
            replay
                .poll()
                .expect("fixture poll")
                .into_iter()
                .filter_map(|event| match event {
                    AdapterEvent::Frame(frame) => Some(frame),
                    _ => None,
                }),
        );
    }

    assert_eq!(emitted, frames);
    let reference = elapsed_time_series(&emitted, 0);
    let comparison = elapsed_time_series(&emitted, 1);
    let grid = uniform_grid(100.0, 25.0).expect("valid grid");
    let delta = calculate_delta(&reference, &comparison, &grid, 50.0).expect("valid delta");

    assert_eq!(delta.valid_samples, 5);
    assert_eq!(delta.samples[0].delta_s, Some(0.0));
    assert_eq!(delta.samples[2].delta_s, Some(1.0));
    assert_eq!(delta.samples[4].delta_s, Some(2.0));
}
