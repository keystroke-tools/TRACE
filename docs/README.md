# TRACE documentation

This index covers TRACE's architecture, simulator boundaries, data formats, analysis,
and development workflow. When documents disagree, code and tests define current
behaviour, while `SPEC.md` defines intended product direction.

## Start here

| Document                                | Purpose                                                         |
| --------------------------------------- | --------------------------------------------------------------- |
| [README](../README.md)                  | Product overview, principles, installation, and quick start     |
| [Specification](../SPEC.md)             | Product and implementation direction                            |
| [Implemented foundation](foundation.md) | Foundational implementation and acceptance evidence             |
| [Development guide](development.md)     | Toolchains, commands, native prerequisites, and troubleshooting |
| [Contributing](../CONTRIBUTING.md)      | Change quality, commit, safety, and LLM-assistance expectations |

## Architecture and boundaries

| Document                                                     | Scope                                                                                                              |
| ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| [Phase 1 architecture review](phase-1-architecture.md)       | Decisions, research, risks, dependency rules, and original implementation plan                                     |
| [Assetto Corsa boundary](assetto-corsa.md)                   | Implemented vanilla AC byte readers, mappings, omissions, and Phase 2 acquisition requirements                     |
| [Corner analysis](corner-analysis.md)                        | Corner detection, braking-zone association, phase loss, presentation, worked examples, tests, and limitations      |
| [Assetto Corsa API reference](ac-shared-memory-reference.md) | Mapping names, page layouts, enums, fields, offsets, units, and TRACE storage keys                                 |
| [Live protocol v1](protocol-v1.md)                           | Versioned telemetry messages, validation, ordering, and compatibility rules                                        |
| [Go Live service](go-live.md)                                | Implemented service endpoints, authentication, buffering, local operation, and remaining slices                    |
| [Arrow IPC storage benchmark](storage-benchmark.md)          | Reproducible 60–333 Hz codec benchmark and compression decision                                                    |
| [TRACE session package](session-package.md)                  | Versioned `.trace` sharing format, contents, import behavior, and safety limits                                    |
| [Setup imports](setup-import.md)                             | Simulator adapter boundary, supported archive layouts, install behavior, and safety bounds                         |
| [Live pedal overlay](pedal-overlay.md)                       | Standalone overlay controls, customisation, OBS capture, and data flow                                             |
| [Asset provenance](assets.md)                                | Origin and processing history for generated project assets                                                         |
