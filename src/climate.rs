use std::f64::consts::PI;

use noise::{Fbm, NoiseFn, Perlin};

use crate::heatmap::HeatMap;
use crate::params::PlanetGenParams;

/// Smooth latitude offset that breaks up otherwise razor-straight
/// circulation bands (`lat_band_factor`/`westerly_weight` are pure functions
/// of latitude with zero longitude variation). Sampled on a cylinder: the
/// x/y coordinates trace a closed circle of longitude (seamless in x), and
/// signed latitude enters as a slowly varying third axis — NOT a full 2D/3D
/// noise field (which would carve blob-shaped "islands" of shifted latitude
/// instead of a wavy boundary, since this codebase's low-frequency FBM is
/// tuned to produce continent-like blobs elsewhere on purpose). The small
/// z-scale preserves per-column coherence while decorrelating the two
/// hemispheres — without it the jitter is applied to `abs_lat` symmetrically,
/// making northern and southern band meanders exact mirror images.
///
/// Amplitude tuned empirically: below ~0.10 the effect was imperceptible;
/// an earlier 3D sphere-sampled noise field at 0.30 produced blob artifacts
/// (which prompted the longitude-circle redesign above); 0.18 with that
/// redesign gives a clear, coherent meander without looking chaotic.
const JITTER_AMPLITUDE: f64 = 0.18;

/// Wavelength parameter (radius on the sampling circle) for `lat_jitter`'s
/// FBM lookup; controls how many meander cycles fit around the planet.
const JITTER_WAVELENGTH: f64 = 1.5;

/// Scale on the signed-latitude z-axis of the jitter cylinder. Pole-to-pole
/// traversal moves ~0.94 noise units vs. the sampling circle's 1.5-unit
/// circumference — enough to decorrelate the hemispheres at mid-latitudes
/// while both converge on the shared z=0 slice at the equator (continuous,
/// no seam). If bands ever read as blobby rather than meandering, reduce
/// this before touching JITTER_AMPLITUDE.
const JITTER_LAT_SCALE: f64 = 0.3;

fn lat_jitter(x: usize, y: usize, width: usize, height: usize, fbm: &Fbm<Perlin>) -> f64 {
    let lon = x as f64 / width as f64 * std::f64::consts::TAU;
    // Radians, unlike `signed_lat`'s normalized [-1, 1] — this feeds the noise
    // sampling cylinder, not a latitude comparison.
    let lat_radians = (y as f64 / height as f64 - 0.5) * PI;
    let r = JITTER_WAVELENGTH / std::f64::consts::TAU;
    let sx = r * lon.cos();
    let sy = r * lon.sin();
    fbm.get([sx, sy, lat_radians * JITTER_LAT_SCALE]) * JITTER_AMPLITUDE
}

/// Signed latitude of row `y`, normalized so -1.0 is the y=0 pole and +1.0 the
/// y=height-1 pole.
///
/// The sign convention follows `generate_elevation`, which maps y=0 to latitude
/// -PI/2 — so **high y is north**. Everything seasonal depends on getting this
/// right: at `season_phase` 0 the subsolar latitude is positive and must warm
/// the high-y hemisphere (northern summer).
fn signed_lat(y: usize, height: usize) -> f64 {
    (y as f64 / height as f64 - 0.5) * 2.0
}

/// Subsolar latitude for a season, in the same normalized units as
/// `signed_lat` (1.0 == pole). Positive = sun overhead in the northern
/// hemisphere. `phase` is a fraction of the year: 0 = northern summer
/// solstice, 0.25 = equinox, 0.5 = southern summer solstice.
///
/// `amplitude_scale` exists because temperature and precipitation want
/// different excursions from the same tilt — see the call sites.
fn subsolar_lat(axial_tilt: f64, phase: f64, amplitude_scale: f64) -> f64 {
    (axial_tilt / 90.0) * amplitude_scale * (phase * std::f64::consts::TAU).cos()
}

/// Latitude cosine + elevation lapse rate, scaled by planet params.
///
/// `temp_baseline` sets the equatorial surface temperature [0,1]; `temp_gradient`
/// controls how steeply it drops toward the poles (1.0 = full drop to 0, 0.1 = nearly flat).
pub fn generate_temperature(elevation: &HeatMap, params: &PlanetGenParams, season_phase: f64, seed: u32) -> HeatMap {
    let width = elevation.width;
    let height = elevation.height;

    // Full axial tilt: the subsolar point genuinely swings the whole ±tilt
    // range. (`generate_precipitation` halves it as a Hadley-cell migration
    // proxy — appropriate for wind belts, but that factor was previously
    // copied here, where it does not belong.)
    let subsolar = subsolar_lat(params.axial_tilt, season_phase, 1.0);

    let jitter_fbm = Fbm::<Perlin>::new(seed.wrapping_add(20));

    let data = (0..width * height)
        .map(|idx| {
            let x = idx % width;
            let y = idx / width;
            let jitter = lat_jitter(x, y, width, height, &jitter_fbm);
            // Distance from the subsolar latitude, in signed space — so one
            // hemisphere warms while the other cools, rather than both
            // shifting the same direction as they did against `abs_lat`.
            let eff_lat = (signed_lat(y, height) - subsolar + jitter).abs().clamp(0.0, 1.0);
            let lat_shape = (eff_lat * std::f64::consts::FRAC_PI_2).cos();
            let lat_temp = params.temp_baseline * (1.0 - params.temp_gradient * (1.0 - lat_shape));
            // Lapse from height *above sea level*, not raw elevation. Against
            // raw elevation, ocean cells were cooled in proportion to their own
            // depth, making deep trenches read warmer than shallow shelves.
            let above_sea = ((elevation.data[idx] - params.sea_level).max(0.0)
                / (1.0 - params.sea_level))
                * params.lapse_factor;
            (lat_temp - above_sea).clamp(0.0, 1.0)
        })
        .collect();

    HeatMap { width, height, data }
}

/// Directional coastline search from an ocean cell along its own row.
/// Returns a signed raw bias in [-1, 1]: positive = nearest coastline is
/// to the west (this ocean cell sits on that landmass's *east* side —
/// warm, western-boundary-current analog), negative = nearest coastline
/// is to the east (cell sits on that landmass's *west* side — cool,
/// eastern-boundary-current analog). Falls off linearly from 1.0 at
/// distance 1 to 0.0 at `search_dist`. Zero if no coastline found within
/// the bound in either direction, or if both directions tie.
fn ocean_cell_bias(x: usize, y: usize, width: usize, is_ocean: &[bool], search_dist: usize) -> f64 {
    let row = y * width;
    let mut dist_w = None;
    for d in 1..=search_dist {
        let nx = (x + width - d) % width;
        if !is_ocean[row + nx] {
            dist_w = Some(d);
            break;
        }
    }
    let mut dist_e = None;
    for d in 1..=search_dist {
        let nx = (x + d) % width;
        if !is_ocean[row + nx] {
            dist_e = Some(d);
            break;
        }
    }
    match (dist_w, dist_e) {
        (Some(dw), Some(de)) => {
            if dw < de {
                1.0 - dw as f64 / search_dist as f64
            } else if de < dw {
                -(1.0 - de as f64 / search_dist as f64)
            } else {
                0.0
            }
        }
        (Some(dw), None) => 1.0 - dw as f64 / search_dist as f64,
        (None, Some(de)) => -(1.0 - de as f64 / search_dist as f64),
        (None, None) => 0.0,
    }
}

/// Per-cell raw current bias (before the latitude envelope), for both
/// ocean and land cells. Ocean cells use `ocean_cell_bias` directly. Land
/// cells inherit a fraction of the *nearest ocean cell's own bias* (same
/// sign, scaled down by linear falloff over `params.current_bleed_dist`)
/// — this bleeds "whatever the adjacent water's temperature signature
/// is" onto the coast, rather than re-deriving a sign from the land
/// cell's own perspective. `pub(crate)` so `render::save_ocean_currents`
/// can reuse it for the debug visualization without duplicating this
/// logic.
pub(crate) fn current_bias_raw(x: usize, y: usize, width: usize, is_ocean: &[bool], params: &PlanetGenParams) -> f64 {
    let idx = y * width + x;
    if is_ocean[idx] {
        return ocean_cell_bias(x, y, width, is_ocean, params.current_search_dist);
    }
    let row = y * width;
    let mut nearest: Option<(usize, usize)> = None;
    for d in 1..=params.current_bleed_dist {
        let wx = (x + width - d) % width;
        if is_ocean[row + wx] {
            nearest = Some((wx, d));
            break;
        }
    }
    for d in 1..=params.current_bleed_dist {
        let ex = (x + d) % width;
        if is_ocean[row + ex] {
            if nearest.is_none_or(|(_, nd)| d < nd) {
                nearest = Some((ex, d));
            }
            break;
        }
    }
    match nearest {
        Some((ox, d)) => {
            let ocean_bias = ocean_cell_bias(ox, y, width, is_ocean, params.current_search_dist);
            let falloff = 1.0 - d as f64 / params.current_bleed_dist as f64;
            ocean_bias * falloff
        }
        None => 0.0,
    }
}

/// Latitude envelope for current strength: ~0 at the equator (currents
/// there are zonal, not coastal-boundary-driven), peaking around 31°
/// (subtropical gyre latitudes), fading to ~0 by the poles (dominated by
/// different circulation). Same piecewise-linear-stops technique as
/// `lat_band_factor`/`westerly_weight` (kept as its own small duplicate,
/// not factored into a shared helper, matching this file's existing
/// precedent of those two functions also duplicating the same
/// interpolation loop rather than sharing one — this task doesn't touch
/// either of them). `pub(crate)` for the same reuse reason as
/// `current_bias_raw`.
pub(crate) fn current_lat_envelope(abs_lat: f64) -> f64 {
    let stops: &[(f64, f64)] = &[
        (0.00, 0.00),
        (0.15, 0.30),
        (0.35, 1.00),
        (0.55, 0.40),
        (0.75, 0.00),
        (1.00, 0.00),
    ];
    for i in 0..stops.len() - 1 {
        let (ta, va) = stops[i];
        let (tb, vb) = stops[i + 1];
        if abs_lat <= tb {
            let t = (abs_lat - ta) / (tb - ta);
            return va + (vb - va) * t;
        }
    }
    stops.last().unwrap().1
}

/// Applies the coastline-relative current bias to a base temperature
/// field. Returns a new field (does not mutate the input), same pattern
/// as `generate_aridity` taking existing fields and producing a derived
/// one.
pub fn apply_ocean_currents(temperature: &HeatMap, is_ocean: &[bool], params: &PlanetGenParams) -> HeatMap {
    let width = temperature.width;
    let height = temperature.height;
    let data = (0..width * height)
        .map(|idx| {
            let x = idx % width;
            let y = idx / width;
            let abs_lat = (y as f64 - height as f64 / 2.0).abs() / (height as f64 / 2.0);
            let raw = current_bias_raw(x, y, width, is_ocean, params);
            let bias = raw * current_lat_envelope(abs_lat) * params.current_temp_bias;
            (temperature.data[idx] + bias).clamp(0.0, 1.0)
        })
        .collect();
    HeatMap { width, height, data }
}

/// Atmospheric band function + row-sweep moisture advection + rain shadow.
///
/// Two moisture fields are accumulated via double-pass row sweeps (one for
/// westerlies, one for easterlies) and blended by a latitude-dependent
/// westerly weight. The double-pass handles the east-west seam: carry from
/// the end of each row's first pass seeds the second pass, so moisture
/// wraps around the globe correctly.
pub fn generate_precipitation(
    elevation: &HeatMap,
    is_ocean: &[bool],
    temperature: &HeatMap,
    is_sea_ice: &[bool],
    params: &PlanetGenParams,
    season_phase: f64,
    seed: u32,
) -> HeatMap {
    let width = elevation.width;
    let height = elevation.height;
    let n = width * height;
    // ITCZ and wind belts migrate with season. Half the tilt angle is a
    // reasonable proxy for the Hadley cell shift — the belts do not track the
    // subsolar point one-for-one, so unlike `generate_temperature` this keeps
    // the 0.5 scale.
    //
    // `season_phase` is a fraction of the year in both functions: 0 = northern
    // summer solstice, 0.25 = equinox, 0.5 = southern summer solstice. (This
    // function previously read it as radians while `generate_temperature` read
    // it as a fraction — invisible while both were called with 0.0, but at
    // phase 0.25 temperature sat at equinox while precipitation was still
    // near solstice.)
    let subsolar = subsolar_lat(params.axial_tilt, season_phase, 0.5);

    let mut moisture_west = vec![0.0f64; n];
    let mut moisture_east = vec![0.0f64; n];

    // Westerly sweep: wind from west, moisture moves east.
    // Scan x=0→width-1 twice; second pass starts with carry from the end of
    // the first, so x=0 correctly inherits moisture wrapping from x=width-1.
    for y in 0..height {
        let mut carry = 0.0f64;
        for pass_x in 0..(width * 2) {
            let x = pass_x % width;
            let idx = y * width + x;
            if is_ocean[idx] {
                // Sea ice dramatically reduces evaporation; open ocean = full moisture.
                carry = if is_sea_ice[idx] { params.sea_ice_evap_factor } else { params.precip_moisture };
            } else {
                let upwind_x = (x + width - 1) % width;
                let raw_gain = elevation.data[idx] - elevation.data[y * width + upwind_x];
                let elev_gain = (raw_gain - params.slope_threshold).max(0.0);
                carry = (carry * params.land_decay - elev_gain * params.slope_loss).max(0.0);
            }
            if pass_x >= width {
                moisture_west[idx] = carry;
            }
        }
    }

    // Easterly sweep: wind from east, moisture moves west.
    // Scan x=width-1→0 twice; second pass starts with carry from x=0
    // so x=width-1 correctly inherits moisture wrapping from x=0.
    for y in 0..height {
        let mut carry = 0.0f64;
        for pass_i in 0..(width * 2) {
            let x = (width - 1) - (pass_i % width);
            let idx = y * width + x;
            if is_ocean[idx] {
                carry = if is_sea_ice[idx] { params.sea_ice_evap_factor } else { params.precip_moisture };
            } else {
                let upwind_x = (x + 1) % width;
                let raw_gain = elevation.data[idx] - elevation.data[y * width + upwind_x];
                let elev_gain = (raw_gain - params.slope_threshold).max(0.0);
                carry = (carry * params.land_decay - elev_gain * params.slope_loss).max(0.0);
            }
            if pass_i >= width {
                moisture_east[idx] = carry;
            }
        }
    }

    // Cold air holds less moisture: this dampens precipitation at high latitudes
    // and high altitudes independently of the circulation band factor.
    // Range: 0.3 (arctic) → 1.0 (tropical), so even the coldest cells get some snowfall.
    let jitter_fbm = Fbm::<Perlin>::new(seed.wrapping_add(21));

    let data = (0..n)
        .map(|idx| {
            let x = idx % width;
            let y = idx / width;
            let jitter = lat_jitter(x, y, width, height, &jitter_fbm);
            // Same signed-space treatment as `generate_temperature`: the belts
            // migrate into the summer hemisphere rather than toward both poles
            // at once. Computed once and shared by both band functions.
            let eff_lat = (signed_lat(y, height) - subsolar + jitter).abs().clamp(0.0, 1.0);
            let w = westerly_weight(eff_lat);
            let moisture = moisture_west[idx] * w + moisture_east[idx] * (1.0 - w);
            let band = lat_band_factor(eff_lat);
            let moisture_capacity = (0.3 + 0.7 * temperature.data[idx]).clamp(0.3, 1.0);
            (band * (params.base_arid + moisture * (1.0 - params.base_arid)) * moisture_capacity).clamp(0.0, 1.0)
        })
        .collect();

    HeatMap { width, height, data }
}

/// Latitude precipitation factor based on Earth's general circulation bands.
/// Returns a [0, 1] multiplier applied before moisture weighting.
///
/// `eff_lat` is distance from the subsolar latitude in [0, 1], already
/// season-shifted and jittered by the caller.
fn lat_band_factor(eff_lat: f64) -> f64 {
    // Piecewise linear through calibrated breakpoints:
    //   equator: 1.0 (ITCZ)
    //   ~30°:    0.2 (subtropical desert)
    //   ~50°:    0.6 (mid-lat cyclone belt)
    //   ~60°:    0.65 (mid-lat peak)
    //   ~70°:    0.3 (sub-polar)
    //   ~90°:    0.1 (polar desert)
    let stops: &[(f64, f64)] = &[
        (0.00, 1.00),
        (0.17, 0.90),
        (0.33, 0.20),
        (0.50, 0.60),
        (0.65, 0.65),
        (0.78, 0.30),
        (1.00, 0.10),
    ];
    for i in 0..stops.len() - 1 {
        let (ta, va) = stops[i];
        let (tb, vb) = stops[i + 1];
        if eff_lat <= tb {
            let t = (eff_lat - ta) / (tb - ta);
            return va + (vb - va) * t;
        }
    }
    stops.last().unwrap().1
}

/// Fraction of moisture contributed by the westerly sweep vs easterly sweep.
/// 1.0 = pure westerlies, 0.0 = pure easterlies.
///
/// `eff_lat` is distance from the subsolar latitude in [0, 1], same as
/// `lat_band_factor`.
fn westerly_weight(eff_lat: f64) -> f64 {
    // Westerlies dominate in mid-latitudes (~35–65°, abs_lat ~0.4–0.72).
    // Easterlies dominate in tropics and polar regions.
    let stops: &[(f64, f64)] = &[
        (0.00, 0.00),
        (0.25, 0.10),
        (0.40, 0.70),
        (0.55, 1.00),
        (0.70, 0.70),
        (0.78, 0.10),
        (1.00, 0.00),
    ];
    for i in 0..stops.len() - 1 {
        let (ta, va) = stops[i];
        let (tb, vb) = stops[i + 1];
        if eff_lat <= tb {
            let t = (eff_lat - ta) / (tb - ta);
            return va + (vb - va) * t;
        }
    }
    stops.last().unwrap().1
}

pub fn generate_aridity(temperature: &HeatMap, precipitation: &HeatMap, et_factor: f64) -> HeatMap {
    let data = temperature.data.iter().zip(precipitation.data.iter())
        .map(|(&t, &p)| ((p - t * et_factor + et_factor) / (1.0 + et_factor)).clamp(0.0, 1.0))
        .collect();
    HeatMap { width: temperature.width, height: temperature.height, data }
}

/// Peak day→night temperature gap, in normalized-temperature units, at full
/// dryness on a hot, long-rotation cell. ~0.5 norm ≈ 35°C via `normalized_temp_to_celsius`.
const MAX_DIURNAL_SWING: f64 = 0.55;

/// Diurnal temperature swing field: the day→night gap per cell, in the same
/// normalized [0,1] units as the temperature field. Derived from the seasonal
/// temperature snapshot it is paired with, local moisture, and rotation period.
///
/// - Hotter cells swing more in absolute terms (`temp`).
/// - Drier cells swing more — low humidity means no thermal blanket at night.
/// - Longer planetary days heat and cool for longer, with diminishing returns (`sqrt`).
///
/// NOTE: the `aridity` field is inverted relative to its name — a HIGH value is
/// humid/wet and a LOW value is arid/dry (see `aridity_color`). Dryness, which
/// drives the swing, is therefore `1 - aridity` (the same moisture inversion the
/// `solvent_val` term in the scoring functions uses).
///
/// `temp_day = mean + swing/2`, `temp_night = mean - swing/2` (both clamped to [0,1]).
pub fn generate_diurnal_swing(temperature: &HeatMap, aridity: &HeatMap, params: &PlanetGenParams) -> HeatMap {
    let rotation_factor = params.rotation_period.sqrt();
    let data = temperature.data.iter()
        .zip(&aridity.data)
        .map(|(&temp, &moisture)| {
            let dryness = 1.0 - moisture;
            (MAX_DIURNAL_SWING * temp * dryness * rotation_factor).clamp(0.0, 1.0)
        })
        .collect();
    HeatMap { width: temperature.width, height: temperature.height, data }
}

/// Per-cell lake-effect halo strength in [0, 1]: 1.0 adjacent to a large
/// lake, falling off linearly to 0.0 at `lake_halo_dist`, 0.0 everywhere
/// else (including small lakes — a pond doesn't moderate its shoreline's
/// climate; `lake_effect_min_size` sets the cutoff in connected lake cells).
///
/// This is the one-way lake→climate pass: hydrology consumes the *base*
/// precipitation, then large lakes found in its output feed this halo,
/// which adjusts the precipitation/diurnal-swing fields every downstream
/// consumer (aridity, region detection, renders) sees. No feedback into
/// hydrology itself.
pub fn compute_lake_halo(hydro_map: &HeatMap, is_ocean: &[bool], params: &PlanetGenParams) -> Vec<f64> {
    use std::collections::VecDeque;

    let width = hydro_map.width;
    let height = hydro_map.height;
    let n = width * height;
    // Lake cells occupy (0.3, 0.5] in the hydrology encoding (see
    // hydrology.rs Phase 7); ocean is (0.5, 1.0], rivers (0.0, 0.3].
    let is_lake: Vec<bool> = (0..n)
        .map(|i| !is_ocean[i] && hydro_map.data[i] > 0.3 && hydro_map.data[i] <= 0.5)
        .collect();

    // Wrap-aware 4-neighborhood, fixed W/E/N/S order (x wraps, y clamps).
    let neighbors = |x: usize, y: usize| {
        let mut out = [(0usize, 0usize); 4];
        let mut count = 0;
        out[count] = ((x + width - 1) % width, y);
        count += 1;
        out[count] = ((x + 1) % width, y);
        count += 1;
        if y > 0 {
            out[count] = (x, y - 1);
            count += 1;
        }
        if y + 1 < height {
            out[count] = (x, y + 1);
            count += 1;
        }
        (out, count)
    };

    // Connected components over lake cells, seeded in index order.
    let mut comp = vec![u32::MAX; n];
    let mut comp_sizes = Vec::new();
    for start in 0..n {
        if !is_lake[start] || comp[start] != u32::MAX {
            continue;
        }
        let id = comp_sizes.len() as u32;
        let mut size = 0usize;
        let mut queue = VecDeque::from([start]);
        comp[start] = id;
        while let Some(idx) = queue.pop_front() {
            size += 1;
            let (nbrs, count) = neighbors(idx % width, idx / width);
            for &(nx, ny) in &nbrs[..count] {
                let nidx = ny * width + nx;
                if is_lake[nidx] && comp[nidx] == u32::MAX {
                    comp[nidx] = id;
                    queue.push_back(nidx);
                }
            }
        }
        comp_sizes.push(size);
    }

    // Multi-source BFS outward from large-lake cells over land only.
    let mut dist = vec![usize::MAX; n];
    let mut queue = VecDeque::new();
    for idx in 0..n {
        if comp[idx] != u32::MAX && comp_sizes[comp[idx] as usize] >= params.lake_effect_min_size {
            dist[idx] = 0;
            queue.push_back(idx);
        }
    }
    while let Some(idx) = queue.pop_front() {
        let d = dist[idx];
        if d >= params.lake_halo_dist {
            continue;
        }
        let (nbrs, count) = neighbors(idx % width, idx / width);
        for &(nx, ny) in &nbrs[..count] {
            let nidx = ny * width + nx;
            if !is_ocean[nidx] && dist[nidx] == usize::MAX {
                dist[nidx] = d + 1;
                queue.push_back(nidx);
            }
        }
    }

    dist.into_iter()
        .map(|d| {
            if d == usize::MAX || d == 0 {
                0.0 // unreachable, or the lake cell itself (already water)
            } else {
                1.0 - d as f64 / params.lake_halo_dist as f64
            }
        })
        .collect()
}

/// Lake-effect precipitation: large lakes add moisture to their shores.
pub fn apply_lake_precip(precipitation: &HeatMap, halo: &[f64], params: &PlanetGenParams) -> HeatMap {
    let data = precipitation.data.iter()
        .zip(halo)
        .map(|(&p, &h)| (p + h * params.lake_precip_boost).clamp(0.0, 1.0))
        .collect();
    HeatMap { width: precipitation.width, height: precipitation.height, data }
}

/// Lake-effect temperature moderation: large water bodies damp the
/// day/night swing on their shores (milder extremes, not a shifted mean).
pub fn apply_lake_swing(swing: &HeatMap, halo: &[f64], params: &PlanetGenParams) -> HeatMap {
    let data = swing.data.iter()
        .zip(halo)
        .map(|(&s, &h)| s * (1.0 - h * params.lake_swing_damp))
        .collect();
    HeatMap { width: swing.width, height: swing.height, data }
}

/// Returns a bool mask: true where a land cell is glaciated.
pub fn generate_glacier(temperature: &HeatMap, is_ocean: &[bool], threshold: f64) -> Vec<bool> {
    (0..temperature.data.len())
        .map(|i| !is_ocean[i] && temperature.data[i] < threshold)
        .collect()
}

/// Returns a bool mask: true where an ocean cell is frozen over as sea ice.
pub fn generate_sea_ice(temperature: &HeatMap, is_ocean: &[bool], threshold: f64) -> Vec<bool> {
    (0..temperature.data.len())
        .map(|i| is_ocean[i] && temperature.data[i] < threshold)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::PlanetGenParams;

    const W: usize = 64;
    const H: usize = 64;

    fn flat_field(value: f64) -> HeatMap {
        HeatMap { width: W, height: H, data: vec![value; W * H] }
    }

    /// Mean temperature of a row, averaging out the longitude jitter.
    fn row_mean(t: &HeatMap, y: usize) -> f64 {
        (0..W).map(|x| t.data[y * W + x]).sum::<f64>() / W as f64
    }

    /// Regression: `season_offset` was applied to unsigned latitude, so both
    /// hemispheres shifted the same direction — the world had a global summer
    /// and a global winter instead of opposed hemispheres. High y is north.
    #[test]
    fn seasons_are_hemisphere_opposed() {
        let params = PlanetGenParams::earth_like();
        let elev = flat_field(params.sea_level);
        // Mirrored mid-latitude rows: equal distance from the equator.
        let (north, south) = (H * 3 / 4, H / 4);

        // Require a substantial margin, not just a direction. Under the old
        // `abs_lat` form these rows were identical up to jitter, so a bare
        // inequality would have been a coin flip rather than a failure. At
        // ±0.5 normalized latitude against a 0.261 subsolar excursion the
        // real gap is ~0.56.
        const MARGIN: f64 = 0.3;

        let summer_n = generate_temperature(&elev, &params, 0.0, 1);
        assert!(
            row_mean(&summer_n, north) > row_mean(&summer_n, south) + MARGIN,
            "at phase 0 (northern summer) the northern row should be much warmer: \
             north {}, south {}",
            row_mean(&summer_n, north),
            row_mean(&summer_n, south)
        );

        let summer_s = generate_temperature(&elev, &params, 0.5, 1);
        assert!(
            row_mean(&summer_s, south) > row_mean(&summer_s, north) + MARGIN,
            "at phase 0.5 (southern summer) the southern row should be much warmer: \
             north {}, south {}",
            row_mean(&summer_s, north),
            row_mean(&summer_s, south)
        );
    }

    /// The equinox should be very nearly symmetric between hemispheres.
    #[test]
    fn equinox_is_hemisphere_symmetric() {
        let params = PlanetGenParams::earth_like();
        let elev = flat_field(params.sea_level);
        let t = generate_temperature(&elev, &params, 0.25, 1);
        let diff = (row_mean(&t, H * 3 / 4) - row_mean(&t, H / 4)).abs();
        // Nonzero only because the band jitter decorrelates the hemispheres.
        assert!(diff < 0.05, "equinox hemispheres should be near-symmetric, diff {diff}");
    }

    /// Regression: the lapse rate read raw elevation, so ocean cells were
    /// cooled in proportion to their own depth and a deep trench read warmer
    /// than a shallow shelf. Below sea level, depth must not affect temperature.
    #[test]
    fn ocean_depth_does_not_affect_temperature() {
        let params = PlanetGenParams::earth_like();
        let y = H / 3;

        let shelf = generate_temperature(&flat_field(params.sea_level - 0.02), &params, 0.25, 1);
        let trench = generate_temperature(&flat_field(0.01), &params, 0.25, 1);

        let (a, b) = (row_mean(&shelf, y), row_mean(&trench, y));
        assert!((a - b).abs() < 1e-9, "ocean depth changed temperature: shelf {a}, trench {b}");
    }

    /// Above sea level, elevation still cools — the lapse rate should not have
    /// been neutered, only re-referenced.
    #[test]
    fn altitude_still_cools_land() {
        let params = PlanetGenParams::earth_like();
        let y = H / 3;

        let lowland = generate_temperature(&flat_field(params.sea_level), &params, 0.25, 1);
        let highland = generate_temperature(&flat_field(1.0), &params, 0.25, 1);

        assert!(
            row_mean(&lowland, y) > row_mean(&highland, y) + 0.1,
            "high terrain should be markedly colder: lowland {}, highland {}",
            row_mean(&lowland, y),
            row_mean(&highland, y)
        );
    }
}
