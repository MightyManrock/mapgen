# mapgen: per-kilometre moisture decay and land recycling

## Context

Continental interiors generate as near-desert regardless of climate zone.
Attribution of the westerly moisture sweep found that **100% of moisture
loss over land is distance decay** — orographic loss measures 0.0%, since
`slope_threshold` sits above the p95 per-cell land gradient — against mean
upwind fetches of 4,100–11,400 km.

Two distinct defects produce that.

**1. `land_decay` is applied per cell, but cells are not a fixed distance.**
On an equirectangular grid a cell is `39.1 · cos(lat)` km wide at
width 1024 — 39.1 km at the equator, 22.4 km at 55°. A fixed per-cell
retention therefore consumes moisture ~1.75× faster *per kilometre* in
midlatitudes than at the equator. Measured over interior land (>1500 km
from the upwind coast), mean moisture falls monotonically with latitude:

```
                        0-20   20-40  40-60  60-90
per-cell decay (today)  0.190  0.145  0.118  0.037
```

That gradient is not climate. It is grid geometry, and it lands precisely
on the midlatitude continental interiors this work is meant to fix.

It also makes climate resolution-dependent: rendering at width 2048 halves
the km per cell and silently dries the entire planet.

**2. Moisture never regenerates over land.** `carry` is set to full only at
ocean cells and decays monotonically thereafter, so an airmass crossing a
continent asymptotes to zero. On Earth roughly a third of continental
precipitation is recycled evapotranspiration — the mechanism that keeps the
Amazon and Congo interiors wet rather than desert.

## Goal

Make overland moisture decay a function of physical distance, and let land
return moisture to the airmass, so interior precipitation is set by climate
rather than by grid resolution and raw fetch.

Explicitly **not** in scope: rain-shadow activation (still needs
`slope_threshold` near 0.004–0.006 plus a windward gain term), continent
size/frequency, and the precipitation→colour ramp gamma. All remain in the
deferred list from the land-fraction spec.

## Architecture

Both changes are confined to the two row sweeps in `generate_precipitation`
(`src/climate.rs`). No signatures change; no other pipeline stage is
touched.

### Per-kilometre decay

`land_decay` is **redefined** as the fraction of moisture retained per
1000 km of overland travel, and applied as:

```rust
let km = EQUATOR_KM_PER_CELL * cos_lat(y);   // 40030 / width, times cos(lat)
let decay = params.land_decay.powf(km / 1000.0);
```

Redefining the unit rather than keeping it cell-relative is deliberate:
a per-cell figure remains tied to the render width, so the same params would
produce a different climate at a different resolution. Per-1000-km is
self-documenting and resolution-independent.

The value changes `0.985` → `0.68`. That is `0.985^(1000/39.1)`, i.e. it
reproduces today's *equatorial* behaviour exactly; everything poleward of
the equator gets wetter, which is the fix.

The decay factor is computed once per row, not per cell — it depends only
on latitude.

### Land moisture recycling

The airmass relaxes toward a local equilibrium instead of toward zero:

```rust
let eq = params.land_recycle_floor * temperature.data[idx];
carry = eq + (carry - eq) * decay;
carry = (carry - elev_gain * params.slope_loss).max(0.0);
```

`land_recycle_floor: 0.25`, scaled by local temperature so warm land
sustains an airmass while cold or glaciated land does not — the Amazon
versus Siberia distinction. Scaling by raw temperature rather than by
`moisture_capacity` (`0.3 + 0.7·T`) is intentional: the raw field spans the
full [0,1] and so differentiates hot from cold land far more strongly.

**Relaxation, not an additive term.** `carry += recycle` would need a
separate cap to stop moisture growing without bound across a long fetch;
relaxation is unconditionally stable, because `carry` provably cannot rise
above `eq` — the recycling floor *is* the ceiling on what land alone can
sustain. This matters given fetches over 11,000 km.

Rain-shadow loss still subtracts after the relaxation, so orographic
behaviour is unchanged.

Both sweeps (westerly and easterly) get identical treatment.

### `src/params.rs`

| param | change |
|---|---|
| `land_decay` | `0.985` → `0.68`, **units redefined** to per-1000-km |
| `land_recycle_floor: f64` | **new**, default `0.25` |

## Expected results

Prototyped over seeds 42, 7, 123, 2024 at equinox. Mean moisture by
distance from the upwind coast:

```
km inland:            <250   <1k    <2k    <3k    5k+
current               0.919  0.674  0.482  0.305  0.040
per-km + recycle 0.25 0.949  0.787  0.633  0.473  0.186
```

Interior land (>1500 km inland) by latitude — the monotonic falloff
flattens, which is the grid artifact going away:

```
                        0-20   20-40  40-60  60-90
current                 0.190  0.145  0.118  0.037
per-km alone            0.195  0.175  0.209  0.209
per-km + recycle 0.25   0.334  0.293  0.289  0.250
```

Mean land moisture 0.298 → 0.437.

Note the split of responsibilities: per-km decay fixes the *latitude* bias
(0.037 → 0.209 at high latitude) and barely touches the deep interior
(0.040 → 0.056); recycling fixes the *fetch* problem (0.056 → 0.186). Both
are needed; neither substitutes for the other.

## Testing

Unit tests in `climate.rs`:

- **Resolution independence:** the same world generated at two widths
  produces comparable mean land precipitation. This is the regression for
  defect 1 and fails outright under per-cell decay.
- **Latitude neutrality:** on an all-land band with uniform temperature, an
  equatorial row and a 60° row lose comparable moisture per *kilometre*
  travelled. Also fails under per-cell decay.
- **Recycling floor is a floor:** over a long land fetch, moisture converges
  to `land_recycle_floor · temperature` rather than to zero.
- **Recycling floor is a ceiling:** starting an airmass drier than the floor,
  moisture rises toward but never exceeds it — the stability property that
  justifies relaxation over an additive term.
- **Cold land does not recycle:** at temperature ~0, interior moisture still
  decays to ~0, so glaciated interiors stay dry.

Integration, via `examples/stats.rs` and a rerun of the full pipeline:

- Mean land precipitation rises from 0.172 toward ~0.25.
- **The subtropical desert belt must survive.** Predicted interior
  precipitation there is 0.059 against a 0.15 "Temperate Desert" threshold,
  but this is the main risk of the change and must be measured, not
  assumed — check the biome mix, not just mean precipitation.
- Deep-interior land is no longer uniformly at the ramp's dry end.

Visual: render seeds 42, 7, 123 and confirm continents show a
coast-to-interior gradient rather than a uniform tan field, and that
deserts still appear at ~30° and in rain-shadowed/polar positions.

## Deferred

Unchanged from `2026-07-31-target-land-fraction-design.md`: rain-shadow
activation, continent size/frequency, precipitation→colour ramp gamma.
