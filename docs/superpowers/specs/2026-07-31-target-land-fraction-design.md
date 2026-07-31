# mapgen: target land fraction, elevation normalization, and physical seasons

## Context

Testing across seeds showed continental climates skewing far too dry.
Measurement traced this to the elevation field rather than the climate
model, plus two latent bugs in `generate_temperature`.

`generate_elevation` normalizes the FBM field to `[0, 1]` by its actual
min and max, and `sea_level` is a fixed `0.525`. Because min/max
normalization is driven by two single outlier cells (the deepest trench
and the highest peak), a fixed threshold lands at a different quantile
on every seed. Measured land fraction across 8 seeds:

```
seed | land% (area) | largest mass | mean land precip | land < 0.10
  42 |         36.0 |        29.1% |            0.128 |      62.1%
   7 |         64.9 |        64.9% |            0.068 |      84.7%
  99 |         55.1 |        54.9% |            0.076 |      83.7%
 123 |         40.0 |        39.5% |            0.197 |      47.0%
2024 |         65.5 |        65.5% |            0.082 |      82.8%
 555 |         45.8 |        42.5% |            0.108 |      69.9%
   8 |         42.2 |        42.1% |            0.197 |      44.4%
 314 |         53.5 |        53.5% |            0.090 |      78.9%
```

Earth is 29% land. Every seed overshoots, and dryness tracks land
fraction almost perfectly — seed 7 at 65% land has 85% of its land under
precipitation 0.10, while seed 123 at 40% has 47%. Land fraction is the
dominant driver, and the seed-to-seed swing is the reason results feel
inconsistent.

Attribution of moisture loss in the westerly sweep confirmed the
mechanism is fetch, not blocking: orographic loss contributes **0.0%** of
total moisture loss (`slope_threshold: 0.015` sits above the p95
per-cell land gradient of ~0.007), while mean upwind fetch over land runs
4,100–11,400 km.

Two further bugs surfaced while calibrating:

1. `generate_temperature` applies the lapse rate to **raw** elevation, so
   ocean cells are cooled in proportion to their depth — a deep trench
   reads warmer than a shallow shelf.
2. Both `generate_temperature` and `generate_precipitation` apply
   `season_offset` to `abs_lat`, which is unsigned. Both hemispheres
   therefore shift the same direction: the world has a global summer and
   a global winter instead of opposed hemispheres. A consequence is that
   neither pole ever reaches the cold end of the latitude curve, leaving
   `sea_ice_temp_threshold: 0.14` unreachable — the 0.3% sea ice observed
   at baseline exists only as an artifact of bug (1).

The two functions also disagree on the units of `season_phase`:
`generate_temperature` uses `(phase * 2π).cos()` (fraction of year),
`generate_precipitation` uses `phase.cos()` (radians). Invisible today
because both are called with `0.0`.

## Goal

Make land fraction an explicit, seed-stable parameter; put the elevation
field on a fixed scale so absolute thresholds mean the same thing on
every seed; and correct the lapse and seasonal-latitude bugs so
temperature is physical. This is the foundation pass — later changes to
moisture decay and continent fragmentation will be tuned against it.

Explicitly **not** in scope: activating the rain shadow, per-kilometer
moisture decay, land moisture recycling, and continent size/frequency.
Those are separate changes, sequenced after this one.

## Architecture

### `weighted_sea_level(data, target_land) -> f64` (`src/elevation.rs`)

Solve for the elevation threshold that yields the requested land
fraction, weighting each cell by `cos(lat)` so the equirectangular grid's
polar oversampling does not bias the result. Sort cells by elevation,
accumulate weight, return the value at which `1 - target_land` of the
total weighted area lies below.

Single-pass on the raw field. `roughen_coastline` and `flood_fill_ocean`
run afterward and cause the realized fraction to drift, but measurement
shows the drift is small (+0.1 to +1.9 points, nearly all of it from
`flood_fill_ocean` classifying disconnected below-sea-level basins as
land). Not worth bisecting; `target_land_fraction` is documented as a
target, not a guarantee.

### `normalize_about_sea_level(data, sea_level)` (`src/elevation.rs`)

Piecewise-linear remap so sea level always sits at exactly 0.5:

```
[p0.1,     sea_level] -> [0.0, 0.5]
[sea_level, p99.9]    -> [0.5, 1.0]
```

Both tails clamped. Anchored on quantiles rather than min/max, because
min and max are each a single outlier cell and would reintroduce the
instability this is meant to remove.

Runs at the end of `generate_elevation`, before `roughen_coastline`, so
every downstream consumer sees a field with a fixed sea level. No
downstream signatures change: `render.rs` and `hydrology.rs` already
normalize relative to `params.sea_level`, which simply becomes the
constant 0.5.

This is what makes the field's absolute thresholds meaningful. Land
relief currently spans `1 - sea_level`, i.e. 0.25–0.40 depending on seed,
so a single tuned value means different things on different seeds.
Measured p95 land gradient spread tightens from 1.47x to 1.21x after
normalization.

### `params.rs`

| param | change |
|---|---|
| `target_land_fraction: Option<f64>` | **new**, default `Some(0.30)` |
| `sea_level` | now derived: set to 0.5 after normalization. Used literally only when `target_land_fraction` is `None` |
| `lapse_factor` | `0.30` → `0.70` |
| `max_lake_fill` | `0.040` → `0.062` |
| `slope_threshold` | `0.015` → `0.023` |

`target_land_fraction` is an `Option` so an explicit sea level remains
available for worlds where a specific value is wanted; `None` preserves
today's behavior exactly.

`max_lake_fill` and `slope_threshold` scale proportionally with land
relief (mean scale factor 1.539), preserving current behavior under the
new field scale. Note that `slope_threshold: 0.023` remains **above** the
post-normalization p95 land gradient of ~0.011, so the rain shadow stays
inert. That is intentional here — this change should not alter orographic
behavior. Activating it belongs to the later rain-shadow change, which
will need a value near 0.004–0.006 plus a windward gain term.

`lapse_factor` is recalibrated because the lapse reference changes. The
old coefficient acted on raw elevation averaging ~0.7 over land; the new
one acts on height-above-sea-level averaging ~0.24. Sweep across 8 seeds:

```
lapse_factor | mean land T | glacier% of land
        0.30 |       0.802 |              0.3
        0.60 |       0.705 |              0.7
        0.70 |       0.673 |              1.1
        0.80 |       0.642 |              2.6
        1.00 |       0.585 |              7.2
```

0.70 holds temperature within noise of the 0.681 baseline.

### `generate_temperature` (`src/climate.rs`)

Two changes.

**Lapse from height above sea level:**

```rust
let above = ((elevation.data[idx] - params.sea_level).max(0.0)
             / (1.0 - params.sea_level)) * params.lapse_factor;
lat_temp - above
```

Ocean cells now contribute zero lapse instead of being cooled by depth.

**Signed seasonal latitude.** Replace the `abs_lat` shift with a subsolar
latitude the hemispheres are measured against:

```rust
// Subsolar latitude in normalized units, 1.0 == pole. Full axial tilt:
// the subsolar point genuinely swings the whole ±tilt range.
let subsolar = (params.axial_tilt / 90.0) * (season_phase * TAU).cos();
let signed_lat = (y as f64 / height as f64 - 0.5) * 2.0;
let eff = (signed_lat - subsolar + jitter).abs().clamp(0.0, 1.0);
```

Temperature uses the **full** tilt. The existing `* 0.5` factor is
documented in `generate_precipitation` as a Hadley-cell migration proxy —
appropriate for wind belts, but it was copied into the temperature
function where it does not belong.

**Sign convention**, stated explicitly since this is easy to invert:
`signed_lat` is `-1` at `y = 0` and `+1` at `y = height - 1`, matching
`generate_elevation`, which maps `y = 0` to latitude `-π/2`. So high `y`
is north. At `season_phase = 0` the subsolar latitude is positive, which
must warm the high-`y` hemisphere — consistent with the existing
"phase 0 = northern summer" comment.

Note the jitter term now operates in signed space rather than being added
to `abs_lat`. This is what `lat_jitter` already computes internally, so
the two are consistent for the first time; hemispheres remain
decorrelated as before.

### `generate_precipitation` (`src/climate.rs`)

Apply the same signed-latitude treatment to `lat_band_factor` and
`westerly_weight`, so the ITCZ and wind belts migrate into the summer
hemisphere rather than toward both poles at once. Retain the `* 0.5`
Hadley proxy on the amplitude.

Change `season_offset` to use `(season_phase * TAU).cos()`, unifying on
fraction-of-year. `season_phase` means the same thing in both functions
after this.

### `main.rs`

Default `season_phase` becomes `0.25` (equinox) for both temperature and
precipitation. With hemispheres now opposed, a solstice default would
render one pole heavily glaciated and the other bare. Equinox gives a
symmetric basemap; solstice variants remain available for the seasonal
layers on the roadmap.

## Expected results

Measured from a prototype of the full change, 8 seeds, at equinox:

| metric | baseline | after |
|---|---|---|
| land fraction | 36.0–65.5% | 30.3–32.4% |
| mean land precipitation | 0.138 | 0.170 |
| mean land temperature | 0.681 | 0.600 |
| glacier % of land | 1.5 | 6.4 |
| sea ice % of ocean | 0.3 | 1.1 |

At solstice, glacier reaches 16.5% of land and sea ice 8.3% of ocean,
against Earth's roughly 10% and 5–8%. The two solstices differ (16.5 vs
13.2) — that asymmetry is the hemispheres being genuinely opposed,
reflecting where land sits on a given seed.

The precipitation gain is real but modest: +23% at the default equinox
phase. Part of the gain from land fraction alone (0.138 → 0.206) is given
back once seasons work, because more ice means less evaporation. The
large precipitation wins remain in the later per-kilometer decay,
moisture recycling, and continent fragmentation changes. This change's
primary value is a seed-stable land fraction and a temperature field
that is physically correct enough to tune those against.

## Testing

The measurement harness used throughout this design lives in
`examples/` (`stats.rs`, `wind.rs`, `drift.rs`, `norm.rs`, `glacier.rs`,
`season.rs`). Retain `stats.rs` and `glacier.rs` as regression harnesses;
the others were single-question probes and can be dropped.

Unit tests:

- `weighted_sea_level` on a synthetic field with known distribution
  returns the expected quantile; a uniform field with `target = 0.30`
  returns the 0.70 value.
- `weighted_sea_level` weights by latitude: a field that is entirely high
  near the poles and low at the equator yields a different threshold than
  the unweighted quantile.
- `normalize_about_sea_level` maps `sea_level` to exactly 0.5, keeps
  ordering monotonic, and clamps both tails into `[0, 1]`.
- `generate_temperature` at `season_phase = 0.0` produces a warmer
  northern hemisphere than southern at equal `abs_lat`; at `0.5` the
  reverse. This is the regression test for the season bug.
- Ocean cells at differing depths produce identical temperature at equal
  latitude. This is the regression test for the lapse bug.

Integration check: run `stats.rs` and `glacier.rs` across the 8 seeds and
confirm land fraction lands in 30–32% on every seed and the table above
reproduces.

Visual check: render seeds 42, 7, and 123 and confirm coastlines,
ice caps, and biome distribution read as plausible — the numbers can be
right while the map looks wrong.

## Deferred

Found during this work, deliberately not addressed:

- Rain shadow is inert; needs `slope_threshold` near 0.004–0.006 and a
  windward moisture-gain term.
- `land_decay` is applied per cell, but cell width is `39.1·cos(lat)` km —
  moisture decays ~1.75× faster per kilometer at 55° than at the equator,
  which lands precisely on continental climates.
- No land moisture recycling: `carry` is only ever reset at ocean cells
  and decays monotonically inland.
- Base FBM frequency (~3.5 wavelengths at the equator) produces very
  large landmasses; at the new 30% land fraction this may want raising to
  fragment continents further.
- `biome_base_color` lerps linearly on precipitation over `[0, 1]`, but
  land precipitation occupies roughly `[0, 0.35]`, so land renders far
  toward the dry end of the ramp regardless of classification.
