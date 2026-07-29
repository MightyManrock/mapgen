# mapgen: climate-space biome terrain coloring — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

## Context

`mapgen`'s composite render currently colors land purely by elevation
(`terrain_color` in `src/render.rs:133-148`, a single 1D gradient from
coastal sand through plains/forest/highland/rock to snow peak — no
temperature or precipitation input at all). That made sense for the
original sci-fi "Demiurge" context, where habitability/life was deferred
entirely, but for TTRPG fantasy worldbuilding (Earth-like plant life
assumed everywhere), a single elevation-only tint reads as flat and
uninformative — a desert and a rainforest at the same elevation currently
render identically.

This plan replaces the elevation-only gradient with a continuous
climate-space biome color: land hue comes from bilinearly interpolating
across four biome-archetype colors positioned at the corners of
(temperature, precipitation) space, then the existing elevation ramp
still governs the transition to bare rock/snow at altitude. No discrete
per-pixel classification is introduced — both axes are already continuous
`[0,1]` HeatMaps, so there's no seam to blend across by construction.

This was discussed and agreed in conversation (no separate spec doc this
time — the design is small and fully nailed down below; still gets a
docs/superpowers/plans/ file and the normal subagent-driven-development
review cycle). A follow-up feature (freshwater "greening" near
rivers/lakes, gated by temperature/elevation and dampened near ocean) was
explicitly deferred — NOT part of this plan.

This project has no automated test suite by design — verification is
`cargo build`/`cargo run` plus manual/visual inspection, consistent with
every prior mapgen plan.

## Task 1: Add `biome_terrain_color` to `render.rs`

**File:** `src/render.rs`

Add a new private function, placed near the other color functions (e.g.
right after the existing `terrain_color`, which this supersedes):

```rust
/// Land color from climate-space biome interpolation, elevation-shaded.
/// `temp_t`/`precip_t` are the raw (undithered) normalized [0,1] climate
/// values at this pixel — continuous by construction, no dithering needed
/// since there's no discrete category to break banding on. `land_t` is
/// the dithered, normalized elevation-above-sea-level value (same role as
/// the old `terrain_color`'s input).
fn biome_terrain_color(temp_t: f64, precip_t: f64, land_t: f64) -> [u8; 3] {
    const COLD_DRY: [u8; 3] = [150, 150, 120]; // tundra
    const COLD_WET: [u8; 3] = [40, 90, 70];    // taiga / boreal forest
    const HOT_DRY: [u8; 3]  = [210, 170, 100]; // desert
    const HOT_WET: [u8; 3]  = [30, 110, 40];   // tropical rainforest

    let low  = lerp_color(COLD_DRY, COLD_WET, precip_t);
    let high = lerp_color(HOT_DRY, HOT_WET, precip_t);
    let base = lerp_color(low, high, temp_t);
    let base_dark = [
        (base[0] as f64 * 0.75) as u8,
        (base[1] as f64 * 0.75) as u8,
        (base[2] as f64 * 0.75) as u8,
    ];

    sample_gradient(
        land_t,
        &[
            ([220, 200, 150], 0.00), // coastal sand (shared, unchanged)
            (base,            0.05), // climate-space biome color
            (base_dark,       0.55), // mild darkening toward "hills"
            ([110, 120, 70],  0.60), // highland (unchanged)
            ([140, 110, 80],  0.75), // rocky terrain (unchanged)
            ([170, 160, 150], 0.88), // grey rock (unchanged)
            ([230, 235, 240], 0.95), // snow line (unchanged)
            ([255, 255, 255], 1.00), // peak snow (unchanged)
        ],
    )
}
```

Remove the old `terrain_color` function (`src/render.rs:131-148`) entirely
— it's fully superseded and Task 2 removes its only call sites, so
leaving it in would just be dead code.

Verify: `cargo build`. Since nothing calls `biome_terrain_color` yet
(Task 2 wires it in) and `terrain_color`'s call sites are also still
present until Task 2, do NOT remove `terrain_color` in isolation — see
Task 2, these two steps happen together in one commit since removing
`terrain_color` before its replacement is wired in would break the build.
**Combine Task 1 and Task 2 into a single implementation task** (see
below) — they're too tightly coupled to land as separate commits without
an intermediate broken-build state.

## Task 2 (combined with Task 1 above): Wire `biome_terrain_color` in and remove `terrain_color`

**Files:** `src/render.rs`, `src/main.rs`

`composite_pixel_color` (`src/render.rs:292-403`) currently takes
`elevation`/`temperature` HeatMaps but not `precipitation`. Add a
`precipitation: &HeatMap` parameter (insert after `temperature: &HeatMap`
in the signature, matching the existing parameter-ordering convention of
"data inputs, then flags, then params").

Both existing call sites of `terrain_color(d)` inside
`composite_pixel_color` — one in the glacier branch's "non-glacier land"
fallback (~line 378-381), one in the default land branch (~line 384-387)
— change from:
```rust
let elev_t = elevation.sample(nx, ny); // (or `let t = elevation.sample(nx, ny);` in the other branch)
let land_t = ((elev_t - params.sea_level) / (1.0 - params.sea_level)).clamp(0.0, 1.0);
let d = bayer_dither(land_t, rx as usize, ry as usize, N_DITHER_LEVELS);
terrain_color(d)
```
to:
```rust
let elev_t = elevation.sample(nx, ny);
let land_t = ((elev_t - params.sea_level) / (1.0 - params.sea_level)).clamp(0.0, 1.0);
let d = bayer_dither(land_t, rx as usize, ry as usize, N_DITHER_LEVELS);
let temp_t = temperature.sample(nx, ny);
let precip_t = precipitation.sample(nx, ny);
biome_terrain_color(temp_t, precip_t, d)
```
(Only `land_t`/`d` — the elevation axis — gets dithered, matching the
design discussion: `temp_t`/`precip_t` are already continuous via
bilinear `.sample()`, no banding to dither away.)

`save_composite` (`src/render.rs:405-...`) and `save_regions` both call
`composite_pixel_color` and need the same new `precipitation: &HeatMap`
parameter threaded through (added to their own signatures, passed through
to the inner `composite_pixel_color` call).

`src/main.rs`: both call sites — `render::save_composite(...)` and
`render::save_regions(...)` — gain a `&precipitation` argument (the
`precipitation` binding already exists in `main()` from the climate
pipeline, inserted in the same relative position as the new parameter in
each function's signature).

Verify:
1. `source "$HOME/.cargo/env" && cd /root/tools/mapgen && cargo build` —
   clean, no `terrain_color`-related dead-code warnings (it's fully
   removed), no new warnings beyond the existing pre-existing ones
   (`Region` unused-field warnings, `HydrologyResult` unused-field
   warnings — unrelated to this change).
2. `cargo run --release` — succeeds, `output/composite.png` and
   `output/regions.png` both regenerate (both use
   `composite_pixel_color`).
3. Visual inspection (Read tool on both PNGs): land color should now vary
   with climate — greener/darker in wet/tropical regions, tan/ochre in
   arid belts, grey-brown-green tundra toward the poles — while
   mountains/peaks still fade to the same rock/snow palette as before
   regardless of biome. Compare against the pre-change render mentally
   (or keep a copy) — the *shape* of every boundary (coastlines, contour
   lines, water/glacier/sea-ice rendering) must be unchanged, only the
   land hue selection differs.
4. Determinism check: run twice, `md5sum output/composite.png` — should
   match between the two runs (same seed ⇒ identical output), consistent
   with the project's existing reproducibility guarantee.

Commit:
```bash
cd /root/tools/mapgen
git add src/render.rs src/main.rs
git commit -m "feat: replace elevation-only terrain coloring with climate-space biome colors"
```

## Non-goals (explicitly deferred)

- Freshwater "greening" near rivers/lakes (distance-based post-process,
  gated by temperature/elevation, dampened near ocean) — discussed and
  explicitly deferred to a follow-up plan, not part of this change.
- Any change to the discrete `Region`/`climate_character()` biome-name
  system — that stays exactly as-is for region labels/tooltips, fully
  decoupled from pixel coloring (as discussed).
- Any change to water/glacier/sea-ice coloring, contour lines, or
  dithering/supersampling infrastructure — untouched.

## Verification summary

- `cargo build` clean (no leftover `terrain_color` dead code).
- `cargo run --release` regenerates `composite.png` and `regions.png`
  successfully.
- Visual inspection confirms climate-driven land hue variation while
  preserving all boundary shapes (coastlines, contours, water/ice
  rendering) exactly as before.
- Determinism check: identical output across repeated runs.
