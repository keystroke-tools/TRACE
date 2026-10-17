//! Summarises a redacted LMU fixture recording without needing the simulator.

use std::{
    collections::BTreeSet,
    env, fs,
    io::{self, BufWriter, ErrorKind, Read, Write},
    path::PathBuf,
};

use trace_domain::{ElapsedNanoseconds, FrameSequence};
use trace_lmu::LmuSnapshot;

#[derive(Default)]
struct Summary {
    samples: u64,
    last_elapsed_ms: u64,
    completed_laps: BTreeSet<u32>,
    sectors: BTreeSet<u32>,
    phases: BTreeSet<u8>,
    pit_states: BTreeSet<i64>,
    realtime_samples: u64,
    pit_samples: u64,
    invalidated_samples: u64,
    max_speed_mps: f32,
    max_throttle: f32,
    max_brake: f32,
    min_raw_steering: f64,
    max_raw_steering: f64,
    steering_ranges: BTreeSet<String>,
    visual_steering_ranges: BTreeSet<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args_os().skip(1).map(PathBuf::from);
    let path = arguments.next().ok_or_else(|| {
        io::Error::other("usage: inspect_lmu_recording <snapshots.trlmu> [thin-output.trlmu]")
    })?;
    let thin_path = arguments.next();
    let mut input = fs::File::open(path)?;
    let mut header = [0_u8; 16];
    input.read_exact(&mut header)?;
    if &header[..8] != b"TRLMUR01" {
        return Err(io::Error::other("unsupported LMU recording format").into());
    }
    let interval_ms = u32::from_le_bytes(header[8..12].try_into()?);
    let snapshot_bytes = usize::try_from(u32::from_le_bytes(header[12..16].try_into()?))?;
    let mut thin = thin_path
        .map(|path| ThinWriter::new(path, interval_ms.saturating_mul(10), snapshot_bytes))
        .transpose()?;
    let mut summary = Summary::default();
    let mut session = None;
    let mut previous_lap = None;
    let mut previous_sector = None;
    while let Some((elapsed_ms, bytes)) = read_snapshot(&mut input, snapshot_bytes)? {
        if summary.samples % 10 == 0
            && let Some(output) = &mut thin
        {
            output.write(elapsed_ms, &bytes)?;
        }
        let snapshot = LmuSnapshot::from_bytes(bytes)
            .map_err(|error| io::Error::other(format!("invalid snapshot: {error:?}")))?;
        session.get_or_insert(snapshot.map_session().map_err(page_error)?);
        let frame = snapshot
            .map_frame(
                FrameSequence(summary.samples),
                ElapsedNanoseconds(elapsed_ms.saturating_mul(1_000_000)),
            )
            .map_err(page_error)?;
        if frame.lap.completed_laps != previous_lap
            || frame.lap.current_sector_index != previous_sector
        {
            println!(
                "event elapsed_ms={elapsed_ms} lap={:?} sector={:?} last_sector_ns={:?}",
                frame.lap.completed_laps,
                frame.lap.current_sector_index,
                frame.lap.last_sector_time_ns
            );
            previous_lap = frame.lap.completed_laps;
            previous_sector = frame.lap.current_sector_index;
        }
        summary.samples += 1;
        summary.last_elapsed_ms = elapsed_ms;
        summary.completed_laps.extend(frame.lap.completed_laps);
        summary.sectors.extend(frame.lap.current_sector_index);
        summary
            .phases
            .insert(snapshot.game_phase().map_err(page_error)?);
        summary.realtime_samples += u64::from(snapshot.in_realtime().map_err(page_error)?);
        summary.invalidated_samples += u64::from(frame.lap.invalidated == Some(true));
        summary.max_speed_mps = summary
            .max_speed_mps
            .max(frame.vehicle.speed_mps.unwrap_or(0.0));
        summary.max_throttle = summary
            .max_throttle
            .max(frame.inputs.throttle.unwrap_or(0.0));
        summary.max_brake = summary.max_brake.max(frame.inputs.brake.unwrap_or(0.0));
        if let Some(native) = frame.native {
            summary.pit_samples += u64::from(native.integer_fields.get("in_pits") == Some(&1));
            summary
                .pit_states
                .extend(native.integer_fields.get("pit_state").copied());
            if let Some(steering) = native.float_fields.get("unfiltered_steering") {
                summary.min_raw_steering = summary.min_raw_steering.min(*steering);
                summary.max_raw_steering = summary.max_raw_steering.max(*steering);
            }
            summary.steering_ranges.extend(
                native
                    .float_fields
                    .get("physical_steering_wheel_range")
                    .map(|value| format!("{value:.0}")),
            );
            summary.visual_steering_ranges.extend(
                native
                    .float_fields
                    .get("visual_steering_wheel_range")
                    .map(|value| format!("{value:.0}")),
            );
        }
    }
    let session = session.ok_or_else(|| io::Error::other("recording contains no snapshots"))?;
    println!(
        "samples={} interval_ms={interval_ms} duration_ms={}",
        summary.samples, summary.last_elapsed_ms
    );
    println!("session={session:?}");
    println!(
        "completed_laps={:?} sectors={:?} phases={:?}",
        summary.completed_laps, summary.sectors, summary.phases
    );
    println!(
        "realtime_samples={} pit_samples={} pit_states={:?}",
        summary.realtime_samples, summary.pit_samples, summary.pit_states
    );
    println!(
        "invalidated_samples={} max_speed_mps={:.2} max_throttle={:.3} max_brake={:.3}",
        summary.invalidated_samples, summary.max_speed_mps, summary.max_throttle, summary.max_brake
    );
    println!(
        "raw_steering={:.3}..{:.3} physical_ranges={:?} visual_ranges={:?}",
        summary.min_raw_steering,
        summary.max_raw_steering,
        summary.steering_ranges,
        summary.visual_steering_ranges
    );
    if let Some(output) = thin {
        println!("thin_samples={}", output.finish()?);
    }
    Ok(())
}

struct ThinWriter {
    writer: BufWriter<fs::File>,
    samples: u64,
}

impl ThinWriter {
    fn new(path: PathBuf, interval_ms: u32, snapshot_bytes: usize) -> io::Result<Self> {
        let mut writer = BufWriter::new(fs::File::create(path)?);
        writer.write_all(b"TRLMUR01")?;
        writer.write_all(&interval_ms.to_le_bytes())?;
        writer.write_all(
            &u32::try_from(snapshot_bytes)
                .map_err(|_| io::Error::other("snapshot is too large"))?
                .to_le_bytes(),
        )?;
        Ok(Self { writer, samples: 0 })
    }

    fn write(&mut self, elapsed_ms: u64, bytes: &[u8]) -> io::Result<()> {
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(bytes, 6);
        let length = u32::try_from(compressed.len())
            .map_err(|_| io::Error::other("compressed snapshot is too large"))?;
        self.writer.write_all(&elapsed_ms.to_le_bytes())?;
        self.writer.write_all(&length.to_le_bytes())?;
        self.writer.write_all(&compressed)?;
        self.samples += 1;
        Ok(())
    }

    fn finish(mut self) -> io::Result<u64> {
        self.writer.flush()?;
        self.writer.get_ref().sync_all()?;
        Ok(self.samples)
    }
}

fn read_snapshot(input: &mut fs::File, expected_size: usize) -> io::Result<Option<(u64, Vec<u8>)>> {
    let mut elapsed = [0_u8; 8];
    match input.read(&mut elapsed[..1])? {
        0 => return Ok(None),
        1 => input.read_exact(&mut elapsed[1..])?,
        _ => unreachable!(),
    }
    let mut length = [0_u8; 4];
    input.read_exact(&mut length)?;
    let mut compressed = vec![0; usize::try_from(u32::from_le_bytes(length)).unwrap_or(usize::MAX)];
    input.read_exact(&mut compressed)?;
    let bytes = miniz_oxide::inflate::decompress_to_vec_zlib(&compressed)
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, format!("zlib: {error:?}")))?;
    if bytes.len() != expected_size {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "snapshot size mismatch",
        ));
    }
    Ok(Some((u64::from_le_bytes(elapsed), bytes)))
}

fn page_error(error: trace_lmu::LmuPageError) -> io::Error {
    io::Error::other(format!("LMU page error: {error:?}"))
}
