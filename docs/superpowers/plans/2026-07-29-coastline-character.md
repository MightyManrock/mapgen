# mapgen: coastline character — Implementation Plan

## Context

Every coast currently renders with one universal sandy color — the fixed
`[220, 200, 150]` bottom stop of `biome_terrain_color`'s elevation ramp
(`src/render.rs`). Real coasts vary, and the discussion identified the
signals we already have to drive that variation:

- **Slope** (the primary real-world determinant): flat land meeting the
  sea forms beaches; steep land forms cliffs with no beach band at all.
  Computed from the elevation field by central differences — no new data.
- **Temperature**: cold coasts get grey shingle instead of warm sand;
  hot+wet coasts shade toward muddy mangrove green.
- **River mouths**: big rivers hitting the ocean deposit delta mud.
  Mouth cells are river cells (`hydro.map` in `(0, 0.3]`) with an ocean
  4-neighbor; influence spreads by the same BFS-falloff machinery
  greening already uses.
- **Precipitation**: folded in via the hot+wet mangrove gate; arid
  coasts keep pale sand by default.

Render-only, like greening — no climate data or pipeline-order changes.
Approved as the follow-up to the greening/lake-effect/jitter feature set.
Process: implement directly, no subagent dispatch (user usage limits).
No automated tests by design; verify via `cargo run --release` + visual
inspection + determinism md5s.

## Task 1: River-mouth influence field

**Files:** new `src/shore.rs`, `src/greening.rs`, `src/main.rs`, `src/params.rs`

- In `src/greening.rs`: change `fn bfs_falloff` to `pub(crate) fn` (no
  other changes — it's exactly the machinery needed).
- New param in `params.rs` (+ `earth_like()`): `shore_mouth_radius: usize`
  = 3 — how far delta mud spreads from a river mouth, in cells.
- New module `src/shore.rs`:
  ```rust
  pub fn compute_mouth_influence(hydro_map: &HeatMap, is_ocean: &[bool], params: &PlanetGenParams) -> HeatMap
  ```
  Mouth cells = land cells with `hydro_map.data` in `(0.0, 0.3]` (river
  band) having at least one ocean 4-neighbor (x wraps, y clamps — same
  neighbor pattern as greening's BFS). Then
  `greening::bfs_falloff(&mouths, is_ocean, w, h, params.shore_mouth_radius, 1.0)`
  wrapped into a `HeatMap` so the renderer can sample it bilinearly.
- `main.rs`: `mod shore;` and compute
  `let mouth_influence = shore::compute_mouth_influence(&hydro.map, &is_ocean, &params);`
  after `green` is computed.

Commit: `feat: river-mouth influence field for shore rendering`

## Task 2: Shore-aware beach color in the renderer

**Files:** `src/render.rs`, `src/params.rs`, `src/main.rs`

- New param (+ `earth_like()`): `shore_cliff_slope: f64` = 0.012 (initial guess 0.03 produced zero cliff collapse on this seed; 0.012 gives ~8% full-cliff, ~40% pure-beach, ~51% blended coastal cells) —
  per-cell elevation delta at which the beach band fully vanishes into
  cliff; the collapse fades in from half this value. First-guess tunable
  (coastline roughening injects ±0.08 amplitude noise near sea level, so
  coastal gradients are noisy — expect to tune during verification).
- New helper in `render.rs`, near the biome corner-color consts:
  ```rust
  /// Beach color from local climate + river-mouth proximity. Applied only
  /// at the elevation ramp's waterline stop; cliff collapse happens in
  /// biome_terrain_color via shore_cliff_t.
  fn shore_beach_color(temp_t: f64, precip_t: f64, mouth_t: f64) -> [u8; 3]
  ```
  with consts (all empirically tunable):
  - `SAND: [220, 200, 150]` — the current universal color, still the
    default for temperate/arid coasts;
  - `SHINGLE: [155, 152, 148]` — cold grey gravel; lerp SAND→SHINGLE by
    `((0.25 - temp_t) / 0.10).clamp(0, 1)`;
  - `MANGROVE: [110, 125, 80]` — hot+wet muddy green; lerp toward it by
    `min((temp_t - 0.60) / 0.15, (precip_t - 0.55) / 0.20).clamp(0, 1)`;
  - `DELTA_MUD: [155, 130, 100]` — lerp toward it last by
    `mouth_t * 0.8` (delta mud overrides climate color but never fully
    saturates).
- `biome_terrain_color` gains two params:
  `beach: [u8; 3], shore_cliff_t: f64`. The `0.00` stop becomes
  `lerp_color(beach, base, shore_cliff_t)` — at `shore_cliff_t = 1.0`
  the waterline stop equals the biome color and the beach band
  disappears entirely (cliff: terrain color runs straight into water).
- In `composite_pixel_color` (both `biome_terrain_color` call sites —
  glacier-edge fallthrough and plain land):
  ```rust
  let dxs = 1.0 / width as f64;
  let dys = 1.0 / height as f64;
  let gx = (elevation.sample(nx + dxs, ny) - elevation.sample(nx - dxs, ny)) / 2.0;
  let gy = (elevation.sample(nx, ny + dys) - elevation.sample(nx, ny - dys)) / 2.0;
  let slope = (gx * gx + gy * gy).sqrt();
  let shore_cliff_t = ((slope / params.shore_cliff_slope) * 2.0 - 1.0).clamp(0.0, 1.0);
  let beach = shore_beach_color(temp_t, precip_t, mouth_influence.sample(nx, ny));
  ```
  (`HeatMap::sample` already rem_euclids x and clamps y, so the negative
  offsets are safe. Hoist the slope/beach computation above the
  branches so both call sites share it. `temp_t`/`precip_t` for the
  beach color are the same raw sampled values already computed for
  `biome_terrain_color` — reuse, don't resample.)
- `composite_pixel_color`, `save_composite`, `save_regions` gain a
  `mouth_influence: &HeatMap` parameter, threaded exactly like
  `greening` was; `main.rs` passes `&mouth_influence` to both save
  calls.

Commit: `feat: slope- and climate-aware coastline rendering`

## Task 3: Shore debug render

**Files:** `src/render.rs`, `src/main.rs`

- `save_shore(width, height, elevation, temperature, precipitation, mouth_influence, is_ocean, params, path)`
  in the `save_ocean_currents`/`save_greening` debug style: ocean base
  `[40, 40, 60]`; land cells with an ocean 4-neighbor paint their
  full-strength beach color with cliff collapse applied (compute
  `shore_beach_color` + slope at the cell center, and lerp toward plain
  land grey `[70, 65, 55]` by `shore_cliff_t` so cliffs read as "no
  beach"); interior land plain `[70, 65, 55]`. Gives a 1-cell coastal
  outline color-coded by shore type for tuning.
- `main.rs`: `render::save_shore(..., "output/shore.png")?;` next to the
  other debug renders; bump the final println to
  `"Done — 13 layers written to output/"`.

Commit: `feat: shore character debug render`

## Task 4: Verification (no commit until docs step)

1. `cargo build` clean at task boundaries; `cargo run --release`.
2. `output/shore.png`: coastal outline should show grey shingle at high
   latitudes, sand through the temperate/arid belts, mangrove tones on
   hot+wet coasts (this seed's east-coast rainforest belt), mud patches
   at big river mouths, and gaps/land-grey where coasts are steep.
3. Composite before/after crops at three spots: a steep coast (beach
   band gone — biome color meets water), a flat arid coast (unchanged
   sand), the hot-wet east coast (mangrove-tinted waterline). Also the
   polar coastline (shingle, no warm tan against ice). Tune
   `shore_cliff_slope` and the color-gate thresholds if the coast reads
   wrong — expect at least one tuning iteration.
4. Determinism: two runs, md5 on `composite.png`, `shore.png`,
   `regions.png` — identical.
5. Copy this plan to
   `docs/superpowers/plans/2026-07-29-coastline-character.md`, commit,
   push everything to `origin main`.

## Non-goals

- Substrate geology (black volcanic sand, coral, barrier islands) — no
  data exists for it.
- Any climate/data-field change, marker/tooltip change, or hydrology
  change — strictly render-side.
- Region polygon export for Obsidian — the next feature after this.
