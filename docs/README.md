# TRACE documentation

This index separates implemented behavior from proposals and future product scope.
When documents disagree, code and tests define current behavior, while `SPEC.md`
defines intended product direction.

## Start here

| Document | Purpose | Status |
| --- | --- | --- |
| [README](../README.md) | Product overview, status, principles, and quick start | Current |
| [Specification](../SPEC.md) | Full v0.1 product and implementation direction | Proposal beyond Phase 1 |
| [Implemented foundation](foundation.md) | What exists on `master` and Phase 1 acceptance evidence | Current |
| [Development guide](development.md) | Toolchains, commands, native prerequisites, and troubleshooting | Current |
| [Contributing](../CONTRIBUTING.md) | Change quality, commit, safety, and LLM-assistance expectations | Current |

## Architecture and boundaries

| Document | Scope |
| --- | --- |
| [Phase 1 architecture review](phase-1-architecture.md) | Decisions, research, risks, dependency rules, and original implementation plan |
| [Assetto Corsa boundary](assetto-corsa.md) | Implemented vanilla AC byte readers, mappings, omissions, and Phase 2 acquisition requirements |
| [Corner analysis](corner-analysis.md) | Implemented deterministic corner detection, phase loss decomposition, opportunity ranking, and current limitations |
| [Assetto Corsa API reference](ac-shared-memory-reference.md) | Mapping names, page layouts, enums, fields, offsets, units, and TRACE storage keys |
| [Live protocol v1](protocol-v1.md) | Implemented bounded DTO/validation model; transport remains future work |
| [Arrow IPC storage benchmark](storage-benchmark.md) | Reproducible 60–333 Hz codec benchmark and compression decision |
| [TRACE session package](session-package.md) | Versioned `.trace` sharing format, contents, import behavior, and safety limits |
| [Asset provenance](assets.md) | Origin and processing history for generated project assets |

## Status vocabulary

- **Current** describes behavior implemented and tested on `master`.
- **Spike** proves a representation or boundary but is not yet a finalized production
  implementation.
- **Proposed** describes work that has not been implemented.
- **Deferred** is intentionally outside the current phase.

Phases 1–3 are accepted and Phase 4 is in progress. Windows Assetto Corsa acquisition,
adapter lifecycle orchestration, canonical lap segmentation, SQLite session metadata,
the native session query, recovery-aware completion ordering, and an atomic filesystem
blob store are implemented. Automated orphan reconciliation and the desktop capture
worker are now wired at Tauri startup. Captured ABI fixtures, stale-process detection,
and representative driven/replay validation are complete. Capture frames are
persisted in bounded Arrow record batches rather than retained for an entire session.
Recorded lap sample intervals can be read across batch boundaries without loading the
complete session file into memory.
The 30-minute synthetic storage matrix is complete and Zstandard is now the measured
default compression policy; representative captured-session validation remains.
