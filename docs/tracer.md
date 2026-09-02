# Tracer driving cues

Tracer is TRACE's optional in-game coaching HUD for Assetto Corsa. It is a Custom
Shaders Patch (CSP) Lua app that reads a compact reference lap prepared by the TRACE
desktop application. It does not control the car.

## Install and use

In TRACE, open **Settings → Games** and choose **Install Tracer** below the Assetto
Corsa directory. The bundled app is copied to
`<Assetto Corsa>/apps/lua/TRACE_Tracer`. Once installed, each newer TRACE build updates
those files at startup so the desktop application and HUD stay compatible. Tracer uses
the same manually configured or auto-detected installation shown on the Settings page.

Open Assetto Corsa with CSP enabled. Tracer provides three independently positioned
windows in the in-game app shelf:

- **Tracer - Brake** shows the upcoming braking cue and reference brake pressure;
- **Tracer - Gear** shows the current and reference gears; and
- **Tracer - References** handles session and lap selection.

There is deliberately no live pedal-input display: Tracer is a coaching aid rather
than another pedal telemetry overlay. Users can open only the coaching windows they
want and keep the larger reference browser closed while driving.

Keep TRACE running while choosing a reference. The References window asks TRACE's
loopback-only bridge for recorded sessions matching the current source car, track, and
layout. Expand a session and select any timed lap; the fastest valid lap is marked as a
convenient default rather than being forced. Selecting a lap makes TRACE:

1. load the selected lap;
2. resample it on TRACE's existing five-metre distance grid;
3. derive stable braking zones from the brake channel; and
4. write `reference.json` to CSP's writable per-app configuration directory.

Exact car, track, and layout matches are shown first. **Other tracks** broadens the
list to sessions recorded with the same car. Loading one requires an explicit warning
confirmation because percentage-of-lap alignment can drift when layouts or lengths
differ. The HUD visibly marks an active manual track override.

Tracer shows a preparing state during generation and begins coaching when the reference
is ready. The generated reference remains usable if TRACE is subsequently closed.

The coaching windows show:

- distance to the next reference braking zone;
- a prominent brake-now state while inside that zone;
- reference brake pressure at the current distance; and
- current and reference gear.

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
- Session discovery uses `127.0.0.1:18081`; it does not contact a hosted TRACE service.
- Bridge requests carry a Tracer-specific header and every selection is revalidated
  against the current car. A track mismatch is rejected unless the in-game warning was
  explicitly accepted.
- A prepared reference works without TRACE running, preserving local/offline use, but
  choosing another session requires the desktop app.
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
