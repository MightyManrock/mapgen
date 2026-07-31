# mapgen: calibrate biome thresholds against an explicit rainfall scale

## Context

Half of all land classified as desert (50.2%) while temperate forest sat at
8.0% and tundra at exactly 0.0%, against Earth's roughly 33% arid, 31%
forest, 5–10% tundra. The initial hypothesis was that `lat_band_factor`
capped mid-latitude precipitation too aggressively.

**Measurement refuted that hypothesis.** Three prototype band curves:

```
                              forest   desert   subtropical belt desert
current                         8.0%    50.2%                    89.4%
Earth-shaped reshape            7.5%    50.9%                    79.9%
fill the 36-45 deg dip only     8.7%    49.5%                    88.6%
band removed entirely (=1.0)   22.3%    22.8%                    22.3%
```

Reshaping the curve toward Earth's zonal profile made forest *worse* while
weakening the deserts, because lowering the poleward peak costs more forest
in the 54–63° band than a shallower subtropical minimum gains. Removing the
band entirely does triple forest, but destroys the desert belt — so the band
is doing necessary work and any uniform lift simply trades one for the other.

The band's *shape* is in fact close to Earth's. Comparing ceilings
(`band × moisture_capacity`, i.e. precipitation at saturated moisture) as a
ratio to the equatorial ceiling:

| | mapgen | Earth |
|---|---|---|
| mid-latitude (50–55°) / equator | 0.478 | ~0.45 |
| polar / equator | 0.066 | ~0.075 |

Within 6% and 12%. The climate model reproduces Earth's latitudinal
precipitation shape.

The actual defect is in `Region::character()`. Its thresholds were chosen
independently of the climate model, against a different implicit idea of what
precipitation 1.0 means. The equatorial ceiling of 0.82 corresponds to roughly
2500 mm/yr, putting the model's scale at **1.0 ≈ 3000 mm/yr**. Against that,
every threshold sits roughly 2× too high:

| biome | real mm/yr | implies | `character()` used |
|---|---|---|---|
| desert | <250 | <0.083 | <0.15 |
| steppe | 250–500 | 0.083–0.167 | 0.15–0.35 |
| temperate forest | 700–1500 | 0.23–0.50 | 0.35–0.60 |
| tundra | 200–600 | 0.067–0.20 | ≥0.20 |

So terrain was systematically labelled a drier biome than its precipitation
warranted. Tundra was not merely rare but **structurally impossible**: the
polar ceiling is 0.053–0.097 against a threshold of 0.20, so no polar cell
could ever be anything but Polar Desert.

## Goal

Give the classifier an explicit, documented rainfall scale and derive every
threshold from it, so the labels agree with the climate the model actually
produces.

Explicitly **not** in scope: `lat_band_factor` (measured to be correctly
shaped — leave it alone), any climate change whatsoever, and the colour ramp
gamma, which was fitted empirically to the precipitation distribution and is
independent of labelling.

## Architecture

Confined to `Region::character()`'s helper in `src/regions.rs`. No climate
code is touched, so this cannot alter precipitation, and deserts cannot be
erased by construction — only relabelled.

### Documented scale constant

```rust
/// Annual rainfall, in mm, that a normalized precipitation of 1.0 represents.
///
/// Derived from the climate model rather than chosen: the equatorial
/// precipitation ceiling (`lat_band_factor` x `moisture_capacity` at
/// saturated moisture) is 0.82, and equatorial Earth receives ~2500 mm/yr.
///
/// Every threshold below is derived from this. If the climate model's
/// absolute scale changes, change this one constant and re-derive, rather
/// than nudging individual biome boundaries.
const PRECIP_FULL_SCALE_MM: f64 = 3000.0;
```

### Threshold table

Each boundary is the published annual rainfall for that biome transition,
divided by `PRECIP_FULL_SCALE_MM`:

| temperature band | boundary | mm/yr | threshold |
|---|---|---|---|
| polar (<0.20) | polar desert / tundra | 200 | 0.067 |
| boreal (<0.35) | cold desert / boreal forest | 300 | 0.100 |
| temperate (<0.55) | desert / steppe | 250 | 0.083 |
| | steppe / forest | 500 | 0.167 |
| | forest / rainforest | 2000 | 0.667 |
| subtropical (<0.70) | desert / mediterranean | 250 | 0.083 |
| | mediterranean / forest | 900 | 0.300 |
| tropical | desert / savanna | 250 | 0.083 |
| | savanna / dry forest | 1000 | 0.333 |
| | dry forest / rainforest | 2000 | 0.667 |

## Expected results

Measured across 8 seeds at equinox, by land area:

```
                    desert  steppe/savanna  forest  rainforest  tundra
current              50.2%      36.6%        8.0%      5.2%      0.0%
recalibrated         21.0%      44.7%       28.0%      4.8%      1.5%
Earth (approx)        ~33%       ~25%       ~31%       ~7%     ~5-10%
```

Forest reaches 28% against Earth's ~31%. Tundra becomes reachable for the
first time.

The subtropical desert belt survives: 45.8% of that belt still classifies as
desert against a 21.0% global land average, i.e. 2.2× the concentration of
land overall. That ratio is what makes it read as a desert belt, and it is the
check that matters — not the absolute figure.

Temperate rainforest remains near-unreachable, correctly. Real temperate
rainforest is orographic, produced by windward uplift, and the rain shadow is
still inert. It should appear only once the deferred rain-shadow work adds a
windward gain term — creating it by lowering a threshold would be inventing
the biome rather than generating it.

## Testing

Unit tests in `regions.rs` (the file currently has none):

- Every threshold equals its documented mm value over
  `PRECIP_FULL_SCALE_MM`, so the table and the constant cannot drift apart.
- **Tundra is reachable:** a cell at the measured polar ceiling (~0.08) with
  polar temperature classifies as Tundra, not Polar Desert. This is the
  regression for the structural-impossibility bug and fails against the old
  0.20 threshold.
- Thresholds within each temperature band are strictly increasing, so no
  biome is unreachable by construction.
- A subtropical cell at the measured belt precipitation (~0.107) still
  classifies as desert.

Integration, via `examples/biomes.rs`:

- Biome mix matches the table above.
- Subtropical belt desert concentration stays at least 2× the global land
  average.

Visual: render seeds 42, 7, 123 and confirm the regions layer shows a
plausible spread of biome labels rather than near-uniform desert, and that
the ~30° belt still reads arid.

## Deferred

Unchanged: rain-shadow activation (which is what unlocks temperate
rainforest), continent size/frequency. `lat_band_factor` is now explicitly
*not* on the list — it was measured to be correctly shaped.
