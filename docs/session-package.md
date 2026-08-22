# TRACE session package

The `.trace` package is TRACE's self-contained session exchange format. One file is
enough to move a recorded session between two TRACE installations; the recipient does
not need the sender's SQLite database or a separate Arrow export.

## Contents

The package uses a small fixed header followed by a bounded JSON manifest and the
session's original Arrow IPC telemetry bytes:

| Offset | Size | Value |
| ---: | ---: | --- |
| 0 | 8 | ASCII magic `TRACEPKG` |
| 8 | 4 | little-endian package version (`1`) |
| 12 | 4 | little-endian manifest byte length |
| 16 | 8 | little-endian telemetry sample count |
| 24 | variable | UTF-8 JSON manifest |
| after manifest | remainder | Arrow IPC telemetry |

The manifest carries simulator, track, layout and car source identities; the session
type and start time; custom title, driver and tags; every lap's duration, validity,
track-limit evidence and telemetry sample range; sector times; and the telemetry
schema version. The Arrow payload retains all canonical channels, native simulator
field maps and the source-native page payload already present in the recording.

SQLite rows are not copied directly. They are local implementation details and contain
identities that could collide on another machine. Import validates the package and
Arrow stream, assigns fresh local session/lap/blob identities, recreates the relevant
metadata rows, and marks ownership as `other`. The supplied driver attribution, title,
tags, lap evidence and simulator source identities are retained.

## Validation and limits

- Unknown package versions and telemetry schemas newer than the running app are
  rejected.
- The manifest is limited to 1 MiB and telemetry to 2 GiB.
- Every lap must have a non-empty, in-bounds sample range.
- Arrow is streamed and validated to establish the exact sample count before any
  imported session becomes visible.
- A failed import removes partial metadata and any committed telemetry path.

Raw `.arrow` and `.csv` exports remain available for specialist tools. They are data
exports, not complete session exchange files.
