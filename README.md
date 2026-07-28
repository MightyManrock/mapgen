# mapgen

Map generation program in Rust for TTRPG worldbuilding and integration into
the Obsidian `zoom-map` plugin, independent of the Demiurge game codebase.

## Status

Bare scaffold. Nothing is implemented yet.

## Relationship to demiurge-rust

`/root/games/demiurge-rust/` has its own planetary generation pipeline
(`src/planet_gen.rs`, `examples/heatmap_export.rs`) built for the Demiurge
game. `mapgen` is **not** a dependency of, or a library consumer of,
demiurge-rust — it's a fresh implementation for a different purpose (feeding
the Obsidian `zoom-map` plugin for TTRPG maps, not driving a game
simulation).

demiurge-rust's code is a reference for lessons learned, not a shared
codebase:
- `detect_regions()` (`planet_gen.rs:1528`) — flood-fill region detection
  from elevation/temperature/precipitation/aridity heatmaps plus
  ocean/glacier/sea-ice/salt-flat masks. Region growth is seed-relative
  (compared against the seed cell, not the frontier) to avoid drift across
  gradual transitions, and blocked across land/ocean boundaries.
- `Region::character()` — descriptive climate labels ("Temperate Rainforest",
  "Hot Desert Island") derived from region stats.
- `heatmap_export.rs` (~lines 373–438) — region centroid computation
  (circular mean on x, so antimeridian-spanning regions resolve correctly),
  currently only used for numeric ID label placement.
- No boundary/polygon tracing exists there — borders are only detected
  pixel-by-pixel for outline rendering, not traced into paths.
- No fantasy name generator exists there.

## Planned direction

See `/root/ttrpg/Pipeline Plans/World Location Generator Pipeline Plan.md`
for full context (origin, zoom-map plugin data model, open questions).
Rough shape, adapted for a standalone generator:

1. Own elevation/climate/hydrology generation (using the `noise` crate,
   following demiurge-rust's approach as a starting reference, not shared
   code).
2. Region detection + `.character()`-style descriptive labeling, rewritten
   fresh.
3. `markers.json` exporter matching the zoom-map plugin schema: per-region
   centroid + tooltip, normalized `[0,1]` to image dimensions.
4. Region outline tracing (marching-squares-style) for real `drawings[]`
   polygons, not just point markers.
5. `boundBase` layering support (composite/political/habitability renders,
   seasonal variants) so one vault note offers multiple toggleable lenses.
6. Settlement-layer markers with `minZoom`/`maxZoom` collapse.
7. Fantasy name generator (phonemic-set-based) to replace climate-descriptor
   labels with real place names — separate concern, design once the core
   generator exists.

## Commands

```bash
cargo build
cargo run
cargo test
cargo clippy
cargo fmt
```
