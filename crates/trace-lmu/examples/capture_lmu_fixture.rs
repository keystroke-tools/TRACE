//! Captures one privacy-redacted Le Mans Ultimate shared-memory fixture.

#[cfg(windows)]
use std::{env, fs, io, path::PathBuf};

#[cfg(windows)]
use trace_lmu::LmuSharedMemory;

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("usage: capture_lmu_fixture <output-directory>"))?;
    fs::create_dir_all(&output)?;

    let source = LmuSharedMemory::open().map_err(capture_error)?;
    let fixture = source
        .snapshot()
        .map_err(capture_error)?
        .redacted_fixture()
        .map_err(|error| io::Error::other(format!("LMU fixture mapping failed: {error:?}")))?;
    fs::write(output.join("shared-memory.bin"), &fixture.bytes)?;
    fs::write(
        output.join("manifest.txt"),
        format!(
            "format=trace.lmu.fixture.v1\nprivacy=one-player allowlist; driver names, Steam IDs, other cars, streams, paths, and expansion bytes removed\nsimulator_version={}\ncar_model={}\ntrack={}\nsession_type={}\nshared_memory_bytes={}\n",
            fixture.simulator_version.as_deref().unwrap_or("unknown"),
            fixture.car_model.as_deref().unwrap_or("unknown"),
            fixture.track.as_deref().unwrap_or("unknown"),
            fixture.session_type.as_deref().unwrap_or("unknown"),
            fixture.bytes.len(),
        ),
    )?;
    println!("wrote redacted LMU fixture to {}", output.display());
    Ok(())
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
