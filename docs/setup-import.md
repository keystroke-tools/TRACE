# Setup imports

TRACE exposes setup importing as a simulator capability. The desktop asks the backend
for available importers and passes the selected simulator ID into folder detection and
file or archive import. The Setups page does not equate “setup” with one simulator, even though
Assetto Corsa is the first implemented provider.

Each future provider owns its folder convention, accepted archive extensions, identity
rules, validation, and install layout. Unsupported simulator IDs are rejected at the
native command boundary instead of falling through to Assetto Corsa behavior.

## Assetto Corsa provider

The Assetto Corsa provider installs standalone setup INIs or setup INIs from ZIP
archives into the game's normal saved-setup tree. This is a local workflow: it does not
upload setup data.

The behavior is a native Rust port of the workflow first explored in
[`ac-setup-importer-proto`](https://github.com/aosasona/ac-setup-importer-proto). TRACE
uses its existing desktop UI and filesystem boundary rather than starting the
prototype's local HTTP server.

### Using the importer

1. Open **Setups** and choose **Assetto Corsa** as the importer.
2. Verify the detected `Documents\Assetto Corsa\setups` folder, or choose another
   setups folder.
3. Drop one or more ZIP files onto the import area, or click it to use the file picker.
4. Review the destination, installed-file count, and skipped-file count per archive.

For a standalone file, choose **Individual files**, enter the source track identifier,
and optionally enter the source car and layout identifiers. TRACE reads `MODEL=` from the INI's
`[CAR]` section when available. A declared car that conflicts with an explicitly entered
car is rejected. Because AC setup files do not reliably contain a track identity, TRACE
never guesses the destination track.

From a session overview, open **Compatible setups** and choose **Attach setup**. TRACE
uses the session's preserved simulator/car/track/layout identity, installs and indexes
the selected file, then explicitly marks it as used for that session. This setup will be
included in future `.trace` exports. A same-named but different existing file is not
silently associated.

Existing setup files are preserved by default. Enable **Replace existing files** only
when the archive should overwrite files with the same names.

## Setup library and session suggestions

Every installed or deliberately skipped existing setup is indexed in TRACE's local
SQLite setup library with its simulator ID, source car ID, source track ID, layout ID,
filename, installed path, source archive, content digest, and import time. Setup file
contents remain in the simulator's setup directory; the database stores metadata and a
SHA-256 digest, not a second copy.

**Index existing** scans the selected simulator setup folder without copying files.
Automatic indexing can be enabled under **Settings → General → Setup library**. It is
off by default because large libraries add disk activity at startup. Discovery is
bounded to 5,000 setup files and, for AC, only reads the expected
`<car>/<track>/*.ini` depth.

A manually selected setup-library folder is retained per simulator and takes precedence
over autodetection for individual imports, session attachment, explicit indexing, and
automatic startup indexing.

### Library explorer

The **Setups → Library** workspace lists every indexed setup in stable
simulator/car/track/layout order. Cars and tracks show both content-derived friendly
names and their canonical simulator identifiers. Friendly names come from each
simulator adapter's installed-content metadata; TRACE does not maintain a hardcoded
alias list, and unknown or modded content therefore keeps its raw name.

Search covers setup filenames, friendly names, raw IDs, layouts, simulators, and source
archive names. A simulator filter narrows larger libraries. Track groups are collapsible
and expose the indexed files, import source and time, missing-file state, and the number
of sessions that explicitly identify a setup as used. The explorer is read-only; setup
editing belongs to the separate editor capability.

The session overview queries this library using the session's preserved source
identities. A setup is suggested only when simulator, car, track, and layout all match
exactly (case-insensitively for simulator-provided identifiers). Friendly display names
are not used for matching. This intentionally prefers an empty result over suggesting
a setup for the wrong car or layout.

The UI calls these entries **compatible setups**. Compatibility is not evidence that
the driver loaded one. A user may explicitly mark one setup as **used for session**;
that confirmation is stored separately from matching and can be cleared or replaced.
Setups restored from another driver's `.trace` package are labelled **shared as used**
so TRACE preserves the sender's statement without presenting it as a claim made by the
recipient.

An explicitly associated setup is checksum-verified and embedded in `.trace` exports.
Import restores the file beneath TRACE's private setup library, recreates its metadata,
and associates it with the new local session. It does not silently install the shared
file into a simulator's live setup folder.

## Assetto Corsa setup differences

When a session has a confirmed setup and another compatible AC setup is available,
**Compare to used** parses both bounded INI files and shows changed values grouped by
section. Unchanged values are counted but hidden to reduce noise. Missing keys remain
visible as unavailable on the relevant side.

This is a literal configuration diff. TRACE does not infer whether a change helped,
hurt, or caused a lap-time difference. Other simulators can add their own parser behind
the same comparison command without treating their formats as Assetto Corsa INIs.

### Expected archive layout

The archive must contain at least one Assetto Corsa/MoTeC log name in this form:

```text
<track>_&_<car>_&_<driver>_&_stint_<number>.ld
```

For example:

```text
ks_vallelunga_&_tatuusfa1_&_E. Cavalli_&_stint_22.ld
```

TRACE reads the track and car identifiers from that filename and installs every `.ini`
file in the archive at:

```text
<setups folder>\<car>\<track>\<setup filename>.ini
```

Archive directories are deliberately flattened: only each INI's filename is retained.
Repeated INI filenames in different archive directories are rejected rather than
silently choosing one.

The provider can recover the car identifier from a `GHOST_CAR_..._<car>.ghost` name or
from `MODEL=` in an INI `[CAR]` section. Assetto Corsa does not provide the track in
those fallbacks, so an `.ld` filename is still required to determine an install
destination.

### Detection and safety

On Windows, TRACE checks the standard Documents folder and common OneDrive Documents
locations. A missing detected folder is shown as a suggested default, not treated as
proof that the path exists.

Imports are bounded to protect the desktop process and the user's setup tree:

- at most 32 archives in one operation;
- at most 512 MiB per ZIP and 4,096 ZIP entries;
- at most 512 setup INIs per import, 4 MiB per INI, and 64 MiB of expanded INIs per archive;
- `.zip` inputs and `.ini` setup entries only;
- no path traversal, Windows device names, unsafe car/track identifiers, or duplicate
  flattened filenames.

## Not implemented yet

TRACE does not yet identify the setup currently loaded by a simulator, automatically
capture active setup snapshots, install a setup restored from `.trace` into the game,
or recommend setup changes. Those require stronger provenance and simulator-specific
behavior so the app does not attribute a lap to a setup without evidence.
