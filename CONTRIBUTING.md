# Contributing to TRACE

TRACE accepts focused changes that preserve its local-first, simulator-independent,
and evidence-based architecture. Read the [development guide](docs/development.md)
and the relevant boundary document before changing code.

## Change workflow

1. Keep a change within the current development phase and one coherent concern.
2. Add or update tests for observable behavior and failure cases.
3. Run `mise run format` and `mise run check`.
4. Update documentation when changing public behavior, units, schemas, protocols,
   storage, setup, or architectural boundaries.
5. Commit in understandable chunks with imperative summaries.

Do not silently guess telemetry semantics. New simulator fields require primary
source evidence or captured, version-labelled fixtures. Unknown or unreliable values
must remain unavailable, with uncertainty represented explicitly.

## Pull requests

A pull request should explain:

- the user or architectural outcome;
- the tests and manual checks performed;
- any schema, migration, protocol, security, or performance impact;
- assumptions and intentionally deferred work;
- material LLM assistance or generated assets.

CI must pass Rust formatting, Clippy with warnings denied, all Rust tests, frontend
typechecking, and the production frontend build.

## LLM-assisted development

LLM assistance is welcome and must be transparent. It can support research,
planning, implementation, tests, review, and documentation, but it is not an
authority for simulator semantics or telemetry conclusions.

Contributors remain responsible for every submitted change:

- review generated code and prose rather than committing it blindly;
- verify changing or niche technical facts against primary sources;
- run the same checks required for human-written changes;
- disclose material assistance in the pull-request description;
- identify generated visual assets and preserve their prompt/provenance when useful;
- never send secrets, private telemetry, proprietary SDK content, or personal data to
  an external model without explicit authorization.

TRACE v0.1 analysis is deterministic. Do not add an LLM dependency to `trace-core`,
place generated prose in the canonical analysis result, or present model output as
measured evidence.

## Safety and data

Telemetry, imported files, protocol messages, and simulator memory are untrusted
inputs. Bound allocation and message sizes, reject non-finite numeric values, avoid
unsafe layout casts, and preserve local recording independently of network failures.

Do not commit real user telemetry, credentials, machine-specific paths, local
databases, or copyrighted simulator SDK files. Use minimal synthetic fixtures unless
the repository has explicit permission to redistribute a capture.

## License

By contributing, you agree that your contribution is licensed under the repository's
[MIT License](LICENSE).
