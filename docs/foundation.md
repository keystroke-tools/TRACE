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

The desktop contract exposes installed simulator descriptors, the selected adapter,
and a simulator identity on every archived session. Product copy, source labels,
filtering, and native-channel presentation derive from those descriptors rather than
assuming Assetto Corsa. The current registry contains one adapter; adding another
does not require a simulator-specific session browser.

Action feedback uses a shared bottom-right toast provider instead of inserting
messages into page layout. Toasts provide success, error, and informational variants,
accessible live announcements, bounded stacking, animated entry/exit, manual
dismissal, and a caller-selected timeout (including persistent notifications).

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

A user-authorized hotlap capture now supplies the first real regression fixture for
AC 1.16.4/shared-memory 1.7. It corrected previously synthetic-only static-page offsets
for car, track, and ambient/road temperatures. The adapter exposes the observed AC
version and rejects unverified shared-memory versions. Fixture export reconstructs
the static page from a non-personal allowlist so player identity fields are zeroed
before bytes are written.

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

Filesystem publication flushes the staged file through a writable handle before
creating its immutable link. Windows `FlushFileBuffers` rejects a read-only handle,
even when that handle can read and hash the complete file.

Blob paths are normalized portable relative paths. Absolute paths, traversal,
backslashes, empty components, and control characters are rejected. The in-memory
implementation is a tested fixture.

Apache Arrow IPC schema v4 writes random-access files with TRACE format/schema/SI
metadata and 53 aligned nullable columns. It preserves sequence and monotonic time,
lap observations, every driver and vehicle field, explicit gear variants, motion
vectors with coordinate-frame tags, all four wheel states, environment data, and AC's
observed sector index and last-sector time. Two stable extension columns retain a
source-native schema identifier and opaque payload. Three Arrow map columns expose
all documented source floats, integers/enums, and strings under stable names while
allowing later fields to be added without changing the top-level schema. The reader continues to
accept schemas v2/v3 and the seven-column schema v1 projection.
Gear kind and raw value are separate so unknown simulator values round-trip without
colliding with reverse, neutral, or forward gears. Round-trip tests preserve missing
values and reject
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
- sessions, laps, and genuine simulator-observed sector times
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
integer nanoseconds; the command does not read or fabricate telemetry samples. The
archive presents compact searchable, source-filtered, and sortable session rows.
Opening one moves into a dedicated session detail view, where the complete lap list can
use the available workspace without making the archive unwieldy. Fastest laps and
sectors are purple; green/yellow/grey sector bars distinguish improvements, slower
splits, and unavailable data. Persisted invalid laps and laps with three-plus-tyres-out
evidence use a red row treatment and are excluded from clean fastest-lap/sector
comparisons. Export formats remain behind a per-session action popover.

The detail command derives fuel used, maximum speed, and tyre condition independently
for each lap from its bounded Arrow sample range. Canonical columns provide fuel and
speed; the AC-native float map provides the four `tyreWear` values. Hotlap mode can
reset tyre condition near a boundary, so TRACE reports start-to-lowest-observed wear
within the lap rather than a misleading start-to-end recovery. These metrics are read
only when the user opens a session, keeping archive polling independent of recording
length.

## Lap visualization and comparison

Selecting any lap opens a dedicated telemetry workspace. TRACE reads only that lap's
recorded Arrow sample range and projects lap time, normalized position, controls,
speed, engine speed, gear, sector index, and world X/Z position. The native command
turns normalized position into metres using the simulator-reported track length, then
interpolates continuous channels onto a 5 m grid. Gear and sector use hold-previous
interpolation so discrete states are not blended into values that never existed.

The visualizer displays compact, channel-coloured speed, throttle, brake, RPM, and
gear charts alongside the car's recorded world-space path. A fixed telemetry HUD
keeps pedal positions, speed, gear, distance, session identity, and recorded air and
track temperatures visible without turning every value into another graph. Its seek
bar scrubs the shared distance cursor across every chart and the map; chart hover
updates the same persistent cursor. Full-lap and
simulator-reported sector controls filter both charts and the path; TRACE does not
guess sector boundaries when sector telemetry is absent. Invalid and incomplete laps
remain viewable because their telemetry can still diagnose a mistake.

The map supports zooming and panning and marks the start and current cursor position.
For Assetto Corsa, TRACE reads the selected layout's version-7 `ai/fast_lane.ai`
spline and constructs a road ribbon from its world-space centre points and left/right
AI boundary distances. Recorded world positions therefore share the same coordinate
space and need no image transform. The old `map.png`/`map.ini` overlay is deliberately
not used: official and mod tracks provide visually inconsistent rasters whose transforms
do not always agree with captured positions. The recorder stores AC's track configuration
for new sessions; older sessions recover it from immutable native telemetry. Missing,
oversized, malformed, ambiguous, or unsupported spline data falls back to the dotted
driven path and is labelled as lacking road edges. AI boundaries provide spatial
context, not authoritative legal track limits or barrier geometry.

Comparison uses the same projection for two complete valid laps from the same
simulator, track, and layout. Each lap has an independent session selector, so a
driver can compare separate visits, imported drivers, or two laps from one run. The
compact selectors live in the persistent comparison HUD rather than consuming the
analysis canvas. Percentage, speed, RPM, and gear readouts use whole numbers to avoid
presenting noisy precision that does not help the driver. The
default Overview keeps the track lines, finish result, and time-difference chart in
plain language; detailed speed, pedal, gear, and RPM traces live behind an optional
Telemetry view. It
calculates `comparison - reference` elapsed-time delta on their common distance domain
and overlays both channel sets and track lines. Positive delta therefore means the
comparison lap is behind. Both traces are solid: the faster lap is consistently purple
in every graph and on the map, while the other lap uses the channel colour. Missing
values and gaps longer than 30 m remain unavailable
rather than being extrapolated or connected with false precision.

The Live page's “What TRACE records” inventory separates portable analysis-ready
channels from the complete AC-native tyre, powertrain, chassis, session, and static
page groups. Native coverage is shown as recorded source data even when TRACE has not
yet promoted a field into a cross-simulator analytic meaning.
AC's instantaneous tyres-out count is also aggregated into bounded lap metadata for
quick archive display. A maximum of three or four is shown as track-limit evidence;
one or two remains `Recorded`, as do zero or missing evidence, rather than being
mislabelled `Valid`.

Completed sessions can be exported from the Sessions UI into the user's Downloads
directory. Arrow IPC export preserves the immutable full-fidelity recording. CSV
export streams the stable seven-column core projection (`sequence`, elapsed time,
inputs, speed, RPM, and normalized lap position) one bounded Arrow batch at a time;
missing optional values remain empty and units are named explicitly. TRACE does not
yet claim a portable `.trace` bundle or proprietary MoTeC writer.

Every recorded drive or replay can be given an optional custom display name, driver or
author, ownership marker (`mine`, `other`, or `unknown`), and up to 12 tags. SQLite
keeps these annotations separate from simulator-provided track/car identity. The
archive searches every annotation and visibly marks another driver's telemetry so an
imported reference does not look like the user's own lap. Removing a custom name
reveals the original track label again.

TRACE also reads Assetto Corsa's session classification from the graphics page and
stores it with the recording. The archive therefore distinguishes practice,
qualifying, race, hotlap, time attack, drift, and drag sessions instead of reducing
all native drives to a generic session label.

The Settings page lists the install directory used by each configurable simulator
adapter. Assetto Corsa is auto-detected from Steam libraries and can be overridden with
a validated game-root path stored in SQLite. TRACE uses that root to read AC's own car
and track UI metadata. It deliberately preserves raw source identifiers when metadata
is unavailable instead of accumulating an incomplete alias list.

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

The Tauri composition root selects an installed adapter and passes it into a generic
polling worker at roughly 60 Hz. Canonical adapter events feed `SessionRecorder`;
session starts create SQLite
identity rows, and completion flows through Arrow/blob/metadata persistence. A torn
packet is retried without closing the recording, while an explicit connection-loss
error finalizes it conservatively. Persistence failures are surfaced in worker status
and logged without terminating future capture attempts. The React shell polls worker
status and refreshes the Sessions archive while it is visible.

Session deletion is explicit and user-confirmed. The capture worker publishes the
active session identity so the desktop command can reject deletion while its Arrow
writer or persistence transaction is live. Completed and abandoned inactive sessions
are removed in a SQLite transaction; foreign-key cascades remove their laps and blob
metadata, after which the validated relative Arrow path is removed from local storage.
Filesystem cleanup failures are returned as visible warnings rather than being hidden.

The source descriptor follows each recording into session metadata. Live AC sessions
are stored as `native_capture`; telemetry observed while AC reports replay mode is
stored as `simulator_replay`; future file adapters use `imported`. Replay capture is
therefore analyzable through the same canonical pipeline without losing how the data
entered TRACE.

A completed-lap counter regression or jump indicates a seek, restart, or missed source
transition. The recorder closes the current partial stream and starts a conservatively
unbounded replacement at that counter. It never fills in the missing laps, and the
replacement's first observed lap is persisted as invalid and partial, with no claimed
duration. Subsequent laps receive complete sample ranges and monotonic boundary-to-
boundary durations when both boundaries are observed.

Live capture uses `SessionRecorder::streaming`, which retains only the previous frame,
lap boundaries, and sample counters. Each accepted frame moves directly into an Arrow
IPC encoder. The encoder flushes at 240 frames (approximately four seconds at the
current 60 Hz polling target) and writes into the bounded filesystem staging handle.
Session completion writes the standard Arrow footer, verifies the encoder and recorder
sample counts agree, and then follows the normal blob-before-metadata commit ordering.
The resulting artifact remains one standard Arrow IPC file; TRACE does not introduce
a proprietary container for streaming.
