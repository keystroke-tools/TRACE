# MoTeC telemetry import

TRACE can import paired MoTeC i2 `.ld`/`.ldx` outings into the desktop session
library. It also has a parser foundation for CSV files exported from MoTeC i2, but
CSV is not connected to the desktop workflow yet.

MoTeC is the telemetry tool, not necessarily the simulator which produced a log.
Importing must preserve source identity and must not silently assume Assetto Corsa.

## Native `.ld` and `.ldx` foundation

The `trace-motec` crate can inspect and decode a native log from bytes. It uses the
MIT-licensed `i3rs-core` community reader behind TRACE-owned validation, resource
limits, and panic containment. TRACE does not use memory mapping for imports and does
not expose the dependency's writer.

The reader:

- rejects oversized or malformed inputs before decoding;
- limits channels, per-channel samples, sample rates, output frames, sidecar size,
  and lap markers;
- retains every readable numeric source channel in the frame's native data map;
- maps conservative channel-and-unit matches into TRACE's canonical telemetry model;
- resamples channels onto the highest source sample-rate grid without interpolation;
- reads `.ldx` lap markers and rebases the outing to local lap numbers; and
- exposes source metadata and a complete channel inventory before persistence.

The binary layout is not an official open interchange specification. Native support
therefore remains compatibility work backed by authorised real-world fixtures, not a
claim that every producer or i2 version is supported.

### Validated ACTI fixture

The initial fixture is an Assetto Corsa hotlap recorded by ACTI v1.1.2 and supplied
by Eduardo Cavalli. Two paired logs for the Mazda MX-5 Cup at Zandvoort 2023 are kept
under `crates/trace-motec/tests/fixtures/acti-zandvoort` with their provenance and
SHA-256 hashes.

Observed behaviour:

- both logs contain 169 numeric channels aligned at 20 Hz;
- `.ld` metadata identifies the driver, vehicle, venue, session, and event;
- `.ldx` version 1.5 records lap crossings as microsecond `Marker` timestamps;
- the first and last portions of each log are partial laps, while marker-to-marker
  spans are complete laps;
- ACTI's session lap count is absolute within the original game session, so TRACE
  uses `.ldx` crossings rather than importing that count directly; and
- ACTI calls its horizontal axes `Car Coord X/Y` and height `Car Coord Z`. TRACE maps
  those to source-world X/Z and height Y for its track-map convention, reflecting
  ACTI's X axis to match AC's fast-lane spline frame; and
- ACTI does not export AC's spline length, so TRACE derives a bounded track-length
  estimate from the recorded position path between complete `.ldx` lap markers.

The sidecars report a 1:51.996 fastest lap in stint 7 and 1:51.885 in stint 9. Tests
assert those markers, metadata, frame counts, representative values, the complete
native channel count, coordinate orientation, and rejection of malformed inputs.

## Canonical mappings

Mappings are deliberately narrower than fuzzy name matching. A recognised name with
an unknown or incompatible unit remains available as native data rather than being
guessed.

| Source data                      | Accepted native unit | TRACE value                                |
| -------------------------------- | -------------------- | ------------------------------------------ |
| Throttle, brake, clutch position | percent              | normalized input ratio                     |
| Steering angle                   | degrees              | radians                                    |
| Ground speed                     | km/h                 | metres per second                          |
| Engine speed                     | rpm                  | revolutions per minute                     |
| Fuel level                       | litres               | litres                                     |
| Gear                             | integral value       | reverse, neutral, forward, or source value |
| Car Coord X/Y/Z                  | metres               | source-world position                      |
| Car Pos Norm                     | ratio                | normalized lap position                    |
| Num Tires Off Track              | count                | tyres outside the track                    |
| Air/Road Temp                    | Celsius              | environment temperature                    |
| Surface Grip                     | percent              | normalized track grip                      |
| Wheel Angular Speed              | radians/second       | per-wheel angular speed                    |
| Tire Pressure                    | psi                  | per-wheel pascals                          |
| Tire Temp Core                   | Celsius              | per-wheel core temperature                 |
| Suspension Travel                | millimetres          | per-wheel metres                           |

`Lap Invalidated` and all other ACTI channels remain in native fields even when no
canonical field exists yet. Brake pressure is not treated as brake pedal position.

## MoTeC CSV foundation

The same crate provides a bounded streaming reader for files identifying themselves
as `MoTeC CSV File`. It inspects the preamble, channels, and units without buffering
the telemetry body; establishes elapsed time from the first row; rejects decreasing
timestamps; retains unrecognised numeric and text fields; and enforces byte, row,
column, preamble, and field-size limits.

CSV exports still need an authorised real-world fixture. Channels may have independent
sample rates and validity in i2, so TRACE must verify how i2 flattens interpolation,
gaps, duplicate names, lap beacons, source details, and position channels before the
CSV path is exposed in the application.

## Desktop import flow

Select the `.ld` file from **Sessions → Import telemetry**. TRACE automatically looks
for a same-named `.ldx` beside it, decodes the canonical and native channels, writes
the normal Arrow telemetry representation, and creates SQLite session and lap records
with imported provenance. The original driver is attributed as another driver so the
outing is not confused with the local profile.

This initial UI path requires the sidecar. It indexes the partial leading and trailing
segments as invalid laps and uses marker-to-marker spans as complete laps. ACTI's
`Lap Invalidated` channel determines validity when present; the fastest complete valid
lap is highlighted normally. An `AC_LIVE` event identifies Assetto Corsa without
making that simulator the default for other MoTeC producers.

A later inspection step will let users confirm missing or ambiguous simulator, car,
track/layout, driver, and session identity before writing. CSV desktop import also
remains future work.

TRACE-to-MoTeC export is separate work. Creating `.ld` files or importing datasets
into i2 may involve MoTeC software and feature licences, so any future export must
explain those requirements and be tested in supported MoTeC software.

## References

- [MoTeC i2 product and data-export overview](https://www.motec.com.au/products/I2)
- [MoTeC i2 API user guide](https://website.motec.com.au/hessian/uploads/i2_API_User_Guide_330ed28c83.pdf)
- [Tracking issue #1](https://github.com/keystroke-tools/TRACE/issues/1)
