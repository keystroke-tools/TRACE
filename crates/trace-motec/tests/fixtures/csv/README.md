# Synthetic MoTeC CSV parser fixtures

These small CSV files are authored by the TRACE project and exercise the bounded
CSV parser without containing third-party telemetry. They are parser regression
fixtures, not evidence that TRACE supports every CSV variant emitted by MoTeC i2.

- `canonical.csv` covers supported canonical units, neutral gear, source metadata,
  three-dimensional position, and preservation of an unknown numeric/text channel.
- `decreasing-time.csv` covers rejection of a source whose time base moves backwards.

Desktop CSV import remains disabled until TRACE obtains and validates an authorised
real-world i2 export fixture.
