# Assetto Corsa shared-memory boundary

Status: Phase 2 in progress. Page readers, canonical mapping, Windows named-mapping
detection, packet-stable owned snapshots, and adapter lifecycle orchestration are
implemented. Recording and persistence are not yet implemented.

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

`trace-windows-shmem` opens existing mappings read-only, maps only the validated
prefix size, and volatile-copies every byte into an owned buffer. It is the sole TRACE
crate permitted to contain unsafe Rust. Handles and views use RAII cleanup, and raw
pointers or borrowed mapped slices are never exposed. This follows Microsoft's
[named shared-memory model](https://learn.microsoft.com/en-us/windows/win32/memory/sharing-files-and-memory),
including opening by name with
[`OpenFileMappingW`](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-openfilemappingw),
mapping with
[`MapViewOfFile`](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-mapviewoffile),
and releasing the view and handle.

Physics and graphics pages are accepted only when the `packetId` read before copying,
the identifier embedded in the owned copy, and the identifier read after copying all
match. A torn page is retried at most three times. The static page has no packet
identifier and is copied directly.

## Adapter lifecycle

`AcAdapter` implements the common `SimulatorAdapter` contract and emits bounded,
ordered events. A successful first poll produces detection, connection, canonical
capabilities, optional pause state, and one frame. Later polls can produce session
changes, pause/resume transitions, frames, and normal disconnection when AC reports
the graphics status as off.

Every connection resets the monotonic frame sequence and elapsed clock. Static-page
car/track changes emit `SessionChanged` before the corresponding frame. An unstable
packet is a temporary error and retains the connection so the next poll can recover;
invalid page data is rejected. Unknown graphics-status values are not guessed.

Windows retains a named mapping while TRACE holds an open view. A normal AC shutdown
reports the off status and is handled, but a hard simulator crash can leave the last
live packet visible to TRACE. Bounded stale-packet timeout detection is therefore a
remaining requirement before capture is considered production-ready.

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

## Recording boundary

`trace-recorder` consumes only canonical adapter events. It validates increasing frame
sequence and elapsed time, closes sessions on source session changes or disconnects,
and records a lap only after observing both its opening and closing completed-lap
counter boundaries. The first lap seen after attachment is therefore retained in the
raw session stream but excluded from lap metadata because it may be partial.

Counter regression or a jump larger than one is rejected as ambiguous rather than
silently producing incorrect sample ranges. Simulator-specific validity evidence is
not yet authoritative, so this layer does not claim a lap is valid.

## Remaining Phase 2 acquisition requirements

The Windows reader must:

1. verify the shared-memory version from the static page;
2. detect stale packets after an abnormal simulator exit without treating a long
   legitimate pause as a disconnect;
3. keep local recording independent from any live network path;
4. add captured, version-labelled byte fixtures for every supported ABI.
