# mapgen: simple ocean current modeling

## Context

mapgen's climate model currently has no ocean circulation at all — sea
surface temperature comes purely from latitude + elevation (via
`generate_temperature`'s lapse-rate term), identically for every ocean
cell regardless of position relative to nearby coastlines. Real ocean
currents introduce a significant, visually distinctive asymmetry: warm
poleward-flowing western-boundary currents (Gulf Stream, Kuroshio) run
along the *east* coasts of continents, while cool equatorward-flowing
eastern-boundary currents (California, Canary, Humboldt) run along their
*west* coasts, with associated coastal upwelling. This pattern holds in
both hemispheres — the hemisphere only flips the gyre's circulation
direction, not which coast is warm vs. cool — so no hemisphere-dependent
sign flip is needed.

This spec adds a simplified proxy for that effect: a coastline-relative
temperature bias, not a fluid-dynamics simulation. Raised in discussion as
a "what if" alongside the band-jitter fix; the user asked to explore it
before freshwater greening since it may meaningfully reshape the climate
profile (and therefore biome coloring, region detection, etc.) that
freshwater greening would otherwise be tuned against.

## Goal

After base temperature generation, apply a coastline-relative bias: ocean
cells near a coastline get warmed or cooled depending on which side of
the coastline they're on, tapered by a latitude envelope (strongest in
the subtropics, ~25–35°, weak at the equator and poles) and by distance
from the coastline. Nearby land cells inherit a smaller, shorter-range
version of the adjacent ocean's bias (coastal climate moderation).
Applied before sea ice / precipitation generation so downstream fields
respond to the biased temperature correctly.

## Architecture

### New function `apply_ocean_currents` (`src/climate.rs`)

```rust
pub fn apply_ocean_currents(temperature: &HeatMap, is_ocean: &[bool], params: &PlanetGenParams) -> HeatMap
```

Takes the base temperature field (already latitude + elevation +
band-jitter adjusted) and returns a new field with the current bias
added — same pattern as `generate_aridity` (existing field(s) in, new
derived field out), not a mutation.

**Phase 1 — per-row bounded coastline search**, for every cell (land and
ocean alike, single pass per row):

For cell `(x, y)`, scan outward from `x` in both directions (wrapping at
the antimeridian via `% width`, same pattern as `neighbors_4`/existing
row sweeps) up to `params.current_search_dist` steps, looking for the
nearest cell of the *opposite* kind (`is_ocean[idx]` flipped relative to
the current cell):

- If the current cell is **ocean**: find nearest **land** to the west
  (`dist_w`) and east (`dist_e`), each capped at
  `current_search_dist` (treated as "not found" beyond that).
  - If land is closer to the west (`dist_w < dist_e`): this ocean cell
    sits on that landmass's *east* side → **warm** bias.
  - If land is closer to the east (`dist_e < dist_w`): this ocean cell
    sits on that landmass's *west* side → **cool** bias.
  - Ties, or no land found within the search bound in either direction:
    zero bias (open ocean, far from any coast).
  - Magnitude falls off linearly from full strength at `dist = 1` to `0`
    at `dist = current_search_dist`, using whichever direction won.

- If the current cell is **land**: find the nearest **ocean** cell within
  `params.current_bleed_dist` (shorter bound) in either direction. If
  found, inherit a fraction of *that ocean cell's own bias value*
  (computed via the same per-cell logic above, evaluated at the ocean
  cell's position) — same sign, scaled down by linear falloff over the
  shorter bleed distance. This correctly bleeds "whatever the adjacent
  water's temperature signature is" onto the coast, rather than
  re-deriving a sign from the land cell's own perspective.

**Phase 2 — latitude envelope**, reusing the piecewise-stops technique
from `lat_band_factor`:

```rust
fn current_lat_envelope(abs_lat: f64) -> f64 {
    let stops: &[(f64, f64)] = &[
        (0.00, 0.00), // equator: currents here are zonal, not coastal
        (0.15, 0.30),
        (0.35, 1.00), // ~31° — subtropical gyre peak
        (0.55, 0.40),
        (0.75, 0.00),
        (1.00, 0.00), // poles: dominated by different circulation
    ];
    // same piecewise-linear interpolation as lat_band_factor/westerly_weight
}
```

**Phase 3 — combine**: `bias = raw_directional_bias * envelope(abs_lat) * params.current_temp_bias`,
added to the input temperature, clamped to `[0, 1]`.

### New `PlanetGenParams` fields

```rust
pub current_temp_bias: f64,    // max bias magnitude, normalized temp units
pub current_search_dist: usize, // ocean-side falloff bound, in cells
pub current_bleed_dist: usize,  // land-side bleed bound, in cells
```

`earth_like()`: `current_temp_bias: 0.10, current_search_dist: 40,
current_bleed_dist: 8` — first-guess tunable defaults, consistent with
how every other calibrated constant in this codebase started (e.g.
`JITTER_AMPLITUDE`'s empirical-tuning precedent) and may need adjustment
after visual QA.

### `main.rs` wiring

```rust
let base_temperature = climate::generate_temperature(&elev, &params, 0.0, SEED);
let temperature = climate::apply_ocean_currents(&base_temperature, &is_ocean, &params);
let is_sea_ice = climate::generate_sea_ice(&temperature, &is_ocean, params.sea_ice_temp_threshold);
let precipitation = climate::generate_precipitation(&elev, &is_ocean, &temperature, &is_sea_ice, &params, 0.0, SEED);
```

(`is_ocean` is already computed before this point in the existing
pipeline; no reordering of anything else needed.)

### Debug render `render::save_ocean_currents`

New function in `render.rs`, matching the `regions.png` precedent — a
plain-silhouette render (dark grey land / light grey ocean, no elevation
shading) with a red/blue overlay: red where the current bias is warm,
blue where cool, opacity/intensity scaled by bias magnitude. Lets us
visually confirm the west-coast-cool/east-coast-warm pattern directly,
independent of how it reads once blended into the full composite.

Signature:
```rust
pub fn save_ocean_currents(width: usize, height: usize, is_ocean: &[bool], params: &PlanetGenParams, path: &str) -> Result<(), image::ImageError>
```

Recomputes the bias via the same logic as `apply_ocean_currents` (or
factors the per-cell bias computation into a shared private helper both
call — implementer's choice, avoid duplicating the search/envelope math
verbatim in two places). Output: `output/ocean_currents.png`, gitignored
under the existing `/output` entry, added to the `main.rs` render
sequence.

## Non-goals

- Real fluid dynamics (gyre circulation, Coriolis effect, thermohaline
  circulation, seasonal current shifts) — this is a static, simplified
  coastline-relative proxy only.
- Equatorial currents (zonal, not coastal-boundary-driven) — the latitude
  envelope deliberately zeroes out near the equator rather than modeling
  them.
- Interaction with the freshwater-greening follow-up (still deferred,
  separate plan) or the equator-jitter-symmetry issue (still deferred,
  separate plan).
- Any change to `generate_temperature`, `generate_sea_ice`,
  `generate_precipitation`, or `lat_band_factor`/`westerly_weight`
  themselves — this is purely an additional bias layer applied between
  base temperature generation and everything downstream.

## Testing

Visual/manual, consistent with the rest of the project:
1. `cargo run` produces `output/ocean_currents.png` alongside existing
   outputs.
2. Inspect `ocean_currents.png`: red (warm) should appear along
   continents' east coasts, blue (cool) along west coasts, strongest
   around 25–35° latitude, fading out near the equator and poles, with a
   visible (smaller/shorter) bleed of the same color onto adjacent
   coastal land.
3. Inspect `output/temperature.png` and `output/composite.png` before
   and after: coastal temperature (and therefore biome color, since
   `biome_terrain_color` reads temperature) should visibly differ between
   a landmass's east and west coasts at matching latitudes, where it was
   previously symmetric.
4. Determinism check: re-run, confirm identical output (this feature adds
   no new randomness/seed — it's a deterministic function of `is_ocean`
   and latitude, so this should hold trivially, but verify per project
   convention).
5. Sanity-check that downstream fields respond sensibly: sea ice should
   be somewhat suppressed near warm (east) coasts and somewhat expanded
   near cool (west) coasts at matching high latitudes, if any coastline
   at those latitudes exists on this seed's planet.

---

## Amendment (2026-07-31): meridional smoothing

The per-row coastline search specified above has no north-south coupling.
A small island casts a `current_search_dist`-wide bias strip along its own
row and nothing on the row above, and the nearest-coast *direction* can
flip between adjacent rows, reversing the bias sign. Measured on seed 42:
row-to-row differences averaged 2.6x the along-row ones, with 544 vertical
sign flips and a peak jump of 0.195 normalized temperature (~13.7 C) across
a single row boundary — hard horizontal streaks across open ocean.

This was always present but was camouflaged by a separate bug: the lapse
rate read raw elevation, giving the ocean a depth-driven temperature
texture (std dev 2.5 C, range 11 C). Fixing that lapse bug (see
`2026-07-31-target-land-fraction-design.md`) left the ocean smooth and
exposed the streaking.

`current_bias_field` now computes the raw per-cell bias as specified, then
applies a triangular blur (two box passes) down each column, radius
`current_smooth_rows` (default 4, 0 disables). Vertical-only by design: the
east/west asymmetry along a row *is* the feature, so blurring horizontally
would erode the signal rather than the artifact. Two passes rather than one
because a single box blur trades the one-row streak for a weaker step at
its window edge.

`render::save_ocean_currents` consumes the same smoothed field, so the
debug layer cannot disagree with the climate it explains.

Regression tests in `climate.rs`: `current_bias_is_meridionally_coherent`
(row-to-row variation comparable to along-row; measures 36x unsmoothed) and
`current_bias_keeps_east_west_asymmetry` (opposite-signed bias either side
of a coast, i.e. smoothing did not flatten the feature).
