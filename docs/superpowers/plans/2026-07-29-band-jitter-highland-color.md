# mapgen: highland color fix + climate band jitter — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

## Context

Visual inspection of the climate-space biome render (`docs/superpowers/plans/2026-07-28-biome-terrain-coloring.md`) surfaced two issues, both confirmed against the actual data (not just visual impression):

1. **False "green ring" around mountains.** Comparing the same map region across `composite.png` and `precipitation.png` showed flat, dry precipitation near mountain massifs with no corresponding wet spot — the green halo visible in the composite is a pure rendering artifact. `biome_terrain_color`'s `highland` elevation stop (`src/render.rs`) is a single fixed color, `[110, 120, 70]`, applied to all land crossing that elevation regardless of climate (by the earlier deliberate "shared elevation shading curve" design choice). That color has G>R, which reads as olive-green next to a desert's tan, creating a false-positive vegetation band around any tall terrain even in bone-dry climates.

2. **Unrealistically straight east-west climate bands.** `lat_band_factor` and `westerly_weight` in `src/climate.rs` are both pure functions of latitude (`y` only, piecewise-linear stops) with zero longitude variation — so wherever local terrain-driven moisture doesn't override them, they produce mathematically exact horizontal lines. Real circulation belts exist for the same underlying reason but get roughened by effects this simplified static model doesn't capture.

This plan fixes both. Both fixes were discussed and approved in conversation — no separate spec doc, the design is small and fully nailed down below. Ocean currents (a related idea raised in discussion) and freshwater greening (deferred earlier) are explicitly NOT part of this plan.

This project has no automated test suite by design — verification is `cargo build`/`cargo run` plus manual/visual inspection, consistent with every prior mapgen plan.

## Task 1: Fix the highland color

**File:** `src/render.rs`

In `biome_terrain_color`, change the `highland` gradient stop from:
```rust
([110, 120, 70],  0.60), // highland (unchanged)
```
to:
```rust
([150, 135, 110], 0.60), // highland — neutral rock-brown, no green cast
```
(R>G>B — reads unambiguously as bare rock/scree regardless of the biome color it's blending from, unlike the old `[110,120,70]` which had G>R.) No other stops change — `rocky`/`grey rock`/`snow line`/`peak snow` were already correctly neutral/brown/grey and are not implicated.

Verify:
1. `source "$HOME/.cargo/env" && cd /root/tools/mapgen && cargo build` — clean, same pre-existing warnings only.
2. `cargo run --release` — regenerates `output/composite.png` and `output/regions.png`.
3. Crop and inspect the same mountain region checked during diagnosis (roughly x∈[0.30,0.60]×width, y∈[0.20,0.50]×height of `composite.png`) — the green halo around the mountain massifs should be gone, replaced by a neutral brown-grey transition into the rocky/snow-capped peaks.

Commit:
```bash
cd /root/tools/mapgen
git add src/render.rs
git commit -m "fix: replace green-tinted highland color with neutral rock-brown"
```

## Task 2: Add longitude jitter to climate bands

**Files:** `src/climate.rs`, `src/main.rs`

**Step 1 — Add the shared jitter helper to `src/climate.rs`.**

Add imports at the top of the file (alongside the existing `use std::f64::consts::PI;`):
```rust
use noise::{Fbm, NoiseFn, Perlin};
```

Add a private constant and helper function (placed near the top of the file, above `generate_temperature`):
```rust
/// Smooth per-cell latitude offset that breaks up otherwise razor-straight
/// circulation bands (`lat_band_factor`/`westerly_weight` are pure functions
/// of latitude with zero longitude variation). Sampled at 3D sphere-surface
/// coordinates — same technique as `elevation::generate_elevation` — so it's
/// seamless in x and converges naturally at the poles.
const JITTER_AMPLITUDE: f64 = 0.04;

fn lat_jitter(x: usize, y: usize, width: usize, height: usize, fbm: &Fbm<Perlin>) -> f64 {
    let lon = x as f64 / width as f64 * std::f64::consts::TAU;
    let lat = (y as f64 / height as f64 - 0.5) * PI;
    let cos_lat = lat.cos();
    let r = 3.5 / std::f64::consts::TAU;
    let sx = r * cos_lat * lon.cos();
    let sy = r * cos_lat * lon.sin();
    let sz = r * lat.sin();
    fbm.get([sx, sy, sz]) * JITTER_AMPLITUDE
}
```

**Step 2 — Wire it into `generate_temperature`.**

Add a `seed: u32` parameter (append at the end of the existing parameter list — least invasive, matches how `elevation::generate_elevation` takes `seed` as an explicit param):
```rust
pub fn generate_temperature(elevation: &HeatMap, params: &PlanetGenParams, season_phase: f64, seed: u32) -> HeatMap {
```
Construct the noise field once, before the per-cell loop (use a seed offset of `+20`, decorrelated from every other `wrapping_add` offset already in use — elevation's warps use `+1`/`+2`/`+3`, `roughen_coastline` uses `+10`):
```rust
    let jitter_fbm = Fbm::<Perlin>::new(seed.wrapping_add(20));
```
Inside the per-cell closure, add `let x = idx % width;` (currently only `y` is computed) and fold jitter into `shifted_lat`:
```rust
        .map(|idx| {
            let x = idx % width;
            let y = idx / width;
            let abs_lat = (y as f64 - height as f64 / 2.0).abs() / (height as f64 / 2.0);
            let jitter = lat_jitter(x, y, width, height, &jitter_fbm);
            let shifted_lat = (abs_lat - season_offset + jitter).abs().clamp(0.0, 1.0);
            let lat_shape = (shifted_lat * std::f64::consts::FRAC_PI_2).cos();
            let lat_temp = params.temp_baseline * (1.0 - params.temp_gradient * (1.0 - lat_shape));
            (lat_temp - elevation.data[idx] * params.lapse_factor).clamp(0.0, 1.0)
        })
```

**Step 3 — Wire it into `generate_precipitation`.**

Add a `seed: u32` parameter (append at the end):
```rust
pub fn generate_precipitation(
    elevation: &HeatMap,
    is_ocean: &[bool],
    temperature: &HeatMap,
    is_sea_ice: &[bool],
    params: &PlanetGenParams,
    season_phase: f64,
    seed: u32,
) -> HeatMap {
```
Construct the noise field once, before the final `data` computation (use seed offset `+21` — decorrelated from temperature's `+20` so the two wobbles aren't identical):
```rust
    let jitter_fbm = Fbm::<Perlin>::new(seed.wrapping_add(21));
```
In the final `data` closure, add `let x = idx % width;` and pass jittered latitude into both `westerly_weight` and `lat_band_factor` — **do not modify those two functions themselves**, just perturb the value passed in at the call site:
```rust
        .map(|idx| {
            let x = idx % width;
            let y = idx / width;
            let abs_lat = (y as f64 - height as f64 / 2.0).abs() / (height as f64 / 2.0);
            let jitter = lat_jitter(x, y, width, height, &jitter_fbm);
            let w = westerly_weight(abs_lat + jitter, season_offset);
            let moisture = moisture_west[idx] * w + moisture_east[idx] * (1.0 - w);
            let band = lat_band_factor(abs_lat + jitter, season_offset);
            let moisture_capacity = (0.3 + 0.7 * temperature.data[idx]).clamp(0.3, 1.0);
            (band * (params.base_arid + moisture * (1.0 - params.base_arid)) * moisture_capacity).clamp(0.0, 1.0)
        })
```

**Step 4 — Update `src/main.rs` call sites.**

```rust
let temperature = climate::generate_temperature(&elev, &params, 0.0, SEED);
```
and
```rust
let precipitation =
    climate::generate_precipitation(&elev, &is_ocean, &temperature, &is_sea_ice, &params, 0.0, SEED);
```
(Pass the raw `SEED` constant to both — each function derives its own decorrelated sub-seed internally via `wrapping_add`, matching the existing convention.)

Verify:
1. `source "$HOME/.cargo/env" && cd /root/tools/mapgen && cargo build` — clean, same pre-existing warnings only, no new ones.
2. `cargo run --release` — regenerates all outputs (temperature/precipitation feed into region detection, hydrology, and every render, so everything downstream regenerates).
3. Visual inspection: `output/precipitation.png` and `output/temperature.png` should show gently wobbling band boundaries instead of razor-straight horizontal lines — compare against the pre-change crop taken during diagnosis. The wobble should be subtle (a few large meanders, not chaotic noise) — if it looks unchanged, `JITTER_AMPLITUDE` may need bumping; if it looks chaotic/unrecognizable as circulation bands, it may need reducing. Report what you see rather than just asserting success.
4. Determinism check: run `cargo run --release` twice, `md5sum output/composite.png output/precipitation.png` — must match between runs (same seed ⇒ identical output).
5. Confirm `output/composite.markers.json` regenerates successfully and region tooltips still read as plausible biome names (region detection consumes the now-jittered temperature/precipitation fields, so region shapes will shift slightly — that's expected, not a bug).

Commit:
```bash
cd /root/tools/mapgen
git add src/climate.rs src/main.rs
git commit -m "feat: add longitude jitter to climate circulation bands"
```

## Non-goals (explicitly deferred)

- Ocean currents (discussed — ocean-heat-transport proxy based on coastline position relative to landmass) — a meaningfully bigger feature, deferred to its own future plan.
- Freshwater "greening" near rivers/lakes — deferred earlier, still not part of this plan.
- Any change to `lat_band_factor`/`westerly_weight`'s own stop values/shapes — only the latitude value fed into them changes, not the functions themselves.
- Trying multiple seeds to find a climatically-varied "control" planet — a separate exploration task, not a code change.

## Verification summary

- `cargo build` clean after each task.
- `cargo run --release` regenerates all outputs successfully.
- Visual inspection confirms: no green ring around mountains (Task 1), gently wobbling (not razor-straight) circulation band boundaries (Task 2).
- Determinism check: identical output across repeated runs.
