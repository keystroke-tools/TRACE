use std::{env, fs};

use i3rs_core::LdFile;

fn main() -> Result<(), String> {
    let path = env::args().nth(1).ok_or_else(|| {
        "usage: cargo run -p trace-motec --example inspect_ld -- <file.ld>".to_owned()
    })?;
    let bytes = fs::read(&path).map_err(|error| format!("failed to read {path}: {error}"))?;
    let log = LdFile::from_bytes(bytes)?;
    println!(
        "driver={:?} vehicle={:?} venue={:?} date={:?} time={:?} comment={:?} event={:?} session={:?} channels={} duration={:.3}s",
        log.session.driver,
        log.session.vehicle_id,
        log.session.venue,
        log.session.date,
        log.session.time,
        log.session.short_comment,
        log.event.event_name,
        log.event.session,
        log.channels.len(),
        log.duration_secs()
    );
    if env::args().nth(2).as_deref() != Some("--selected-only") {
        for channel in &log.channels {
            println!(
                "{:3} {:5} Hz {:8} samples {:8} {:12} {}",
                channel.index,
                channel.freq,
                channel.n_data,
                channel.data_type.name(),
                channel.unit,
                channel.name
            );
        }
    }
    for name in [
        "Brake Pos",
        "Throttle Pos",
        "Clutch Pos",
        "Steering Angle",
        "Ground Speed",
        "Engine RPM",
        "Fuel Level",
        "Max Fuel",
        "Gear",
        "Car Coord X",
        "Car Coord Y",
        "Car Coord Z",
        "Car Pos Norm",
        "Session Lap Count",
        "Lap Invalidated",
        "Lap Time",
        "Num Tires Off Track",
    ] {
        let Some(channel) = log.find_channel_by_name(name) else {
            continue;
        };
        let Some(values) = log.read_channel_data(channel) else {
            continue;
        };
        let minimum = values.iter().copied().fold(f64::INFINITY, f64::min);
        let maximum = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        println!(
            "selected {name:?}: unit={:?} min={minimum:.6} max={maximum:.6} first={:?}",
            channel.unit,
            values.first()
        );
    }
    Ok(())
}
