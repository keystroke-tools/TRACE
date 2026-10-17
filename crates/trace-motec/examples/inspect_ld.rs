use std::{env, fs, path::Path};

use i3rs_core::LdFile;
use trace_motec::{LdImportLimits, MotecLdReader};

fn main() -> Result<(), String> {
    let path = env::args().nth(1).ok_or_else(|| {
        "usage: cargo run -p trace-motec --example inspect_ld -- <file.ld>".to_owned()
    })?;
    let bytes = fs::read(&path).map_err(|error| format!("failed to read {path}: {error}"))?;
    let log = LdFile::from_bytes(bytes.clone())?;
    let sidecar = fs::read(Path::new(&path).with_extension("ldx")).ok();
    let mut trace_reader = MotecLdReader::new(bytes, sidecar.as_deref(), LdImportLimits::default())
        .map_err(|error| format!("TRACE compatibility error: {error:?}"))?;
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
    println!(
        "TRACE resolved {} lap boundaries: {:?}",
        trace_reader.metadata().lap_boundaries.len(),
        trace_reader.metadata().lap_boundaries
    );
    let positions = std::iter::from_fn(|| trace_reader.next_frame())
        .filter_map(|frame| frame.motion.position_m)
        .collect::<Vec<_>>();
    if let Some(first) = positions.first() {
        let (min_x, max_x, min_z, max_z) = positions.iter().fold(
            (first.x, first.x, first.z, first.z),
            |(min_x, max_x, min_z, max_z), point| {
                (
                    min_x.min(point.x),
                    max_x.max(point.x),
                    min_z.min(point.z),
                    max_z.max(point.z),
                )
            },
        );
        println!(
            "TRACE mapped {} positions: x={min_x:.1}..{max_x:.1}m z={min_z:.1}..{max_z:.1}m",
            positions.len()
        );
    }
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
        "Lap Number",
        "Beacon (Internal)",
        "Marker",
        "Session Elapsed Time",
        "Steering",
        "Steering Wheel Position",
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
        if name == "Lap Number" {
            let transitions = values
                .windows(2)
                .enumerate()
                .filter_map(|(index, pair)| {
                    pair[0]
                        .total_cmp(&pair[1])
                        .is_ne()
                        .then_some((index + 1, pair[0], pair[1]))
                })
                .collect::<Vec<_>>();
            println!("transitions={transitions:?}");
        }
    }
    Ok(())
}
