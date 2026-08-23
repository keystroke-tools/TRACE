# Live pedal overlay

TRACE's Overlays page is a catalog and workshop for standalone telemetry views. The
left side lists available overlays; selecting one opens its correctly proportioned
preview, telemetry source, playback controls, and customisation options on the right.
The pedal overlay is the first catalog entry.

The preview can use a generated demonstration, the active live adapter, or any
recorded lap in the session library. Recorded playback follows the lap's canonical
elapsed clock and includes a play/pause button and seek bar. Imported `.trace`
sessions work through the same session library path. This makes overlay configuration
testable without running a simulator or driving another lap.

TRACE can open the configured preview as a compact, independent telemetry window. It
reads the latest canonical driver inputs from the active simulator adapter, so the
overlay does not connect to Assetto Corsa or any future simulator directly.

The overlay combines a scrolling pedal-input graph with narrow live throttle, brake,
and clutch bars and a circular steering-position indicator. Pedal values are whole
percentages; steering retains the simulator's degree value. The HUD itself has no
window chrome; its entire surface can be dragged, and customisation stays in the main
Overlays page. Close it from the Overlays page or from its right-click menu.

Users can change the history duration, graph width, overlay height, corner radius,
each signal colour, and the neutral background colour and opacity. Horizontal and
vertical graph guides can be toggled independently, as can the graph, clutch,
steering, labels, and numeric values. Preferences are stored locally and shared
across subsequent overlay windows. The standalone window has a stable
`TRACE // PEDALS` title for OBS Window Capture.

## OBS Browser Source

While TRACE is running it serves the overlay locally at
`http://127.0.0.1:18081/overlays/pedals`. The Overlays page shows a copyable URL with
the current customisation encoded in its query string. Add that URL as an OBS Browser
Source with the fixed dimensions shown beside the URL (540 × 180 by default). The page
does not stretch to the OBS viewport; update the Browser Source dimensions to match the
values shown by TRACE. The server binds only to the local loopback interface,
does not expose telemetry to the network, and stops when TRACE exits.

The Tauri frontend and local OBS endpoint poll the same lightweight canonical input
snapshot at approximately 30 Hz. Neither reads from storage, creates another simulator
reader, nor alters recording cadence. When capture stops or errors, input values reset
to zero instead of leaving stale pedal positions on screen.

Assetto Corsa throttle, brake, and its documented clutch ratio are mapped to canonical
TRACE ratios at the adapter boundary. Other simulators can populate the same fields
without changing the overlay.
