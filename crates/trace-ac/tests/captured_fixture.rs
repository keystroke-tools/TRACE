use trace_ac::{AC_NATIVE_SCHEMA, AcSnapshot, decode_native_payload};
use trace_domain::{ElapsedNanoseconds, FrameSequence};

const PHYSICS: &[u8] = include_bytes!("fixtures/ac-1.16.4-sm-1.7/physics.bin");
const GRAPHICS: &[u8] = include_bytes!("fixtures/ac-1.16.4-sm-1.7/graphics.bin");
const STATIC: &[u8] = include_bytes!("fixtures/ac-1.16.4-sm-1.7/static.bin");

#[test]
fn captured_ac_1_16_4_fixture_maps_through_the_canonical_boundary() {
    let snapshot = AcSnapshot::from_pages(PHYSICS.to_vec(), GRAPHICS.to_vec(), STATIC.to_vec())
        .expect("version-labelled capture");
    let fixture = snapshot.redacted_fixture().expect("fixture metadata");
    assert_eq!(fixture.shared_memory_version.as_deref(), Some("1.7"));
    assert_eq!(fixture.assetto_corsa_version.as_deref(), Some("1.16.4"));
    assert_eq!(fixture.car_model.as_deref(), Some("ks_mazda_mx5_cup"));
    assert_eq!(fixture.track.as_deref(), Some("zandvoort2023"));
    assert_eq!(fixture.static_page, STATIC);

    let frame = snapshot
        .map_frame(FrameSequence(42), ElapsedNanoseconds(99))
        .expect("canonical frame");
    assert_eq!(frame.sequence, FrameSequence(42));
    assert!(frame.inputs.throttle.is_some());
    assert!(frame.vehicle.speed_mps.is_some());
    assert_eq!(frame.lap.completed_laps, Some(23));
    assert_eq!(frame.lap.current_lap_time_ns, Some(243_430_000_000));
    assert_eq!(frame.lap.normalized_position, Some(0.028_426_468));
    assert_eq!(frame.lap.current_sector_index, Some(0));
    assert_eq!(frame.lap.last_sector_time_ns, Some(36_370_000_000));
    let native = frame.native.as_ref().expect("lossless native sample");
    assert_eq!(native.schema, AC_NATIVE_SCHEMA);
    let pages = decode_native_payload(&native.payload).expect("native pages");
    assert_eq!(pages.physics, PHYSICS);
    assert_eq!(pages.graphics, GRAPHICS);
    assert_eq!(pages.static_page, STATIC);
    assert_eq!(frame.wheels.len(), 4);
}
