# MoTeC telemetry import

TRACE is building MoTeC interoperability around documented interchange paths. The
first implementation boundary is CSV data exported by MoTeC i2. Native `.ld` parsing
is not currently supported because MoTeC does not publish that binary layout as an
open interchange specification; its documented integration is the licensed Windows
COM API supplied with i2 Pro.

This page distinguishes implemented parser foundations from user-facing support.
TRACE must not advertise an import format until it has been validated against an
authorised real-world fixture and connected to session persistence.

## Current implementation

The `trace-motec` crate provides a bounded streaming reader for files identifying
themselves as `MoTeC CSV File`. It:

- inspects source details, channel names, and the associated unit row without
  buffering the telemetry body;
- establishes elapsed time from the first sample and rejects decreasing timestamps;
- converts only conservative name-and-unit channel matches into TRACE's canonical
  telemetry model;
- retains other numeric and text values in the source-native sample maps;
- streams frames so the complete CSV does not need to be held in memory; and
- enforces byte, row, column, preamble, and field-size limits.

The crate does not assign a simulator, car, track, driver, session type, or lap
validity. MoTeC is the telemetry tool, not necessarily the simulator that produced the
data. Missing identity must be selected or entered by the user during the eventual
import workflow.

## Provisional canonical mappings

Mappings are deliberately narrower than fuzzy name matching. A recognised name with
an unknown or incompatible unit remains native data.

| Source meaning | Accepted units | TRACE value |
| --- | --- | --- |
| Time | seconds, milliseconds, microseconds | elapsed nanoseconds |
| Throttle, brake, clutch position | percent or ratio | normalized input ratio |
| Steering angle | degrees or radians | radians |
| Ground/vehicle speed | km/h, m/s, mph | metres per second |
| Engine RPM | rpm | revolutions per minute |
| Fuel level/remaining | litres | litres |
| Gear | integral value, `R`, or `N` | canonical gear state |
| Position X/Y/Z | metres, millimetres, kilometres | source-world metres |

Brake pressure is not treated as brake pedal position. A track point is exposed only
when both planar X and Z coordinates exist for that sample. These mappings remain
provisional until a real i2 export establishes the actual channel spelling, units,
missing-value representation, and interpolation behaviour.

## Semantics still requiring a fixture

MoTeC channels can have independent sampling rates, time ranges, units, and sample
validity through the official API. A real CSV export is required to determine what
i2 does when flattening those channels into CSV. TRACE must validate:

- whether rows are sampled, interpolated, or emitted at a configured common rate;
- how invalid samples and gaps are represented;
- whether full-outing exports include lap or beacon boundaries;
- how distance, GPS, and local position channels are named and oriented;
- whether source details reliably contain venue, vehicle, driver, and session data;
- how duplicate channel names are represented; and
- how exports vary between i2 versions and Standard versus Pro data.

Until then, the parser does not infer laps or claim track-map compatibility. Telemetry
without position can still become useful for input, speed, RPM, and other graphs once
the persistence workflow is connected.

## Fixture requirements

The first fixture should be a small whole-outing export containing all available
channels. Record the i2 version, export settings, known car, track, driver, session
type, lap count, and lap times. If its owner permits it, retain the corresponding
`.ld` privately for comparison. A second export without GPS or position data should
exercise partial imports.

Before committing any fixture, confirm redistribution permission and remove personal
or identifying data that is not needed by the test. Synthetic fixtures remain clearly
labelled synthetic and cannot be used as evidence of compatibility.

## Planned desktop flow

1. Detect `.trace` packages and MoTeC CSV as distinct import formats.
2. Inspect the CSV and show its source details, channels, warnings, and missing
   identity before writing anything.
3. Let the user confirm simulator, car, track, layout, driver, and session type when
   they are absent or ambiguous.
4. Stream canonical frames into the normal Arrow writer and create SQLite metadata
   with `imported` provenance.
5. Index laps only from validated source evidence; otherwise retain the import without
   inventing lap validity or boundaries.
6. Report unsupported channels and fidelity limitations in the completion summary.

TRACE-to-MoTeC export is a separate concern. Importing CSV datasets into i2 and
creating `.ld` files are MoTeC feature-licence workflows, so any future export option
must explain its software/licence requirements and be tested in supported MoTeC
software.

## Official references

- [MoTeC i2 product and data-export overview](https://www.motec.com.au/products/I2)
- [MoTeC i2 API user guide](https://website.motec.com.au/hessian/uploads/i2_API_User_Guide_330ed28c83.pdf)
- [Tracking issue #1](https://github.com/keystroke-tools/TRACE/issues/1)
