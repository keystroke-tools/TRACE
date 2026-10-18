# TRACE session package

The `.trace` package is TRACE's self-contained session exchange format. One file is
enough to move a recorded session between two TRACE installations; the recipient does
not need the sender's SQLite database or a separate Arrow export.

## Contents

The package uses a small fixed header followed by a bounded JSON manifest and a
compact Zstandard-compressed Arrow IPC telemetry file:

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
schema version. When the user explicitly confirms a setup for the session, the
manifest also carries its simulator/content identities, filename, original source,
confirmation time, SHA-256 digest, and bounded base64 content. The Arrow payload retains all canonical channels used by lap review,
visualisation, comparison and future cross-simulator analysis. It also retains the
source-native values currently required for tyre wear, fuel capacity and track
geometry/configuration.

The driver value comes from the session when one has been assigned. For a self-owned
session without explicit attribution, export falls back to the local driver profile
configured in Settings. Imported attribution is never replaced by that local profile,
so a recipient can identify the original driver when choosing a comparison lap.

The share export deliberately omits the opaque source-memory page snapshot and native
key/value fields TRACE does not currently consume. Those representations duplicate
the canonical sample data at every frame and dominated package size even after Arrow
compression. On a measured 69,739-sample AC session, this reduced the package payload
from 111.4 MB to 11.2 MB (about 90%). Choose the raw Arrow export when exact native
bytes and every unpromoted simulator field are required for archival or decoder work.

Before a local package export or hosted upload, the user chooses which completed laps
to include. TRACE copies only those immutable sample ranges into the package, preserves
their original lap numbers and metadata, and rebases each package-local sample offset.
Omitted outlaps, invalid laps, and race laps contribute no telemetry bytes unless they
are explicitly selected. This keeps endurance-race packages proportional to the laps
someone actually wants to review.

SQLite rows are not copied directly. They are local implementation details and contain
identities that could collide on another machine. Import validates the package and
Arrow stream, assigns fresh local session/lap/blob identities, recreates the relevant
metadata rows, and marks ownership as `other`. The supplied driver attribution, title,
tags, lap evidence and simulator source identities are retained. A confirmed setup is
checksum-verified, restored beneath TRACE's local setup library, indexed against the
fresh session identity, and labelled as a confirmation supplied by the package—not a
new claim made by the recipient.

## Validation and limits

- Unknown package versions and telemetry schemas newer than the running app are
  rejected.
- The manifest is limited to 6 MiB, an optional setup to 4 MiB, and telemetry to 2 GiB.
- Setup payloads require matching simulator/car/track/layout identities, a valid
  SHA-256 digest, and a portable filename that cannot traverse or target Windows device
  names.
- Every lap must have a non-empty, in-bounds sample range.
- Arrow is streamed and validated to establish the exact sample count before any
  imported session becomes visible.
- A failed import removes partial metadata and any committed telemetry path.

Raw `.arrow` and `.csv` exports remain available for specialist tools. They are data
exports, not complete session exchange files. Existing package version 1 files remain
readable because the setup field is optional and compact telemetry uses the same Arrow
schema and package framing. Older readers ignore the additive setup field when the
manifest remains within their legacy size limit; unusually large bundled setups require
a current TRACE build.

## Hosted sharing

The share action builds this same compact package and uploads it to the configured
TRACE service. The service authenticates the publisher, enforces its upload limit,
parses the bounded package manifest, reads the Arrow stream, and verifies the declared
sample count before publishing an unlisted link. A package is written atomically and
remains available across server restarts until its configured expiry (seven days by
default). The browser page exposes a normal `.trace` download, so recipients retain a
self-contained file and can import it without access to the sender's database.

The desktop remembers a small bounded list of active links on that installation until
their server-reported expiry. Reopening the lap picker allows a link to be copied again;
requesting the exact same lap selection reuses the active link rather than uploading a
duplicate package. Expired history is discarded automatically.

Hosted download pages expose both a normal package download and an **Open in TRACE**
link. Installed desktop builds register the `trace:` URL scheme. Activating that link
hands the HTTP(S) package URL to the existing single TRACE instance, bounds the download
to 128 MiB, validates the package and Arrow stream through the normal importer, and
opens the imported session in the library. The browser download remains available when
TRACE is not installed or protocol launching is blocked.

Local `.trace` export remains separate and never contacts the hosted service.
