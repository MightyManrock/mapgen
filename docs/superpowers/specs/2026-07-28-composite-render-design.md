# mapgen: composite terrain render

## Context

The elevation/climate/hydrology prototype (see
`2026-07-28-prototype-pipeline-design.md`) renders each physical layer to
its own PNG for debugging/validation. This spec adds one more output: a
single naturalistic composite render — terrain colored by elevation band,
water blended in from the hydrology layer, contour lines — matching
demiurge-rust's `composite.png`. This is the image the user will actually
look at going forward; the per-layer PNGs stay as diagnostic output.

This composite is also a prerequisite for the next piece of work: once
it exists, the user will experiment with the Obsidian `zoom-map` plugin
against it to understand its JSON marker/region data format, which will
guide the later region-detection → `markers.json` exporter work.

## Goal

Port demiurge-rust's `render_composite_map` (`planet_gen.rs:2060-2192`)
into mapgen, adapted to drop the one branch mapgen has no generator for
(salt flats — `is_salt_flat`/`salt_flat_dist`/`salt_flat_color`, and the
`aridity` parameter that branch was the only consumer of). Everything
else — 3× supersampling, Bayer ordered-dithering on elevation bands,
anti-aliased water/glacier/sea-ice edges, contour lines — ports at full
fidelity.

## Architecture

Extends `src/render.rs` (no new module — composite rendering is still
"HeatMap(s) → PNG", the same responsibility every other function in that
file already has):

- New private helpers, ported from `planet_gen.rs`: `terrain_color`
  (1145-1160, not previously ported since it was composite-only),
  `BAYER_4X4` (1289-1294), `is_contour` (1298-1301), `bayer_dither`
  (1305-1311).
- New `pub fn save_composite(...)`, ported from `render_composite_map`
  (2060-2192) with the salt-flat `else if` branch removed — control
  falls through directly from the glacier branch to the terrain
  (default) branch, matching the order every other branch already
  checks in first (water → glacier → terrain).
- Signature:
  ```rust
  pub fn save_composite(
      width: usize, height: usize,
      hydro_map: &HeatMap, elevation: &HeatMap, temperature: &HeatMap,
      is_ocean: &[bool], is_glacier: &[bool], is_sea_ice: &[bool],
      params: &PlanetGenParams, path: &str,
  ) -> Result<(), image::ImageError>
  ```
  `render_width`/`render_height` are not parameters (unlike the
  reference, which threaded them in from a shared call site) — computed
  internally as `width * RENDER_SCALE` / `height * RENDER_SCALE`, since
  mapgen only ever calls this once. `aridity` is dropped (only used by
  the removed salt-flat branch). `params` supplies `sea_level`,
  `sea_ice_temp_threshold`, `glacier_temp_threshold`.

## Algorithm (verified against source, ported verbatim minus salt flats)

Constants: `RENDER_SCALE = 3`, `N_DITHER_LEVELS = 16`, `N_CONTOURS = 40`,
`CONTOUR_DARKEN = 0.90`, `CONTOUR_DARKEN_WATER = 0.95`.

For each supersampled output pixel `(rx, ry)`:
1. Determine `is_water` from `hydro_map.sample_nearest`: `<=0.0` dry,
   `<=0.3` river/lake (always water), else ocean — ocean gets
   Bayer-dithered edge anti-aliasing against dry neighbor cells so
   coastlines don't look blocky at the data-cell boundary.
2. Color selection, in order:
   - Water + sea-ice: edge-blended between `sea_ice_color` (dithered by
     local temperature) and `water_color` (dithered by hydro value).
   - Water (no sea ice): `water_color`, Bayer-dithered.
   - Glacier: edge-blended between `glacier_color` (dithered by local
     temperature) and `terrain_color` (dithered by land-elevation) —
     this is the fallback for non-glacier neighboring land.
   - Default (land, no glacier): `terrain_color`, dithered by
     `(elevation - sea_level) / (1 - sea_level)`.
3. Contour overlay: compare this pixel's bilinear-sampled elevation
   against its immediate right and down render-space neighbors; if they
   fall in different `N_CONTOURS`-quantized bands, darken the color by
   `CONTOUR_DARKEN` (or `CONTOUR_DARKEN_WATER` over water).

## Output

`output/composite.png` at 3072×1536 (3× the base 1024×512). Added to
the existing `main.rs` render sequence, after the per-layer renders.
Still under the existing `output/` gitignore entry — no new ignore rule
needed.

## Non-goals

Salt flats (no generator exists), region/political overlays, habitability
rendering — all still out of scope, unrelated to this change.

## Testing

Same as the rest of the prototype: visual check by eye. Expect smooth
elevation-band terrain coloring (no hard banding, thanks to dithering),
anti-aliased coastlines and ice edges, and visible topographic contour
lines following the terrain. Re-run determinism check (same seed →
identical composite.png) alongside the existing per-layer checks.
