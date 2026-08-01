# mapgen: worldgen roadmap

State as of 2026-07-31. Consolidates the deferred items from the specs in
`docs/superpowers/specs/`, with the measurements behind each and the ordering
constraints between them.

## Where things stand

Five changes landed today, in this order, each measured rather than eyeballed:

| commit | change |
|---|---|
| `775a58b` | target land fraction, elevation normalization, physical seasons |
| `abb8951` | monotonic sequential colour ramps, meridional ocean-current smoothing |
| `a66b048` | per-kilometre moisture decay and land recycling |
| `e2e9e8c` | precipitation→colour ramp gamma |
| `c94fc38` | tunable continent scale, defaulting to Earth-like |

Cumulative effect across 8 seeds at equinox:

```
                          before        now      Earth
land fraction         36.0-65.5%  30.4-31.6%       ~29%
beyond Earth's extreme     18.0%        2.6%         0%
major landmasses             1.8       3-5      ~5
largest % of land          90.6%       58.0%       ~57%
mean land precipitation    0.138       0.278          —
desert / forest      50.2 / 8.0%  43.7 / 13.0%  ~33 / ~31%
```

Four bugs were found and fixed that nobody was looking for: the lapse rate
read raw elevation (deep ocean read warmer than shallow), seasons were applied
to unsigned latitude (the whole planet shared one season), `season_phase` meant
different things in two functions, and the ocean-current bias streaked because
its coastline search had no north–south coupling.

## Next, in order

The order matters — each item changes the inputs the next one is tuned against.

### 1. Rain shadow activation

Currently **inert by design**: `slope_threshold` is 0.023 while the p95 per-cell
land gradient is 0.0186, so orographic loss measures ~0% of all moisture loss.
Every unit of overland moisture loss is distance decay.

Needs `slope_threshold` around 0.005 **and a windward gain term** — the sweep
currently only ever subtracts, so mountains can dry a leeward side but never wet
a windward one.

This is what unlocks **temperate rainforest**, which is currently structurally
impossible at every latitude outside the tropics (0% of land could reach the
threshold even at saturated moisture). Real temperate rainforest — Pacific
Northwest, Chile, Tasmania — is orographic. Creating it by lowering a threshold
instead would be inventing the biome rather than generating it.

Continent fragmentation already raised p95 land gradient 0.0108 → 0.0186, so
this starts from a better place than it would have.

### 2. Re-derive biome thresholds

A full spec exists — `2026-07-31-biome-threshold-calibration-design.md` — but is
marked DEFERRED and **its numbers must not be reused**. It was written against
the pre-fragmentation geology. Prototyped against the current continents it
gives desert 14.0% / forest 36.9%, overshooting Earth in the opposite direction.

The method in it is still right: pick an explicit mm/yr scale for what
precipitation 1.0 means, then derive every threshold from published biome
rainfall ranges, rather than letting the climate model and the classifier drift
onto different implicit scales. Re-measure and re-derive after the rain shadow
lands, since that changes the precipitation distribution again.

Fold in the **tundra fix** here. Tundra is structurally impossible for reasons
independent of geology: the polar precipitation ceiling is 0.053–0.097 against a
0.20 threshold, so no polar cell can ever be anything but Polar Desert. Our
polar ceiling is roughly correct (Earth's poles get ~0.075 of equatorial); the
threshold is ~3x too high.

### 3. Smaller items

- **`roughen_coastline` constants.** `AMPLITUDE: 0.08` and `BANDWIDTH: 0.05` are
  absolute elevation values that were never rescaled when normalization fixed
  land relief at 0.5. Coastline detail is ~1.5x smoother relative to relief than
  it used to be. Retune to ~0.12 / ~0.077 if coastlines read as too smooth.
- **Sea ice extent.** 1.3% of ocean at equinox, 8.3% at solstice, against
  Earth's ~5–8%. Plausible but never deliberately calibrated.
- **`cargo fmt`.** The repo is not fmt-clean at baseline — `regions.rs` and
  `render.rs` alone carry 45 diffs in code untouched by any of this work. A
  formatting pass is a separate, deliberate decision; running it inside a
  feature change buries the change.

## Explicitly not a suspect

**`lat_band_factor` is correctly shaped.** This was investigated as the
suspected cause of the desert-heavy biome mix and *refuted by measurement*.
Three prototype curves: reshaping toward Earth's zonal profile made forest
*worse* (8.0% → 7.5%) while weakening deserts; removing the band entirely
tripled forest but destroyed the desert belt. Its ceilings match Earth's
latitudinal shape within 6–12% (mid-latitude/equator ratio 0.478 vs ~0.45,
polar 0.066 vs ~0.075).

Do not reshape it without new evidence.

## Measurement harnesses

All in `examples/`, all reusable. These exist because in this work the
measurement usually contradicted the hypothesis.

| harness | answers |
|---|---|
| `stats` | land fraction, landmass size distribution, polar bridging |
| `glacier` | temperature, glacier and sea ice extent; `-- seasons` for the year |
| `biomes` | biome mix by land area, and whether the desert belt survives a change |
| `ceiling` | per-latitude precipitation ceiling vs the threshold each biome needs — finds *structurally impossible* biomes |
| `continentality` | distance from land to ocean, against Earth's known extreme |
| `fragment` | continent scale sweep; `-- render` writes candidate terrains |

`ceiling` and `continentality` are the two that changed the direction of the
work, and are worth reaching for early.

## Standing lesson

Twice a plausible hypothesis was wrong and measurement caught it: the "land
blocks wind too much" theory (mountains block ~nothing; the real causes were a
per-cell decay constant and absent moisture recycling), and the
"`lat_band_factor` caps precipitation" theory. Both would have produced
confident, wrong changes.

Judge a biome mix against the geology that produced it, not against Earth's,
until the geology itself is Earth-like.
