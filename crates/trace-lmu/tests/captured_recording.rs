use std::{
    collections::BTreeSet,
    io::{Cursor, Read},
};

use trace_domain::{ElapsedNanoseconds, FrameSequence, Gear};
use trace_lmu::LmuSnapshot;

const RECORDING: &[u8] = include_bytes!("fixtures/lmu-v01-spa-practice/snapshots.trlmu");

#[test]
fn captured_lmu_v01_run_maps_through_the_canonical_boundary() {
    let mut input = Cursor::new(RECORDING);
    let (interval_ms, snapshot_bytes) = read_header(&mut input);
    assert_eq!(interval_ms, 1_000);
    assert_eq!(snapshot_bytes, 324_824);

    let mut samples = 0_u64;
    let mut laps = BTreeSet::new();
    let mut sectors = BTreeSet::new();
    let mut pit_states = BTreeSet::new();
    let mut saw_realtime = false;
    let mut saw_monitor = false;
    let mut saw_pits = false;
    let mut saw_forward = false;
    let mut saw_neutral = false;
    let mut max_speed = 0.0_f32;
    let mut max_throttle = 0.0_f32;
    let mut max_brake = 0.0_f32;
    let mut min_steering = f32::INFINITY;
    let mut max_steering = f32::NEG_INFINITY;

    while input.position() < u64::try_from(RECORDING.len()).unwrap() {
        let (elapsed_ms, bytes) = read_snapshot(&mut input, snapshot_bytes);
        let snapshot = LmuSnapshot::from_bytes(bytes).expect("valid redacted LMU snapshot");
        let session = snapshot.map_session().expect("session metadata");
        assert_eq!(session.car_id.as_deref(), Some("Duqueine D09 P3"));
        assert_eq!(
            session.track_id.as_deref(),
            Some("Circuit de Spa-Francorchamps")
        );
        assert_eq!(session.session_type.as_deref(), Some("practice"));
        assert_eq!(snapshot.driver_name(), None);

        let frame = snapshot
            .map_frame(
                FrameSequence(samples),
                ElapsedNanoseconds(elapsed_ms * 1_000_000),
            )
            .expect("canonical frame");
        laps.extend(frame.lap.completed_laps);
        sectors.extend(frame.lap.current_sector_index);
        assert_ne!(frame.lap.invalidated, Some(true));
        assert_eq!(frame.wheels.len(), 4);
        max_speed = max_speed.max(frame.vehicle.speed_mps.unwrap_or_default());
        max_throttle = max_throttle.max(frame.inputs.throttle.unwrap_or_default());
        max_brake = max_brake.max(frame.inputs.brake.unwrap_or_default());
        if let Some(steering) = frame.inputs.steering_angle_rad {
            min_steering = min_steering.min(steering);
            max_steering = max_steering.max(steering);
        }
        saw_forward |= matches!(frame.vehicle.gear, Some(Gear::Forward(_)));
        saw_neutral |= frame.vehicle.gear == Some(Gear::Neutral);
        let realtime = snapshot.in_realtime().expect("realtime flag");
        saw_realtime |= realtime;
        saw_monitor |= !realtime;
        let native = frame.native.expect("native LMU payload");
        saw_pits |= native.integer_fields.get("in_pits") == Some(&1);
        pit_states.extend(native.integer_fields.get("pit_state").copied());
        samples += 1;
    }

    assert_eq!(samples, 600);
    assert_eq!(laps, BTreeSet::from([0, 1, 2, 3]));
    assert_eq!(sectors, BTreeSet::from([0, 1, 2]));
    assert_eq!(pit_states, BTreeSet::from([0, 1, 5]));
    assert!(saw_realtime && saw_monitor && saw_pits && saw_forward && saw_neutral);
    assert!(max_speed > 75.0 && max_throttle > 0.99 && max_brake > 0.99);
    assert!(
        min_steering < -0.1 && max_steering > 0.1,
        "steering range was {min_steering}..{max_steering} rad"
    );
}

fn read_header(input: &mut Cursor<&[u8]>) -> (u32, usize) {
    let mut header = [0_u8; 16];
    input.read_exact(&mut header).unwrap();
    assert_eq!(&header[..8], b"TRLMUR01");
    (
        u32::from_le_bytes(header[8..12].try_into().unwrap()),
        usize::try_from(u32::from_le_bytes(header[12..16].try_into().unwrap())).unwrap(),
    )
}

fn read_snapshot(input: &mut Cursor<&[u8]>, expected_size: usize) -> (u64, Vec<u8>) {
    let mut elapsed = [0_u8; 8];
    let mut length = [0_u8; 4];
    input.read_exact(&mut elapsed).unwrap();
    input.read_exact(&mut length).unwrap();
    let mut compressed = vec![0; usize::try_from(u32::from_le_bytes(length)).unwrap()];
    input.read_exact(&mut compressed).unwrap();
    let bytes = miniz_oxide::inflate::decompress_to_vec_zlib(&compressed).unwrap();
    assert_eq!(bytes.len(), expected_size);
    (u64::from_le_bytes(elapsed), bytes)
}
