//! Captures one privacy-redacted Assetto Corsa shared-memory regression fixture.

#[cfg(windows)]
use std::{env, fs, io, path::PathBuf};

#[cfg(windows)]
use trace_ac::AcSharedMemory;

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("usage: capture_fixture <output-directory>"))?;
    fs::create_dir_all(&output)?;

    let mut source = AcSharedMemory::open().map_err(capture_error)?;
    let fixture = source
        .snapshot()
        .map_err(capture_error)?
        .redacted_fixture()
        .map_err(capture_error)?;
    fs::write(output.join("physics.bin"), &fixture.physics)?;
    fs::write(output.join("graphics.bin"), &fixture.graphics)?;
    fs::write(output.join("static.bin"), &fixture.static_page)?;
    fs::write(
        output.join("manifest.txt"),
        format!(
            "format=trace.ac.fixture.v1\nprivacy=static page rebuilt from non-personal allowlist\nshared_memory_version={}\nassetto_corsa_version={}\ncar_model={}\ntrack={}\nphysics_bytes={}\ngraphics_bytes={}\nstatic_bytes={}\n",
            fixture
                .shared_memory_version
                .as_deref()
                .unwrap_or("unknown"),
            fixture
                .assetto_corsa_version
                .as_deref()
                .unwrap_or("unknown"),
            fixture.car_model.as_deref().unwrap_or("unknown"),
            fixture.track.as_deref().unwrap_or("unknown"),
            fixture.physics.len(),
            fixture.graphics.len(),
            fixture.static_page.len(),
        ),
    )?;
    println!("wrote redacted AC fixture to {}", output.display());
    Ok(())
}

#[cfg(windows)]
fn capture_error(error: trace_ac::AcCaptureError) -> io::Error {
    io::Error::other(format!("Assetto Corsa capture failed: {error:?}"))
}

#[cfg(not(windows))]
fn main() {
    eprintln!("capture_fixture must be compiled for and run on Windows");
    std::process::exit(1);
}
