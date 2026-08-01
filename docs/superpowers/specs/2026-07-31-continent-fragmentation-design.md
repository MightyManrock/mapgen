# mapgen: tunable continent scale

## Context

Generated worlds are far more continental than Earth. Measured across 8 seeds,
**18% of land sits further from the ocean than Earth's most continental
point** — the Eurasian pole of inaccessibility at 2,645 km, which is desert.
On Earth that figure is 0% by definition. Median distance to ocean is 1,165 km
against a p90 of 3,362 km and a maximum of 6,439 km, 2.4x Earth's extreme.
Typically 1.8 landmasses hold 90.6% of all land in one mass.

This is the root cause of a biome mix that reads desert-heavy (50.2% desert,
8.0% forest). That mix is the *correct* climate for these continents — the
climate model is not miscalibrated, the geology is unlike Earth's. Attempting
to fix the symptom by recalibrating biome thresholds was specced and then
deferred for exactly this reason (see
`2026-07-31-biome-threshold-calibration-design.md`).

Two constants govern continent scale. `generate_elevation` samples FBM on a
sphere whose radius is set so the equator spans a fixed number of feature
wavelengths — hardcoded at 3.5 — and `warp_strength` (0.2) domain-warps the
sample coordinates.

## Goal

Make continent scale an explicit, tunable parameter, defaulting to an
Earth-like world, and spanning Pangaea through archipelago on request.

## Architecture

Two values in `src/params.rs`; `generate_elevation` gains one argument. No
other pipeline stage changes.

| param | change |
|---|---|
| `continent_wavelengths: f64` | **new**, default `5.5` (was hardcoded 3.5) |
| `warp_strength` | `0.2` → `0.65` |

Measured across 6 seeds, the pair spans:

```
wavelengths warp | beyond |  p50 |  p90 | masses | largest% | character
        2.5 0.20 |  24.8% | 1416 | 3760 |    1.2 |     97.5 | Pangaea
        3.5 0.20 |  19.0% | 1198 | 3409 |    1.8 |     90.6 | previous default
        5.5 0.65 |   1.9% |  602 | 1798 |    4.2 |     58.0 | Earth-like (new default)
        8.0 0.65 |   1.6% |  469 | 1554 |    7.5 |     42.6 | many continents
       11.0 0.65 |   0.0% |  331 |  955 |    9.5 |     24.6 | archipelago
       18.0 0.70 |   0.0% |  229 |  664 |   14.2 |     21.7 | island world
```

Earth for reference: ~5 major landmasses, largest ~57% of land area.

**The two interact and must be documented as a pair.** High warp only helps
above roughly 5 wavelengths — at 4.5 wavelengths, raising warp to 0.65 still
leaves 9.2% of land beyond Earth's extreme. Warp does not shrink continents;
it indents them, adding bays and peninsulas. At 5.5 wavelengths, raising warp
0.20 → 0.65 *increases* the largest landmass (51.7% → 58.0%) while *reducing*
continentality (3.3% → 1.9%).

Warp strengths up to 0.80 were rendered and inspected. The concern that strong
domain warping would produce a visibly swirled, unnatural terrain did not
materialise; coastlines stay plausible. 0.65 is chosen over 0.80 because 0.80
scatters noticeably more island clutter for a marginal metric gain.

## Expected results

Across 8 seeds at equinox, with **no changes to the climate model or the biome
classifier**:

| metric | before | after |
|---|---|---|
| beyond Earth's extreme | 18.0% | 2.6% |
| p50 distance to ocean | 1,165 km | 623 km |
| p90 distance to ocean | 3,362 km | 1,882 km |
| major landmasses | 1.8 | 4.2 |
| largest % of land | 90.6% | 58.0% |
| mean land precipitation | 0.232 | 0.278 |
| desert / forest / rainforest | 50.2 / 8.0 / 5.2% | 43.7 / 13.0 / 8.2% |

Forest rises from 8.0% to 13.0% purely from geology — no threshold was touched.

The subtropical desert belt survives at 81.2% desert (from 89.4%), so the
circulation-driven deserts remain intact while fetch-driven ones recede. That
is the intended distinction and the main risk to check.

p95 land gradient rises 0.0108 → 0.0186. Not enough to activate the rain
shadow on its own — `slope_threshold` is 0.023 — so that work still needs its
own threshold change, but it starts from a better place.

## Testing

Unit tests in `elevation.rs`:

- `continent_wavelengths` reaches the sampling radius: two different values
  produce different fields from the same seed.
- Determinism holds — same seed and params give an identical field.
- Land fraction is unaffected by continent scale, since
  `target_land_fraction` is solved after generation. Guards against the
  two knobs silently coupling.

Integration:

- `examples/continentality.rs`: beyond-Earth's-extreme falls under 5%.
- `examples/fragment.rs`: retained as the sweep harness that produced the
  table above, so the range can be re-derived if generation changes.
- `examples/biomes.rs`: desert share falls and forest share rises, with the
  subtropical belt still above 75% desert.

Visual: render seeds 42, 7, 123 and confirm several distinct continents with
plausible coastlines, not one blob and not scattered islands.

## Deferred

- **Rain shadow activation** — `slope_threshold` ~0.023 → ~0.005 plus a
  windward gain term. This is what unlocks temperate rainforest, which is
  orographic and currently impossible at any latitude.
- **Biome threshold calibration** — re-derive *after* this lands. Prototyping
  the deferred thresholds against fragmented continents gives desert 14.0% /
  forest 36.9%, which overshoots Earth in the opposite direction, confirming
  those numbers must be re-measured rather than reused.
- **Tundra is structurally unreachable** (polar ceiling 0.053–0.097 against a
  0.20 threshold) — independent of geology, to fix whenever the classifier is
  next touched.
