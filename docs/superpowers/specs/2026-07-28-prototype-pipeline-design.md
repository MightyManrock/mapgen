# mapgen prototype: elevation + climate + hydrology pipeline

## Context

`mapgen` (`/root/tools/mapgen/`) is a standalone Rust planetary/world map
generator for TTRPG worldbuilding, scaffolded as a bare skeleton in an
earlier session (see `README.md`). It is deliberately independent of
`demiurge-rust` — no dependency, no shared code at runtime — but
`demiurge-rust`'s `src/planet_gen.rs` and `examples/heatmap_export.rs`
serve as a mature reference to port from, adapted rather than reused
wholesale.

This spec covers the first working prototype: enough of the generation
pipeline to produce a rendered planet, nothing more.

## Goals

- A single, fixed seed so repeated runs during iteration produce the same
  planet — a reproducible "control" to compare changes against.
- Earth-like conditions with humans as the (only, for now) habitability
  target — no per-species generality in the params model.
- Elevation, climate (temperature/precipitation/aridity/diurnal swing),
  and hydrology (rivers/lakes/glaciers/sea ice) — the physical-simulation
  layer, rendered as individual PNGs per layer.
- Since this is a one-and-done generator (not a runtime system), maps can
  eventually be much larger/more detailed than demiurge-rust's — but
  sizing that up is explicitly deferred; v1 uses the same resolution
  demiurge-rust used (1024×512).

## Explicit non-goals (for this prototype)

- Region detection, habitability scoring, or any per-species scoring.
- Settlements or political borders — likely permanently manual/out of
  scope for automatic generation, not just deferred.
- A composite/combined render — each layer renders to its own PNG for
  now; a composite comes later once individual layers are validated.
- CLI argument parsing / runtime configurability — seed, resolution, and
  all generation params are hardcoded constants.
- Raw data persistence (serialized heightmap/climate arrays) — the fixed
  seed makes regeneration cheap and deterministic, so there's nothing to
  cache yet.
- Automated tests — success is judged by eye on the rendered PNGs for
  this pass.

## Architecture

New modules under `src/`, each with one clear responsibility:

- **`heatmap.rs`** — the `HeatMap { width: usize, height: usize, data:
  Vec<f64> }` struct, ported as-is from demiurge-rust. Holds a single
  scalar field over the planet's surface, with `[0,1]`-normalized values
  except where a layer's semantics require otherwise (documented at each
  producer).
- **`params.rs`** — a `PlanetGenParams` struct: the trimmed set of fields
  the elevation/climate/hydrology functions actually consume (elevation
  warp strength, temperature baseline/gradient/lapse, precipitation
  moisture/decay/rain-shadow tuning, sea level, aquifer/river/glacier
  thresholds, axial tilt, rotation period — the exact field list is an
  implementation detail worked out during the port, not fixed here).
  Provides one constructor, `PlanetGenParams::earth_like(seed: u32)`,
  adapted from demiurge-rust's `PlanetParams::earth_like()` but dropping
  everything that constructor carries for game/species/atmosphere
  purposes mapgen doesn't need (no `AtmosphereTag` map, no per-species
  scoring fields).
- **`elevation.rs`** — spherical FBM elevation generation with
  domain-warped noise (ported from `generate_elevation`), including the
  ocean/glacier/sea-ice mask derivation (flood fill + threshold
  classification) that downstream climate/hydrology functions need.
- **`climate.rs`** — temperature (latitude + elevation lapse rate +
  season phase), precipitation (moisture transport with rain-shadow
  effect), aridity, and diurnal temperature swing. Ported from the
  corresponding `generate_*` functions in `planet_gen.rs`.
- **`hydrology.rs`** — rivers/lakes (flow accumulation), glacier
  presence, sea ice. Ported from `generate_hydrology` /
  `generate_glacier` / `generate_sea_ice`.
- **`render.rs`** — per-layer `HeatMap` → PNG color-ramp rendering, using
  the `image` crate. Color functions (`elevation_color`,
  `temperature_color`, `precipitation_color`, etc.) ported from
  `planet_gen.rs`'s color-ramp functions, one per layer this prototype
  produces.
- **`main.rs`** — orchestrates the pipeline in order: build
  `PlanetGenParams::earth_like(SEED)` → generate elevation + masks →
  generate climate layers → generate hydrology layers → render each
  layer to `output/`.

## Data flow

```
elevation (spherical FBM + domain warp)
  → ocean / glacier / sea-ice masks (flood fill + thresholds)
  → temperature (latitude + elevation lapse + season phase)
  → precipitation (moisture transport + rain shadow, needs temperature + masks)
  → aridity (needs temperature + precipitation)
  → hydrology: rivers/lakes/glacier-melt (needs elevation + precipitation + masks)
  → diurnal swing (needs temperature + aridity)
```

Same dependency order demiurge-rust uses; each stage only depends on
outputs already computed.

## Config

- `const SEED: u32 = 42;` in `main.rs` — the fixed control seed. Changing
  it produces a different planet on purpose; leaving it alone during
  iteration is what makes runs comparable.
- `const WIDTH: usize = 1024;` / `const HEIGHT: usize = 512;` — matches
  demiurge-rust's prior default. Revisit when we deliberately scale up
  map size later (explicitly deferred, not part of this prototype).
- All `PlanetGenParams` values come from `earth_like(SEED)` — no other
  preset needed since humans/Earth-like is the only target for now.

## Output

- Each generated layer renders to its own file under `output/`:
  `output/elevation.png`, `output/temperature.png`,
  `output/precipitation.png`, `output/aridity.png`,
  `output/diurnal_swing.png`, `output/hydrology.png` (rivers/lakes
  overlay), `output/glacier.png`, `output/sea_ice.png`.
- `output/` is added to `.gitignore` — generated artifacts never get
  committed.
- No composite render in this pass; that's a follow-up once individual
  layers are confirmed correct.

## Error handling

Generation is a linear, in-process pipeline over deterministic inputs —
there's no I/O to fail except writing PNGs to `output/`, which should
just create the directory if missing and propagate any write error via
`main`'s `Result` return (panic-free but not elaborately handled; a
failed write is a real bug to fix, not a recoverable runtime condition).

## Testing

None automated for this prototype. Validation is visual: run `cargo run`,
inspect the PNGs in `output/` by eye against expectations (plausible
coastlines, temperature gradient pole-to-equator, rain shadows behind
mountain ranges, etc.). Automated regression testing (e.g. a golden hash
of the fixed-seed output to catch accidental determinism breaks) was
considered and explicitly deferred — can be added later if regressions
become a real problem.
