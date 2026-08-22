# Development and verification

TRACE uses Mise as the single toolchain entry point. `mise.toml` pins Rust, Node.js,
and pnpm; `pnpm-lock.yaml` and `Cargo.lock` pin resolved dependencies. Do not install
project dependency versions ad hoc when they belong in these files.

The desktop frontend uses Tailwind CSS 4 through the official Vite plugin. TRACE
tokens are defined with Tailwind's CSS-first `@theme` directive in
`apps/desktop/src/styles.css`; there is intentionally no legacy JavaScript Tailwind
configuration file. Prefer named TRACE utilities and restrained arbitrary values over
reintroducing a parallel component-class styling system.

References: [Tailwind's Vite installation](https://tailwindcss.com/docs/installation/using-vite)
and [theme-variable documentation](https://tailwindcss.com/docs/theme).

## Bootstrap

Install [Mise](https://mise.jdx.dev/), then from the repository root run:

```sh
mise trust
mise install
mise run install
```

`mise run install` uses pnpm's frozen lockfile mode. A lockfile mismatch is therefore
an error that should be resolved and reviewed, not bypassed.

## Standard commands

```sh
mise run format
mise run check
mise run bench-storage
mise run build-ac-fixture-collector
mise run build-windows-desktop
```

`mise run check` is the local equivalent of the required CI verification:

1. Rust formatting check
2. Clippy for every workspace target and feature, with warnings denied
3. Rust unit, integration, and documentation tests
4. frontend TypeScript checking
5. frontend production build

Useful focused commands still run inside the pinned Mise environment:

```sh
mise exec -- cargo test -p trace-core
mise exec -- cargo test -p trace-adapter --test replay_pipeline
mise exec -- cargo clippy -p trace-storage --all-targets -- -D warnings
mise exec -- pnpm --filter @trace/desktop check
mise exec -- pnpm --filter @trace/desktop dev
```

The storage benchmark uses an optimized build and defaults to a 60-second synthetic
stream at each target sample rate. Set `TRACE_BENCH_SECONDS=1800` for the 30-minute
acceptance case. See [the recorded method and baseline](storage-benchmark.md).

`build-ac-fixture-collector` uses the Mise-pinned Clang/LLVM and cargo-xwin tools to
produce a Windows executable without requiring a separate Windows Rust installation.
Run the resulting `capture_fixture.exe <output-directory>` as a Windows process while
AC is on track. Its static page is reconstructed from a non-personal allowlist before
being written; never replace this with a raw static-page dump.

`build-windows-desktop` uses the same cross environment plus task-scoped LLD and LLVM
resource/library tools. It builds the frontend first and emits
`target/x86_64-pc-windows-msvc/release/trace.exe`, which can be launched as a Windows
process from WSL. This produces an unpackaged acceptance executable, not an installer.

GitHub Actions runs the same checks in `CI`. Publishing a GitHub release triggers the
separate `Build executables` workflow; pushes and pull requests never build release
binaries. Successful release runs retain a downloadable artifact containing `trace.exe`
and the privacy-redacted `capture_fixture.exe` collector for 14 days, and attach both
executables directly to the corresponding GitHub release. The workflow currently
produces unpackaged binaries rather than signed installers.

## Native desktop prerequisites

The React frontend can be checked and built anywhere Node.js is supported. Compiling
or running the native Tauri application also requires platform libraries that Mise
does not manage.

On Ubuntu/Debian, CI installs:

```sh
sudo apt-get install --no-install-recommends \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libwebkit2gtk-4.1-dev
```

Use Tauri's platform setup documentation for equivalent packages on other systems.
Windows is the eventual primary capture platform because Assetto Corsa publishes
telemetry through named shared-memory mappings. Phase 1 does not open those mappings.

## Repository map

```text
apps/desktop/             React UI and Tauri composition root
crates/trace-domain/      canonical simulator-independent types
crates/trace-adapter/     acquisition lifecycle and deterministic replay
crates/trace-core/        deterministic telemetry mathematics
crates/trace-ac/          private Assetto Corsa byte decoding and mapping
crates/trace-windows-shmem/ audited Win32 mapping and volatile-copy boundary
crates/trace-recorder/    canonical session/lap state and persistence orchestration
crates/trace-storage/     atomic filesystem blobs, SQLite metadata, Arrow IPC
crates/trace-protocol/    bounded live protocol DTOs and validation
docs/                     architecture, boundaries, and operational guidance
```

Keep dependencies pointed inward. In particular, `trace-core` must not depend on a
simulator, Tauri, React, storage, networking, or an LLM provider. Simulator-specific
values must cross the adapter boundary as canonical domain values before analysis.

Workspace crates forbid unsafe Rust except `trace-windows-shmem`. That crate is a
deliberately small platform boundary: it may own Win32 handles and perform documented
volatile reads, but it must never expose raw pointers or borrowed mapped memory.

The Windows-specific acquisition crates can be checked from a configured cross-target
host with:

```sh
mise exec -- cargo check \
  -p trace-windows-shmem \
  -p trace-ac \
  --target x86_64-pc-windows-msvc
```

The native application creates `trace.sqlite` under Tauri's platform-specific
application-data directory when the Sessions command is first queried. This local
database is runtime data and must never be committed. Browser development does not
open it and continues to use the explicitly labelled replay fixture.

The desktop capture worker also creates `telemetry/sessions` beneath that directory.
Interrupted or metadata-unreferenced files are moved to `telemetry/.orphaned` on the
next startup and retained for diagnosis/recovery. Do not treat `.orphaned` as a cache
that can be deleted automatically.

## Tests and fixtures

Tests live beside their owning crate. The replay integration fixture is
`crates/trace-adapter/tests/fixtures/two_laps.json`; it is intentionally small,
human-readable, deterministic, and free of proprietary telemetry. Assetto Corsa byte
fixtures must identify the supported ABI/version and must not be fabricated from
undocumented field assumptions.

When changing a serialized shape, migration, protocol limit, physical unit, or field
offset, add a regression test that would fail under the previous behavior. Treat all
imported telemetry and protocol messages as untrusted input.

## Generated files and caches

Commit source lockfiles and reviewed project assets. Do not commit `target/`, pnpm
stores, frontend build output, Mise caches, local databases, telemetry captures, or
secrets. Check `.gitignore` before introducing a new generator or tool.

## Troubleshooting

- A Tauri build that cannot find GTK, Pango, or WebKitGTK needs native host packages;
  this is not a Rust dependency failure.
- Cross-checking the complete Windows Tauri shell from Linux additionally requires a
  Windows resource compiler such as `llvm-rc`; the Windows CI runner performs the
  authoritative native check.
- A `mise` trust error is resolved with `mise trust mise.toml` after reviewing the
  file.
- A frozen pnpm install failure means `package.json` and `pnpm-lock.yaml` disagree.
- Run `mise current` to confirm the active versions before diagnosing toolchain-only
  failures.
