# Setup imports

TRACE exposes setup importing as a simulator capability. The desktop asks the backend
for available importers and passes the selected simulator ID into folder detection and
archive import. The Setups page does not equate “setup” with one simulator, even though
Assetto Corsa is the first implemented provider.

Each future provider owns its folder convention, accepted archive extensions, identity
rules, validation, and install layout. Unsupported simulator IDs are rejected at the
native command boundary instead of falling through to Assetto Corsa behavior.

## Assetto Corsa provider

The Assetto Corsa provider installs setup INIs from ZIP archives into the game's normal
saved-setup tree. This is a local workflow: it does not upload an archive, modify a
recorded TRACE session, or claim that a setup was active for a lap.

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

Existing setup files are preserved by default. Enable **Replace existing files** only
when the archive should overwrite files with the same names.

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
- at most 512 setup INIs, 4 MiB per INI, and 64 MiB of expanded INIs per archive;
- `.zip` inputs and `.ini` setup entries only;
- no path traversal, Windows device names, unsafe car/track identifiers, or duplicate
  flattened filenames.

## Not implemented yet

TRACE does not yet parse setup values for comparison, attach a setup snapshot to a
session or `.trace` package, identify the setup currently loaded by a simulator, or
recommend setup changes. Those require a separate provenance model so the app does not
attribute a lap to a setup without evidence.
