# mapgen: region detection + Obsidian marker placement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

## Context

`mapgen` (`/root/tools/mapgen/`) currently generates elevation/climate/
hydrology layers, a composite terrain render, and an Obsidian `zoom-map`
markers.json scaffold with an empty `markers: []`. This plan implements
the approved design spec
`docs/superpowers/specs/2026-07-28-region-detection-and-markers-design.md`:
port `demiurge-rust`'s region-detection BFS (`src/planet_gen.rs:1411-1804`,
verified against source), add a guaranteed-inside-region label point per
region (circular-mean centroid + nearest-cell snap), render a debug PNG,
and populate `composite.markers.json` with one pin per land region. This
is explicitly the point-marker prerequisite to a later polygon-boundary
feature — not that feature itself.

No dependency on demiurge-rust; source line numbers below are references
for verified-against-source porting only, mapgen stays standalone.

This project has no automated test suite by design — verification is
`cargo build`/`cargo run` plus manual/visual inspection, consistent with
every prior mapgen plan.

## Task 1: Add region-detection params to `PlanetGenParams`

**File:** `src/params.rs`

Add 6 fields after `radius_km`:
```rust
    pub land_threshold: f64,
    pub ocean_threshold: f64,
    pub region_min_size: usize,
    pub island_coast_dist: usize,
    pub island_arch_dist: usize,
    pub lon_weight: f64,
```
Set in `earth_like()` after `radius_km: 6371.0,` (exact values ported from
`demiurge-rust`'s `earth_like()`, `planet_gen.rs:103-108`):
```rust
            land_threshold: 0.15,
            ocean_threshold: 0.59,
            region_min_size: 150,
            island_coast_dist: 3,
            island_arch_dist: 25,
            lon_weight: 0.82,
```
Verify: `cargo build` clean (new dead-code warnings for the 6 fields are
expected until Task 2 consumes them — same pattern as the `radius_km`
task in the prior plan).

Commit: `feat: add region-detection thresholds to PlanetGenParams`

## Task 2: Implement `src/regions.rs`

**File:** create `src/regions.rs`, add `mod regions;` to `main.rs`
(alphabetical, after `mod params;`... actually after `mod hydrology;`,
before `mod params;` — alphabetical order: climate, elevation, export,
heatmap, hydrology, params, regions, render).

Port from `demiurge-rust/src/planet_gen.rs`, verified against current
source at these exact line ranges:

- `CellKind` enum (1504): `#[derive(Clone, Copy, PartialEq, Debug)]`,
  `Frozen, Ocean, Land`.
- `cell_kind()` (1506-1510): private fn, verbatim, using mapgen's
  `is_ocean`/`is_glacier`/`is_sea_ice` slices (same shape as
  demiurge-rust's).
- `neighbors_4()` (1411-1418): private fn, verbatim (x wraps mod width,
  y clamps at 0/height-1, no pole-crossing wrap — distinct from
  `heatmap::neighbors_8`).
- `Region` struct (1421-1434), adapted:
  ```rust
  pub struct Region {
      pub id: u32,
      pub kind: CellKind,
      pub size: usize,
      pub mean_elev: f64,
      pub mean_temp: f64,
      pub mean_swing: f64,
      pub mean_precip: f64,
      pub mean_aridity: f64,
      pub ocean_frac: f64,
      pub glacier_frac: f64,
      pub sea_ice_frac: f64,
      pub island_components: usize,
      pub label_pos: (usize, usize),
  }
  ```
  (`salt_flat_frac` dropped; `kind` and `label_pos` are new — not in the
  reference struct.)
- `impl Region`: port `temp_zone()` (1447-1453) and `climate_character()`
  (1455-1487) verbatim. Port `character()` (1489-1499) with the
  `salt_flat_frac > 0.5` branch removed — falls through directly to
  `climate_character()` + island suffix. **Do not port**
  `mean_temp_day()`/`mean_temp_night()` (1438-1445) — they depend on
  `effective_temp`/`ActivityPattern` machinery mapgen has no equivalent
  of and nothing in this plan calls them.
- `detect_regions()` (1528-1804), signature drops `is_salt_flat: &[bool]`:
  ```rust
  pub fn detect_regions(
      elevation: &HeatMap, temperature: &HeatMap, diurnal_swing: &HeatMap,
      precipitation: &HeatMap, aridity: &HeatMap,
      is_ocean: &[bool], is_glacier: &[bool], is_sea_ice: &[bool],
      land_threshold: f64, ocean_threshold: f64, min_size: usize,
      coast_dist: usize, arch_dist: usize, lon_weight: f64,
  ) -> (Vec<u32>, Vec<Region>)
  ```
  Phases 1, 2, 2.5 (lines 1553-1769: BFS classification, small-region
  absorption, island/archipelago merge) port verbatim — same queue-based
  BFS, same `HashMap`-based neighbor-vote absorption, same two-step
  coastal-absorb-then-archipelago-group island logic. Drop all
  `is_salt_flat`/`salt_flat_frac` references in Phase 3's `Region`
  construction (lines 1781-1800).

  **New in Phase 3** (while each region's `cells: Vec<usize>` is still in
  scope, before the final `Region` struct is built): compute the label
  point.
  ```rust
  // Circular-mean centroid (x is circular to handle antimeridian-spanning
  // regions; y is a plain mean) — ported from
  // demiurge-rust/examples/heatmap_export.rs:377-388,427-431.
  let mut sin_sum = 0.0;
  let mut cos_sum = 0.0;
  let mut y_sum = 0u64;
  for &idx in cells {
      let x = idx % width;
      let y = idx / width;
      let angle = std::f64::consts::TAU * x as f64 / width as f64;
      sin_sum += angle.sin();
      cos_sum += angle.cos();
      y_sum += y as u64;
  }
  let n = cells.len() as f64;
  let mean_angle = (sin_sum / n).atan2(cos_sum / n);
  let cx = (mean_angle / std::f64::consts::TAU * width as f64).rem_euclid(width as f64);
  let cy = y_sum as f64 / cells.len() as f64;

  // Snap to the nearest cell actually in this region (guarantees the
  // label point lands inside the region, unlike a raw centroid).
  let label_pos = cells.iter().copied().min_by(|&a, &b| {
      let dist = |idx: usize| -> f64 {
          let x = (idx % width) as f64;
          let y = (idx / width) as f64;
          let raw_dx = (x - cx).abs();
          let dx = raw_dx.min(width as f64 - raw_dx);
          let dy = y - cy;
          dx * dx + dy * dy
      };
      dist(a).partial_cmp(&dist(b)).unwrap()
  }).map(|idx| (idx % width, idx / width)).unwrap();
  ```
  Set `kind: region_kind[old_id]` (already tracked internally in the
  ported Phase 1/2.5 logic) on each `Region`.

Verify: `cargo build` clean (the 6 params fields from Task 1 are now
consumed — their dead-code warnings disappear; `regions.rs` itself isn't
called from `main` yet, so `pub` items in it will show dead-code warnings
until Task 3 wires it in — expected, same pattern as prior plans).

Commit: `feat: port region detection with label-point placement`

## Task 3: Wire `detect_regions` into `main.rs`

**File:** `src/main.rs`

After the `diurnal_swing` computation (region detection doesn't depend on
hydrology — its inputs are elevation/temperature/diurnal_swing/
precipitation/aridity/is_ocean/is_glacier/is_sea_ice, all already
available at that point):
```rust
    let (region_map, regions) = regions::detect_regions(
        &elev, &temperature, &diurnal_swing, &precipitation, &aridity,
        &is_ocean, &is_glacier, &is_sea_ice,
        params.land_threshold, params.ocean_threshold, params.region_min_size,
        params.island_coast_dist, params.island_arch_dist, params.lon_weight,
    );
```
Placed before the `hydrology::generate_hydrology` call (or after — order
doesn't matter functionally since neither depends on the other; keep
region detection right after `diurnal_swing` since that's its last input,
matching the plan's dependency-ordering convention from prior work).

Verify: `cargo build` clean (no more dead-code warnings on `regions.rs`
public items — `region_map`/`regions` are unused in `main` until Tasks 4-5
consume them, so `#[allow(unused_variables)]` is NOT needed — instead
expect two new `unused variable` warnings for `region_map`/`regions`
themselves until later tasks; this is fine/expected, same
incremental-warning pattern as every prior task in this project).

Commit: `feat: call region detection in main pipeline`

## Task 4: Debug render `render::save_regions`

**File:** `src/render.rs`, modify `src/main.rs`

New function, reusing `save_composite`'s exact terrain-rendering logic as
a base (do not duplicate — factor the shared per-pixel color logic if
needed, or call through; implementer's judgment on cleanest reuse given
`save_composite`'s current structure — see that function, ~line 292-380s):
```rust
pub fn save_regions(
    width: usize, height: usize,
    hydro_map: &HeatMap, elevation: &HeatMap, temperature: &HeatMap,
    is_ocean: &[bool], is_glacier: &[bool], is_sea_ice: &[bool],
    params: &PlanetGenParams,
    region_map: &[u32], regions: &[crate::regions::Region],
    path: &str,
) -> Result<(), image::ImageError>
```
Algorithm:
1. Render the same base terrain color per supersampled pixel as
   `save_composite` (water/glacier/terrain branches, dithering, contours
   — identical logic).
2. Boundary overlay: for each data cell, check its 4 neighbors (wrap x,
   clamp y — same pattern as `neighbors_4` in `regions.rs`, or reuse it
   since `render.rs` can depend on `regions::neighbors_4` if made
   `pub(crate)`); if any neighbor has a different `region_map` value,
   darken that supersampled pixel's block to `[220, 30, 30]` (ported from
   `demiurge-rust/examples/heatmap_export.rs:359-364`).
3. Label-point markers: for each region where `kind == CellKind::Land`,
   draw a small filled circle (~4px radius in render-space) at
   `label_pos * RENDER_SCALE` — white fill, 1px black outline. No text/font
   rendering needed.

Wire into `main.rs` after the existing `render::save_composite` call:
```rust
    render::save_regions(
        WIDTH, HEIGHT, &hydro.map, &elev, &temperature, &is_ocean, &is_glacier, &is_sea_ice,
        &params, &region_map, &regions, "output/regions.png",
    )?;
```

Verify: `cargo build` clean, `cargo run --release`, visually inspect
`output/regions.png` (Read tool on the PNG): region boundaries should
roughly track visible terrain/climate transitions in the composite
underneath; land region label dots should sit visibly on land, inside
their own region.

Commit: `feat: add region boundary + label-point debug render`

## Task 5: Populate markers in `export.rs`

**File:** `src/export.rs`, modify `src/main.rs`

Change `MarkersFile.markers` field type from `Vec<()>` to `Vec<Marker>`;
add the struct:
```rust
#[derive(Serialize)]
struct Marker {
    id: String,
    x: f64,
    y: f64,
    layer: &'static str,
    link: &'static str,
    #[serde(rename = "iconKey")]
    icon_key: &'static str,
    tooltip: String,
}
```
`save_markers_json` signature gains `regions: &[crate::regions::Region]`,
`grid_width: usize`, `grid_height: usize`:
```rust
pub fn save_markers_json(
    render_width: usize, render_height: usize,
    params: &PlanetGenParams, image_filename: &str,
    regions: &[crate::regions::Region], grid_width: usize, grid_height: usize,
    path: &str,
) -> std::io::Result<()>
```
Build `markers` by filtering to `kind == CellKind::Land`:
```rust
let markers: Vec<Marker> = regions.iter()
    .filter(|r| r.kind == crate::regions::CellKind::Land)
    .map(|r| Marker {
        id: format!("region_{}", r.id),
        x: (r.label_pos.0 as f64 + 0.5) / grid_width as f64,
        y: (r.label_pos.1 as f64 + 0.5) / grid_height as f64,
        layer: "default",
        link: "",
        icon_key: "pinRed",
        tooltip: r.character(),
    })
    .collect();
```
Update the `main.rs` call site to pass `&regions, WIDTH, HEIGHT`.

Verify: `cargo build` clean, `cargo run --release`, inspect
`output/composite.markers.json`: `markers` array non-empty, reasonable
count (dozens, not thousands, given `region_min_size=150` on a 1024×512
grid), each entry has `x`/`y` in `[0,1]` and a plausible biome `tooltip`
(e.g. "Temperate Forest", "Tropical Rainforest Island"). Re-run and
diff/hash `composite.markers.json` against the prior run to confirm
byte-identical output (deterministic IDs + same seed).

Commit: `feat: export region markers in composite.markers.json`

## Task 6: Manual end-to-end Obsidian check

No code changes — same pattern as the markers-export plan's Task 5.

1. Copy `output/composite.png` and `output/composite.markers.json` into
   `/root/ttrpg/Pipeline Plans/Testing/` (overwriting the prior
   `mapgen-composite.*` files from the last feature).
2. User opens the existing `zoommap` block pointing at those files in
   Obsidian, confirms pins appear at plausible locations across the map
   (on land, not floating in ocean), with sensible biome tooltips.
3. Report back — no commit for this task, it's a verification checkpoint.

## Verification summary

- `cargo build` clean at each task boundary (dead-code warnings only for
  not-yet-consumed items, resolving as later tasks wire them in — same
  incremental pattern as every prior plan in this project).
- `cargo run --release` produces `output/regions.png` and a populated
  `output/composite.markers.json`.
- Visual inspection of `regions.png` (boundary sanity, label dots on
  land) and `composite.markers.json` (marker count/positions/tooltips).
- Determinism check: re-run, confirm identical output.
- Manual Obsidian check (Task 6).
