# Le Mans Ultimate shared-memory integration

TRACE's LMU integration targets the official shared-memory interface bundled with
Le Mans Ultimate. It does not require the older community rFactor 2 shared-memory
plugin, and it does not treat imported MoTeC logs as a substitute for live simulator
state.

This document describes the first implemented foundation: synchronized acquisition,
bounded V01 decoding, and canonical channel mapping. Desktop adapter selection and a
privacy-redacted real-session fixture are the next integration slices.

## SDK and licensing boundary

An LMU installation includes the authoritative SDK under:

```text
<LMU install>/Support/SharedMemoryInterface/
```

The installed `SharedMemoryInterface.hpp`, `InternalsPlugin.hpp`, and
`PluginObjects.hpp` files are proprietary and expressly prohibit redistributing or
modifying the headers. TRACE therefore does **not** vendor them. The byte layout used
by `trace-lmu` was measured against the locally installed headers with Microsoft's
x64 ABI and the SDK's required four-byte structure packing.

Every decoder read is bounds checked. Raw mapped pointers remain inside
`trace-windows-shmem`, the workspace's only audited unsafe-code boundary.

## Windows objects and synchronization

| Object                      | Purpose                                    |
| --------------------------- | ------------------------------------------ |
| `LMU_Data`                  | 324,824-byte shared data mapping           |
| `LMU_SharedMemoryLockData`  | Shared `{ waiters, busy }` lock state      |
| `LMU_SharedMemoryLockEvent` | Wakes consumers waiting on the shared lock |
| `LMU_Data_HoldEvent`        | Signals the start of an update cycle       |
| `LMU_Data_DataEvent`        | Signals that the next update is ready      |

TRACE opens the data and lock objects created by LMU and makes a bounded spin-lock
attempt before copying the complete mapping into owned memory. It always releases the
lock and wakes existing waiters. Parsing happens only after the lock is released.
Temporary lock contention is reported distinctly and can be retried on the next poll.

The update events are not needed for TRACE's polling capture loop. They remain useful
for a future event-driven acquisition mode.

## Validated layout

The current decoder supports the SDK's V01 structures with `#pragma pack(4)` on
64-bit Windows:

| Structure / region      | Size or mapping offset |
| ----------------------- | ---------------------- |
| `SharedMemoryObjectOut` | 324,824 bytes          |
| scoring region          | byte 1,632             |
| player scoring record   | 584 bytes              |
| telemetry region        | byte 128,464           |
| player telemetry record | 1,888 bytes            |
| maximum vehicles        | 104                    |

LMU identifies the player separately in telemetry and scoring. TRACE validates both
vehicle counts, checks the telemetry player index, then matches the scoring record by
the player flag or vehicle ID. It never assumes both arrays use the same index.

## Canonical data mapped now

| TRACE data                        | LMU source                                                                 |
| --------------------------------- | -------------------------------------------------------------------------- |
| track, vehicle, driver            | scoring and player telemetry identity                                      |
| session type                      | test day, practice, qualifying, warmup, or race                            |
| lap count and normalized position | scoring lap count/distance plus reported track length                      |
| current lap and sector timing     | elapsed/lap-start time plus current and last scoring splits                |
| lap validity                      | authoritative `lap invalidated` flag                                       |
| track-limit progression           | simulator track-limit steps                                                |
| throttle, brake, clutch           | normalized player inputs                                                   |
| steering angle                    | normalized input × physical range, falling back to visual range            |
| speed, RPM, gear, fuel            | player telemetry                                                           |
| position, velocity, acceleration  | player world/vehicle vectors                                               |
| tyres                             | angular speed, slip angle, load, pressure, carcass temperature, suspension |
| conditions                        | ambient and track temperature                                              |
| pit state                         | scoring pit flags/state                                                    |
| additional native values          | fuel capacity, best-lap delta, brake bias, steering ranges                 |

LMU uses `-1` for reverse, `0` for neutral, and positive values for forward gears.
Its SDK world coordinates are left-handed with Y pointing up. The adapter reflects
source X once and marks the result as TRACE-world coordinates so maps use the same
horizontal orientation as other integrations. When reading older LMU recordings,
TRACE applies that repair only to positions still marked as source-world; new
recordings are therefore never reflected twice.
Telemetry's `mCurrentSector` is already zero-based (`0`, `1`, `2`) and must not be
confused with the separate scoring-sector byte whose documented numbering is unusual.
Current-sector scoring values may arrive one scoring update after the sector changes,
so the recorder holds a pending crossing until its authoritative duration is available.
At the finish line, it derives sector three from the last lap and cumulative sector-two
values. A completed lap with an observed false invalidation flag is stored as valid;
sources without an authoritative validity channel remain unknown.

Tyre slip angle is derived from LMU's lateral and longitudinal ground-contact velocity
for each wheel and stored in TRACE's canonical wheel channel. Values below 1 m/s contact
speed are omitted because their direction is unstable. This enables the rule-based
corner balance estimate while preserving the underlying per-wheel signal for future
analysis.

In multiclass races, LMU can switch the global game phase to session-over when the
overall winner finishes while the player is still completing a final lap. TRACE keeps
recording in that state until LMU marks the player finished or removes them from
realtime driving, so a later-class chequered-flag lap is not truncated.

Returning from a live drive to LMU's monitor or main menu ends the current TRACE
recording. LMU's explicit paused phase remains a pause and does not split the session.
The adapter also watches the player telemetry session clock: if LMU closes while TRACE
still holds readable shared-memory handles, five seconds without a source-time update
is treated as a lost connection so the completed laps are finalized and saved.

## Replay capture and recovery

LMU keeps publishing the selected car through shared memory while a saved replay is
playing. Its scoring page marks replay as the vehicle's control source, which TRACE
uses to classify it as a simulator replay instead of a live drive or the normal
garage monitor. Replay telemetry is ignored
by default so merely reviewing footage cannot create sessions accidentally. Choose
**Arm next replay** on TRACE's Live page before loading or restarting the replay to
record it once.

The replay must be played through the laps that should be recovered. Result XML files
can supply authoritative lap and sector times, but they do not contain the sampled
inputs, vehicle motion, tyres, or racing line needed for lap visualisation and
comparison. A matching `.Vcr` replay can republish those samples for TRACE to capture.
When playback reaches a multiclass race finish, TRACE continues until the selected
player is marked finished rather than stopping as soon as the overall winner crosses
the line.

## Native preservation

Each canonical frame retains a compact `le-mans-ultimate.shared-memory/v01` envelope
containing exactly three source records:

1. scoring information;
2. the resolved player scoring record;
3. the resolved player telemetry record.

This is roughly 3 KiB per sample rather than copying LMU's entire 325 KiB mapping,
which also contains every other vehicle and large scoring streams. It preserves the
player data needed to promote more fields later without multiplying recording sizes by
two orders of magnitude.

## Compatibility and fixtures

LMU can extend its SDK independently of TRACE. A changed mapping must be validated
against the installed header before offsets or expected sizes are updated. TRACE must
fail closed on truncated or structurally impossible snapshots instead of interpreting
unknown bytes.

The suite includes constructed identity-free snapshots and an authorised, redacted
Spa practice capture. The real fixture covers an outlap, two completed timed laps, an
inlap, pit/monitor transitions, all three sectors, both steering directions, full pedal
travel, and Arrow/SQLite persistence. It strips player identity and unrelated scoring
data, then thins the original 10 Hz capture to 1 Hz for CI.

Build the Windows collector from WSL with:

```bash
mise run build-lmu-fixture-collector
```

With LMU in an active session, run
`capture_lmu_fixture.exe <output-directory> [duration-seconds]`. The collector prints
`READY` before sampling at 10 Hz for up to ten minutes by default. Create an empty
`STOP` file in the output directory to finish early without corrupting the compressed
recording. It rebuilds every successful sample as a one-player mapping and removes
driver names, Steam IDs, other vehicles, result streams, filesystem paths, and reserved
expansion bytes before writing `snapshots.trlmu` and its text manifest. Inspect that
output before adding an explicitly authorised fixture under
`crates/trace-lmu/tests/fixtures/`.

Inspect and optionally thin a capture from 10 Hz to 1 Hz with:

```bash
cargo run -p trace-lmu --example inspect_lmu_recording -- \
  <output-directory>/snapshots.trlmu [thin-output.trlmu]
```

The inspector decodes every snapshot through the production LMU boundary and reports
lap, sector, phase, pit, input, motion, and steering-range coverage. The optional thin
output retains every tenth redacted snapshot for a smaller CI regression fixture; keep
the full local capture until the thinned fixture has passed its regression test.
