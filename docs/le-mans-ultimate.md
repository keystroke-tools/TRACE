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

| Object                         | Purpose                                      |
| ------------------------------ | -------------------------------------------- |
| `LMU_Data`                     | 324,824-byte shared data mapping             |
| `LMU_SharedMemoryLockData`     | Shared `{ waiters, busy }` lock state        |
| `LMU_SharedMemoryLockEvent`    | Wakes consumers waiting on the shared lock   |
| `LMU_Data_HoldEvent`           | Signals the start of an update cycle         |
| `LMU_Data_DataEvent`           | Signals that the next update is ready        |

TRACE opens the data and lock objects created by LMU and makes a bounded spin-lock
attempt before copying the complete mapping into owned memory. It always releases the
lock and wakes existing waiters. Parsing happens only after the lock is released.
Temporary lock contention is reported distinctly and can be retried on the next poll.

The update events are not needed for TRACE's polling capture loop. They remain useful
for a future event-driven acquisition mode.

## Validated layout

The current decoder supports the SDK's V01 structures with `#pragma pack(4)` on
64-bit Windows:

| Structure / region              | Size or mapping offset |
| ------------------------------- | ---------------------- |
| `SharedMemoryObjectOut`         | 324,824 bytes          |
| scoring region                  | byte 1,632             |
| player scoring record           | 584 bytes              |
| telemetry region                | byte 128,464           |
| player telemetry record         | 1,888 bytes            |
| maximum vehicles                | 104                    |

LMU identifies the player separately in telemetry and scoring. TRACE validates both
vehicle counts, checks the telemetry player index, then matches the scoring record by
the player flag or vehicle ID. It never assumes both arrays use the same index.

## Canonical data mapped now

| TRACE data                        | LMU source                                                         |
| --------------------------------- | ------------------------------------------------------------------ |
| track, vehicle, driver            | scoring and player telemetry identity                              |
| session type                      | test day, practice, qualifying, warmup, or race                    |
| lap count and normalized position | scoring lap count/distance plus reported track length              |
| current lap and sector timing     | elapsed/lap-start time and cumulative scoring sectors              |
| lap validity                      | authoritative `lap invalidated` flag                               |
| track-limit progression           | simulator track-limit steps                                        |
| throttle, brake, clutch           | normalized player inputs                                           |
| steering angle                    | normalized input × reported physical wheel range                   |
| speed, RPM, gear, fuel            | player telemetry                                                   |
| position, velocity, acceleration  | player world/vehicle vectors                                       |
| tyres                             | angular speed, load, pressure, carcass temperature, suspension     |
| conditions                        | ambient and track temperature                                      |
| pit state                         | scoring pit flags/state                                            |
| additional native values          | fuel capacity, best-lap delta, rear brake bias                      |

LMU uses `-1` for reverse, `0` for neutral, and positive values for forward gears.
The source sector encoding is `0 = sector 3`, `1 = sector 1`, `2 = sector 2`; TRACE
normalizes that to zero-based sector indexes while preserving simulator timing.

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

The unit suite uses constructed, identity-free snapshots. Before declaring live LMU
capture supported, record a real session fixture that strips player identity and any
unrelated scoring stream data, then test connection, pause, pit, session-end, validity,
sector, and reconnect behaviour against it.

Build the Windows collector from WSL with:

```bash
mise run build-lmu-fixture-collector
```

With LMU in an active session, run `capture_fixture.exe <output-directory>`. The
collector rebuilds a one-player mapping and removes driver names, Steam IDs, other
vehicles, result streams, filesystem paths, and reserved expansion bytes before it
writes `shared-memory.bin` and its text manifest. Inspect that output before adding an
explicitly authorised fixture under `crates/trace-lmu/tests/fixtures/`.
