# Corner analysis

Status: Phase 4 initial implementation.

TRACE detects corners and explains time loss with deterministic distance-domain
analysis. The implementation lives in `trace-core`; it has no simulator, storage,
desktop, or presentation dependencies. It is rule-based analysis, not AI-generated
coaching.

The UI calls the faster benchmark the **Reference** and the lap being reviewed the
**Analysed Lap**. Core and storage fields retain `comparison` as the mathematical
second operand for compatibility; in the equations below it means the Analysed Lap.

## Inputs and availability

The comparison command first aligns both laps onto the existing 5 m distance grid.
Corner analysis consumes lap distance, cumulative comparison delta, speed, brake,
throttle, steering input, and world X/Z position where available.

Speed plus at least one direction/braking signal is required. When those channels are
missing, the structured result reports unsupported channels. Short or invalid distance
ranges report insufficient samples or an invalid range rather than guessed corners.

## Algorithm version 1

A reference sample is considered active corner telemetry when any of these conditions
is true:

- brake input is at least 5%;
- absolute steering input is at least 5%;
- the recorded path turns at least 0.045 radians across the local trajectory window.

Active regions separated by no more than 30 m are joined so a braking zone, brake
release, turn-in, and apex remain one corner. Regions shorter than 20 m are discarded.
Detected regions are labelled `T1`, `T2`, and so on in lap-distance order. These are
TRACE identifiers, not claims about an official circuit corner number.

Within a region, the minimum reference speed defines the approximate apex. Entry ends
at the last observed brake application before that apex. Exit begins at the first 20%
throttle observation after the apex. Missing phase signals use the adjacent apex sample
as a conservative boundary.

## Loss decomposition

Positive delta means the Analysed Lap is behind the Reference. Corner loss is:

```text
delta(corner end) - delta(corner start)
```

Entry, mid, and exit use the same subtraction at their shared boundaries. Therefore,
when all four boundary deltas are available, the phase values sum exactly to total
corner loss. Tests assert this invariant.

The result also retains both laps' measured braking points, minimum speeds, and
throttle-pickup points. The UI may describe these differences but must not infer that
one input difference caused the complete time loss.

## Desktop presentation

The comparison workspace places a collapsible **Analysis** dock on the left. It ranks
positive corner losses and shows at most four at once. Selecting a card filters every
synchronized graph and the map to that corner. The strongest positive phase is labelled
as the area where most loss developed, accompanied by an available measured difference
such as minimum speed or throttle-pickup distance. The dock explicitly identifies the
result as rule-based and non-AI.

Track maps draw recorded brake applications over the driving line in red. Segment
opacity follows recorded brake percentage, producing a brake-intensity gradient rather
than a binary braking marker. Reference uses the wider overlay; the Analysed Lap uses
the narrower overlay.

## Current limitations

- Compound corners without a straight or inactive gap may be represented as one range.
- A corner crossing the lap start/finish boundary is not joined across the file edge.
- `T1`–`Tn` labels are stable for the selected reference trace but are not official
  track metadata.
- Setup and condition differences are not yet included in corner confidence.
- Opportunity text reports measured differences and where delta accumulated; it does
  not claim driver or setup causality.

Future revisions must increment the algorithm identity when thresholds or boundary
semantics change so cached results remain attributable.
