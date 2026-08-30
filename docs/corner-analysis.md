# Corner analysis

Status: Implemented. Corner detection algorithm version 3.

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

## Corner detection

A reference sample is considered active corner telemetry when any of these conditions
is true:

- brake input is at least 5%;
- absolute steering input is at least 5%;
- the recorded path turns at least 0.045 radians across the local trajectory window.

Active regions separated by no more than 30 m are joined so a braking zone, brake
release, turn-in, and apex remain connected. A connected region is not automatically
treated as one corner: TRACE finds local Reference speed minima and splits the region
at the intervening maximum speed when the minima are at least 50 m apart and speed
recovers at least 10 km/h above both minima. This separates consecutive bends even when
steering or path curvature never becomes inactive between them. The recovery maximum
becomes a hard boundary for the following corner's braking search.

Regions shorter than 20 m are discarded. Detected regions are labelled `T1`, `T2`, and
so on in lap-distance order. These are TRACE identifiers, not claims about an official
circuit corner number.

Within a region, the minimum reference speed defines the approximate apex. Entry ends
at the last observed brake application before that apex. Exit begins at the first 20%
throttle observation after the apex. Missing phase signals use the adjacent apex sample
as a conservative boundary.

## Braking-zone detection

Corner ranges are detected from the Reference because both laps need one shared range
for time-loss comparison. Braking measurements are then detected independently for the
Reference and Analysed Lap; TRACE does not assume that both drivers began braking at
the Reference corner boundary.

For each detected corner, TRACE performs these steps:

1. It establishes a search window ending at the detected corner's end. The window can
   extend up to 300 m before the Reference corner start, but never crosses the end of
   the preceding detected corner.
2. It finds each driver's approximate apex independently using that driver's minimum
   speed. The Analysed Lap search is limited to 75 m around the shared Reference apex,
   preventing a compound corner from selecting a different bend merely because that
   bend has a lower minimum speed. If an Analysed Lap apex cannot be measured there,
   the Reference apex is used as the search anchor.
3. Brake observations at or above 10% form candidate zones. Consecutive active
   observations separated by no more than 15 m remain one zone. This tolerance handles
   a brief pressure release and the 5 m analysis grid without connecting clearly
   separate braking events.
4. Zones beginning after the driver's apex are rejected. The Reference zone whose final
   active observation is closest to its apex is selected first. The Analysed Lap zone is
   then paired by braking-point proximity to that Reference zone, and candidates more
   than 125 m away are rejected. This prevents two different bends inside a compound
   range from producing a plausible-looking but impossible braking difference. When no
   credible pair exists, the Analysed braking metric is unavailable rather than guessed.
5. The first active observation is the **braking point**, the final active observation
   is the **release point**, and the maximum pressure within the selected zone is the
   **peak brake pressure**.

The UI derives two approachable distances from those facts:

```text
metres before apex = apex distance - braking-point distance
braking-zone length = release distance - braking-point distance
```

These are distances along the normalized lap, not straight-line GPS distances. Their
precision is limited by the 5 m comparison grid, so a displayed braking point should be
read as approximately that location rather than centimetre-accurate ground truth.
A qualifying input present at only one grid sample has a zero-metre derived span and is
labelled **Brake tap** rather than being presented as a sustained braking zone.

The two thresholds serve different purposes: 5% brake can help establish the broad
corner region, while 10% is required for a driver-facing braking point. This keeps
pedal noise and light brake contact from being presented as the start of a braking zone.

## Loss decomposition

Positive delta means the Analysed Lap is behind the Reference. Corner loss is:

```text
delta(corner end) - delta(corner start)
```

Entry, mid, and exit use the same subtraction at their shared boundaries. Therefore,
when all four boundary deltas are available, the phase values sum exactly to total
corner loss. Tests assert this invariant.

The result also retains both laps' braking points, release points, peak brake pressure,
minimum speeds, and throttle-pickup points. The UI may describe these measured
differences but must not infer that one input difference caused the complete time loss.

## Desktop presentation

The comparison workspace places a collapsible **Analysis** dock on the left. It ranks
positive corner losses and shows at most four at once. Selecting a card filters every
synchronized graph and the map to that corner. The strongest positive phase is labelled
as the area where most loss developed, accompanied by an available measured difference.
When braking is available, each card shows the Reference and Analysed Lap's metres before
apex, zone length, and peak pressure. Its short summary follows the phase where most
positive loss developed: entry may report earlier/later braking, mid-corner may report
minimum speed, and exit may report later throttle pickup. This prevents a true braking
difference from being presented as the main explanation when the measured loss actually
developed after the apex. The dock explicitly identifies the result as rule-based and
non-AI.

## Repeated driving observations

TRACE also runs driving-observation algorithm version 1 over the detected corners. It
does not classify a driver globally or use a learned model. It looks for the same
measured difference recurring across multiple corners in the selected lap pair:

- braking or brake release at least 5 m earlier or later;
- minimum speed at least 3 km/h lower;
- throttle application at least 5 m later;
- a majority of positive measured time loss accumulating on entry, mid-corner, or exit;
- more steering-direction reversals above the noise threshold.

A pattern needs evidence from at least two corners and at least 40% of the corners where
that measurement was available. Three or more supporting corners with at least 60%
coverage are shown as **high confidence**. Two-corner patterns remain **low confidence**
and are hidden in a collapsed section by default. Steering-correction observations are
always low confidence because steering ratio, car behaviour, and driving technique can
all change the measured input without indicating a mistake.

The result is returned as a versioned structured analysis alongside the individual
corner analysis. Each observation retains its confidence score, supporting corner
indices, eligible-corner count, average measured difference, unit, source channels, and
comparison context. This makes the UI wording auditable and keeps future consumers from
having to parse display text.

TRACE deliberately does **not** claim that a lap or car is understeering. Steering input
alone cannot establish understeer: that requires trustworthy vehicle-response evidence
such as yaw rate, lateral acceleration, slip angle, speed, and track curvature, plus
validation across simulators and cars. Until that evidence is implemented and tested,
an understeer label would be more confident than the telemetry permits.

Track maps draw recorded brake applications over the driving line in red. Segment
opacity follows recorded brake percentage, producing a brake-intensity gradient rather
than a binary braking marker. Reference uses the wider overlay; the Analysed Lap uses
the narrower overlay. Hollow markers identify the selected braking points; when a corner
is selected, `R` means Reference and `A` means Analysed Lap. These markers use the same
distance-aligned samples as the numbers in the Analysis card.

## Worked example

Suppose TRACE detects a corner apex at 1,000 m. The Reference crosses 10% brake at
850 m, releases below 10% at 970 m, and peaks at 78%. The Analysed Lap crosses 10% at
825 m, releases at 965 m, and peaks at 84%. TRACE reports:

|                     | Reference | Analysed Lap |
| ------------------- | --------: | -----------: |
| Braking before apex |     150 m |        175 m |
| Braking-zone length |     120 m |        140 m |
| Peak pressure       |       78% |          84% |

The summary may state **“Brakes 25 m earlier than Reference.”** It does not state that
earlier braking caused the corner's entire time loss; the speed, throttle, delta, line,
conditions, and setup may also differ.

## Verification

Synthetic core tests cover:

- coherent phase and total corner loss;
- an Analysed Lap braking before the Reference-defined corner start;
- a brief brake-pressure gap remaining within one zone;
- selection of the nearest pre-apex zone when an earlier brake event exists;
- missing driver brake data producing no invented zone;
- a preceding corner's braking not leaking into the following corner;
- a continuously steered range splitting into two corners at a meaningful speed
  recovery, with both braking zones retained;
- missing corner signals and invalid distance ordering.

## Current limitations

- Compound corners with less than 10 km/h speed recovery, or apexes less than 50 m
  apart, may remain one range rather than being split speculatively.
- A corner crossing the lap start/finish boundary is not joined across the file edge.
- The detector does not distinguish intentional trail braking from an ordinary braking
  zone; it reports the final qualifying brake observation as the release point.
- A brake application below 10% is not presented as a braking point.
- Results are quantized by the comparison distance grid and may move by one grid sample
  when telemetry is sparse or noisy.
- `T1`–`Tn` labels are stable for the selected reference trace but are not official
  track metadata.
- Setup and condition differences are not yet included in corner confidence.
- Opportunity text reports measured differences and where delta accumulated; it does
  not claim driver or setup causality.

Future revisions must increment the algorithm identity when thresholds or boundary
semantics change so cached results remain attributable. Version 3 splits continuously
active ranges at meaningful speed recoveries, constrains compound-corner apex selection,
and pairs braking zones between laps. Version 2 introduced
independent apex-centric braking-zone association, release points, and peak pressure;
version 1 used the Reference-defined corner range for both laps' first and last threshold
crossings.
