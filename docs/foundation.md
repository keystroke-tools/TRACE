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
```

The crates intentionally have no circular dependencies:

- `trace-domain` owns simulator-independent telemetry and capability types.
- `trace-adapter` owns acquisition lifecycle events and the replay adapter.
- `trace-core` owns deterministic distance-domain mathematics and analysis results.
- `trace-ac` privately reads documented vanilla AC page prefixes and maps them into
  canonical domain values.
- `trace-storage` owns immutable telemetry blob and SQLite metadata boundaries.
- `trace-protocol` owns bounded, versioned network data-transfer objects.

No implemented crate depends on Tauri, React, HTTP, a simulator SDK, or an LLM
provider.

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
to packed Rust structs. This keeps the workspace free of unsafe code and makes prefix
length/offset validation explicit. It currently maps documented driver inputs,
speed/RPM/gear/fuel, velocity, G acceleration, tyre core temperature, suspension
travel, lap observations, world position, car/track identity, and temperatures.

Fields whose units or semantics remain uncertain are intentionally unavailable. Live
Windows shared-memory acquisition and packet-stable copying remain Phase 2 work. See
[the AC boundary document](assetto-corsa.md).

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
implementation is a tested fixture; a filesystem-backed Arrow IPC implementation is
still pending.

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

## Verification

The workspace currently runs formatting, Clippy with warnings denied, unit tests,
and documentation tests through the Mise-managed Rust toolchain. Tests include
identifier/capability behavior, replay ordering, interpolation, lap delta, immutable
blob lifecycle, path security, SQLite migration/foreign keys, and protocol limits.
