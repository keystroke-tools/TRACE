# Tracer driving cues

Tracer is TRACE's optional in-game coaching HUD for Assetto Corsa. It is a Custom
Shaders Patch (CSP) Lua app that reads a compact reference lap prepared by the TRACE
desktop application. It does not control the car.

## Current slice

From a recorded Assetto Corsa session, choose **Tracer** on a lap row. TRACE then:

1. installs or updates the Lua app in `<Assetto Corsa>/apps/lua/TRACE_Tracer`;
2. resamples the selected lap on TRACE's existing five-metre distance grid;
3. derives stable braking zones from the brake channel; and
4. writes `reference.json` to CSP's writable per-app configuration directory.

Open Assetto Corsa with CSP enabled and activate **Tracer** from the in-game app
shelf. The HUD validates the current simulator content against the source car, track,
and layout before showing cues.

The initial HUD shows:

- distance to the next reference braking zone;
- a prominent brake-now state while inside that zone;
- live brake and throttle with reference targets;
- current and reference gear; and
- the reference lap number and time.

## Reference profile

`reference.json` is versioned independently from recorded telemetry. Version 1 stores
source identity, track length, sample spacing, compact distance samples, and precomputed
braking zones. Sample keys are intentionally short because CSP parses the whole file:

| Key | Meaning            | Unit    |
| --- | ------------------ | ------- |
| `d` | lap distance       | metres  |
| `s` | speed              | km/h    |
| `t` | throttle position  | percent |
| `b` | brake position     | percent |
| `g` | gear               | integer |
| `e` | elapsed lap time   | seconds |

The HUD aligns the live car with the reference using `splinePosition × trackLengthM`.
It deliberately does not align by elapsed time: a slower driver still receives the
cue for the same place on the circuit.

## Braking-zone extraction

A zone begins when brake pressure reaches 5%. Releases of at most 15 metres are merged
so pedal noise and brief modulation do not split one corner into several zones. A
candidate must span at least 10 metres and peak at 20% to be retained. These constants
are deterministic and covered by Rust tests; later work can make them configurable
without changing the profile contract.

## Boundaries and limitations

- Tracer currently installs only for Assetto Corsa and requires CSP's Lua app support.
- A prepared reference works without TRACE running, preserving local/offline use.
- It uses recorded pedal position, not hydraulic brake pressure or a generated racing
  line, so cues are references rather than driving instructions guaranteed to be ideal.
- Racing-line placement, turn-by-turn assessment, and richer input history remain
  future slices.
- Simulator-specific installation and live-state access stay in the AC integration;
  telemetry analysis and recorded data remain simulator-agnostic.

The CSP API surface used by the HUD is documented by the
[official Lua SDK](https://github.com/ac-custom-shaders-patch/acc-lua-sdk), with app
patterns available in the
[official default apps repository](https://github.com/ac-custom-shaders-patch/app-csp-defaults).
