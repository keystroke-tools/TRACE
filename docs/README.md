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
| [Live protocol v1](protocol-v1.md) | Implemented bounded DTO/validation model; transport remains future work |
| [Asset provenance](assets.md) | Origin and processing history for generated project assets |

## Status vocabulary

- **Current** describes behavior implemented and tested on `master`.
- **Spike** proves a representation or boundary but is not yet a finalized production
  implementation. Arrow IPC is currently in this category.
- **Proposed** describes work that has not been implemented.
- **Deferred** is intentionally outside the current phase.

Phase 1 is accepted and Phase 2 is in progress. Windows Assetto Corsa acquisition,
adapter lifecycle orchestration, canonical lap segmentation, SQLite session metadata,
and the native session query are implemented. Durable blob orchestration and the
desktop capture worker remain Phase 2 work.
