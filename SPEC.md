# TRACE v0.1 — Codex Implementation Specification

> **Working product name:** TRACE  
> **Working public service/domain:** `simtrace.run`  
> **Working tagline:** `FIND THE TIME`

## 1. Product Definition

You are building **TRACE**, a local-first, simulator-agnostic motorsport telemetry analysis application.

TRACE exists primarily to answer:

> **Where am I losing lap time, why am I losing it, and is the difference actually caused by my driving rather than setup or conditions?**

The first supported simulator is **Assetto Corsa (AC)**, but the architecture **MUST** be designed so additional simulators can be added through independent modules/adapters without modifying the core analysis engine.

TRACE also supports importing external telemetry, initially **MoTeC-compatible telemetry where practical**.

TRACE is **not DriveKit** and should be developed as an independent application.

---

## 2. Product Principles

### 2.1 Local first

Recording, storing, importing, viewing and analysing telemetry **MUST work without the TRACE backend or an internet connection**.

The backend enhances TRACE with:

- live spectating
- shareable recorded sessions

Backend availability must not be required for normal telemetry recording or analysis.

If the backend is unavailable while driving, local recording must continue normally.

### 2.2 Simulator agnostic

Never expose Assetto Corsa-specific structures directly to the analysis engine or UI.

The architecture should conceptually be:

```text
Assetto Corsa ───┐
ACC ─────────────┤
iRacing ─────────┤
LMU ─────────────┼──► Simulator Adapter ──► Canonical TRACE Data
AMS2 ────────────┤                              │
rFactor 2 ───────┤                              ▼
Other Sims ──────┘                        Analysis Engine

MoTeC ─────────────────► Import Adapter ────────┘
```

Only Assetto Corsa needs to be implemented initially.

Other simulator modules are architectural extension points, **not v0.1 deliverables**.

### 2.3 Analysis before features

Do not build features merely because they would make TRACE feel like a larger product.

The core value is telemetry comparison and analysis.

Before implementing optional functionality, ask:

> Does this materially improve the user's ability to understand where or why they are losing time?

If not, defer it.

### 2.4 Evidence before explanation

TRACE should distinguish:

- directly measured facts
- deterministic derived metrics
- heuristics/classifications
- uncertainty
- possible setup/condition influence
- unavailable information

Prefer:

> Unable to determine reliably.

over a confident but unsupported explanation.

---

## 3. Explicitly Out of Scope

Do **NOT** implement in v0.1:

- user accounts
- Steam login
- Discord login
- social profiles
- friends/following
- annotations
- comments
- live chat
- coaching marketplace
- subscriptions
- payments
- public telemetry discovery
- teams
- championships
- DriveKit integration
- LLM coaching UI
- LLM provider integrations
- complicated cloud sync
- automatic setup recommendations
- automatic claims of setup causality

### Future LLM note

LLM coaching is a planned post-v0.1 feature.

Although the LLM integration itself is out of scope, the deterministic analysis architecture **MUST** be designed so that a future coaching layer can consume TRACE analysis without requiring major changes to `trace-core`.

Do not create placeholder LLM integrations, fake providers, API calls, chat interfaces, or speculative unused abstractions in v0.1.

---

## 4. Technology

### 4.1 Desktop

Use:

- Tauri 2
- Rust
- React
- TypeScript

Do **NOT** embed a local HTTP server merely to communicate between React and Rust.

Use Tauri commands/events for desktop IPC.

### 4.2 Frontend

Use React + TypeScript.

Telemetry visualisation must efficiently render large time-series datasets.

Investigate/use **uPlot** or another similarly lightweight high-performance plotting library if appropriate.

Avoid heavyweight generic dashboard/chart libraries unless there is a strong technical justification.

All telemetry graphs must support synchronized cursors.

### 4.3 Local persistence

Use SQLite for metadata such as:

- sessions
- laps
- tracks
- cars
- simulator metadata
- setups
- setup revisions/snapshots
- analysis metadata/cache indexes
- reference/favourite status
- file indexes

Do **NOT** store every raw telemetry sample as an individual SQLite row.

Raw/full-resolution telemetry should use a compact file representation.

Create a storage abstraction so the representation can evolve.

Do not prematurely over-engineer the binary format.

### 4.4 Backend

Use Rust.

Prefer:

- axum
- WebSockets
- PostgreSQL for metadata
- object storage for large telemetry/session blobs

The backend will run on infrastructure controlled by the project owner.

Backend configuration **MUST** be environment/config driven.

Never hardcode `simtrace.run` into core application logic.

---

## 5. Repository Architecture

Prefer a workspace resembling:

```text
trace/
├── apps/
│   ├── desktop/
│   │   ├── src/
│   │   └── src-tauri/
│   │
│   └── web/
│
├── packages/
│   └── telemetry-ui/
│
├── crates/
│   ├── trace-core/
│   ├── trace-storage/
│   ├── trace-ac/
│   ├── trace-motec/
│   ├── trace-format/
│   └── trace-protocol/
│
└── server/
    └── trace-server/
```

Adjust where technically appropriate, but maintain the architectural boundaries.

---

## 6. `trace-core`

This is the most important package.

`trace-core` **MUST** be:

- pure Rust
- simulator agnostic
- storage agnostic
- UI agnostic
- Tauri agnostic
- backend agnostic
- LLM/provider agnostic

It must **NOT** depend on:

- Assetto Corsa
- Tauri
- SQLite
- PostgreSQL
- HTTP
- React
- filesystem layout assumptions
- any LLM SDK/provider

Its input is canonical TRACE telemetry.

Its output is structured analysis.

This crate should be reusable in future applications.

---

## 7. Canonical Telemetry Model

Design a canonical telemetry representation capable of representing data from different simulators.

Do **NOT** assume every simulator supplies every field.

Fields should therefore have clear availability semantics.

Conceptually:

```rust
struct TelemetrySample {
    timestamp: ...,
    lap_distance: ...,

    position: ...,
    velocity: ...,

    speed: ...,
    throttle: ...,
    brake: ...,
    clutch: ...,
    steering: ...,

    gear: ...,
    rpm: ...,

    acceleration: ...,

    wheel_speeds: ...,
    tyre_data: ...,
    suspension_data: ...,

    // etc.
}
```

Do not blindly implement this exact structure.

Research the available AC telemetry and design a sensible canonical representation.

Separate conceptually:

1. driver inputs
2. vehicle state
3. position/trajectory
4. tyre/wheel data
5. suspension data
6. environment/session context

Optional simulator-specific data should not pollute the canonical model unnecessarily.

Provide capability discovery.

For example, TRACE should be able to know:

```text
speed               available
brake               available
throttle            available
steering            available
tyre_core_temp      available
brake_temperature   unavailable
```

The UI must not assume every channel exists.

Units should be explicit and consistent internally.

---

## 8. Simulator Adapter Interface

Define a clear adapter/module boundary.

Conceptually:

```rust
trait SimulatorAdapter {
    fn simulator(&self) -> Simulator;
    fn capabilities(&self) -> Capabilities;
    fn session_info(&self) -> ...;
    fn read_frame(&mut self) -> ...;
}
```

Do not copy this blindly.

Design the interface based on actual requirements.

Each simulator module is responsible for converting simulator-specific telemetry into canonical TRACE data.

The analysis engine **MUST NOT** know which adapter produced the data.

Adapter lifecycle should account for:

- simulator detection
- connection
- session changes
- disconnect
- reconnect
- missing capabilities
- game pause/restart

---

## 9. Assetto Corsa Module

Implement Assetto Corsa as the first simulator adapter.

Investigate the actual AC shared-memory interfaces before implementing mappings.

Capture as much useful information as reliably available.

### Driver inputs

Where available:

- throttle
- brake
- steering
- clutch
- gear

### Vehicle

Where available:

- speed
- RPM
- velocity
- acceleration/G
- wheel speeds
- tyre information
- tyre temperatures
- tyre pressures
- slip information
- suspension travel
- fuel
- damage where useful

### Session

Where available:

- lap number
- current lap time
- last lap
- best lap
- sector information
- session type
- car
- track/layout
- position
- normalized spline/lap position

### Environment

Where available:

- ambient temperature
- track temperature
- track grip
- weather
- other relevant session conditions

Clearly document fields that vanilla AC does not expose.

Investigate whether CSP exposes useful additional telemetry/context.

CSP-specific functionality must be treated as an optional capability enhancement, not required for TRACE to operate with vanilla AC.

Do not guess field semantics.

---

## 10. Track Geometry and Reconstruction

TRACE must **NOT** require a predefined track database.

If reliable track geometry is supplied by the simulator/imported source, use it.

Otherwise reconstruct useful geometry from driven positional telemetry.

At minimum:

1. record world position through a valid lap
2. identify a complete closed lap
3. clean obvious telemetry noise/outliers
4. generate a stable lap path
5. associate path coordinates with lap distance
6. cache resulting geometry for simulator + track + layout

Store provenance:

```text
TRACK GEOMETRY
Source: simulator | recorded | imported
Confidence: ...
```

Do not pretend a recorded racing line is literally the physical track centreline or track boundaries.

Initially it is acceptable for reconstructed geometry to represent a canonical driven path suitable for telemetry visualization and distance mapping.

The system should permit improved centreline/boundary reconstruction later.

When multiple valid laps exist, consider whether combining them can produce a more stable canonical path, but do not overcomplicate v0.1.

---

## 11. Sessions

TRACE must persist sessions.

A session contains where available:

- simulator
- simulator version
- car
- track/layout
- session type
- timestamp
- conditions
- setup snapshot
- laps
- telemetry source
- track geometry reference

Example hierarchy:

```text
Mugello
└── Tatuus FA01
    ├── Practice — 21 Aug
    │   ├── Lap 1
    │   ├── Lap 2
    │   └── Lap 3 ★ PB
    │
    └── Practice — 20 Aug
```

Sessions must remain usable offline.

Preserve provenance for imported versus natively captured data.

---

## 12. Lap Processing

Detect where possible:

- lap start
- lap completion
- invalid laps
- lap time
- sector times
- lap distance
- PB status

Store full-resolution/raw-enough telemetry.

Then produce normalized telemetry suitable for comparison.

Do not discard useful source telemetry merely because v0.1 does not display it yet, provided storing it is practical and well-defined.

---

## 13. Distance-Based Normalization

This is fundamental.

Do **NOT** compare laps primarily by timestamp.

Different laps have samples at different times.

Normalize/interpolate telemetry against **lap distance**.

Conceptually:

```text
0.0m
1.0m
2.0m
3.0m
...
```

Choose an appropriate resolution based on technical analysis rather than blindly using 1 m.

The system should allow:

```text
distance
    ↓
reference sample
comparison sample
    ↓
difference
```

This common distance domain drives:

- telemetry overlays
- delta
- track cursor
- corner analysis
- racing-line comparison

Be careful around start/finish wrapping.

---

## 14. Lap Delta

Implement accurate distance-aligned lap delta calculation.

The delta trace is the central TRACE visualization.

It must show where time is:

- gained
- lost
- stable

Ensure delta calculation is mathematically correct and well tested.

Do not derive conclusions from noisy single samples.

Document the chosen methodology.

---

## 15. Synchronized Telemetry Viewer

Implement synchronized plots for:

- delta
- speed
- throttle
- brake
- steering
- gear
- RPM

Additional available channels may be added later.

All graphs share one distance cursor.

Moving/hovering the cursor on one graph updates:

- all graphs
- displayed telemetry values
- track position
- selected corner where appropriate

Support zooming into a distance range/corner.

Performance must remain smooth with realistic telemetry volumes.

---

## 16. Racing-Line Comparison

Using world position data, overlay reference and comparison trajectories.

Provide:

- full-lap view
- selected-corner view
- synchronized cursor
- reference line
- comparison line

Where technically meaningful, calculate approximate positional difference between trajectories.

Do not report false precision.

Account for coordinate transforms/normalization necessary to compare imported or differently sourced data.

---

## 17. Corner Detection

Implement deterministic automatic corner/braking-zone detection.

This does not need to be perfect initially.

Use signals such as:

- speed
- braking
- steering
- throttle
- heading/trajectory curvature
- lap distance

Represent corners as stable distance ranges.

Where official corner names/numbers are unavailable, use:

```text
T1
T2
T3
...
```

Allow future metadata to provide real corner names.

Avoid producing unstable corner IDs between laps of the same track.

---

## 18. Corner Phases

Split corner analysis into approximately:

```text
ENTRY
MID / APEX
EXIT
```

Determine useful boundaries based on telemetry rather than arbitrary equal thirds.

For each corner calculate where possible:

### Entry

- braking point
- entry speed
- peak braking
- brake duration
- brake release characteristics
- turn-in position

### Mid

- minimum speed
- minimum-speed position
- approximate apex
- steering behaviour
- racing-line difference

### Exit

- throttle pickup
- full-throttle point
- exit speed
- acceleration
- line difference

Expose missing metrics explicitly.

---

## 19. Corner Time-Loss Decomposition

For each detected corner calculate time gain/loss.

Where reliable, split loss into:

```text
ENTRY
MID
EXIT
```

Example:

```text
T6                     +0.349s

Entry                  +0.031
Mid                    +0.171
Exit                   +0.147
```

This is a key feature.

The sum/decomposition must be mathematically coherent and tested.

---

## 20. Biggest Opportunities

Automatically rank where the user loses the most time.

Example:

```text
BIGGEST LOSSES

T6      +0.349s     MID / EXIT
T3      +0.312s     BRAKE RELEASE
T5      +0.096s     THROTTLE
```

Also calculate an appropriately caveated potential improvement.

Avoid claiming that independently combining every best micro-segment always represents an achievable lap.

---

## 21. Deterministic Analysis

Do **NOT** use an LLM in v0.1.

Generate useful observations using deterministic analysis.

Examples:

```text
You brake 11 m later but carry 8 km/h less minimum speed.
```

```text
Full throttle occurs 17 m later than the reference.
```

```text
The majority of the loss develops between minimum speed and corner exit.
```

Analysis must be evidence based.

Prefer:

```text
Unable to determine reliably.
```

over inventing an explanation.

### Structured output requirement

Human-readable strings must **NOT** be the primary representation of analysis.

Prefer structured types such as:

```rust
struct CornerAnalysis {
    // measured facts
    // derived metrics
    // phase losses
    // evidence
    // confidence
    // context
    // possible influences
}
```

The UI/deterministic presentation layer may turn those structures into prose.

---

## 22. Future LLM / Coaching Compatibility

TRACE will eventually support optional LLM-assisted coaching.

This is **NOT part of v0.1 implementation**.

However, `trace-core` **MUST** produce structured, serializable analysis results suitable for consumption by:

1. the TRACE UI
2. deterministic explanation generators
3. a future LLM coaching layer
4. other future analysis consumers

The LLM must never become the source of truth for telemetry mathematics.

Architecture:

```text
                 TRACE CORE
                     │
              AnalysisResult
                ┌────┴─────┐
                │          │
                ▼          ▼
          Deterministic    Future
             TRACE UI      Coach
                              │
                         LLM Provider
```

`trace-core` owns:

- telemetry mathematics
- normalization
- interpolation
- lap delta
- corner segmentation
- time-loss calculation
- braking analysis
- throttle analysis
- trajectory analysis
- setup comparison
- condition comparison
- comparability
- confidence
- evidence
- possible influences

The future LLM layer owns:

- natural-language explanation
- prioritization of coaching advice
- answering driver questions
- converting analysis into practice suggestions
- conversational interaction

The LLM **MUST NOT** be asked to infer numerical telemetry facts that `trace-core` can calculate deterministically.

Do not design around sending raw telemetry to an LLM and asking:

> Why am I slower?

Instead, `trace-core` should be capable of producing something conceptually similar to:

```json
{
  "corner": "T6",
  "time_loss_seconds": 0.349,
  "phase_loss": {
    "entry": 0.031,
    "mid": 0.171,
    "exit": 0.147
  },
  "evidence": {
    "brake_point_delta_m": -8.0,
    "minimum_speed_delta_kmh": -8.0,
    "throttle_pickup_delta_m": 11.0,
    "exit_speed_delta_kmh": -2.0
  },
  "context": {
    "comparability": "good",
    "setup_differs": true,
    "setup_influence": "possible"
  },
  "confidence": 0.87
}
```

This is illustrative only.

Design the real Rust domain types rather than copying this JSON blindly.

Analysis results should expose:

- measured facts
- derived metrics
- relevant comparison context
- confidence
- uncertainty
- data availability
- possible setup influence
- possible condition influence
- evidence supporting classifications

This requirement must **NOT** cause `trace-core` to become coupled to any specific LLM/provider/API.

---

## 23. Future Coaching Provider Architecture

Do **NOT** implement this in v0.1.

TRACE's future LLM coaching feature will use a **Bring Your Own Key (BYOK)** model.

TRACE itself should not pay for or centrally proxy routine LLM inference.

Intended architecture:

```text
TRACE Desktop
     │
     ├── trace-core analysis
     │
     ▼
Coach Layer
     │
     ▼
Provider Adapter
     ├── OpenAI
     ├── Anthropic
     ├── other remote providers
     └── local model providers where practical
```

The provider layer must remain completely separate from `trace-core`.

A future user should be able to configure:

- provider
- API key
- model
- provider-specific options where necessary

API keys **MUST NOT**:

- be stored in SQLite
- be stored in plaintext configuration
- be uploaded to the TRACE backend
- be included in TRACE session bundles
- be included in logs
- be included in crash reports
- be exposed to spectators
- be exposed through shared sessions

Use operating-system credential/keychain facilities for secret storage.

Where possible, remote LLM requests should be made directly from the desktop application to the selected provider.

The TRACE backend should not need access to the user's provider key.

The future provider abstraction should support capabilities rather than assuming every provider behaves identically.

Do not implement this abstraction until the coaching feature is actually being developed unless a small boundary is naturally required earlier.

---

## 24. Comparison Context

Every lap comparison should determine how comparable the laps actually are.

Consider:

- setup
- fuel
- tyre compound
- tyre state
- ambient temperature
- track temperature
- track grip
- weather
- simulator
- car
- track/layout

Present an overall qualitative assessment such as:

```text
COMPARABILITY

Setup             Different
Fuel              +0.9 L
Tyres              Same
Track temp         +2°C
Track grip         Same

Overall            GOOD
```

Use conservative thresholds.

Comparability should be represented structurally, not only as presentation text.

---

## 25. Setup Capture

Setup information should be treated separately from live telemetry.

For Assetto Corsa, investigate the appropriate way to identify/read the active setup.

Snapshot setup state when possible so later changes to a setup file do not alter historical session context.

Do not make the analysis engine depend on AC setup formats.

Normalize setup parameters into a generic representation.

Different cars expose different parameters.

Prefer something conceptually similar to:

```rust
struct SetupParameter {
    category: SetupCategory,
    key: String,
    display_name: String,
    value: SetupValue,
    unit: Option<Unit>,
}
```

rather than a giant fixed `Setup` struct containing every possible adjustment.

Preserve original/source values where useful for round-tripping or diagnostics.

---

## 26. Setup Comparison

Allow setup snapshots to be compared.

Group parameters into useful categories such as:

- tyres
- alignment
- aero
- suspension
- dampers
- differential
- brakes
- drivetrain

Default to:

```text
DIFFERENCES ONLY
```

Provide an option to show all parameters.

Example:

```text
SETUP A             SETUP B

Rear wing
8                   6

Brake bias
56%                 55%

Front ARB
4                   5
```

---

## 27. Setup Influence vs Driving Influence

This feature is extremely important.

TRACE must **NOT** automatically attribute telemetry differences to driver skill when setup/conditions differ.

Example:

```text
Reference:
Rear wing 8

Comparison:
Rear wing 6
```

Suppose:

```text
Corner exit
Reference: 143 km/h
Comparison: 142 km/h

+100m
Reference: 165
Comparison: 167

+300m
Reference: 205
Comparison: 211
```

The comparison lap exits slower but progressively gains speed down the straight.

TRACE should recognize that this pattern is **consistent with** a vehicle/setup performance difference.

A suitable output is:

```text
SETUP INFLUENCE POSSIBLE

The comparison lap exits 1 km/h slower but develops a
6 km/h advantage by the braking zone.

The comparison setup also uses less rear wing.

Do not classify this straight-line gain primarily as a
driving improvement.
```

Do **NOT** say:

```text
Lower rear wing gained 6 km/h.
```

unless causality can genuinely be established.

Use categories such as:

```text
DRIVING DOMINANT
SETUP / CONDITIONS MAY CONTRIBUTE
COMPARISON COMPROMISED
INSUFFICIENT EVIDENCE
```

The system should be conservative.

Each classification should expose supporting evidence and confidence/uncertainty.

---

## 28. Setup Revisions and Notes

Support lightweight setup history.

A setup may have multiple snapshots/revisions associated with sessions.

Allow local notes.

Example:

```text
Baseline
  ↓
v2
Front ARB 4 → 5

  ↓
v3
Brake bias 56 → 54

  ↓
v4
Rear wing 8 → 7
```

Do **NOT** attempt to recommend setup changes in v0.1.

---

## 29. Optimal Laps

Provide:

### Session optimal

Best compatible segments within the current session.

### Historical optimal

Best compatible segments from stored sessions where comparison conditions are sufficiently appropriate.

Display clearly:

```text
PB                  1:52.843
Session optimal     1:52.291
Historical optimal  1:51.972
```

Do not present mathematically stitched laps as guaranteed achievable performance.

Clearly describe how segments are defined.

---

## 30. MoTeC Import

Implement an importer architecture.

`trace-motec` should convert supported imported telemetry into canonical TRACE data.

Investigate actual MoTeC formats and available parsing options before implementation.

Do not invent format details.

If there are licensing/proprietary-format limitations, document them clearly and support the practical subset that can legally/reliably be parsed.

Imported laps should behave like native TRACE laps after conversion.

Source provenance must remain available:

```text
SOURCE
MoTeC import
```

Missing channels are normal.

---

## 31. Portable TRACE Format

Design a portable session/lap bundle format.

Working extension:

```text
.trace
```

or another appropriate extension if conflicts exist.

Conceptually it may contain:

```text
manifest.json
session.json
track.json
setups/
laps/
analysis/
```

Do not blindly use this exact structure if a better representation exists.

Requirements:

- versioned
- portable
- self-describing
- simulator-independent
- capable of containing multiple laps
- capable of containing setup/context
- capable of containing reconstructed track geometry
- suitable for sharing
- future-compatible

The format should distinguish raw/source telemetry from normalized/derived analysis where appropriate.

Treat imported bundles as untrusted input.

---

## 32. Shared Recorded Sessions

Users should be able to share a completed session without creating an account.

The desktop application may upload an immutable/shared telemetry bundle to the backend.

The backend returns an unguessable share ID.

Example:

```text
https://<configured-host>/s/K8c2Fx
```

The web application loads and displays the shared session.

No login.

No comments.

No annotations.

No social functionality.

Provide the ability to disable/delete a share using a locally retained management secret if feasible without creating accounts.

Do not put management secrets into public URLs.

---

## 33. Local Driver Identity

TRACE does not have user accounts.

In local settings allow:

```text
Display name       3X3
Secondary name     deR00kie
```

Secondary name is optional.

Store these locally.

They are included in live-session metadata.

Do not require globally unique names.

Do not build username ownership.

---

## 34. Live Telemetry

TRACE supports live spectating.

This is part of v0.1.

When the user enables LIVE:

```text
Simulator
    ↓
Simulator Adapter
    ↓
Canonical TRACE telemetry
    ├──► full-resolution local recording
    │
    └──► live encoder
              ↓
          WebSocket
              ↓
        TRACE backend
              ↓
         spectators
```

The backend must never need to understand AC-specific telemetry.

It receives TRACE protocol messages only.

---

## 35. Live Telemetry Rate

Do not transmit raw maximum-rate telemetry unnecessarily.

Record locally at the best useful native rate.

For spectators, target approximately:

```text
20 Hz
```

unless testing establishes a better value.

The web client may interpolate presentation between frames.

Completed laps can upload higher/full-resolution telemetry separately.

The live protocol should prioritize:

- low latency
- resilience
- bounded bandwidth
- forward compatibility

Avoid coupling the wire protocol directly to in-memory Rust structs without explicit versioning.

---

## 36. Live Session URL

No account is required.

Generate an unguessable live session identifier.

Example:

```text
https://<configured-host>/live/K8c2Fx
```

The desktop app exposes:

```text
GO LIVE
COPY LIVE LINK
STOP LIVE
```

Do not build permanent vanity URLs in v0.1.

---

## 37. Live Visibility

For v0.1, support the concepts:

```text
PRIVATE
UNLISTED
```

If PRIVATE has no practical server-side use without authentication, it may simply mean live publishing is disabled/local-only.

The important shared mode is:

```text
UNLISTED — anyone with the URL can spectate.
```

Do **NOT** implement public discovery.

---

## 38. Live Spectator Page

The spectator page should prioritize information useful to someone acting as a remote engineer.

Display where available:

```text
● LIVE

Driver
Simulator
Track
Car
Session type

Current lap
Current delta
Best lap
Lap number

Current sector

Speed
Gear
RPM

Throttle
Brake
Steering

Fuel

Tyre state
Temperatures
Pressures

Track temperature
Conditions

Track map
Current vehicle position

Recent lap times
Sector performance

Live-vs-reference/PB telemetry where practical
```

The spectator telemetry view includes a buffered seek bar. Its trailing edge has a
`LIVE ●` control: scrubbing moves that spectator into an explicit behind-live state,
while selecting `LIVE ●` jumps to the newest retained timestamp and resumes following.
Seeking must not pause the publisher or change another spectator's position. If the
requested timestamp has expired from the bounded server buffer, clamp to the oldest
available point and communicate that limit.

Do not add:

- chat
- reactions
- spectator accounts
- annotations

Spectator count is optional if trivial.

---

## 39. Live Resilience

Handle:

- simulator closing
- game pausing
- session restarting
- network loss
- backend restart
- reconnect
- desktop app closing unexpectedly

A network interruption **MUST NOT** affect local recording.

The live page should clearly communicate:

```text
LIVE
RECONNECTING
SESSION ENDED
```

rather than silently freezing.

Define sensible session timeout/expiry behaviour.

---

## 40. Backend Authentication Without Accounts

Do not create user accounts simply to authenticate telemetry publishers.

Generate an installation/device identity or publishing credential locally.

Use it to authorize:

- creating live sessions
- publishing telemetry
- ending live sessions
- creating shared-session uploads
- managing those shares

This mechanism should remain invisible during normal use.

Design it so proper accounts could claim/migrate installations later if TRACE ever adds accounts.

Do not implement that account system now.

Credentials must be handled as secrets.

---

## 41. Frontend Reuse

Avoid maintaining completely independent desktop and web frontends.

Prefer:

```text
telemetry-ui
       │
   ┌───┴────┐
Desktop    Web
```

Build reusable:

- telemetry graphs
- track visualization
- timing components
- session metadata
- lap comparison components
- setup diff
- live telemetry components

Introduce a platform/data-source abstraction.

Desktop data comes from Tauri.

Web data comes from TRACE backend/shared bundles.

Components should not care unnecessarily where the data originated.

Do not contort every desktop-only feature into the web abstraction if a clean capability flag/boundary is sufficient.

---

## 42. Desktop Navigation

Keep navigation small.

Initial top-level sections:

```text
LIVE
SESSIONS
COMPARE
SETUPS
```

Settings can exist separately.

Avoid adding an Analytics section unless functionality genuinely does not fit Compare.

The comparison workspace is the main product.

---

## 43. Visual Design

TRACE must have a distinctive motorsport engineering aesthetic.

Do **NOT** make it look like:

- generic SaaS
- Material Design
- rounded mobile UI
- gaming RGB software
- glassmorphism
- cryptocurrency dashboard

Use the following core palette:

```text
Base                #121212
Primary surface     #1A1A1A
Raised surface      #222222
Dark divider        #2A2A2A
Primary accent      #C9FF00
Primary text        #E8E8E8
```

Additional semantic telemetry colours may be introduced for:

- brake
- throttle
- reference lap
- comparison lap
- gain
- loss
- warnings

Keep them restrained, consistent and accessible.

---

## 44. Shape Language

**IMPORTANT:** TRACE uses a **boxy, hard-edged UI**.

Avoid rounded corners.

Prefer:

- square panels
- sharp borders
- hard dividers
- rectangular buttons
- compact tables
- dense engineering layouts
- tabular/monospaced numbers
- precise grid alignment

Border radius should generally be:

```css
border-radius: 0;
```

Tiny radius may only be used where required for a specific visualization primitive and should not define the visual language.

---

## 45. Typography

Use highly legible typography.

Telemetry values should use tabular numerals.

Monospace typography may be used selectively for:

- times
- deltas
- measurements
- telemetry values
- technical status

Do not make all application text monospace.

Use uppercase compact labels where appropriate:

```text
LAP DELTA / S
VELOCITY / KMH
BRAKE PRESSURE
COMPARISON CONTEXT
SETUP INFLUENCE
```

---

## 46. Accent Usage

`#C9FF00` is the TRACE identity colour.

Use it for:

- current selection
- active navigation
- primary comparison trace
- cursor highlights
- important telemetry state
- primary actions
- selected track/corner
- status indicators

Do **NOT** flood the UI with neon yellow.

Most of the application should remain matte dark.

---

## 47. Comparison Workspace Layout

The primary comparison workspace should approximately contain:

```text
┌──────────────────────────────────────────────────────────┐
│ TRACE │ Track / Car │ Ref Lap │ Compare Lap │ Share     │
├───────┬───────────────────────────────────┬──────────────┤
│       │ LAP DELTA                         │ CORNER       │
│ NAV   │                                   │ ANALYSIS     │
│       ├───────────────────────────────────┤              │
│       │ SPEED                             │              │
│       ├───────────────────────────────────┤              │
│       │ BRAKE / THROTTLE                  ├──────────────┤
│       ├───────────────────────────────────┤ COMPARISON   │
│       │ STEERING                          │ CONTEXT      │
│       ├───────────────────────────────────┤              │
│       │ TRACK MAP        │ CORNER DETAILS │              │
├───────┴──────────────────┴────────────────┴──────────────┤
│ TRACE ENGINE • AC MODULE • SAMPLE RATE • BACKEND STATUS │
└──────────────────────────────────────────────────────────┘
```

Use this as design direction, not an inflexible pixel specification.

---

## 48. Corner Analysis UX

The user should be able to see immediately:

```text
T6
+0.349s

BRAKE POINT
YOU 118m
REF 126m

MIN SPEED
YOU 121
REF 129

THROTTLE PICKUP
YOU 42m
REF 31m
```

and an evidence-based summary:

```text
PRIMARY LOSS //

You brake later but overslow by 8 km/h.
The loss develops through the apex and delays
full throttle by approximately 11m.
```

Clicking a corner should zoom/synchronize relevant graphs and track position.

---

## 49. Loss Distribution

Provide a compact ranking such as:

```text
LOSS DISTRIBUTION

T6       +0.349     MID / EXIT
████████████████

T3       +0.312     BRAKE RELEASE
██████████████

T5       +0.096     THROTTLE
████

T4       -0.037     GAIN
```

This should be one of the fastest ways for the driver to decide what to practise.

---

## 50. Performance

Telemetry applications can contain large datasets.

Be deliberate about:

- allocations
- serialization
- IPC payload size
- chart rendering
- downsampling
- caching
- interpolation
- file IO

Do not send enormous raw telemetry arrays repeatedly across Tauri IPC.

Consider:

- precomputed normalized arrays
- typed/binary payloads where appropriate
- view-range downsampling
- cached analysis results

Do not optimize blindly, but design with realistic telemetry volumes in mind.

Measure before introducing complex optimizations.

---

## 51. Testing

The analysis engine requires strong automated tests.

At minimum test:

- interpolation
- distance normalization
- lap alignment
- delta calculation
- lap boundary detection
- corner segmentation
- braking-point detection
- throttle pickup
- minimum speed
- entry/mid/exit decomposition
- setup comparison
- comparability scoring
- setup/condition influence classifications where deterministic

Create synthetic telemetry fixtures where useful.

For important mathematical operations, test known expected outputs.

Add regression fixtures when real-world telemetry exposes bugs.

---

## 52. Recorded Fixtures / Replay

Create a mechanism for replaying previously captured telemetry through the system.

This allows development without Assetto Corsa constantly running.

Conceptually:

```text
AC live
   │
   ▼
Canonical frames
   │
   ├── record fixture
   │
   ▼
fixture.trace
   │
   ▼
ReplayAdapter
```

A replay/test adapter is encouraged.

It should behave sufficiently like a simulator adapter that UI/live pipelines can be tested offline.

The desktop app should also be able to stream an already-recorded session through the
same live publisher used by simulator capture. Preserve its sample spacing, rebase the
clock to the broadcast start, leave the source session immutable, and identify the
broadcast as replayed telemetry. Spectator seeking operates within the server's
retained broadcast buffer; `LIVE ●` returns to the replay's current broadcast position.

---

## 53. Logging and Diagnostics

Use structured logging.

Important events include:

- simulator detected
- simulator disconnected
- session started
- lap started
- lap completed
- invalid lap
- setup captured
- track geometry captured
- backend connected
- live started
- live disconnected
- live reconnected
- telemetry import success/failure

Avoid noisy per-frame logs by default.

Provide enough diagnostics to debug telemetry capture issues.

Never log secrets or future LLM API keys.

---

## 54. Configuration

Configuration should include at least:

```text
Driver display name
Secondary name
Backend URL
Local storage location
Live publishing enabled/default
Telemetry recording settings
```

Do not expose meaningless technical configuration to ordinary users unless necessary.

Future coaching/provider configuration is not part of v0.1.

---

## 55. Error Handling

Telemetry acquisition will fail in unusual ways.

Never panic merely because:

- a simulator field is unavailable
- setup data is missing
- a track cannot be identified
- one telemetry channel disappears
- backend is offline
- imported data lacks a channel

Represent missing information explicitly.

The UI should degrade gracefully.

Example:

```text
SETUP COMPARISON
Unavailable for this lap.
```

rather than breaking the entire comparison.

Distinguish recoverable data-quality issues from fatal application errors.

---

## 56. Security

Because TRACE publishes data from a desktop client to a public server:

- never expose arbitrary local files
- validate uploaded bundle formats
- validate WebSocket messages
- enforce payload limits
- use unguessable share/live identifiers
- rate limit public endpoints where appropriate
- do not trust client-provided filenames
- keep publisher credentials secret
- do not put secrets into URLs
- use TLS in production
- treat imported telemetry as untrusted input
- protect against decompression/archive bombs in portable bundles
- constrain resource use for malformed telemetry

Do not over-engineer enterprise security, but avoid obvious unsafe shortcuts.

---

## 57. Development Phases

Implement incrementally.

### Phase 1 — Foundation

Build:

- workspace/repository
- canonical telemetry types
- simulator adapter interface
- test/replay adapter
- local storage abstractions
- minimal desktop shell
- TRACE design tokens/components

Do not build a huge UI yet.

### Phase 2 — Assetto Corsa capture

Implement:

- AC detection
- shared-memory capture
- canonical mapping
- session detection
- lap recording
- telemetry persistence
- basic session browser

At the end of this phase, I should be able to drive AC and see recorded sessions/laps in TRACE.

### Phase 3 — Core comparison

Implement:

- distance normalization
- interpolation
- lap delta
- synchronized speed/brake/throttle/steering graphs
- reference/comparison selection

At the end of this phase, TRACE must already be useful for finding where lap time was lost.

**THIS IS THE FIRST MAJOR USABLE MILESTONE.**

### Phase 4 — Track/corner analysis

Implement:

- track reconstruction
- racing-line visualization
- corner detection
- entry/mid/exit phases
- corner metrics
- loss distribution
- biggest opportunities
- deterministic explanations

At the end of this phase, TRACE should answer:

> Where am I losing time and what am I doing differently?

### Phase 5 — Backend/live

Implement:

- TRACE protocol
- backend
- installation publishing credentials
- live session creation
- live WebSocket ingestion
- spectator WebSocket fan-out
- web spectator page
- reconnect handling
- live link generation
- completed-lap upload

At the end of this phase:

> I should be able to hotlap in Assetto Corsa and send someone a URL where they can watch my telemetry in near-real time.

### Phase 6 — Import/export

Implement:

- portable TRACE format
- export/import
- MoTeC importer
- provenance handling

### Phase 7 — Context/setup

Implement:

- setup snapshot/import
- setup comparison
- conditions comparison
- comparability scoring
- setup-influence warnings
- setup revision history
- notes

At the end of this phase, TRACE should help distinguish:

> Driving difference vs potentially setup/conditions-influenced difference.

### Phase 8 — Sharing

Implement:

- upload recorded session
- unguessable share link
- web comparison viewer
- local share-management secret
- delete/disable share where practical

Still **NO accounts**.

### Post-v0.1 — Optional Coach

Not part of the current implementation plan.

Potential future work:

- BYOK LLM provider layer
- session coaching
- corner explanations
- conversational telemetry questions
- structured UI actions such as focusing a corner/range
- practice-plan suggestions

Do not begin this phase unless explicitly instructed after v0.1.

---

## 58. Initial UX Goal

A normal workflow should eventually be:

```text
Open TRACE
    ↓
TRACE detects Assetto Corsa
    ↓
Start driving
    ↓
TRACE automatically records session
    ↓
Finish stint
    ↓
Open session
    ↓
Select PB as reference
    ↓
Select another lap
    ↓
Immediately see:
    - total gap
    - delta graph
    - biggest losses
    - corner analysis
    - racing-line differences
    - setup/context differences
    ↓
Choose T6
    ↓
Understand exactly what differed
    ↓
Return to AC and practise T6
```

Live workflow:

```text
Open TRACE
    ↓
GO LIVE
    ↓
COPY LIVE LINK
    ↓
Send link
    ↓
Spectator opens browser
    ↓
Telemetry appears in near-real time
```

No account creation anywhere.

---

## 59. Critical Architectural Rules

Treat these as hard requirements.

### Rule 1

`trace-core` knows nothing about Assetto Corsa.

### Rule 2

Assetto Corsa-specific structures never reach the frontend.

### Rule 3

The frontend consumes canonical TRACE models.

### Rule 4

The backend consumes TRACE protocol messages, not AC messages.

### Rule 5

Network failure never stops local telemetry recording.

### Rule 6

Raw telemetry does not become millions of SQLite rows.

### Rule 7

Lap comparison is distance-aligned.

### Rule 8

Missing telemetry channels are normal and must be supported.

### Rule 9

TRACE never claims setup causality without sufficient evidence.

### Rule 10

No accounts in v0.1.

### Rule 11

No annotations/chat/social functionality in v0.1.

### Rule 12

No rounded SaaS design. TRACE is boxy and engineering-oriented.

### Rule 13

Do not implement other simulators yet. Build the abstraction correctly and implement AC.

### Rule 14

Do not sacrifice a usable comparison tool to build infrastructure prematurely.

### Rule 15

`trace-core` analysis output is structured and machine-consumable; presentation strings are secondary.

### Rule 16

Future LLM coaching never performs telemetry mathematics that TRACE can calculate deterministically.

### Rule 17

Future LLM provider integrations and API keys remain outside `trace-core`.

### Rule 18

The future coaching model is BYOK; TRACE's backend should not require access to user LLM keys.

---

## 60. Before Writing Significant Code

Before implementing, perform a technical investigation and produce a concise implementation plan covering:

1. Assetto Corsa shared-memory APIs and available fields.
2. CSP-specific additional telemetry worth optionally supporting.
3. AC setup-file discovery and parsing.
4. MoTeC import feasibility/formats.
5. Suitable high-performance React plotting library.
6. Raw telemetry storage representation.
7. Track reconstruction strategy.
8. Distance normalization/interpolation strategy.
9. Delta calculation methodology.
10. Corner detection methodology.
11. Live telemetry protocol.
12. Backend persistence/object-storage strategy.
13. Monorepo/workspace structure.
14. Review all proposed `trace-core` analysis outputs for future machine consumption. Confirm that analysis facts, evidence, confidence, uncertainty and comparison context are represented structurally rather than embedded only in presentation strings.

For each uncertain external format/API, verify against real documentation/source material rather than guessing.

Identify technical risks before implementation.

Do **NOT** design or implement the LLM coaching system during this investigation. The purpose of item 14 is only to ensure the telemetry analysis foundation will support it cleanly later.

---

## 61. How to Work

Do not attempt to generate the entire application in one enormous pass.

Work phase-by-phase.

At the beginning of each phase:

1. state the goal
2. inspect existing code
3. propose the concrete changes
4. identify uncertainties
5. implement the smallest coherent slice
6. add tests
7. run tests/lint/typecheck
8. summarize what now works
9. state what remains

Avoid speculative abstractions that do not yet have a consumer.

However, preserve the hard architectural boundaries described above.

When you encounter ambiguity, prefer the solution that keeps TRACE:

- local-first
- simulator-agnostic
- deterministic
- evidence-based
- testable
- lightweight
- easy to extend

Do not silently expand scope.

---

## 62. First Task

Start with **Phase 1 only**.

Do **NOT** begin implementing the complete product.

First:

1. Research the technical questions in Section 60.
2. Propose the final workspace architecture.
3. Define the canonical domain model.
4. Define simulator adapter boundaries.
5. Define storage boundaries.
6. Define frontend/backend shared protocol boundaries.
7. Identify which AC telemetry fields map cleanly into the canonical model.
8. Identify missing/uncertain fields.
9. Propose the raw telemetry storage format.
10. Propose the initial database schema.
11. Propose the testing/fixture strategy.
12. Define structured analysis result conventions that remain suitable for a future optional LLM coach.
13. Produce an implementation plan for Phase 1.

Once the architecture has been reviewed, begin implementing Phase 1.

Do not move onto Phase 2 automatically unless instructed.

The most important thing is not the number of features implemented.

The most important thing is creating a solid telemetry foundation that lets TRACE reliably answer:

> **Where did the time go?**
