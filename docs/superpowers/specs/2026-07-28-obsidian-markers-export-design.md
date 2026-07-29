# mapgen: Obsidian markers.json scaffold export

## Context

The composite terrain render (`output/composite.png`) is meant to be dropped
into an Obsidian vault and viewed with the `zoom-map` community plugin. That
plugin stores per-map metadata (image bindings, measurement scale, markers,
drawings, etc.) in a sidecar `<name>.markers.json` file next to the note.

The user manually created a test map in Obsidian
(`/root/ttrpg/Pipeline Plans/Testing/test_map.markers.json`) to explore the
plugin's JSON format. Inspecting that file plus the plugin's own source
(`/root/ttrpg/.obsidian/plugins/zoom-map/main.js`) surfaced the plugin's
`MarkerStore.ensureExists()` method — the exact object it writes when it
creates a fresh markers file for a new map:

```json
{
  "size": {"w": 0, "h": 0},
  "layers": [{"id": "default", "name": "Default", "visible": true, "locked": false}],
  "markers": [],
  "bases": ["<path>"],
  "overlays": [],
  "activeBase": "<path>",
  "measurement": {
    "scales": {},
    "customUnitPxPerUnit": {},
    "travelTimePresetIds": [],
    "travelDaysEnabled": false
  },
  "pinSizeOverrides": {},
  "grids": [],
  "panClamp": true,
  "drawLayers": [],
  "drawings": [],
  "secondScreen": {},
  "textLayers": []
}
```

This spec adds a mapgen export step that produces exactly this shape, with
`size`/`bases`/`activeBase`/`layers[0]` filled in from the render, and
`measurement.scales` pre-populated with a computed real-world scale — so the
composite render can be dropped straight into a vault with no manual JSON
editing. Region/marker auto-generation is explicitly out of scope (deferred,
tracked separately, guides future region-detection work).

## Goal

After `save_composite` writes `output/composite.png`, also write
`output/composite.markers.json`, valid input to `zoom-map`, requiring no
manual edits beyond placing both files in the vault (renaming `composite.png`
first if the user wants a different filename — see Non-goals).

## Architecture

**`src/params.rs`**: add `radius_km: f64` to `PlanetGenParams`. `earth_like()`
sets it to `6371.0` (Earth's mean radius). Not consumed by any generator —
purely a physical-scale annotation for export.

**`Cargo.toml`**: add `serde = { version = "1", features = ["derive"] }` and
`serde_json = "1"`.

**New module `src/export.rs`**:

```rust
pub fn save_markers_json(
    render_width: usize, render_height: usize,
    params: &PlanetGenParams, image_filename: &str, path: &str,
) -> std::io::Result<()>
```

Builds the fresh-file scaffold above via serde-derived structs matching the
plugin's field names/order, and writes it pretty-printed (`serde_json`'s
`to_string_pretty`, matching the plugin's own `JSON.stringify(data, null, 2)`
formatting) to `path`.

Field values:
- `size` = `{w: render_width, h: render_height}` — the composite image's
  actual pixel dimensions (3072×1536 at current settings), not the base
  1024×512 simulation grid.
- `layers` = `[{id: "default", name: "Default", visible: true, locked: false}]`
  (matches the plugin's own default; no `boundBase` field — omitted via
  `#[serde(skip_serializing_if = "Option::is_none")]`, since it's optional
  and only meaningful with multiple bases).
- `markers` = `[]`, `overlays` = `[]`.
- `bases` = `[image_filename]`, `activeBase` = `image_filename`.
- `measurement.scales` = `{image_filename: meters_per_pixel}` (see formula
  below); other `measurement` fields match the plugin's fresh-file defaults
  (`customUnitPxPerUnit: {}`, `travelTimePresetIds: []`,
  `travelDaysEnabled: false`).
- `pinSizeOverrides` = `{}`, `grids` = `[]`, `panClamp` = `true`,
  `drawLayers` = `[]`, `drawings` = `[]`, `secondScreen` = `{}` (empty
  object, not `{showGrids: true}` — that field only appears once a user
  toggles it in the UI), `textLayers` = `[]`.

**Meters-per-pixel formula**:

```
meters_per_pixel = (PI * params.radius_km * 1000.0) / render_height as f64
```

Pole-to-pole distance (half the planet's circumference, `π × R`) divided by
the image's pixel height. This is the *vertical* scale, which is constant
across the whole equirectangular map. The *horizontal* scale is only equal
to this at the equator and shrinks by `cos(latitude)` moving poleward — the
plugin has no way to represent a latitude-varying scale (it stores one
scalar per base image), so the vertical value is the closest thing to a
single "correct" number: distances measured north-south read accurately
anywhere on the map, while east-west measurements increasingly overstate
real distance near the poles. This caveat is not encoded in the output
JSON (the plugin has nowhere to put it) — worth remembering when using the
in-app ruler at high latitudes.

**`src/main.rs`**: after the `save_composite` call, add:

```rust
export::save_markers_json(
    WIDTH * render::RENDER_SCALE, HEIGHT * render::RENDER_SCALE,
    &params, "composite.png", "output/composite.markers.json",
)?;
```

(`RENDER_SCALE` needs `pub` visibility if not already, to avoid a duplicate
`3` constant in `main.rs`.)

## Non-goals

- Region/marker auto-generation (coastlines, biome regions, settlements) —
  deferred, separate future work.
- Configurable output filename/vault path — always writes
  `output/composite.markers.json` referencing `"composite.png"`; if the user
  renames the image when copying it into their vault, they rename it in the
  JSON too (single `bases`/`activeBase`/`scales` key each, trivial manual
  edit).
- Merging into an *existing* markers.json (e.g. preserving hand-placed pins
  across regenerations) — this only ever writes a fresh scaffold. Overwriting
  a vault file that already has markers/drawings in it would destroy them;
  that's a user-workflow concern (don't overwrite your working vault copy
  with a freshly generated scaffold), not something this export step guards
  against.
- Any other measurement-scale representation (per-latitude, non-linear,
  etc.) — the plugin's schema only supports one scalar; this ports that
  constraint as-is.

## Testing

Visual/manual, consistent with the rest of the prototype:
1. `cargo run` produces `output/composite.markers.json` alongside the PNGs.
2. JSON is valid (`cargo run` succeeding with `?`-propagated
   `serde_json`/`io` errors is sufficient evidence; no separate linter).
3. Copy both files into the `/root/ttrpg` vault's `Pipeline Plans/Testing/`
   folder (or wherever the user chooses) and confirm Obsidian's `zoom-map`
   plugin opens the map without errors, shows the correct image, and the
   ruler tool reports plausible real-world distances.
4. Sanity-check the scale number by hand: `radius_km=6371` →
   `meters_per_pixel = π × 6,371,000 / 1536 ≈ 13,024` — a ~1536px-tall map
   spanning pole-to-pole at ~13 km/pixel matches Earth's actual scale.
