# mapgen: simple ocean current modeling — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

## Context

mapgen's climate model has no ocean circulation — sea temperature comes
purely from latitude + elevation, identical for every ocean cell
regardless of position relative to nearby coastlines. This plan
implements the approved design spec
`docs/superpowers/specs/2026-07-29-ocean-currents-design.md`: a
simplified coastline-relative temperature bias proxying real boundary
currents (warm on continents' east coasts, cool with upwelling on west
coasts, in both hemispheres — no sign flip needed), scaled by a
subtropical-peaked latitude envelope, with a smaller/shorter bleed onto
adjacent coastal land. Not a fluid-dynamics simulation.

This project has no automated test suite by design — verification is
`cargo build`/`cargo run` plus manual/visual inspection (including a new
debug render for this feature, per the spec), consistent with every
prior mapgen plan.

## Task 1: Add ocean-current params to `PlanetGenParams`

**File:** `src/params.rs`

Add 3 fields after `lon_weight`:
```rust
    pub current_temp_bias: f64,
    pub current_search_dist: usize,
    pub current_bleed_dist: usize,
```
Set in `earth_like()` after `lon_weight: 0.82,`:
```rust
            current_temp_bias: 0.10,
            current_search_dist: 40,
            current_bleed_dist: 8,
```
Verify: `cargo build` clean (new dead-code warnings for these 3 fields
expected until Task 2 consumes them — same pattern as every prior
params-only task in this project's history).

Commit: `feat: add ocean current params to PlanetGenParams`

## Task 2: Implement `climate::apply_ocean_currents`

**File:** `src/climate.rs`

Add the following, placed after the existing `generate_temperature`
function (before `generate_precipitation`):

```rust
/// Directional coastline search from an ocean cell along its own row.
/// Returns a signed raw bias in [-1, 1]: positive = nearest coastline is
/// to the west (this ocean cell sits on that landmass's *east* side —
/// warm, western-boundary-current analog), negative = nearest coastline
/// is to the east (cell sits on that landmass's *west* side — cool,
/// eastern-boundary-current analog). Falls off linearly from 1.0 at
/// distance 1 to 0.0 at `search_dist`. Zero if no coastline found within
/// the bound in either direction, or if both directions tie.
fn ocean_cell_bias(x: usize, y: usize, width: usize, is_ocean: &[bool], search_dist: usize) -> f64 {
    let row = y * width;
    let mut dist_w = None;
    for d in 1..=search_dist {
        let nx = (x + width - d) % width;
        if !is_ocean[row + nx] {
            dist_w = Some(d);
            break;
        }
    }
    let mut dist_e = None;
    for d in 1..=search_dist {
        let nx = (x + d) % width;
        if !is_ocean[row + nx] {
            dist_e = Some(d);
            break;
        }
    }
    match (dist_w, dist_e) {
        (Some(dw), Some(de)) => {
            if dw < de {
                1.0 - dw as f64 / search_dist as f64
            } else if de < dw {
                -(1.0 - de as f64 / search_dist as f64)
            } else {
                0.0
            }
        }
        (Some(dw), None) => 1.0 - dw as f64 / search_dist as f64,
        (None, Some(de)) => -(1.0 - de as f64 / search_dist as f64),
        (None, None) => 0.0,
    }
}

/// Per-cell raw current bias (before the latitude envelope), for both
/// ocean and land cells. Ocean cells use `ocean_cell_bias` directly. Land
/// cells inherit a fraction of the *nearest ocean cell's own bias* (same
/// sign, scaled down by linear falloff over `params.current_bleed_dist`)
/// — this bleeds "whatever the adjacent water's temperature signature
/// is" onto the coast, rather than re-deriving a sign from the land
/// cell's own perspective. `pub(crate)` so `render::save_ocean_currents`
/// can reuse it for the debug visualization without duplicating this
/// logic.
pub(crate) fn current_bias_raw(x: usize, y: usize, width: usize, is_ocean: &[bool], params: &PlanetGenParams) -> f64 {
    let idx = y * width + x;
    if is_ocean[idx] {
        return ocean_cell_bias(x, y, width, is_ocean, params.current_search_dist);
    }
    let row = y * width;
    let mut nearest: Option<(usize, usize)> = None;
    for d in 1..=params.current_bleed_dist {
        let wx = (x + width - d) % width;
        if is_ocean[row + wx] {
            nearest = Some((wx, d));
            break;
        }
    }
    for d in 1..=params.current_bleed_dist {
        let ex = (x + d) % width;
        if is_ocean[row + ex] {
            if nearest.is_none_or(|(_, nd)| d < nd) {
                nearest = Some((ex, d));
            }
            break;
        }
    }
    match nearest {
        Some((ox, d)) => {
            let ocean_bias = ocean_cell_bias(ox, y, width, is_ocean, params.current_search_dist);
            let falloff = 1.0 - d as f64 / params.current_bleed_dist as f64;
            ocean_bias * falloff
        }
        None => 0.0,
    }
}

/// Latitude envelope for current strength: ~0 at the equator (currents
/// there are zonal, not coastal-boundary-driven), peaking around 31°
/// (subtropical gyre latitudes), fading to ~0 by the poles (dominated by
/// different circulation). Same piecewise-linear-stops technique as
/// `lat_band_factor`/`westerly_weight` (kept as its own small duplicate,
/// not factored into a shared helper, matching this file's existing
/// precedent of those two functions also duplicating the same
/// interpolation loop rather than sharing one — this task doesn't touch
/// either of them). `pub(crate)` for the same reuse reason as
/// `current_bias_raw`.
pub(crate) fn current_lat_envelope(abs_lat: f64) -> f64 {
    let stops: &[(f64, f64)] = &[
        (0.00, 0.00),
        (0.15, 0.30),
        (0.35, 1.00),
        (0.55, 0.40),
        (0.75, 0.00),
        (1.00, 0.00),
    ];
    for i in 0..stops.len() - 1 {
        let (ta, va) = stops[i];
        let (tb, vb) = stops[i + 1];
        if abs_lat <= tb {
            let t = (abs_lat - ta) / (tb - ta);
            return va + (vb - va) * t;
        }
    }
    stops.last().unwrap().1
}

/// Applies the coastline-relative current bias to a base temperature
/// field. Returns a new field (does not mutate the input), same pattern
/// as `generate_aridity` taking existing fields and producing a derived
/// one.
pub fn apply_ocean_currents(temperature: &HeatMap, is_ocean: &[bool], params: &PlanetGenParams) -> HeatMap {
    let width = temperature.width;
    let height = temperature.height;
    let data = (0..width * height)
        .map(|idx| {
            let x = idx % width;
            let y = idx / width;
            let abs_lat = (y as f64 - height as f64 / 2.0).abs() / (height as f64 / 2.0);
            let raw = current_bias_raw(x, y, width, is_ocean, params);
            let bias = raw * current_lat_envelope(abs_lat) * params.current_temp_bias;
            (temperature.data[idx] + bias).clamp(0.0, 1.0)
        })
        .collect();
    HeatMap { width, height, data }
}
```

Note: `.is_none_or(...)` is stable on `Option` since Rust 1.82 — if the
project's toolchain is older, use `nearest.map_or(true, |(_, nd)| d < nd)`
instead (equivalent). Check with `rustc --version` if the build fails on
that line.

Verify: `cargo build` — clean (the 3 params fields from Task 1 are now
consumed, their dead-code warnings disappear; `apply_ocean_currents`
itself isn't called from `main` yet, so expect a dead-code warning on it
and the two `pub(crate)` helpers until Task 3/4 wire them in — expected,
same incremental pattern as every prior task).

Commit: `feat: implement coastline-relative ocean current bias`

## Task 3: Wire `apply_ocean_currents` into `main.rs`

**File:** `src/main.rs`

Change:
```rust
    let temperature = climate::generate_temperature(&elev, &params, 0.0, SEED);
    let is_sea_ice = climate::generate_sea_ice(&temperature, &is_ocean, params.sea_ice_temp_threshold);
```
to:
```rust
    let base_temperature = climate::generate_temperature(&elev, &params, 0.0, SEED);
    let temperature = climate::apply_ocean_currents(&base_temperature, &is_ocean, &params);
    let is_sea_ice = climate::generate_sea_ice(&temperature, &is_ocean, params.sea_ice_temp_threshold);
```
Everything downstream continues to reference `temperature` — no other
line in `main.rs` changes for this task (currents apply before sea ice
and precipitation generation, so both correctly respond to the biased
temperature, per the spec's stated ordering requirement).

Verify: `cargo build` clean (no more dead-code warnings on
`apply_ocean_currents`/`current_bias_raw` — `current_lat_envelope`
remains warned-about until Task 4 wires in the debug render, which is
the only other consumer — expected). `cargo run --release` succeeds.

Commit: `feat: apply ocean currents to the climate pipeline`

## Task 4: Debug render `render::save_ocean_currents`

**Files:** `src/render.rs`, `src/main.rs`

New function in `render.rs`, matching the `save_glacier`/`save_sea_ice`
masked-render style (direct per-cell index lookup, not `.sample()`):

```rust
/// Debug/verification render: plain dark land / ocean silhouette with a
/// red (warm) / blue (cool) overlay showing the current bias magnitude
/// and sign at each cell, independent of how it reads once blended into
/// the full composite/temperature renders.
pub fn save_ocean_currents(
    width: usize,
    height: usize,
    is_ocean: &[bool],
    params: &PlanetGenParams,
    path: &str,
) -> Result<(), image::ImageError> {
    let img = ImageBuffer::from_fn(width as u32, height as u32, |x, y| {
        let xi = x as usize;
        let yi = y as usize;
        let idx = yi * width + xi;
        let abs_lat = (yi as f64 - height as f64 / 2.0).abs() / (height as f64 / 2.0);
        let raw = crate::climate::current_bias_raw(xi, yi, width, is_ocean, params);
        let signed = raw * crate::climate::current_lat_envelope(abs_lat);
        let base: [u8; 3] = if is_ocean[idx] { [40, 40, 60] } else { [70, 65, 55] };
        let intensity = (signed.abs() * 255.0).clamp(0.0, 255.0) as u8;
        let color = if signed > 0.0 {
            [base[0].saturating_add(intensity), base[1], base[2]]
        } else if signed < 0.0 {
            [base[0], base[1], base[2].saturating_add(intensity)]
        } else {
            base
        };
        Rgb(color)
    });
    img.save(path)
}
```

Wire into `main.rs`, after the existing `render::save_sea_ice(...)?;`
call (grouping with the other simple masked/debug renders, before the
heavier composite/regions renders):
```rust
    render::save_ocean_currents(WIDTH, HEIGHT, &is_ocean, &params, "output/ocean_currents.png")?;
```

Also update the final `println!` count from 10 to 11 layers (this
project's established convention — see the region-detection plan's
Task-4-adjacent fix for precedent of keeping this count accurate):
```rust
    println!("Done — 11 layers written to output/");
```

Verify: `cargo build` clean (no more dead-code warnings on
`current_lat_envelope`). `cargo run --release` succeeds, produces
`output/ocean_currents.png`. Visually inspect (Read tool on the PNG, or
crop/resize with Python+Pillow): red should appear along continents' east
coasts, blue along west coasts, strongest roughly 25–35° from the
equator, fading out near the equator and poles, with a visible smaller
bleed of matching color onto adjacent coastal land.

Commit: `feat: add ocean current debug render`

## Task 5: Manual verification pass

No code changes.

1. Inspect `output/ocean_currents.png` per Task 4's visual check.
2. Inspect `output/temperature.png` and `output/composite.png`: coastal
   temperature (and therefore biome color, since `biome_terrain_color`
   reads temperature) should now visibly differ between a landmass's east
   and west coasts at matching latitudes, where it was previously
   symmetric.
3. Determinism check: run `cargo run --release` twice, `md5sum
   output/composite.png output/temperature.png output/ocean_currents.png`
   — must match between runs (this feature adds no new randomness — it's
   a deterministic function of `is_ocean` and latitude — but verify per
   project convention rather than assuming).
4. Report findings back — no commit for this task, it's a verification
   checkpoint (same pattern as the markers-export and region-detection
   plans' final manual-check tasks).

## Non-goals (explicitly deferred, per the spec)

- Real fluid dynamics (gyres, Coriolis, thermohaline circulation,
  seasonal current shifts).
- Equatorial (zonal) currents — the latitude envelope deliberately zeroes
  out near the equator.
- Freshwater greening, the equator-jitter-symmetry issue — both still
  separately deferred.
- Any change to `generate_temperature`, `generate_sea_ice`,
  `generate_precipitation`, `lat_band_factor`, or `westerly_weight`
  themselves.

## Verification summary

- `cargo build` clean at each task boundary (dead-code warnings only for
  not-yet-consumed items, resolving as later tasks wire them in).
- `cargo run --release` produces `output/ocean_currents.png` and
  regenerates every downstream output.
- Visual inspection confirms the east-coast-warm/west-coast-cool pattern,
  latitude tapering, and coastal land bleed.
- Determinism check: identical output across repeated runs.
