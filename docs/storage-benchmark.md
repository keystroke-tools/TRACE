# Arrow IPC storage benchmark

TRACE stores canonical telemetry as standard Apache Arrow IPC files. This benchmark
checks the v2 schema at the supported 60–333 Hz range before confirming the default
record-batch compression policy.

## Method

The repository-owned `arrow_benchmark` example generates a deterministic 30-minute
stream at 60, 120, and 333 Hz. Every sample populates all canonical channel families
with smoothly changing synthetic values, including motion vectors and four wheel
states. Each stream is written in 240-frame record batches, then the common analysis
projection is decoded and its sample count verified.

Run it through the pinned Mise toolchain:

```sh
TRACE_BENCH_SECONDS=1800 mise run bench-storage
```

`TRACE_BENCH_SECONDS` defaults to 60 for a quick local comparison. The results below
were recorded on 21 August 2026 under WSL2/Linux x86-64, using Rust 1.91.1 on an AMD
Ryzen 5 8400F. Times are single-run elapsed measurements from an optimized build;
they are decision evidence, not general-purpose hardware claims.

| Rate | Codec | Samples | Bytes | Write | Projection read |
| ---: | --- | ---: | ---: | ---: | ---: |
| 60 Hz | none | 108,000 | 26,398,298 | 59.65 ms | 13.59 ms |
| 60 Hz | LZ4 frame | 108,000 | 21,748,634 | 78.20 ms | 24.41 ms |
| 60 Hz | Zstandard | 108,000 | 17,563,866 | 167.67 ms | 44.10 ms |
| 120 Hz | none | 216,000 | 52,789,898 | 105.79 ms | 14.67 ms |
| 120 Hz | LZ4 frame | 216,000 | 43,491,210 | 157.72 ms | 40.12 ms |
| 120 Hz | Zstandard | 216,000 | 35,085,834 | 354.41 ms | 93.40 ms |
| 333 Hz | none | 599,400 | 146,483,610 | 289.21 ms | 55.41 ms |
| 333 Hz | LZ4 frame | 599,400 | 122,908,378 | 454.17 ms | 136.90 ms |
| 333 Hz | Zstandard | 599,400 | 98,678,938 | 977.10 ms | 267.80 ms |

## Decision

Zstandard is the capture default. It reduced these files by 32.6–33.5% relative to
uncompressed IPC, versus 16.1–17.6% for LZ4 frame. Even at 333 Hz, encoding a
30-minute synthetic stream took less than one second on the measured system, leaving
a large margin for real-time capture. LZ4 and uncompressed writing remain selectable
for future diagnostics and representative hardware comparisons.

Compression is declared in Arrow record-batch metadata and decoded by ordinary Arrow
readers. It does not add a TRACE-specific wrapper or change telemetry schema version
2. Synthetic smooth signals are useful for repeatability but cannot model every real
session; captured Assetto Corsa fixtures and lower-spec Windows hardware should be
added to later regression runs.
