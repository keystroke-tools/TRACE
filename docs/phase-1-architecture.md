# TRACE Phase 1 architecture review

Status: proposed for review  
Scope: Phase 1 only  
Date: 2026-08-21

## Executive decision

Build a Rust workspace around a small, dependency-light `trace-core`. Keep capture,
storage, packaging, transport, and presentation at its edges. Phase 1 should prove
those boundaries with canonical types, a replay adapter, a storage contract, a
minimal Tauri shell, and synthetic mathematical tests. It should not include live
AC capture, production telemetry analysis, import, sharing, or a backend.

The proposed dependency direction is:

```text
trace-ac / trace-motec / replay adapters
                    |
                    v
              trace-domain
                    |
                    v
               trace-core
                    |
        +-----------+-----------+
        v                       v
  trace-storage             trace-protocol
        |                       |
        v                       v
  Tauri desktop            server / web
```

`trace-domain` contains canonical data and stable identifiers. `trace-core`
contains deterministic algorithms and structured results. Neither knows about a
simulator, filesystem, database, UI, network, or provider.

## Investigation findings

### Assetto Corsa shared memory

Vanilla AC exposes three Windows named mappings: `acpmf_physics`,
`acpmf_graphics`, and `acpmf_static`. The public shared-memory reference documents
4-byte packing and packet IDs on changing pages. Physics is updated at the physics
rate, graphics at the rendered-frame rate, and static data is session-scoped.
TRACE must copy each page to an owned snapshot and accept it only when the packet
ID is stable before and after the copy. It must validate the shared-memory version
before decoding; a Rust `repr(C, packed(4))` mirror must remain private to
`trace-ac`.

Reference: [AC shared-memory reference](https://assettocorsamods.net/threads/doc-shared-memory-reference.58/).

Clean canonical mappings:

| AC page/field | Canonical channel | Conversion / note |
|---|---|---|
| physics `gas`, `brake` | throttle, brake | bounded ratio; retain out-of-range source values diagnostically |
| physics `steerAngle` | steering angle | radians; sign convention declared by adapter |
| physics `gear` | gear | AC encoding normalized to reverse/neutral/forward |
| physics `rpms` | engine speed | revolutions/minute |
| physics `speedKmh` | speed | convert to metres/second |
| physics `velocity[3]` | world velocity | metres/second, source coordinate frame recorded |
| physics `accG[3]` | vehicle acceleration | convert standard gravity to metres/second squared; frame recorded |
| physics wheel arrays | wheel state | fixed LF/RF/LR/RR ordering in adapter only |
| physics tyre temperature/pressure | tyre state | core temperature and pressure; units explicit |
| physics `suspensionTravel` | suspension travel | metres |
| physics `fuel` | fuel quantity | litres |
| graphics timing/lap fields | lap/session timing | simulator-reported observations, not assumed authoritative |
| graphics `normalizedCarPosition` | normalized lap position | ratio with wrap semantics |
| graphics `carCoordinates` | world position | metres, AC coordinate frame metadata |
| graphics `surfaceGrip` | track grip | dimensionless source value; semantics preserved |
| static car/track/version | session identity | source IDs preserved separately from display names |
| physics `airTemp`, `roadTemp` | conditions | degrees Celsius |

Available but semantically qualified: wheel slip, wheel load, tyre wear/dirty
level, camber, damage, DRS, ABS/TC activity, KERS, ride height, turbo boost,
ballast, penalties, flags, and assists. These enter canonical models only with a
documented unit and meaning; otherwise they remain namespaced source extras in the
raw capture manifest, not arbitrary values consumed by analysis.

Missing or insufficient in vanilla AC: individual brake temperatures, detailed
tyre surface temperature bands, reliable active setup identity, physical track
boundaries/centreline, weather model details, and an explicit lap-validity reason.
Tyre slip arrays require fixture validation before analysis use. `distanceTraveled`
is not assumed to be lap distance; distance is reconstructed/aligned from position
and normalized spline position.

### CSP enhancement

CSP has public configuration and Lua documentation, but no equally stable,
authoritative external shared-memory ABI was found. Phase 2 should probe a
versioned CSP capability provider behind `trace-ac`, never extend or reinterpret
vanilla structs based on presence alone. Candidate enhancements are richer tyre,
weather, track, and car state only after validating field definitions against the
installed CSP version and recorded fixtures. Vanilla operation must remain complete.

References: [CSP public configuration repository](https://github.com/ac-custom-shaders-patch/acc-extension-config),
[CSP Lua API documentation](https://lint069.github.io/csp-lua-api-docs/guides/getting-started/).

### AC setup discovery

Saved setups are INI-like files conventionally under the user's Documents tree,
partitioned by car and track. This does not prove which setup is active, and modded
cars define different keys. The safe design is a separate `SetupSource` that can
snapshot a specifically identified file, preserves its bytes/hash and source keys,
and maps known parameters through per-car metadata. Phase 2 must validate active
setup discovery against AC logs/runtime behaviour before claiming automatic capture.
Unknown values remain source values; they are not discarded or guessed.

### MoTeC feasibility

MoTeC i2 supports MoTeC log files and CSV exports; its API feature licence permits
export to other formats. The `.ld` binary layout is not publicly specified by
MoTeC in the material found. Therefore v0.1's reliable baseline is a channel-mapped
CSV importer with explicit units and provenance. Native `.ld` support is conditional
on a documented/licensed SDK or a separately reviewed, legally usable parser; it
must not be implemented from guessed layouts.

References: [MoTeC i2 product page](https://motec.com.au/products/i2),
[MoTeC data export documentation](https://help.motec.com.au/m1/tune/1.5/Topics/Data_Export_1.5.html).

### Plotting

Use uPlot behind a small React wrapper. It is Canvas-based, accepts aligned arrays,
supports missing data, zoom, streaming, and synchronized cursors. Keep cursor/range
state in a shared controller and update charts imperatively rather than rerendering
React on mouse movement. Benchmark actual TRACE arrays before adding downsampling;
use min/max envelope downsampling for wide views if required.

Reference: [uPlot project and performance notes](https://github.com/leeoniya/uPlot).

## Workspace

```text
apps/
  desktop/                 React application
    src-tauri/             Tauri composition root and commands
  web/                     reserved package shell; no Phase 1 feature UI
packages/
  telemetry-ui/            reusable React components and view models
crates/
  trace-domain/            canonical telemetry, IDs, units, provenance
  trace-core/              normalization/analysis contracts and algorithms
  trace-adapter/           adapter lifecycle traits and replay adapter contract
  trace-storage/           repository/blob traits plus SQLite/file implementation
  trace-ac/                private AC ABI and mapping (Phase 1 types/tests only)
  trace-motec/             import boundary (Phase 1 contract only)
  trace-format/            versioned portable bundle definitions
  trace-protocol/          explicit live/share wire messages
server/
  trace-server/            reserved workspace member; no Phase 1 service
docs/
```

Avoid empty speculative implementations: reserved members need only a README or
minimal compilable boundary when another Phase 1 member consumes them.

## Canonical domain model

Prefer typed groups over a sparse universal map while retaining a channel registry
for discovery and forward-compatible extras:

```rust
TelemetryFrame {
  sequence: FrameSequence,
  elapsed: Duration,
  lap: LapObservation,
  inputs: DriverInputs,
  vehicle: VehicleState,
  motion: MotionState,
  wheels: WheelStates,
  environment: Option<EnvironmentState>,
  extensions: ExtensionChannels,
}
```

Rules:

- SI units internally. Newtypes name dimensions where confusion is plausible.
- `Option<T>` means absent at this sample; a `ChannelCapabilities` descriptor says
  unsupported, available, intermittent, or unknown for the source/session.
- `SampleValue<T>` is unnecessary for every scalar. Quality flags belong in a
  parallel per-channel quality map and provenance in the stream/session descriptor.
- Vectors carry a declared coordinate frame and handedness. Transforming them
  creates derived data with recorded transform provenance.
- Raw simulator IDs and values remain in source metadata, never in analysis APIs.
- Timestamps are monotonic durations from stream start. Wall time belongs to the
  session envelope.
- IDs are opaque UUID/newtypes; display labels never serve as identity.

Core enums and records include `SimulatorId`, `TelemetrySource`, `ChannelId`,
`ChannelDescriptor`, `ChannelAvailability`, `Unit`, `CoordinateFrame`,
`SessionContext`, `Lap`, `SetupSnapshot`, `TrackGeometry`, and `Provenance`.
Serde is permitted for stable boundary DTOs, but storage/wire schemas are explicitly
versioned and are not blindly derived from every in-memory type.

## Adapter boundary

The adapter is a stateful producer, not an analysis dependency:

```rust
trait SimulatorAdapter {
    fn identity(&self) -> AdapterIdentity;
    fn poll(&mut self) -> Result<Vec<AdapterEvent>, AdapterError>;
}

enum AdapterEvent {
    Detected(SourceDescriptor),
    Connected(SessionSeed),
    CapabilitiesChanged(ChannelCapabilities),
    SessionChanged(SessionSeed),
    Frame(TelemetryFrame),
    Paused,
    Resumed,
    Disconnected(DisconnectReason),
}
```

`poll` is cancellation-friendly and may return multiple ordered lifecycle events.
Recoverable absence/staleness is data, not panic. The host owns retry/backoff and
fans frames independently to local recording and optional live publishing. A
`ReplayAdapter` emits the same events using a controllable clock, pause, seek, and
fault injection.

## Storage boundary and raw format

SQLite stores relational metadata and indexes. Telemetry lives in immutable,
content-addressed blob files written through `TelemetryBlobStore`; repositories
commit blob references only after the file is finalized and checksummed. An
orphan-reconciliation job handles crashes between those steps.

For v0.1, use versioned Apache Arrow IPC files, one schema per canonical stream,
with record batches (for example, 1–4 seconds) and Zstandard buffer compression.
Arrow provides nullable columns, explicit schemas, efficient scans/random batch
access, and a standardized language-neutral format. Keep this behind the storage
trait so benchmarks can justify a later representation change. The raw source page
bytes, if diagnostic capture is enabled, use a separate bounded/versioned sidecar;
they never masquerade as canonical telemetry.

Reference: [Apache Arrow columnar and IPC specification](https://arrow.apache.org/docs/format/Columnar.html).

Implementation result: schema v2 uses 240-frame batches and standard Arrow
Zstandard compression. The planned 30-minute 60–333 Hz synthetic benchmark is
complete; see [the recorded storage benchmark](storage-benchmark.md).

Initial SQLite tables:

```text
schema_migrations(version, applied_at)
simulators(id, key, version)
tracks(id, simulator_id, source_track_id, layout_id, display_name)
cars(id, simulator_id, source_car_id, display_name)
sessions(id, simulator_id, track_id, car_id, started_at, ended_at,
         session_type, source_kind, source_metadata_json, conditions_json)
telemetry_blobs(id, session_id, relative_path, format, schema_version,
                byte_length, sample_count, sha256, created_at)
laps(id, session_id, lap_index, started_offset_ns, duration_ns, validity,
     validity_reason, telemetry_blob_id, sample_start, sample_count,
     distance_m, is_personal_best)
track_geometries(id, track_id, blob_path, source, confidence, algorithm_version,
                 sha256, created_at)
setup_snapshots(id, session_id, source, source_hash, captured_at,
                normalized_json, original_blob_path)
setup_revisions(id, setup_snapshot_id, parent_id, label, notes, created_at)
analysis_cache(id, reference_lap_id, comparison_lap_id, algorithm_version,
               input_hash, result_blob_path, created_at)
favourites(lap_id, created_at)
```

JSON columns hold genuinely variable metadata, not high-rate samples. Enforce
foreign keys, uniqueness for source identities within a simulator, checksums,
relative normalized paths, and migrations. Settings and publishing credentials are
separate; secrets use the OS credential store, never SQLite.

## Analysis methodology proposals

### Distance normalization and interpolation

Construct a monotonic lap-distance coordinate from cleaned 3D trajectory arc
length, anchored/corrected by normalized spline position when trustworthy. Remove
duplicate/non-monotonic points and split start/finish wrap before interpolation.
Choose grid spacing from source spatial density with a configurable bounded target
(initial default 0.5 m, subject to fixture benchmarks), rather than encoding 1 m in
the API. Use piecewise-linear interpolation for continuous channels; zero-order hold
for gear and discrete state; never interpolate across gaps exceeding a declared
threshold. Preserve a validity mask.

### Delta

For each lap, integrate elapsed time as a monotonic function `t(d)` over the common
valid distance domain. Delta is `t_comparison(d) - t_reference(d)`, offset to zero
at the comparison range/lap start. Positive means comparison is behind. Do not
integrate `1/speed` as the primary method when observed timestamps are available;
interpolate measured time against distance. Smooth only presentation/classification,
never the authoritative cumulative delta. Tests cover constant-speed analytic laps,
different sample rates, gaps, stationary samples, and wrap.

### Track reconstruction and corners

Clean a valid closed lap with speed-aware outlier rejection, resample by arc length,
and lightly smooth geometry without moving start/finish. Store it as a canonical
driven path with source and confidence, not a centreline. Later laps project onto
nearby path segments with continuity constraints.

Detect candidate corners from smoothed signed curvature and steering, merge short
gaps, then extend ranges using braking/throttle and speed-gradient evidence. Stable
IDs derive from canonical track distance ordering. Entry begins at braking/lift or
meaningful turn-in; mid spans turn-in to the minimum-speed/apex neighbourhood; exit
ends at sustained throttle/low curvature. Every boundary includes evidence and
confidence. Synthetic fixtures cover straights, hairpins, chicanes, and noisy traces.

## Structured analysis conventions

All machine-consumable results use this shape conceptually:

```rust
AnalysisResult<T> {
  schema_version: AnalysisSchemaVersion,
  algorithm: AlgorithmIdentity,
  inputs: ComparisonIdentity,
  availability: Availability,
  value: Option<T>,
  evidence: Vec<Evidence>,
  confidence: Confidence,
  uncertainty: Vec<Uncertainty>,
  context: ComparisonContext,
}
```

`Evidence` references typed metrics and distance/time ranges, not prose. A metric
contains value, unit, derivation (`measured`, `derived`, `heuristic`), source
channels, and optional uncertainty bounds. `Confidence` is calibrated per algorithm
and includes reasons; it is not a decorative percentage. `Availability` distinguishes
unsupported channels, insufficient samples, invalid range, incomparable inputs, and
algorithm failure. Influence classifications are conservative enums with supporting
evidence and possible setup/condition factors. Natural-language summaries live in a
presentation crate and are never the sole result.

This is sufficient for a future coach to consume facts without coupling core to an
LLM or asking an LLM to perform telemetry mathematics.

## Frontend and protocol boundaries

`telemetry-ui` consumes versioned view DTOs and a `TelemetryDataSource` interface.
Desktop implements it with Tauri commands/channels; web later implements it with
HTTP/WebSocket. Send metadata as JSON and large plot arrays as bounded binary
buffers/chunks. Do not emit an event per native sample across IPC.

`trace-protocol` owns explicit wire envelopes independent of canonical Rust layout:

```text
Envelope { protocol_version, message_id, session_id, sequence, sent_at, payload }
Payload  { hello | session_state | telemetry_batch | lap_event | heartbeat | end }
```

Telemetry batches have declared channels/units, base time, sequence range, and
bounded sample counts. The server validates version, ordering, sizes, rates, and
identifiers. Publisher reconnect resumes from an acknowledged sequence where
possible; local recording has no dependency on this path. Backend metadata belongs
in PostgreSQL and immutable bundles in S3-compatible object storage; configuration
supplies URLs, limits, retention, and bucket details.

## Testing and fixtures

- Unit tests: units, capability merging, interpolation, validity masks, wrap and IDs.
- Property tests: normalized distance/time monotonicity, finite outputs, delta
  antisymmetry, serialization round trips, malformed input never panics.
- Golden synthetic fixtures: constant-speed lap, braking corner, hairpin, chicane,
  missing channels, pauses, packet resets, and session reconnects.
- ABI tests in `trace-ac`: struct sizes/offsets and byte fixtures for each supported
  shared-memory version. No live AC required in CI.
- Replay integration tests: lifecycle events, pacing, pause/restart, corruption,
  and local recording while a mock network sink fails.
- Storage tests: crash-safe finalize, checksum mismatch, migrations, missing blob,
  and untrusted path/archive limits.
- Real captures, once obtained, are redacted, version-labelled regression fixtures;
  expected derived summaries are reviewed rather than snapshotting unstable prose.

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| AC ABI/version drift and torn page reads | version gates, packet-stable copies, ABI byte fixtures |
| Ambiguous AC field semantics | qualified channel descriptors; exclude from analysis until validated |
| Active setup cannot be identified reliably | snapshot only identified source; report unavailable otherwise |
| CSP compatibility churn | optional versioned provider; vanilla baseline |
| `.ld` is proprietary/undocumented | CSV baseline; native import only with reviewed legal parser/API |
| Position drift or bad spline data | provenance, continuity-constrained reconstruction, confidence and fallback |
| False precision in delta/corners | validity masks, uncertainty, deterministic fixtures, no gap interpolation |
| Arrow dependency/size is excessive | storage abstraction and benchmark checkpoint before Phase 2 |
| IPC/chart memory copies | batches, binary buffers, shared cursor controller, measured downsampling |
| Premature protocol lock-in | explicit versioned envelope and canonical-to-wire conversion |

## Phase 1 implementation plan

1. Scaffold Cargo and pnpm workspaces with lint/format/test CI and architecture
   dependency checks.
2. Implement `trace-domain`: units, IDs, channel descriptors, provenance, canonical
   frames, session/setup/track records, and serialization tests.
3. Implement `trace-adapter`: lifecycle contract and deterministic replay adapter.
4. Implement the first `trace-core` slice: distance-series validation and linear /
   held interpolation, with analytic and property tests. Define analysis envelopes
   without speculative corner fields.
5. Implement `trace-storage` traits plus SQLite migrations and an Arrow IPC spike.
   Benchmark representative 60–333 Hz, 30-minute streams before confirming Arrow.
6. Add private AC ABI types and mapping tests from documented byte fixtures; no live
   process detection/capture yet.
7. Define protocol envelopes and round-trip/limit tests; do not start a server.
8. Build the minimal Tauri/React shell and TRACE design tokens; display replay
   connection/session/channel capability state through Tauri IPC.
9. Add CI verification: Rust fmt/clippy/test, TypeScript lint/typecheck/test, and
   fixture replay integration test.
10. Write Phase 1 acceptance notes and stop. Phase 2 begins only on instruction.

### Review gate

No significant implementation should begin until the owner approves or amends:

- the `trace-domain` split from `trace-core`;
- Arrow IPC + SQLite as the storage baseline;
- CSV-first MoTeC scope;
- optional, version-gated CSP treatment;
- the canonical availability/provenance conventions;
- the adapter lifecycle and protocol envelopes.
