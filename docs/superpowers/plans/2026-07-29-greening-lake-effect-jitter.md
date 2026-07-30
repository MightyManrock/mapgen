# mapgen: freshwater greening + lake-effect climate + equator-jitter fix — Implementation Plan

## Context

Three items approved in discussion, shipped as one feature set (coastline
character is explicitly deferred to a follow-up):

1. **Freshwater greening (render-only).** Land near rivers/lakes renders
   greener — a vegetation effect, not a climate change (the Nile-through-
   desert argument: region tooltips correctly keep calling it arid).
   Aquifer cells (`HydrologyResult::aquifer_zones` — currently computed
   and consumed by nothing) act as weaker greening sources, producing
   scattered oasis patches tracing buried river courses through endorheic
   basins. Greening is gated by temperature (which already folds in both
   latitude and altitude via the lapse rate, covering the user's
   "high latitudes and altitudes ignore this") and dampened near ocean.
2. **Lake-effect climate (data-level, one-way).** Large lakes (connected
   lake area over a cell threshold) genuinely moderate nearby climate:
   a precipitation boost and diurnal-swing damping in a halo. Applied
   *after* hydrology as a single one-way pass — hydrology itself still
   consumes the unadjusted precipitation, so there is no feedback loop.
   Region detection moves after hydrology (verified: no dependency in
   either direction, it's purely call order in `main.rs`) so tooltips
   and aridity see the adjusted fields.
3. **Equator-jitter symmetry fix.** `lat_jitter` is per-column and
   applied to `abs_lat`, so northern/southern band meanders are exact
   mirror images. Fix: add signed latitude as a third noise coordinate
   (cylinder sampling) with a small vertical scale — hemispheres
   decorrelate smoothly while staying continuous at the equator.

Process note: implement directly in the main session (no subagent
dispatch or per-task review cycles — user is near usage limits). This
project has no automated tests by design; verification is
`cargo build`/`cargo run --release` + visual inspection + determinism
md5 checks.

## Task 1: Equator-jitter symmetry fix

**File:** `src/climate.rs`

- Extend `lat_jitter(x, width, fbm)` → `lat_jitter(x, y, width, height, fbm)`:
  add `let signed_lat = (y as f64 / height as f64 - 0.5) * PI;` and a new
  third coordinate `signed_lat * JITTER_LAT_SCALE` to the `fbm.get([...])`
  call (2D → 3D sample).
- New constant `const JITTER_LAT_SCALE: f64 = 0.3;` with a doc comment:
  pole-to-pole traversal moves ~0.94 noise units vs. the sampling
  circle's 1.5-unit circumference — enough to decorrelate the
  hemispheres at mid-latitudes while keeping the jitter continuous at
  the equator (both hemispheres share the z=0 slice there). Tune
  visually if bands turn blobby (the 3D-sphere lesson) — the key
  difference from the old rejected 3D approach is the z-scale is small
  and the x/y stay on the closed circle, so per-column coherence is
  preserved.
- In `generate_temperature` and `generate_precipitation`: drop the
  `jitter_by_x` per-column precompute; call
  `lat_jitter(x, y, width, height, &jitter_fbm)` inline in the existing
  per-cell closures (both already have `x` and `y` in scope). ~1M FBM
  calls total — negligible next to elevation generation.

Expected: all outputs change slightly (3D noise at z=0 ≠ the old 2D
sample), band meanders no longer mirror across the equator.

Commit: `fix: decorrelate climate band jitter between hemispheres`

## Task 2: Params

**File:** `src/params.rs` — add after the `current_*` fields, set in
`earth_like()`:

```rust
// Freshwater greening (render-only)
pub greening_radius: usize,          // 6  — BFS reach of river/lake greening, cells
pub greening_aquifer_strength: f64,  // 0.4 — aquifer source strength vs. 1.0 for surface water
pub greening_ocean_damp_dist: usize, // 5  — greening scales down within this distance of ocean
pub greening_strength: f64,          // 0.35 — max precip_t boost at render time
pub greening_temp_floor: f64,        // 0.30 — no greening below this temperature
pub greening_temp_full: f64,         // 0.45 — full greening above (linear fade between)
// Lake-effect climate (one-way, post-hydrology)
pub lake_effect_min_size: usize,     // 12 — connected lake cells to count as "large"
pub lake_halo_dist: usize,           // 6  — BFS halo reach over land, cells
pub lake_precip_boost: f64,          // 0.08 — max precipitation added at halo center
pub lake_swing_damp: f64,            // 0.4 — max fractional diurnal-swing reduction
```

All first-guess tunables per project precedent. Dead-code warnings until
Tasks 3–4 consume them — expected.

Commit: `feat: add greening and lake-effect params`

## Task 3: Lake-effect climate + pipeline reorder

**Files:** `src/climate.rs`, `src/main.rs`

In `climate.rs`:

- `pub fn compute_lake_halo(hydro_map: &HeatMap, is_ocean: &[bool], params: &PlanetGenParams) -> Vec<f64>`
  1. Lake cells = land cells with `hydro_map.data[idx]` in `(0.3, 0.5]`
     (the documented lake encoding band in `hydrology.rs` Phase 7).
  2. Connected components over lake cells, 4-connected, x-wrap-aware
     (same neighbor pattern as `regions.rs`'s `neighbors_4`; write a
     small local helper — `neighbors_4` is private to regions and this
     doesn't justify making it pub). Component labeling must iterate in
     index order (deterministic).
  3. Components with size ≥ `lake_effect_min_size` are large lakes.
  4. Multi-source BFS from all large-lake cells outward over **land**
     cells (skip ocean; x-wrap, y-clamp) up to `lake_halo_dist`.
     Per-cell falloff `1.0 - d / lake_halo_dist`, 0.0 elsewhere.
     Use `VecDeque`, push neighbors in fixed W/E/N/S order — BFS layer
     order makes the result value deterministic regardless of tie order.
- `pub fn apply_lake_precip(precipitation: &HeatMap, halo: &[f64], params: &PlanetGenParams) -> HeatMap`
  — `(p + halo * lake_precip_boost).clamp(0.0, 1.0)`, new HeatMap.
- `pub fn apply_lake_swing(swing: &HeatMap, halo: &[f64], params: &PlanetGenParams) -> HeatMap`
  — `s * (1.0 - halo * lake_swing_damp)`, new HeatMap.

In `main.rs`, reorder to (only the moved/new lines shown):

```rust
let precipitation = climate::generate_precipitation(...);   // unchanged, base field
let is_glacier = ...;                                        // unchanged
let hydro = hydrology::generate_hydrology(&elev, &is_ocean, &precipitation, &is_glacier, &params);
let lake_halo = climate::compute_lake_halo(&hydro.map, &is_ocean, &params);
let precipitation = climate::apply_lake_precip(&precipitation, &lake_halo, &params); // shadow: adjusted
let aridity = climate::generate_aridity(&temperature, &precipitation, params.et_factor);
let diurnal_swing = climate::apply_lake_swing(
    &climate::generate_diurnal_swing(&temperature, &aridity, &params), &lake_halo, &params);
let (region_map, regions) = regions::detect_regions(...);   // moved after hydrology, unchanged args
```

Hydrology consumes the *base* precipitation (line order above enforces
this); aridity, diurnal swing, region detection, and every render
consume the adjusted fields. Everything else in `main.rs` stays put.

Commit: `feat: one-way lake-effect climate from large lakes`

## Task 4: Greening map + render integration + debug PNG

**Files:** new `src/greening.rs`, `src/render.rs`, `src/main.rs`

New module `src/greening.rs`:

```rust
pub fn compute_greening(
    hydro_map: &HeatMap,
    aquifer_zones: &[(usize, usize)],
    is_ocean: &[bool],
    temperature: &HeatMap,
    params: &PlanetGenParams,
) -> HeatMap
```

1. Surface-water sources: land cells with `hydro_map.data[idx] > 0.0`
   (rivers `(0,0.3]` and lakes `(0.3,0.5]`). Aquifer sources:
   `aquifer_zones` entries (already deterministic — position-hash
   thinning, no RNG state).
2. Two multi-source BFS passes over land (same wrap/clamp/order rules
   as Task 3's halo BFS), radius `greening_radius`:
   surface pass source strength 1.0, aquifer pass
   `greening_aquifer_strength`. Per-cell value
   `strength * (1.0 - d / greening_radius)`; combine passes with `max`.
   (Aquifers reach the same radius but weaker everywhere — no separate
   radius param needed.)
3. Ocean damping: multi-source BFS from ocean cells up to
   `greening_ocean_damp_dist`; scale greening by
   `min(1.0, d_ocean / greening_ocean_damp_dist)`.
4. Temperature gate: scale by linear fade from 0.0 at
   `greening_temp_floor` to 1.0 at `greening_temp_full` (lapse rate
   means this also kills greening at altitude — no separate elevation
   gate).
5. Return as `HeatMap` (so render can `.sample()` bilinearly).

In `render.rs`:

- `composite_pixel_color` gains a `greening: &HeatMap` parameter. In
  both land branches (glacier-edge fallthrough at the `biome_terrain_color`
  call, and the plain-land branch), replace
  `let precip_t = precipitation.sample(nx, ny);` with:
  ```rust
  let precip_t = (precipitation.sample(nx, ny)
      + greening.sample(nx, ny) * params.greening_strength).min(1.0);
  ```
  (Undithered, matching how temp_t/precip_t are already sampled raw.)
- `save_composite` and `save_regions` gain the `greening` parameter and
  thread it through.
- New `save_greening(width, height, is_ocean, greening, path)` debug
  render, matching `save_ocean_currents` style: base `[40,40,60]` ocean /
  `[70,65,55]` land, green channel `saturating_add((greening * 255.0) as u8)`
  on land cells. Output `output/greening.png` (covered by the existing
  `/output` gitignore).

In `main.rs`: compute `greening` after regions (needs hydro + temperature
— note temperature itself is unadjusted by lakes, only precip/swing are);
add `save_greening` call; pass `greening` into
`save_composite`/`save_regions`; bump the final println to
`"Done — 12 layers written to output/"`.

Commit: `feat: freshwater greening from rivers, lakes, and aquifers`

## Task 5: Verification (no commit)

1. `cargo build` clean at each task boundary (transient dead-code
   warnings only); `cargo run --release` after Tasks 1, 3, 4.
2. **Jitter fix:** compare `output/temperature.png` /
   `output/precipitation.png` band edges across the equator at the same
   longitude — meanders should no longer mirror. Bands must still read
   as coherent meanders, not blobs (else reduce `JITTER_LAT_SCALE`).
3. **Lake effect:** locate the seed's large lakes in `output/hydrology.png`;
   confirm `precipitation.png` shows a halo bump, `diurnal_swing.png` a
   damped halo, and shore regions' tooltips in
   `output/composite.markers.json` read milder/wetter where a large
   lake dominates a region.
4. **Greening:** `output/greening.png` shows green corridors along
   rivers/lake shores, scattered weaker oasis patches in endorheic
   basins (aquifers), fading near coasts and absent in cold/high
   terrain. Composite shows green river corridors through dry biomes
   (crop and compare against pre-change composite).
5. **Determinism:** run twice, `md5sum` on composite/precipitation/
   greening/diurnal_swing PNGs + `composite.markers.json` — identical.
6. Copy this plan to
   `docs/superpowers/plans/2026-07-29-greening-lake-effect-jitter.md`
   and commit (project convention), then push all commits to
   `origin main`.

## Non-goals (deferred)

- Coastline character (slope/climate-based beach rendering) — agreed
  follow-up, next feature.
- Aquifer effects on anything beyond rendering; equatorial currents;
  seasonal variation; any change to hydrology generation itself.
