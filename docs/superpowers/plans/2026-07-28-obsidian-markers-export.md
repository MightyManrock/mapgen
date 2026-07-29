# Obsidian markers.json scaffold export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After generating `output/composite.png`, also write `output/composite.markers.json` — a ready-to-use `zoom-map` Obsidian plugin sidecar file, pre-filled with image bindings and a computed real-world measurement scale.

**Architecture:** Add a `radius_km` field to `PlanetGenParams`. Add a new `src/export.rs` module with one function, `save_markers_json`, that builds the plugin's exact "fresh file" JSON shape (verified against the plugin's own source) via serde-derived structs and writes it pretty-printed. Wire it into `main.rs` after the composite render.

**Tech Stack:** Rust, `serde`/`serde_json` (new dependencies), existing `mapgen` crate structure.

**Note on testing:** This project has no automated test suite by design (established in the prototype-pipeline and composite-render plans) — verification throughout is `cargo build`/`cargo run` plus manual/visual inspection of output. This plan follows that same convention; there are no TDD test-writing steps.

---

### Task 1: Add `radius_km` to `PlanetGenParams`

**Files:**
- Modify: `src/params.rs`

- [ ] **Step 1: Add the field**

In `src/params.rs`, add `radius_km: f64` to the `PlanetGenParams` struct (after `glacier_melt_factor` at line 24):

```rust
    pub glacier_melt_factor: f64,
    pub radius_km: f64,
}
```

- [ ] **Step 2: Set it in `earth_like()`**

In the same file, add to the `earth_like()` constructor (after `glacier_melt_factor: 2.5,` at line 50):

```rust
            glacier_melt_factor: 2.5,
            radius_km: 6371.0,
        }
```

- [ ] **Step 3: Build to verify**

Run: `source "$HOME/.cargo/env" && cd /root/tools/mapgen && cargo build`
Expected: clean compile (same pre-existing `HydrologyResult` dead-code warning as before, nothing new — `radius_km` isn't read anywhere yet, but it's a `pub` field on a struct already reachable from `main`, so it won't trigger dead-code warnings the way an unused free function would).

- [ ] **Step 4: Commit**

```bash
cd /root/tools/mapgen
git add src/params.rs
git commit -m "feat: add radius_km to PlanetGenParams for real-world scale export"
```

---

### Task 2: Add serde dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add the dependencies**

In `Cargo.toml`, under `[dependencies]`, add:

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

- [ ] **Step 2: Build to pull the lockfile**

Run: `source "$HOME/.cargo/env" && cd /root/tools/mapgen && cargo build`
Expected: clean compile, `Cargo.lock` updated with `serde`/`serde_json` and their transitive deps.

- [ ] **Step 3: Commit**

```bash
cd /root/tools/mapgen
git add Cargo.toml Cargo.lock
git commit -m "build: add serde/serde_json dependencies"
```

---

### Task 3: Expose `RENDER_SCALE` from `render.rs`

**Files:**
- Modify: `src/render.rs:280`

- [ ] **Step 1: Make the constant public**

In `src/render.rs`, change line 280 from:

```rust
const RENDER_SCALE: usize = 3;
```

to:

```rust
pub const RENDER_SCALE: usize = 3;
```

- [ ] **Step 2: Build to verify**

Run: `source "$HOME/.cargo/env" && cd /root/tools/mapgen && cargo build`
Expected: clean compile, no new warnings (widening visibility of an already-used constant doesn't change reachability).

- [ ] **Step 3: Commit**

```bash
cd /root/tools/mapgen
git add src/render.rs
git commit -m "refactor: expose RENDER_SCALE as pub for use in export.rs"
```

---

### Task 4: Implement `src/export.rs`

**Files:**
- Create: `src/export.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write the module**

Create `src/export.rs`:

```rust
use crate::params::PlanetGenParams;
use serde::Serialize;
use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::fs;

#[derive(Serialize)]
struct Size {
    w: usize,
    h: usize,
}

#[derive(Serialize)]
struct Layer {
    id: &'static str,
    name: &'static str,
    visible: bool,
    locked: bool,
}

#[derive(Serialize)]
struct Measurement {
    scales: BTreeMap<String, f64>,
    #[serde(rename = "customUnitPxPerUnit")]
    custom_unit_px_per_unit: BTreeMap<String, f64>,
    #[serde(rename = "travelTimePresetIds")]
    travel_time_preset_ids: Vec<String>,
    #[serde(rename = "travelDaysEnabled")]
    travel_days_enabled: bool,
}

#[derive(Serialize)]
struct SecondScreen {}

#[derive(Serialize)]
struct MarkersFile {
    size: Size,
    layers: Vec<Layer>,
    markers: Vec<()>,
    bases: Vec<String>,
    overlays: Vec<()>,
    #[serde(rename = "activeBase")]
    active_base: String,
    measurement: Measurement,
    #[serde(rename = "pinSizeOverrides")]
    pin_size_overrides: BTreeMap<String, f64>,
    grids: Vec<()>,
    #[serde(rename = "panClamp")]
    pan_clamp: bool,
    #[serde(rename = "drawLayers")]
    draw_layers: Vec<()>,
    drawings: Vec<()>,
    #[serde(rename = "secondScreen")]
    second_screen: SecondScreen,
    #[serde(rename = "textLayers")]
    text_layers: Vec<()>,
}

/// Pole-to-pole distance divided by render height: the vertical scale is
/// constant across an equirectangular map, unlike the horizontal scale
/// (which shrinks by cos(latitude) toward the poles). See
/// docs/superpowers/specs/2026-07-28-obsidian-markers-export-design.md.
fn meters_per_pixel(params: &PlanetGenParams, render_height: usize) -> f64 {
    (PI * params.radius_km * 1000.0) / render_height as f64
}

pub fn save_markers_json(
    render_width: usize,
    render_height: usize,
    params: &PlanetGenParams,
    image_filename: &str,
    path: &str,
) -> std::io::Result<()> {
    let mut scales = BTreeMap::new();
    scales.insert(
        image_filename.to_string(),
        meters_per_pixel(params, render_height),
    );

    let data = MarkersFile {
        size: Size {
            w: render_width,
            h: render_height,
        },
        layers: vec![Layer {
            id: "default",
            name: "Default",
            visible: true,
            locked: false,
        }],
        markers: vec![],
        bases: vec![image_filename.to_string()],
        overlays: vec![],
        active_base: image_filename.to_string(),
        measurement: Measurement {
            scales,
            custom_unit_px_per_unit: BTreeMap::new(),
            travel_time_preset_ids: vec![],
            travel_days_enabled: false,
        },
        pin_size_overrides: BTreeMap::new(),
        grids: vec![],
        pan_clamp: true,
        draw_layers: vec![],
        drawings: vec![],
        second_screen: SecondScreen {},
        text_layers: vec![],
    };

    let json = serde_json::to_string_pretty(&data).expect("MarkersFile serialization is infallible");
    fs::write(path, json)
}
```

- [ ] **Step 2: Wire it into `main.rs`**

In `src/main.rs`, add `mod export;` to the module list at the top (after `mod elevation;`, keeping alphabetical order alongside the existing `mod` lines):

```rust
mod climate;
mod elevation;
mod export;
mod heatmap;
mod hydrology;
mod params;
mod render;
```

Then, after the `render::save_composite(...)?;` call and before the final `println!`, add:

```rust
    export::save_markers_json(
        WIDTH * render::RENDER_SCALE,
        HEIGHT * render::RENDER_SCALE,
        &params,
        "composite.png",
        "output/composite.markers.json",
    )?;
```

- [ ] **Step 3: Build to verify**

Run: `source "$HOME/.cargo/env" && cd /root/tools/mapgen && cargo build`
Expected: clean compile (same pre-existing `HydrologyResult` dead-code warning, nothing new — every new item is reachable from `main`).

- [ ] **Step 4: Run and inspect the output**

Run: `source "$HOME/.cargo/env" && cd /root/tools/mapgen && cargo run --release`
Expected: `Done — 9 layers written to output/` printed, and `output/composite.markers.json` exists.

Then inspect it: `cat output/composite.markers.json`
Expected: valid JSON matching the scaffold shape from the design spec, with:
- `"size": {"w": 3072, "h": 1536}`
- `"bases": ["composite.png"]`, `"activeBase": "composite.png"`
- `"layers": [{"id": "default", "name": "Default", "visible": true, "locked": false}]`
- `"measurement": {"scales": {"composite.png": 13024.33...}, ...}` — sanity check: `π × 6,371,000 / 1536 ≈ 13024.33`, matching Earth's real pole-to-pole scale at this resolution.
- `"markers": []`, `"overlays": []`, `"grids": []`, `"drawLayers": []`, `"drawings": []`, `"textLayers": []`, `"pinSizeOverrides": {}`, `"secondScreen": {}`, `"panClamp": true`

- [ ] **Step 5: Commit**

```bash
cd /root/tools/mapgen
git add src/export.rs src/main.rs
git commit -m "feat: export Obsidian zoom-map markers.json scaffold alongside composite render"
```

---

### Task 5: Manual end-to-end check in Obsidian

**Files:** none (manual verification only, no code changes)

- [ ] **Step 1: Copy output into the vault**

```bash
cp /root/tools/mapgen/output/composite.png "/root/ttrpg/Pipeline Plans/Testing/mapgen-composite.png"
cp /root/tools/mapgen/output/composite.markers.json "/root/ttrpg/Pipeline Plans/Testing/mapgen-composite.markers.json"
```

- [ ] **Step 2: Point a note at it**

This step is for the user to perform in the Obsidian UI (not scriptable): create or edit a `zoommap` code block in an Obsidian note to reference `Pipeline Plans/Testing/mapgen-composite.png` as its image base and `Pipeline Plans/Testing/mapgen-composite.markers.json` as its markers file (mirroring the structure seen in `Test Map.md`), then open it and confirm:
- The composite render displays correctly, no load errors.
- The plugin's ruler/measurement tool is active (not showing "no scale set") and reports plausible real-world distances when measuring across the map.

- [ ] **Step 3: Report back**

No commit for this task — it's a verification checkpoint. Report the outcome (works / doesn't work, and what if anything looked wrong) before considering this plan complete.

---

## Self-Review Notes

- **Spec coverage:** `radius_km` field (Task 1), serde deps (Task 2), `RENDER_SCALE` visibility (Task 3), `export.rs` module + scaffold shape + meters-per-pixel formula + `main.rs` wiring (Task 4), manual Obsidian verification (Task 5) — all spec sections covered. Non-goals (region export, configurable filename, merging into existing files) are correctly absent from the plan.
- **Placeholder scan:** no TBD/TODO; all code blocks are complete and copy-pasteable.
- **Type consistency:** `save_markers_json` signature matches its call site in Task 4 Step 2 exactly (`render_width, render_height, params, image_filename, path`). `PlanetGenParams::radius_km` (Task 1) is the only new field `export.rs` (Task 4) reads.
