# mapgen: region detection + Obsidian marker placement

## Context

mapgen currently generates elevation/climate/hydrology layers and a
composite terrain render, plus an Obsidian `zoom-map` markers.json scaffold
(`docs/superpowers/specs/2026-07-28-obsidian-markers-export-design.md`) with
an empty `markers: []`. This spec adds region detection — segmenting the
map into geographic/climate regions — and places one marker per land region
at a representative "label point," so the exported markers.json is
populated with real pins instead of an empty array.

This is explicitly a first step toward polygon region boundaries (drawn as
`zoom-map` `drawings` overlays), not that feature itself. Polygon tracing
is deferred; this spec only gets a correctly-placed point per region.

`demiurge-rust`'s `src/planet_gen.rs` (`detect_regions`, lines 1411-1804)
and `examples/heatmap_export.rs` (centroid/label logic, lines 373-437) are
the reference implementation, verified directly against source. As with
all prior mapgen work, this is a fresh, standalone port — no dependency on
demiurge-rust.

## Goal

After the elevation/climate pipeline runs, segment the map into regions,
compute a real-world scale and a guaranteed-inside-region label point for
each land region, render a debug PNG for visual verification, and export
one marker per land region in `output/composite.markers.json`.

## Architecture

### New module `src/regions.rs`

Ported from `planet_gen.rs`, with two deliberate deviations from the
reference (both are extensions, not simplifications — see below):

**`CellKind`** (`#[derive(Clone, Copy, PartialEq, Debug)]`, ported from
line 1504): `Frozen | Ocean | Land`.

**`cell_kind(idx, is_ocean, is_glacier, is_sea_ice) -> CellKind`** (private,
ported from lines 1506-1510).

**`neighbors_4(x, y, width, height) -> Vec<(usize, usize)>`** (private,
ported verbatim from lines 1411-1418 — x wraps, y clamps at poles, no
pole-crossing wrap; distinct from `heatmap::neighbors_8`, which does need
pole-wrap logic for its own 8-neighbor use case).

**`Region`** struct, ported from lines 1421-1434 with two changes:
`salt_flat_frac` dropped (no salt-flat generator, matching the composite
spec's precedent), and two new fields added:
```rust
pub struct Region {
    pub id: u32,
    pub kind: CellKind,              // NEW — which BFS pool this region belongs to
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
    pub label_pos: (usize, usize),   // NEW — snapped label-point cell coords
}
```
`kind` is cheap to add (Phase 1 already tracks each region's kind
internally; the original just didn't return it) and is needed to filter to
land-only markers without re-deriving it from `ocean_frac == 0.0`-style
heuristics. `label_pos` is this spec's actual payload — see below.

Methods on `Region` (ported from lines 1436-1499, `character()`'s
salt-flat branch removed):
- `mean_temp_day()` / `mean_temp_night()` (1438-1445) — **not portable
  as-is**: they call `effective_temp(..., &ActivityPattern::Diurnal/Nocturnal)`,
  machinery mapgen has no equivalent of (species activity patterns are out
  of scope per mapgen's README). **Drop these two methods** — nothing in
  this spec's scope needs them, and `climate_character()`/`character()`
  don't depend on them.
- `temp_zone()` (1447-1453) — verbatim.
- `climate_character()` (1455-1487) — verbatim.
- `character()` (1489-1499) — verbatim minus the `salt_flat_frac > 0.5`
  branch (falls through directly to `climate_character()` + island suffix).

**`detect_regions(...)`**, ported from lines 1528-1804, signature adjusted
(drop `is_salt_flat: &[bool]`; everything else identical):
```rust
pub fn detect_regions(
    elevation: &HeatMap, temperature: &HeatMap, diurnal_swing: &HeatMap,
    precipitation: &HeatMap, aridity: &HeatMap,
    is_ocean: &[bool], is_glacier: &[bool], is_sea_ice: &[bool],
    land_threshold: f64, ocean_threshold: f64, min_size: usize,
    coast_dist: usize, arch_dist: usize, lon_weight: f64,
) -> (Vec<u32>, Vec<Region>)
```
Phases 1 (BFS classification), 2 (small-region absorption), and 2.5
(island/archipelago merging) port verbatim (lines 1553-1769). Phase 3
(lines 1771-1804, building final `Region` structs) gains the label-point
computation described next, plus setting `kind` from the already-tracked
`region_kind[old_id]`.

### Label-point computation (inside Phase 3, per region)

For each non-empty region, while its `cells: Vec<usize>` list is still in
scope (before Phase 3 discards it):

1. **Circular-mean centroid** (ported from `heatmap_export.rs:377-388,
   427-431`): x uses a circular mean (`atan2` of averaged `sin`/`cos` of
   each cell's longitude angle `TAU * x / width`) so regions spanning the
   antimeridian get a correct center instead of averaging toward the
   map's middle; y is a plain arithmetic mean.
2. **Snap to nearest actual region cell** (new — this spec's addition,
   the "guaranteed inside the region" upgrade): scan the region's own
   `cells` list, find the cell minimizing wrap-aware Euclidean distance
   to the centroid (`dx = min(|x1-x2|, width-|x1-x2|)`, plain `dy`).
   `O(region size)`, and total cost across all regions is `O(n)` since
   regions partition the grid. Store as `label_pos: (usize, usize)`.

This guarantees the label point is a real cell belonging to the region,
unlike the old code's raw centroid (fine for a numeric ID label with no
positional stakes; not fine for an Obsidian pin that needs to visibly sit
on the region it names).

### `PlanetGenParams` additions

New fields, values ported verbatim from `demiurge-rust`'s `earth_like()`
(`planet_gen.rs:103-108` / `253-258`, identical in both):
```rust
pub land_threshold: f64,
pub ocean_threshold: f64,
pub region_min_size: usize,
pub island_coast_dist: usize,
pub island_arch_dist: usize,
pub lon_weight: f64,
```
`earth_like()`: `land_threshold: 0.15, ocean_threshold: 0.59,
region_min_size: 150, island_coast_dist: 3, island_arch_dist: 25,
lon_weight: 0.82`.

### Debug render: `render::save_regions(...)`

New function in `render.rs`, simplified from the old `regions.png` +
`political.png` (skipping colored per-region fill and text-drawing — not
needed to validate BFS segmentation + label placement, the actual things
this spec needs verified):
```rust
pub fn save_regions(
    width: usize, height: usize,
    hydro_map: &HeatMap, elevation: &HeatMap, temperature: &HeatMap,
    is_ocean: &[bool], is_glacier: &[bool], is_sea_ice: &[bool],
    params: &PlanetGenParams,
    region_map: &[u32], regions: &[Region],
    path: &str,
) -> Result<(), image::ImageError>
```
Reuses `save_composite`'s exact terrain rendering as a base (same
supersampling/dithering/water/glacier logic — region overlay is drawn on
top, not a replacement rendering path), then:
- Darkens any supersampled pixel whose underlying data cell has a
  4-neighbor (`is_boundary` check, ported from
  `heatmap_export.rs:359-364`) with a different `region_map` value, to
  `[220, 30, 30]` (matches the old `regions.png`'s red boundary color).
- For each land region, draws a small filled circle (~4px radius at
  render scale) at `label_pos * RENDER_SCALE` in solid white with a black
  1px outline (simple, no font rendering needed — unlike the old
  `political.png`'s numeric-ID text labels, which required a bitmap font
  helper (`draw_text`) this spec doesn't need to port).

Output: `output/regions.png`, gitignored under the existing `/output`
entry.

### `export.rs` changes

`MarkersFile.markers` field type changes from `Vec<()>` to `Vec<Marker>`:
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
This mirrors the plugin's own minimal marker-creation shape (verified
against the plugin's `main.js` — a user-placed pin is created as exactly
`{id, x, y, layer, link, iconKey, tooltip}`, `type` defaults to `"pin"`
when absent, so it's omitted here same as the plugin's own draft object).
`icon_key: "pinRed"` — the plugin's own fallback default when no icon is
configured, verified against `main.js` (`settings.defaultIconKey || ...
"pinRed"`).

`save_markers_json`'s signature gains two parameters:
```rust
pub fn save_markers_json(
    render_width: usize, render_height: usize,
    params: &PlanetGenParams, image_filename: &str,
    regions: &[Region], grid_width: usize, grid_height: usize,
    path: &str,
) -> std::io::Result<()>
```
`grid_width`/`grid_height` are the base simulation grid (1024×512) —
distinct from `render_width`/`render_height` (3072×1536), needed because
`Region::label_pos` is in base-grid cell coordinates, not render-scaled
pixels. Builds one `Marker` per region where `kind == CellKind::Land`:
`id = format!("region_{}", region.id)` (deterministic — not the plugin's
random IDs, so re-running mapgen with the same seed produces byte-identical
output, matching this project's existing reproducibility guarantee),
`x = (label_pos.0 as f64 + 0.5) / grid_width as f64`,
`y = (label_pos.1 as f64 + 0.5) / grid_height as f64` (cell-center,
normalized [0,1]), `tooltip = region.character()`.

### `main.rs` wiring

`mod regions;` added alphabetically. `regions::detect_regions(...)` called
after `diurnal_swing` is computed (its actual inputs — elevation,
temperature, diurnal_swing, precipitation, aridity, is_ocean, is_glacier,
is_sea_ice — are all available at that point; hydrology is not an input to
region detection at all, so this doesn't need to wait for the hydrology
call). `render::save_regions(...)` added to the render sequence.
`export::save_markers_json(...)`'s call site gains `&regions, WIDTH,
HEIGHT` arguments.

## Non-goals

- Polygon region boundaries / `drawings` overlay export — this spec is
  the point-marker prerequisite step only, as you specified.
- `political.png`-style colored region fill + numeric text labels — not
  needed to validate this spec's logic; the red-boundary + label-dot debug
  render is sufficient.
- Ocean/frozen region markers — detected and classified (required for the
  BFS itself), never exported as pins.
- Species/habitability scoring (`score_region_for_species`,
  `political_color`, `mean_temp_day`/`mean_temp_night`) — out of scope,
  consistent with mapgen's existing Earth-like/human-only, no-per-species
  stance.
- Salt flats — no generator exists, consistent with prior work.
- Any change to existing per-layer PNGs or the composite render itself —
  `save_regions` reuses `save_composite`'s rendering logic but is an
  additional output, not a modification.

## Testing

Visual/manual, consistent with the rest of the prototype:
1. `cargo run` produces `output/regions.png` alongside existing outputs,
   and `output/composite.markers.json`'s `markers` array is non-empty.
2. Inspect `regions.png` by eye: region boundaries (red lines) should
   roughly track visible climate/terrain transitions in the composite
   underneath — coastlines, major mountain ranges, obvious biome shifts.
   No visibly nonsensical fragmentation (a sea of tiny 1-cell regions
   would mean the similarity thresholds or min-size absorption aren't
   behaving as ported).
3. Each land region's label dot should sit visibly on land, inside its
   own region's boundary — not floating in the ocean or across a boundary
   line into a neighboring region. This is the direct visual check for
   the centroid-snap logic.
4. Inspect `composite.markers.json`'s `markers` array by hand: reasonable
   count (not thousands — `region_min_size=150` on a 1024×512 grid
   should yield a modest number of regions), plausible `tooltip` strings
   (biome names), `x`/`y` in `[0,1]`.
5. Re-run `cargo run` and confirm `composite.markers.json` is
   byte-identical (same seed ⇒ same regions ⇒ same deterministic marker
   IDs) — consistent with the existing determinism check.
6. Repeat the manual Obsidian check from the markers-export feature: copy
   the new composite.png/composite.markers.json into the vault, confirm
   pins appear in plausible locations across the map.
