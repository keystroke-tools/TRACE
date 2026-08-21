# Asset provenance

This record identifies project assets produced with generative tools. Generated
assets are reviewed and accepted by maintainers under the same standards as code.

## TRACE application icon

| Field | Value |
| --- | --- |
| Repository paths | `apps/desktop/src-tauri/icons/icon.png`, `apps/desktop/src-tauri/icons/icon.ico` |
| Added | 2026-08-21 |
| Generator | OpenAI built-in image generation through Codex |
| Use | Tauri default window and application icon |
| Final formats | 512 × 512, 8-bit RGBA PNG; seven-size Windows ICO |
| License | Distributed as part of TRACE under the MIT License |

Prompt:

```text
Use case: logo-brand
Asset type: desktop application icon for TRACE, a motorsport telemetry and lap-analysis tool
Primary request: create a crisp, minimal square app icon based on a single stylized racing trace line forming an abstract capital T
Style/medium: flat geometric vector-like graphic rendered as a high-resolution raster icon
Composition/framing: centered, bold silhouette, generous safe padding, readable at 32px
Color palette: near-black charcoal background, warm signal orange trace line, small off-white accent only if necessary
Constraints: perfectly square; no words, no letters beyond the abstract T-shaped trace motif, no gradients, no shadows, no transparency, no border, no mockup, no watermark
Avoid: cars, steering wheels, checkered flags, speedometers, photorealism, fine detail
```

The selected 1254 × 1254 RGB output was converted without visual redesign to a
512 × 512 RGBA PNG because Tauri requires an RGBA default icon. The conversion used
Pillow 12.3.0 with Lanczos resampling and optimized PNG output.

The Windows ICO was derived from that accepted RGBA PNG with Pillow 12.3.0. It
contains 16, 24, 32, 48, 64, 128, and 256 pixel variants and introduces no visual
redesign.
