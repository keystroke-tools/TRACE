# Assetto Corsa shared-memory boundary

Status: Phase 1 page readers and canonical mapping implemented; live acquisition is
not implemented.

Vanilla Assetto Corsa exposes three named Windows mappings:

```text
acpmf_physics
acpmf_graphics
acpmf_static
```

The published interface uses 4-byte structure packing. TRACE does not cast these
pages to packed Rust structs. `trace-ac` accepts an owned page snapshot, checks its
minimum documented prefix length, and decodes explicit little-endian offsets. This
avoids unaligned references, unsafe code, platform `wchar_t` differences, and
accidental exposure of AC-specific layouts.

References used for the implemented prefix are the published
[AC shared-memory reference](https://assettocorsamods.net/threads/doc-shared-memory-reference.58/)
and the independently maintained
[AC Japan shared-memory field reference](https://labs.assettocorsa.jp/documents/reference/shared_memory).
The installed game SDK header and real captured byte fixtures must be checked before
Phase 2 declares a supported shared-memory version.

## Implemented page prefixes

| Page | Minimum bytes | Last decoded field |
|---|---:|---|
| Physics | 200 | `suspensionTravel[4]` |
| Graphics | 292 | `surfaceGrip` (prefix validated; not yet mapped) |
| Static | 476 | `roadTemp` |

Later fields can exist in current AC/CSP pages. A longer page is accepted, but bytes
beyond the validated prefix are ignored. CSP extensions are not inferred from page
length.

## Canonical mapping

| Source | TRACE value | Treatment |
|---|---|---|
| `gas`, `brake` | throttle, brake | accepted only as finite 0–1 ratios |
| `fuel` | fuel litres | finite and non-negative |
| `gear` | canonical gear | AC 0 reverse, 1 neutral, 2+ forward conversion |
| `rpms` | engine RPM | non-negative and bounded by canonical conversion |
| `speedKmh` | speed m/s | finite/non-negative, divided by 3.6 |
| `velocity[3]` | velocity | finite, AC source-world coordinate frame |
| `accG[3]` | acceleration m/s² | finite, multiplied by standard gravity |
| `tyreCoreTemperature[4]` | tyre core °C | canonical wheel ordering |
| `suspensionTravel[4]` | suspension travel m | canonical wheel ordering |
| `completedLaps` | completed laps | non-negative only |
| `iCurrentTime` | current lap time ns | milliseconds converted with checked arithmetic |
| `normalizedCarPosition` | normalized position | accepted only as finite 0–1 ratio |
| `carCoordinates[3]` | position m | AC source-world coordinate frame |
| `carModel`, `track` | session source IDs | decoded from fixed UTF-16, NUL terminated |
| `airTemp`, `roadTemp` | ambient/track °C | finite values only |

Invalid numeric values degrade to missing canonical values rather than panicking.
Short pages return a typed `TooShort` error containing expected and actual lengths.

## Intentionally not mapped yet

- `steerAngle`: published field name does not establish the unit/sign convention.
- wheel pressure, slip, load, wear, and angular speed: require unit/semantic fixture
  validation before analysis use.
- `distanceTraveled`: not assumed to be lap distance.
- graphics `surfaceGrip`: source scale and interpretation need fixture validation.
- clutch and later physics-page fields: outside the currently validated prefix.
- CSP additions: optional future capability provider, never a vanilla requirement.

Keeping these values unavailable is preferable to silently attaching a false unit or
meaning.

## Phase 2 acquisition requirements

The Windows reader must:

1. detect and open all three named mappings without blocking application startup;
2. copy changing pages to owned bytes only when `packetId` is stable before/after;
3. verify the shared-memory version from the static page;
4. emit lifecycle/capability changes through `trace-adapter`;
5. tolerate simulator close, pause, session reset, and reconnect;
6. keep local recording independent from any live network path;
7. add captured, version-labelled byte fixtures for every supported ABI.

