//! Reproducible synthetic Arrow IPC storage benchmark.

use std::{collections::BTreeMap, env, time::Instant};

use trace_domain::{
    CoordinateFrame, DriverInputs, ElapsedNanoseconds, EnvironmentState, FrameSequence, Gear,
    LapObservation, MotionState, TelemetryFrame, Vector3, VehicleState, WheelCorner, WheelState,
};
use trace_storage::ipc::{IpcCompression, TelemetryIpcWriter, decode_columns};

const DEFAULT_SECONDS: u64 = 60;
const BATCH_FRAMES: usize = 240;

fn main() {
    let seconds = env::var("TRACE_BENCH_SECONDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_SECONDS);
    assert!(seconds > 0, "TRACE_BENCH_SECONDS must be positive");

    println!("TRACE Arrow IPC synthetic benchmark");
    println!("duration={seconds}s batch_frames={BATCH_FRAMES} schema=2");
    println!("rate_hz\tcodec\tsamples\tbytes\twrite_ms\tread_ms");

    for rate_hz in [60_u32, 120, 333] {
        for compression in [
            IpcCompression::None,
            IpcCompression::Lz4Frame,
            IpcCompression::Zstd,
        ] {
            benchmark(rate_hz, seconds, compression);
        }
    }
}

fn benchmark(rate_hz: u32, seconds: u64, compression: IpcCompression) {
    let samples = u64::from(rate_hz)
        .checked_mul(seconds)
        .expect("benchmark sample count overflow");
    let started = Instant::now();
    let mut writer = TelemetryIpcWriter::with_compression(Vec::new(), BATCH_FRAMES, compression)
        .expect("writer");
    for sequence in 0..samples {
        writer
            .push(synthetic_frame(sequence, rate_hz))
            .expect("synthetic frame");
    }
    let (bytes, written) = writer.finish().expect("finish");
    let write_ms = started.elapsed().as_secs_f64() * 1_000.0;

    let started = Instant::now();
    let decoded = decode_columns(&bytes).expect("decode projection");
    let read_ms = started.elapsed().as_secs_f64() * 1_000.0;
    assert_eq!(written, samples);
    assert_eq!(
        decoded.len(),
        usize::try_from(samples).expect("sample count")
    );

    println!(
        "{rate_hz}\t{}\t{samples}\t{}\t{write_ms:.2}\t{read_ms:.2}",
        compression_name(compression),
        bytes.len()
    );
}

fn compression_name(compression: IpcCompression) -> &'static str {
    match compression {
        IpcCompression::None => "none",
        IpcCompression::Lz4Frame => "lz4-frame",
        IpcCompression::Zstd => "zstd",
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::too_many_lines
)]
fn synthetic_frame(sequence: u64, rate_hz: u32) -> TelemetryFrame {
    let cycle = u32::try_from(sequence % 10_000).expect("bounded cycle");
    let phase = f64::from(cycle) / 10_000.0;
    let wave = (phase * std::f64::consts::TAU).sin();
    let lap_samples = u64::from(rate_hz) * 90;
    let lap_sample = sequence % lap_samples;
    let lap_position = u32::try_from(lap_sample).expect("bounded lap sample");
    let lap_denominator = rate_hz * 90;
    let speed_mps = 52.0 + (wave * 18.0);
    let mut wheels = BTreeMap::new();
    for (corner, offset) in [
        (WheelCorner::FrontLeft, 0.0_f32),
        (WheelCorner::FrontRight, 0.2),
        (WheelCorner::RearLeft, 0.4),
        (WheelCorner::RearRight, 0.6),
    ] {
        wheels.insert(
            corner,
            WheelState {
                angular_speed_rad_s: Some((speed_mps as f32 / 0.33) + offset),
                tyre_pressure_pa: Some(190_000.0 + (wave as f32 * 3_000.0) + offset),
                tyre_core_temperature_c: Some(82.0 + (wave as f32 * 7.0) + offset),
                suspension_travel_m: Some(0.05 + (wave as f32 * 0.01) + (offset / 100.0)),
            },
        );
    }

    TelemetryFrame {
        sequence: FrameSequence(sequence),
        elapsed: ElapsedNanoseconds(sequence * 1_000_000_000 / u64::from(rate_hz)),
        lap: LapObservation {
            completed_laps: Some(u32::try_from(sequence / lap_samples).expect("lap count")),
            normalized_position: Some(lap_position as f32 / lap_denominator as f32),
            current_lap_time_ns: Some(lap_sample * 1_000_000_000 / u64::from(rate_hz)),
            simulator_distance_m: Some(
                f64::from(lap_position) * 5_400.0 / f64::from(lap_denominator),
            ),
            current_sector_index: None,
            last_sector_time_ns: None,
            tyres_out: None,
        },
        inputs: DriverInputs {
            throttle: Some((0.65 + wave * 0.3).clamp(0.0, 1.0) as f32),
            brake: Some((-wave * 0.7).clamp(0.0, 1.0) as f32),
            clutch: Some(0.0),
            steering_angle_rad: Some((wave * 0.35) as f32),
        },
        vehicle: VehicleState {
            speed_mps: Some(speed_mps as f32),
            engine_rpm: Some((6_200.0 + wave * 1_900.0) as f32),
            gear: Some(Gear::Forward(4)),
            fuel_litres: Some((48.0 - phase * 3.0) as f32),
        },
        motion: MotionState {
            position_m: Some(Vector3 {
                x: phase * 5_400.0,
                y: wave * 120.0,
                z: wave * 2.0,
                frame: CoordinateFrame::TraceWorld,
            }),
            velocity_mps: Some(Vector3 {
                x: speed_mps,
                y: wave * 3.0,
                z: 0.0,
                frame: CoordinateFrame::Vehicle,
            }),
            acceleration_mps2: Some(Vector3 {
                x: wave * 7.0,
                y: -wave * 4.0,
                z: 0.0,
                frame: CoordinateFrame::Vehicle,
            }),
        },
        wheels,
        environment: Some(EnvironmentState {
            ambient_temperature_c: Some(22.0),
            track_temperature_c: Some(31.0 + wave as f32),
            track_grip: Some(0.98),
        }),
        native: None,
    }
}
