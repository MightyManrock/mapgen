# mapgen: region polygons for the Obsidian draw layer — Implementation Plan

## Context

Region markers already export to the zoom-map sidecar; this feature adds
the regions themselves as polygon drawings so Obsidian displays region
boundaries as a toggleable draw layer. Schema confirmed by mining the
plugin's `main.js` and a plugin-written sidecar
(`/root/ttrpg/Pipeline Plans/Testing/test_map.markers.json`):

- `drawLayers`: `[{id, name, visible, locked}]`
- `drawings`: `[{id, layerId, kind: "polygon", visible, polygon: [{x, y}, ...],
  style: {strokeColor, strokeWidth, fillColor, fillOpacity}}]`
- coordinates normalized [0,1], same space as markers.

User-approved choices: color by climate character (reusing the biome
corner-color interpolation so polygons read as a biome overlay matching
the map palette); land regions only (matching markers); one "Regions"
draw layer. Antimeridian-spanning regions split at the seam (the
plugin's coordinate space is flat — no wrap). Holes are dropped;
enclosed regions draw their own polygon on top (fill is 0.15 opacity,
plugin has no hole support).

Our `export.rs` `MarkersFile` already writes `draw_layers: Vec<()>` and
`drawings: Vec<()>` placeholders — they just need real types.

Process: implement directly, no subagent dispatch (user usage limits).
No automated tests by design; verify via debug render + determinism
md5s + user's Obsidian import.

## Task 1: Boundary tracing + simplification (`src/polygons.rs`, `src/params.rs`)

New param (+ `earth_like()`): `polygon_simplify_epsilon: f64` = 1.2 —
Douglas-Peucker tolerance in cell units.

New module `src/polygons.rs`:

```rust
pub struct RegionPolygons {
    pub region_id: u32,
    /// Closed outer loops (one per connected component), vertices in
    /// cell-corner coordinates (x in 0..=width, y in 0..=height).
    pub loops: Vec<Vec<(f64, f64)>>,
}

pub fn extract_region_polygons(
    region_map: &[u32],
    regions: &[crate::regions::Region],
    width: usize,
    height: usize,
    epsilon: f64,
) -> Vec<RegionPolygons>
```

Per land region (`kind == CellKind::Land`), in region order (already
sorted by id — deterministic):

1. **Components**: connected components of the region's cells,
   4-connected, **no x-wrap** — the seam acts as a hard edge so
   antimeridian-spanning regions split into separate components
   automatically. Seed scan in index order (deterministic).
2. **Boundary edges**: for each component cell, each of its 4 sides
   whose neighbor is outside the component (out-of-grid counts as
   outside) emits a directed edge between cell corners, oriented with
   the interior on the left. Corner space: `(cx, cy)`, `cx ∈ 0..=width`,
   `cy ∈ 0..=height`.
3. **Loop walking**: link directed edges into closed loops via a map
   from start-corner to outgoing edges (`BTreeMap` — deterministic). At
   junction corners (diagonally-touching cells produce two outgoing
   edges from one corner), pick the edge turning most counterclockwise
   relative to the incoming direction — keeps loops simple
   (non-self-crossing). Keep only outer loops, identified by signed
   (shoelace) orientation — with interior-on-left emission, holes have
   the opposite winding; drop them.
4. **Simplification**, two stages per loop:
   a. merge collinear runs (consecutive segments with the same
      direction) — grid boundaries are mostly straight runs;
   b. Douglas-Peucker with `epsilon`, applied to the closed loop by
      splitting it at its two mutually-farthest vertices and
      simplifying each half (keeps closure stable). Loops that
      degenerate below 3 vertices are dropped.

## Task 2: Export + shared biome color (`src/render.rs`, `src/export.rs`, `src/main.rs`)

- In `render.rs`: extract the corner-lerp portion of
  `biome_terrain_color` (the `COLD_DRY`/`COLD_WET`/`HOT_DRY`/`HOT_WET`
  consts and the two-axis `lerp_color` reduction producing `base`) into
  `pub(crate) fn biome_base_color(temp_t: f64, precip_t: f64) -> [u8; 3]`;
  `biome_terrain_color` calls it. Byte-identical composite output —
  verify by md5 against the previous run.
- In `export.rs`:
  - New serialize structs: `DrawLayer {id, name, visible, locked}`,
    `DrawingStyle {stroke_color, stroke_width, fill_color, fill_opacity}`
    (serde-renamed to strokeColor etc.), `Point {x, y}`,
    `Drawing {id, layer_id, kind, visible, polygon: Vec<Point>, style}`.
  - `MarkersFile.draw_layers` becomes `Vec<DrawLayer>`, `.drawings`
    becomes `Vec<Drawing>`.
  - `save_markers_json` gains a `polygons: &[crate::polygons::RegionPolygons]`
    parameter. One draw layer `{id: "draw_regions", name: "Regions",
    visible: true, locked: false}`. Per `RegionPolygons`, look up the
    region (by id) for `mean_temp`/`mean_precip`, color =
    `render::biome_base_color(mean_temp, mean_precip)` formatted as
    `#rrggbb`; per loop, one drawing with id
    `format!("draw_region_{}_{}", region_id, loop_index)`, kind
    "polygon", vertices normalized (`cx / grid_width`,
    `cy / grid_height`), style `{stroke, 2, same fill, 0.15}`.
- `main.rs`: `mod polygons;`; after region detection compute
  `let region_polys = polygons::extract_region_polygons(&region_map, &regions, WIDTH, HEIGHT, params.polygon_simplify_epsilon);`
  and pass `&region_polys` to `save_markers_json`.

## Task 3: Debug render (`src/render.rs`, `src/main.rs`)

- `save_polygons(width, height, is_ocean, polygons, regions, path)` in
  the established debug style: ocean `[40, 40, 60]` / land `[70, 65, 55]`
  silhouette at data resolution; each polygon's edges rasterized with a
  small Bresenham line helper in the region's `biome_base_color`
  (vertices are cell-corner coords — draw at 1:1). Output
  `output/polygons.png`; println bumps to `"Done — 14 layers written to output/"`.

## Task 4: Verification

1. `cargo build` clean at task boundaries; `cargo run --release`.
2. Composite md5 unchanged after the `biome_base_color` extraction
   (pure refactor).
3. `output/polygons.png`: outlines hug the same boundaries as
   `regions.png`'s red borders (visual compare), colors vary
   tan→green→grey with climate, no wild self-crossing artifacts, seam
   regions split cleanly at x=0.
4. Report polygon/vertex counts (drawings count, min/mean/max vertices)
   — sanity: hundreds of vertices max per loop, not thousands.
5. Determinism: two runs, md5 on `composite.markers.json` +
   `polygons.png` — identical.
6. JSON sanity: `python3 -c` load, assert every drawing has ≥3 points,
   all coords in [0,1], `layerId == "draw_regions"`.
7. Copy plan to `docs/superpowers/plans/2026-07-29-region-polygons.md`,
   commit, push. User then copies `composite.png` +
   `composite.markers.json` into the vault to confirm rendering in
   Obsidian (same manual step as the marker feature).

Commits: Task 1 `feat: trace region boundaries into simplified polygons`;
Task 2 `feat: export region polygons as zoom-map draw layer`;
Task 3 `feat: region polygon debug render`; docs commit at the end.

## Non-goals

- Ocean/frozen region polygons; polygon holes; multi-loop polygons as a
  single drawing (plugin draws one loop per drawing).
- Any change to region detection, markers, or the composite render
  beyond the color-helper extraction.
- Label text placement (markers already carry tooltips at label_pos).
