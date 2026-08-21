# Assetto Corsa shared-memory boundary

Status: Phase 2 in progress. Page readers, canonical mapping, Windows named-mapping
detection, packet-stable owned snapshots, and adapter lifecycle orchestration are
implemented. Bounded capture, recording, persistence, stale-packet detection, and the
basic session browser are connected through the desktop host.

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
live packet visible to TRACE. While AC reports live or replay state, the adapter now
requires at least one of the physics or graphics packet identifiers to advance within
five seconds. A stale pair closes the canonical connection so the host finalizes the
recording and returns to detection.

Explicit AC pause state suspends and resets the stale timer, preventing a long
legitimate pause from being classified as a crash. A crash that occurs while paused
cannot be distinguished from a real pause using the documented pages alone, so TRACE
conservatively waits for the mapping or reported status to change in that case.

References used for the implemented prefix are the published
[AC shared-memory reference](https://assettocorsamods.net/threads/doc-shared-memory-reference.58/)
and the independently maintained
[AC Japan shared-memory field reference](https://labs.assettocorsa.jp/documents/reference/shared_memory).
The installed game's SDK directory does not contain the original shared-memory C
header. TRACE therefore limits support to the published layout corroborated by a
redacted live AC 1.16.4/shared-memory 1.7 fixture, and rejects other shared-memory
versions at connection time. Additional versions require their own captured fixture
before being accepted.

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

The desktop starts the production adapter on a dedicated polling thread. An unstable
packet remains a temporary acquisition error and does not split the session. Loss of
the underlying mappings is a distinct adapter error that closes the current canonical
recording, allowing a later simulator detection to begin a fresh session cleanly.

## Recording an Assetto Corsa replay

TRACE can record telemetry while Assetto Corsa plays a replay because AC exposes the
playback through the same documented shared-memory pages as a live session. This is
not direct parsing of an `.acreplay` file: AC must be running and playing the replay.
The stored session is labelled `simulator_replay`, while an on-track session is
labelled `native_capture`, so downstream comparisons retain their provenance.

For a reliable recording:

1. Start TRACE before starting replay playback.
2. Play the replay forward at normal speed without seeking, rewinding, or changing
   playback speed.
3. Allow at least two complete start/finish crossings to pass. The first observed lap
   is deliberately excluded from lap metadata because TRACE may have attached partway
   through it.
4. Let playback finish or exit the replay normally so AC reports a clean source close
   and TRACE can finalize the session.

Replay transport controls are not represented by the vanilla shared-memory contract.
Seeking or reversing can make frame time and lap counters regress, so TRACE rejects
the ambiguous boundary, closes the partial recording, and resynchronizes into a new
session rather than manufacturing a plausible-looking lap or stopping acquisition.

## Remaining Phase 2 acquisition requirements

The privacy-redacted AC 1.16.4/shared-memory 1.7 fixture validates the currently
supported ABI. The capture exposed and corrected four static-page offsets: car model,
track, air temperature, and road temperature. Local recording is already independent
from any future live network path.

Phase 2 still needs an observed end-to-end Windows desktop run proving that a driven
or normally played replay lap appears in the session browser. Every additional
shared-memory ABI must add a version-labelled capture before TRACE accepts it.
