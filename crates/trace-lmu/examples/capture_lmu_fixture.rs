//! Records privacy-redacted Le Mans Ultimate shared-memory fixtures over time.

#[cfg(windows)]
use std::{
    env, fs,
    io::{self, BufWriter, Write},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

#[cfg(windows)]
use trace_lmu::LmuSharedMemory;

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = env::args_os().nth(1).map(PathBuf::from).ok_or_else(|| {
        io::Error::other("usage: capture_lmu_fixture <output-directory> [duration-seconds]")
    })?;
    let duration = env::args_os()
        .nth(2)
        .map(|value| value.to_string_lossy().parse::<u64>())
        .transpose()?
        .unwrap_or(600);
    if duration == 0 {
        return Err(io::Error::other("duration must be greater than zero").into());
    }
    fs::create_dir_all(&output)?;
    let stop_path = output.join("STOP");
    if stop_path.exists() {
        fs::remove_file(&stop_path)?;
    }

    let source = LmuSharedMemory::open().map_err(capture_error)?;
    let first = source
        .snapshot()
        .map_err(capture_error)?
        .redacted_fixture()
        .map_err(|error| io::Error::other(format!("LMU fixture mapping failed: {error:?}")))?;
    let simulator_version = first
        .simulator_version
        .clone()
        .unwrap_or_else(|| "unknown".into());
    let car_model = first.car_model.clone().unwrap_or_else(|| "unknown".into());
    let track = first.track.clone().unwrap_or_else(|| "unknown".into());
    let session_type = first
        .session_type
        .clone()
        .unwrap_or_else(|| "unknown".into());
    let snapshot_bytes = first.bytes.len();
    let temporary_path = output.join("snapshots.trlmu.tmp");
    let recording_path = output.join("snapshots.trlmu");
    let mut writer = BufWriter::new(fs::File::create(&temporary_path)?);
    writer.write_all(b"TRLMUR01")?;
    writer.write_all(&100_u32.to_le_bytes())?;
    writer.write_all(&u32::try_from(first.bytes.len())?.to_le_bytes())?;

    println!(
        "READY: recording redacted LMU telemetry for up to {duration}s; create {} to stop",
        stop_path.display()
    );
    io::stdout().flush()?;

    let started = Instant::now();
    let mut next_sample = started;
    let mut sample_count = 0_u64;
    let mut skipped_samples = 0_u64;
    let mut fixture = Some(first);
    while started.elapsed() < Duration::from_secs(duration) && !stop_path.exists() {
        if let Some(current) = fixture.take() {
            write_snapshot(&mut writer, started.elapsed(), &current.bytes)?;
            sample_count += 1;
        } else {
            match source.snapshot().and_then(|snapshot| {
                snapshot
                    .redacted_fixture()
                    .map_err(trace_lmu::LmuCaptureError::InvalidPage)
            }) {
                Ok(current) => {
                    write_snapshot(&mut writer, started.elapsed(), &current.bytes)?;
                    sample_count += 1;
                }
                Err(_) => skipped_samples += 1,
            }
        }
        next_sample += Duration::from_millis(100);
        thread::sleep(next_sample.saturating_duration_since(Instant::now()));
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    drop(writer);
    if recording_path.exists() {
        fs::remove_file(&recording_path)?;
    }
    fs::rename(&temporary_path, &recording_path)?;
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    fs::write(
        output.join("manifest.txt"),
        format!(
            "format=trace.lmu.recording.v1\ncompression=zlib-records\nprivacy=one-player allowlist; driver names, Steam IDs, other cars, streams, paths, and expansion bytes removed\nsimulator_version={simulator_version}\ncar_model={car_model}\ntrack={track}\nsession_type={session_type}\nsnapshot_bytes={snapshot_bytes}\nsample_interval_ms=100\nsample_count={sample_count}\nskipped_samples={skipped_samples}\nduration_ms={elapsed_ms}\n",
        ),
    )?;
    println!(
        "COMPLETE: wrote {sample_count} redacted snapshots ({skipped_samples} skipped) to {}",
        recording_path.display()
    );
    Ok(())
}

#[cfg(windows)]
fn write_snapshot(
    writer: &mut BufWriter<fs::File>,
    elapsed: Duration,
    bytes: &[u8],
) -> io::Result<()> {
    let elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    let compressed = miniz_oxide::deflate::compress_to_vec_zlib(bytes, 6);
    let compressed_len = u32::try_from(compressed.len())
        .map_err(|_| io::Error::other("compressed LMU snapshot is too large"))?;
    writer.write_all(&elapsed_ms.to_le_bytes())?;
    writer.write_all(&compressed_len.to_le_bytes())?;
    writer.write_all(&compressed)
}

#[cfg(windows)]
fn capture_error(error: trace_lmu::LmuCaptureError) -> io::Error {
    io::Error::other(format!("Le Mans Ultimate capture failed: {error:?}"))
}

#[cfg(not(windows))]
fn main() {
    eprintln!("capture_lmu_fixture must be compiled for and run on Windows");
    std::process::exit(1);
}
