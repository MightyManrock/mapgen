# Composite Terrain Render Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a single naturalistic composite terrain render (`output/composite.png`) to the mapgen prototype, ported from demiurge-rust's `render_composite_map`.

**Architecture:** Extend `src/render.rs` with new private color/dither helpers (`terrain_color`, `BAYER_4X4`, `is_contour`, `bayer_dither`) and one new public function `save_composite`, then wire one more call into `main.rs`'s existing render sequence.

**Tech Stack:** Rust, `image` crate (already a dependency) — no new dependencies.

**Note on testing:** This project has no automated test suite (an explicit, approved decision in `docs/superpowers/specs/2026-07-28-prototype-pipeline-design.md` — validation is visual, by eye, on the rendered PNGs). Steps below use `cargo build`/`cargo run` plus visual checks in place of unit tests.

---

### Task 1: Add composite-only color and dithering helpers to `render.rs`

**Files:**
- Modify: `/root/tools/mapgen/src/render.rs`

- [ ] **Step 1: Add `terrain_color`, `BAYER_4X4`, `is_contour`, and `bayer_dither`**

Add these four items right after the existing `aridity_color` function (the last color function currently in the file), before the `save_sampled` helper:

```rust
/// Terrain color for land in the composite. Accepts a pre-normalized land_t
/// in [0, 1] where 0 = coastline and 1 = highest peak.
fn terrain_color(land_t: f64) -> [u8; 3] {
    sample_gradient(
        land_t,
        &[
            ([220, 200, 150], 0.00), // coastal sand / beach
            ([180, 210, 120], 0.05), // lowland
            ([120, 175, 80], 0.20),  // plains / grassland
            ([80, 140, 60], 0.40),   // forest / hills
            ([110, 120, 70], 0.60),  // highland
            ([140, 110, 80], 0.75),  // rocky terrain
            ([170, 160, 150], 0.88), // grey rock
            ([230, 235, 240], 0.95), // snow line
            ([255, 255, 255], 1.00), // peak snow
        ],
    )
}

const BAYER_4X4: [[f64; 4]; 4] = [
    [ 0.0 / 16.0,  8.0 / 16.0,  2.0 / 16.0, 10.0 / 16.0],
    [12.0 / 16.0,  4.0 / 16.0, 14.0 / 16.0,  6.0 / 16.0],
    [ 3.0 / 16.0, 11.0 / 16.0,  1.0 / 16.0,  9.0 / 16.0],
    [15.0 / 16.0,  7.0 / 16.0, 13.0 / 16.0,  5.0 / 16.0],
];

/// Returns true if the elevation value crosses a contour boundary between
/// this pixel and either of its right/down neighbors.
fn is_contour(e: f64, e_right: f64, e_down: f64, n_contours: usize) -> bool {
    let level = |v: f64| (v * n_contours as f64).floor() as i64;
    level(e) != level(e_right) || level(e) != level(e_down)
}

/// Ordered dither: quantize t to n_levels steps, using Bayer threshold at
/// render pixel (rx, ry) to break ties at level boundaries.
fn bayer_dither(t: f64, rx: usize, ry: usize, n_levels: usize) -> f64 {
    let threshold = BAYER_4X4[ry % 4][rx % 4];
    let scaled = t * n_levels as f64;
    let lo = scaled.floor();
    let level = if scaled - lo > threshold { lo + 1.0 } else { lo };
    (level / n_levels as f64).clamp(0.0, 1.0)
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cd /root/tools/mapgen && source "$HOME/.cargo/env" && cargo build 2>&1 | tail -30`

Expected: compiles with only `dead_code` warnings for the four new items (`terrain_color`, `BAYER_4X4`, `is_contour`, `bayer_dither` — none are called yet, that's Task 2). No errors.

---

### Task 2: Add `save_composite` to `render.rs`

**Files:**
- Modify: `/root/tools/mapgen/src/render.rs`

- [ ] **Step 1: Add the `use` import for `PlanetGenParams`**

At the top of `render.rs`, alongside the existing `use crate::heatmap::HeatMap;` line, add:

```rust
use crate::params::PlanetGenParams;
```

- [ ] **Step 2: Add `save_composite`**

Add this function at the end of `render.rs`, after `save_sea_ice`:

```rust
const RENDER_SCALE: usize = 3;
const N_DITHER_LEVELS: usize = 16;
const N_CONTOURS: usize = 40;
const CONTOUR_DARKEN: f64 = 0.90;
const CONTOUR_DARKEN_WATER: f64 = 0.95;

/// Naturalistic composite render: terrain colored by elevation band, water
/// blended in from the hydrology layer, contour lines. 3x supersampled with
/// Bayer ordered-dithering for smooth elevation-band transitions and
/// anti-aliased water/glacier/sea-ice edges. Ported from demiurge-rust's
/// render_composite_map, minus the salt-flat branch (mapgen has no salt-flat
/// generator).
pub fn save_composite(
    width: usize,
    height: usize,
    hydro_map: &HeatMap,
    elevation: &HeatMap,
    temperature: &HeatMap,
    is_ocean: &[bool],
    is_glacier: &[bool],
    is_sea_ice: &[bool],
    params: &PlanetGenParams,
    path: &str,
) -> Result<(), image::ImageError> {
    let render_width = width * RENDER_SCALE;
    let render_height = height * RENDER_SCALE;
    let img = ImageBuffer::from_fn(render_width as u32, render_height as u32, |rx, ry| {
        let nx = rx as f64 / render_width as f64;
        let ny = ry as f64 / render_height as f64;
        let hydro_nearest = hydro_map.sample_nearest(nx, ny);
        let is_water = if hydro_nearest <= 0.0 {
            false
        } else if hydro_nearest <= 0.3 {
            true
        } else {
            const EDGE_COVERAGE: f64 = 0.0025;
            let dx = rx as usize / RENDER_SCALE;
            let dy = ry as usize / RENDER_SCALE;
            let off_x = rx as usize % RENDER_SCALE;
            let off_y = ry as usize % RENDER_SCALE;
            let neighbor = |ndx: i64, ndy: i64| -> f64 {
                let nnx = ndx.rem_euclid(width as i64) as usize;
                let nny = ndy.clamp(0, height as i64 - 1) as usize;
                hydro_map.data[nny * width + nnx]
            };
            let mut coverage = 1.0f64;
            if off_x == 0 && neighbor(dx as i64 - 1, dy as i64) <= 0.0 { coverage = EDGE_COVERAGE; }
            if off_x == 2 && neighbor(dx as i64 + 1, dy as i64) <= 0.0 { coverage = EDGE_COVERAGE; }
            if off_y == 0 && neighbor(dx as i64, dy as i64 - 1) <= 0.0 { coverage = EDGE_COVERAGE; }
            if off_y == 2 && neighbor(dx as i64, dy as i64 + 1) <= 0.0 { coverage = EDGE_COVERAGE; }
            BAYER_4X4[ry as usize % 4][rx as usize % 4] < coverage
        };
        let data_idx = (ry as usize / RENDER_SCALE) * width + (rx as usize / RENDER_SCALE);
        let dx = rx as usize / RENDER_SCALE;
        let dy = ry as usize / RENDER_SCALE;
        let off_x = rx as usize % RENDER_SCALE;
        let off_y = ry as usize % RENDER_SCALE;
        let mut color = if is_water && is_sea_ice[data_idx] {
            let sea_ice_neighbor = |ndx: i64, ndy: i64| -> bool {
                let nnx = ndx.rem_euclid(width as i64) as usize;
                let nny = ndy.clamp(0, height as i64 - 1) as usize;
                is_sea_ice[nny * width + nnx]
            };
            const SEA_ICE_EDGE: f64 = 0.05;
            let mut coverage = 1.0f64;
            if off_x == 0 && !sea_ice_neighbor(dx as i64 - 1, dy as i64) { coverage = SEA_ICE_EDGE; }
            if off_x == 2 && !sea_ice_neighbor(dx as i64 + 1, dy as i64) { coverage = SEA_ICE_EDGE; }
            if off_y == 0 && !sea_ice_neighbor(dx as i64, dy as i64 - 1) { coverage = SEA_ICE_EDGE; }
            if off_y == 2 && !sea_ice_neighbor(dx as i64, dy as i64 + 1) { coverage = SEA_ICE_EDGE; }
            if BAYER_4X4[ry as usize % 4][rx as usize % 4] < coverage {
                let t = temperature.sample(nx, ny);
                let d = bayer_dither(t / params.sea_ice_temp_threshold, rx as usize, ry as usize, N_DITHER_LEVELS);
                sea_ice_color(d, params.sea_ice_temp_threshold)
            } else {
                let d = bayer_dither(hydro_nearest, rx as usize, ry as usize, N_DITHER_LEVELS).max(0.01);
                water_color(d)
            }
        } else if is_water {
            let d = bayer_dither(hydro_nearest, rx as usize, ry as usize, N_DITHER_LEVELS).max(0.01);
            water_color(d)
        } else if is_glacier[data_idx] {
            let non_glacier_land = |ndx: i64, ndy: i64| -> bool {
                let nnx = ndx.rem_euclid(width as i64) as usize;
                let nny = ndy.clamp(0, height as i64 - 1) as usize;
                let nidx = nny * width + nnx;
                !is_glacier[nidx] && !is_ocean[nidx] && hydro_map.data[nidx] <= 0.0
            };
            const GLACIER_EDGE: f64 = 0.05;
            let mut coverage = 1.0f64;
            if off_x == 0 && non_glacier_land(dx as i64 - 1, dy as i64) { coverage = GLACIER_EDGE; }
            if off_x == 2 && non_glacier_land(dx as i64 + 1, dy as i64) { coverage = GLACIER_EDGE; }
            if off_y == 0 && non_glacier_land(dx as i64, dy as i64 - 1) { coverage = GLACIER_EDGE; }
            if off_y == 2 && non_glacier_land(dx as i64, dy as i64 + 1) { coverage = GLACIER_EDGE; }
            if BAYER_4X4[ry as usize % 4][rx as usize % 4] < coverage {
                let t = temperature.sample(nx, ny);
                let d = bayer_dither(t / params.glacier_temp_threshold, rx as usize, ry as usize, N_DITHER_LEVELS);
                glacier_color(d, params.glacier_temp_threshold)
            } else {
                let elev_t = elevation.sample(nx, ny);
                let land_t = ((elev_t - params.sea_level) / (1.0 - params.sea_level)).clamp(0.0, 1.0);
                let d = bayer_dither(land_t, rx as usize, ry as usize, N_DITHER_LEVELS);
                terrain_color(d)
            }
        } else {
            let t = elevation.sample(nx, ny);
            let land_t = ((t - params.sea_level) / (1.0 - params.sea_level)).clamp(0.0, 1.0);
            let d = bayer_dither(land_t, rx as usize, ry as usize, N_DITHER_LEVELS);
            terrain_color(d)
        };
        let nx_r = (rx as usize + 1) as f64 / render_width as f64;
        let ny_d = (ry as usize + 1) as f64 / render_height as f64;
        let e = elevation.sample(nx, ny);
        let e_r = elevation.sample(nx_r, ny);
        let e_d = elevation.sample(nx, ny_d);
        if is_contour(e, e_r, e_d, N_CONTOURS) {
            let factor = if is_water { CONTOUR_DARKEN_WATER } else { CONTOUR_DARKEN };
            color = [
                (color[0] as f64 * factor) as u8,
                (color[1] as f64 * factor) as u8,
                (color[2] as f64 * factor) as u8,
            ];
        }
        Rgb(color)
    });
    img.save(path)
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /root/tools/mapgen && source "$HOME/.cargo/env" && cargo build 2>&1 | tail -30`

Expected: compiles cleanly, no warnings (the Task 1 helpers are now all used by `save_composite`), no errors.

---

### Task 3: Wire `save_composite` into `main.rs`

**Files:**
- Modify: `/root/tools/mapgen/src/main.rs`

- [ ] **Step 1: Add the composite render call**

In `main.rs`, after the existing `render::save_sea_ice(...)?;` line and before the `println!("Done...")` line, add:

```rust
    render::save_composite(
        WIDTH, HEIGHT, &hydro.map, &elev, &temperature, &is_ocean, &is_glacier, &is_sea_ice,
        &params, "output/composite.png",
    )?;
```

Update the final `println!` from `"Done — 8 layers written to output/"` to `"Done — 9 layers written to output/"`.

- [ ] **Step 2: Build and run**

Run: `cd /root/tools/mapgen && source "$HOME/.cargo/env" && cargo build 2>&1 | tail -30 && cargo run --release 2>&1 | tail -5`

Expected: clean build (no warnings — `sample_nearest` on `HeatMap`, previously dead-code-warned, is now used by `save_composite`), then `Done — 9 layers written to output/`.

- [ ] **Step 3: Verify `output/composite.png` exists and looks right**

Run: `ls -la /root/tools/mapgen/output/composite.png`

Then view the image and visually confirm:
- Smooth terrain color bands (sand → grass → forest → rock → snow) with no hard color-step banding — the dithering should make transitions look gradient-like, not stair-stepped.
- Anti-aliased coastlines (no jagged pixel-block edges where land meets water at native resolution).
- Visible topographic contour lines following the terrain, darker over water than land.
- Sea ice and glacier read as distinct icy colors from open water/bare land, with soft edges.
- Image dimensions are 3072×1536 (3× the base 1024×512).

- [ ] **Step 4: Confirm determinism**

Run:
```bash
cd /root/tools/mapgen
md5sum output/composite.png > /tmp/composite_run1.md5
cargo run --release 2>&1 | tail -2
md5sum output/composite.png > /tmp/composite_run2.md5
diff /tmp/composite_run1.md5 /tmp/composite_run2.md5 && echo "DETERMINISTIC"
```

Expected: `DETERMINISTIC` printed, hashes match.

- [ ] **Step 5: Commit**

```bash
cd /root/tools/mapgen
git add src/render.rs src/main.rs
git commit -m "$(cat <<'EOF'
feat: add composite terrain render output

Ports render_composite_map from demiurge-rust into render.rs, dropping
the salt-flat branch (mapgen has no salt-flat generator). 3x
supersampled, Bayer-dithered terrain/water coloring with anti-aliased
water/glacier/sea-ice edges and topographic contour lines. Wired into
main.rs as a 9th output layer, output/composite.png.
EOF
)"
git push origin main
```

---

## Self-Review Notes

- **Spec coverage:** Architecture (extend `render.rs`, no new module) ✓ Task 1+2. Signature matches spec exactly ✓ Task 2 Step 2. Algorithm (water → sea-ice sub-case → glacier → terrain, contour overlay) ✓ ported verbatim in Task 2. Output path/resolution (`output/composite.png`, 3072×1536) ✓ Task 3. Testing (visual + determinism, matching existing pattern) ✓ Task 3 Steps 3-4.
- **Placeholder scan:** No TBD/TODO; all code blocks are complete, verbatim-or-adapted source, not descriptions.
- **Type consistency:** `save_composite` signature in Task 2 matches the call site in Task 3 exactly (same argument order: `WIDTH, HEIGHT, &hydro.map, &elev, &temperature, &is_ocean, &is_glacier, &is_sea_ice, &params, path`). `PlanetGenParams` field names (`sea_ice_temp_threshold`, `glacier_temp_threshold`, `sea_level`) match `src/params.rs` as written in the prior implementation.
