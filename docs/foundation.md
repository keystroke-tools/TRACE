# Implemented foundation

This document describes code that currently exists on `master`. It is not a list of
planned v0.1 features. The broader direction remains in [the specification](../SPEC.md).

## Workspace boundaries

```text
trace-adapter ---> trace-domain <--- trace-ac
                        ^
                        |
                   trace-core

trace-storage     trace-protocol

trace-ac ---> trace-windows-shmem (Windows platform boundary only)
```

The crates intentionally have no circular dependencies:

- `trace-domain` owns simulator-independent telemetry and capability types.
- `trace-adapter` owns acquisition lifecycle events and the replay adapter.
- `trace-core` owns deterministic distance-domain mathematics and analysis results.
- `trace-ac` privately reads documented vanilla AC page prefixes and maps them into
  canonical domain values.
- `trace-storage` owns immutable telemetry blob and SQLite metadata boundaries.
- `trace-protocol` owns bounded, versioned network data-transfer objects.

No implemented domain or analysis crate depends on Tauri, React, HTTP, a simulator
SDK, or an LLM provider. `apps/desktop/src-tauri` is the desktop composition root;
`trace-windows-shmem` is the isolated Win32 acquisition boundary.

## Desktop shell

The Tauri 2, React, and Tailwind CSS 4 shell establishes the four top-level sections,
hard-edged TRACE palette/layout, tabular status values, and a `TelemetryDataSource`
boundary. TRACE colors and typography are CSS-first Tailwind theme variables. Browser
development uses an explicitly labelled replay fixture; the Tauri build obtains DTOs
through commands rather than a local HTTP server.

The Sessions route now defines the first typed session/lap summary presentation.
Browser development displays one replay fixture, while the native command returns an
empty archive until SQLite recording persistence is connected. The UI therefore does
not imply that capture data has been stored when it has not. Compare and setup
workspaces remain intentionally absent.

Frontend typechecking and production builds run through pnpm/Mise. Building the
native shell on Linux additionally requires Tauri's GTK 3, Pango, and WebKitGTK host
packages; these are system prerequisites and are not installed by this repository.

## Canonical telemetry

`trace-domain` groups a `TelemetryFrame` by meaning:

- sequence and monotonic elapsed time
- lap observations
- driver inputs
- vehicle state
- motion and position
- four-corner wheel/tyre state
- optional environment state

Internal physical values use SI units. Names retain unit suffixes where that avoids
ambiguity, such as `speed_mps`, `position_m`, and `tyre_pressure_pa`. Vector values
carry an explicit coordinate frame.

Missing values use `Option<T>`. Session-wide channel discovery separately describes
each channel as available, intermittent, unsupported, or unknown. A channel also
records whether it was measured, simulator-derived, or TRACE-derived and may preserve
its source field name. Missing data is therefore different from a measured zero.

Canonical gear values do not leak simulator encodings. Wheels use fixed canonical
corners (`FrontLeft`, `FrontRight`, `RearLeft`, `RearRight`). Simulator-specific
structures must be converted before producing these models.

## Adapter lifecycle

`trace-adapter::SimulatorAdapter` is a stateful, polled event source. It produces
bounded ordered batches of:

```text
Detected
Connected
CapabilitiesChanged
SessionChanged
Frame
Paused / Resumed
Disconnected
```

Errors distinguish temporary unavailability, invalid source data, and fatal adapter
failure. Retry and backoff remain host responsibilities so a network publisher cannot
control or stop local capture.

`ReplayAdapter` emits the same events deterministically with a configured maximum
batch size. It is the current basis for offline development and later integration
fixtures.

## Assetto Corsa boundary

`trace-ac` reads owned little-endian byte snapshots rather than casting shared memory
to packed Rust structs. The unavoidable Win32 handle and volatile-read operations are
isolated in `trace-windows-shmem`; no mapped pointer or borrowed view escapes that
crate. This keeps unsafe code out of the telemetry pipeline and makes prefix
length/offset validation explicit. `trace-ac` currently maps documented driver inputs,
speed/RPM/gear/fuel, velocity, G acceleration, tyre core temperature, suspension
travel, lap observations, world position, car/track identity, and temperatures.

Fields whose units or semantics remain uncertain are intentionally unavailable.
Windows mapping detection and bounded packet-stable owned snapshots are implemented;
the AC adapter now emits canonical detection, connection, capability, session,
pause/resume, frame, disconnect, and reconnect transitions. When live/replay packet
identifiers stop advancing for five seconds, it closes the stale connection; explicit
pause state suspends that timer. Recording and persistence are connected through the
desktop capture worker. See [the AC boundary document](assetto-corsa.md).

## Distance-domain analysis

`trace-core` validates telemetry series before analysis:

- distance and values must be finite
- distance must be non-negative and strictly increasing
- elapsed lap time must be non-negative and non-decreasing
- interpolation never extrapolates outside the observed range
- intervals larger than a configured maximum gap remain unavailable

Continuous channels use piecewise-linear interpolation. Discrete channels such as
gear use previous-value hold. Uniform grids include the exact lap endpoint and reject
invalid or impractically large configurations.

### Lap delta

Lap delta is computed from measured elapsed time as a function of distance:

```text
delta(d) = comparison_time(d) - reference_time(d) - initial_common_offset
```

A positive delta means the comparison lap is behind. Removing the first jointly
valid offset accounts for independently captured clock origins. Missing intervals
remain missing; the authoritative delta is not smoothed. Tests cover analytic
constant-speed laps, different source sampling, offsets, and telemetry gaps.

## Structured analysis results

Analysis uses a serializable envelope containing:

- schema and algorithm version
- explicit availability
- typed result value
- numerical evidence with unit, derivation, channels, and distance range
- validated confidence from zero to one
- uncertainty reasons
- comparison context

Human-readable prose is not the primary analysis representation. This keeps the
mathematics usable by the UI, deterministic presentation, tests, and a possible
future coaching layer without coupling `trace-core` to an LLM.

## Telemetry blob storage

Raw high-rate telemetry does not use per-sample SQLite rows. `trace-storage` defines
an immutable blob lifecycle:

```text
begin -> append bounded chunks -> verify and commit
                         `-----> abort
```

Pending bytes are invisible to readers. Commit checks a SHA-256 digest when the
caller supplies one, calculates and persists the authoritative digest, rejects path
collisions, and then atomically makes metadata and bytes visible. Failed validation
leaves the blob pending for explicit cleanup or crash reconciliation.

Blob paths are normalized portable relative paths. Absolute paths, traversal,
backslashes, empty components, and control characters are rejected. The in-memory
implementation is a tested fixture.

Apache Arrow IPC schema v2 writes random-access files with TRACE format/schema/SI
metadata and 46 aligned nullable columns. It preserves sequence and monotonic time,
lap observations, every driver and vehicle field, explicit gear variants, motion
vectors with coordinate-frame tags, all four wheel states, and environment data.
Gear kind and raw value are separate so unknown simulator values round-trip without
colliding with reverse, neutral, or forward gears. The reader continues to accept the
seven-column schema v1 projection. Round-trip tests preserve missing values and reject
malformed or foreign schemas. Standard Arrow record-batch metadata declares
compression without a custom wrapper. Zstandard is the default; LZ4 frame and
uncompressed policies remain available for comparisons. The checked-in benchmark
verified 30-minute synthetic streams at 60, 120, and 333 Hz. Zstandard reduced size
by 32.6–33.5% relative to uncompressed IPC, and the largest case encoded in under one
second on the reference development system. The complete reproducible method and raw
measurements are in [the storage benchmark](storage-benchmark.md).

SQLite lap metadata provides a validated blob path, sample start, and sample count.
The Arrow range reader uses that interval to return the common analysis-entry
projection (sequence, elapsed time, throttle, brake, speed, RPM, and lap position)
while retaining at most one record batch. Ranges may cross batch boundaries; zero,
overflowing, missing, or out-of-file ranges are rejected. Arrow's footer does not
contain row counts per record batch, so the current reader visits preceding batches
to establish their sample offsets without retaining them. A persisted batch-row index
can remove that scan if benchmarks show it matters.

## SQLite metadata

The initial forward-only migration creates tables for:

- simulators, tracks, and cars
- sessions and laps
- telemetry blob indexes
- reconstructed track geometry
- setup snapshots and revisions
- analysis cache entries
- favourite laps

Foreign keys and integrity checks are enabled. Indexes cover session time, track/car
lookup, session laps, and session blob lookup. SQLite's `user_version` records the
schema version, and databases created by a newer unsupported version are rejected.
There is deliberately no raw telemetry sample table.

The metadata repository now creates sessions together with normalized simulator,
track, and car identities. Session completion atomically inserts the committed blob
index, validates every lap sample range against that blob, inserts laps, and closes
the session. Invalid ranges or missing/open-state conflicts roll back without partial
lap metadata.

A bounded recent-session query returns display-safe session and lap summaries. The
native Tauri command opens `trace.sqlite` in the platform application-data directory
and maps those summaries into the typed Sessions UI. Lap durations are formatted from
integer nanoseconds; the command does not read or fabricate telemetry samples.

`FileBlobStore` stages bounded writes beneath `.pending` in the dedicated telemetry
root. Commit syncs the staged file and publishes it with a same-volume hard link, so
an existing destination is never overwritten. Blob identity is the SHA-256 digest of
its bytes. Opening the store rebuilds the identifier-to-path index from committed
files, while interrupted staging files remain isolated and enumerable for recovery.
Symbolic links are not followed during reconstruction.

## Verification

The workspace currently runs formatting, Clippy with warnings denied, unit tests,
and documentation tests through the Mise-managed Rust toolchain. Tests include
identifier/capability behavior, replay ordering, interpolation, lap delta, immutable
blob lifecycle, path security, SQLite migration/foreign keys, and protocol limits.

The recorded replay integration fixture additionally proves that canonical frames
can be emitted in bounded adapter batches, separated into laps, normalized onto a
uniform distance grid, and compared as a deterministic cumulative delta trace.

## Phase 1 acceptance

The Phase 1 foundation scope in `SPEC.md` is implemented:

| Requirement | Acceptance evidence |
| --- | --- |
| Workspace and repository | Cargo and pnpm workspaces, Mise-pinned tools, MIT license, and full-workspace CI |
| Canonical telemetry types | `trace-domain` owns simulator-independent frames, units, identity, availability, and provenance |
| Simulator adapter interface | `SimulatorAdapter` defines bounded polling and lifecycle events; `ReplayAdapter` is deterministic |
| Test/replay adapter | Recorded two-lap fixture exercises replay through distance-aligned delta analysis |
| Local storage abstractions | Immutable content-addressed blobs, SQLite metadata migrations, and Arrow IPC telemetry batches |
| Minimal desktop shell | Tauri/React shell reports foundation state through a typed command boundary |
| TRACE design system | Shared dark telemetry workspace tokens, typography, spacing, panels, status, and channel components |

Phase 1 deliberately does not claim a usable telemetry recorder. Phase 2 now includes
live Assetto Corsa acquisition, conservative canonical lap segmentation, transactional
session metadata writes, and the native session archive query. Durable Arrow blob
orchestration and the continuously running desktop capture worker are now connected.
The Arrow representation is accepted as the current storage baseline. Synthetic
30-minute 60–333 Hz benchmarks now confirm Zstandard as its default compression;
real Assetto Corsa fixtures remain necessary to track production data characteristics.

Completed canonical recordings pass through `trace-recorder::persistence`: frames are
encoded before staging begins, the immutable Arrow blob is committed first, and the
blob reference, lap ranges, and session end are then committed in one SQLite
transaction. Append failures abort staging. If SQLite fails after the blob commit,
the error carries the committed blob metadata as an explicit reconciliation record;
the blob is never silently deleted or reported as indexed.

At desktop startup, reconciliation compares committed filesystem paths with the
SQLite blob index. Unreferenced committed blobs and interrupted staging files are
hard-linked into `.orphaned` and then removed from their live locations. This is a
recoverable quarantine, not deletion. Reconciliation runs before capture opens a new
pending handle.

The Tauri composition root starts a dedicated Assetto Corsa polling thread at roughly
60 Hz. Canonical adapter events feed `SessionRecorder`; session starts create SQLite
identity rows, and completion flows through Arrow/blob/metadata persistence. A torn
packet is retried without closing the recording, while an explicit connection-loss
error finalizes it conservatively. Persistence failures are surfaced in worker status
and logged without terminating future capture attempts. The React shell polls worker
status and refreshes the Sessions archive while it is visible.

Live capture uses `SessionRecorder::streaming`, which retains only the previous frame,
lap boundaries, and sample counters. Each accepted frame moves directly into an Arrow
IPC encoder. The encoder flushes at 240 frames (approximately four seconds at the
current 60 Hz polling target) and writes into the bounded filesystem staging handle.
Session completion writes the standard Arrow footer, verifies the encoder and recorder
sample counts agree, and then follows the normal blob-before-metadata commit ordering.
The resulting artifact remains one standard Arrow IPC file; TRACE does not introduce
a proprietary container for streaming.
