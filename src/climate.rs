use std::f64::consts::PI;

use noise::{Fbm, NoiseFn, Perlin};

use crate::heatmap::HeatMap;
use crate::params::PlanetGenParams;

/// Smooth per-cell latitude offset that breaks up otherwise razor-straight
/// circulation bands (`lat_band_factor`/`westerly_weight` are pure functions
/// of latitude with zero longitude variation). Sampled at 3D sphere-surface
/// coordinates — same technique as `elevation::generate_elevation` — so it's
/// seamless in x and converges naturally at the poles.
const JITTER_AMPLITUDE: f64 = 0.04;

fn lat_jitter(x: usize, y: usize, width: usize, height: usize, fbm: &Fbm<Perlin>) -> f64 {
    let lon = x as f64 / width as f64 * std::f64::consts::TAU;
    let lat = (y as f64 / height as f64 - 0.5) * PI;
    let cos_lat = lat.cos();
    let r = 3.5 / std::f64::consts::TAU;
    let sx = r * cos_lat * lon.cos();
    let sy = r * cos_lat * lon.sin();
    let sz = r * lat.sin();
    fbm.get([sx, sy, sz]) * JITTER_AMPLITUDE
}

/// Latitude cosine + elevation lapse rate, scaled by planet params.
///
/// `temp_baseline` sets the equatorial surface temperature [0,1]; `temp_gradient`
/// controls how steeply it drops toward the poles (1.0 = full drop to 0, 0.1 = nearly flat).
pub fn generate_temperature(elevation: &HeatMap, params: &PlanetGenParams, season_phase: f64, seed: u32) -> HeatMap {
    let width = elevation.width;
    let height = elevation.height;

    // Axial tilt shifts the insolation peak north/south with the season.
    // Convention matches generate_precipitation: phase 0 = northern summer.
    let season_offset = (params.axial_tilt.to_radians() * 0.5 / (PI / 2.0))
        * (season_phase * 2.0 * PI).cos();

    let jitter_fbm = Fbm::<Perlin>::new(seed.wrapping_add(20));

    let data = (0..width * height)
        .map(|idx| {
            let x = idx % width;
            let y = idx / width;
            let abs_lat = (y as f64 - height as f64 / 2.0).abs() / (height as f64 / 2.0);
            let jitter = lat_jitter(x, y, width, height, &jitter_fbm);
            let shifted_lat = (abs_lat - season_offset + jitter).abs().clamp(0.0, 1.0);
            let lat_shape = (shifted_lat * std::f64::consts::FRAC_PI_2).cos();
            let lat_temp = params.temp_baseline * (1.0 - params.temp_gradient * (1.0 - lat_shape));
            (lat_temp - elevation.data[idx] * params.lapse_factor).clamp(0.0, 1.0)
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
    // ITCZ and wind belts migrate with season. Half the tilt angle (in
    // normalised lat units) is a reasonable proxy for the Hadley cell shift.
    // Convention: phase 0 = northern summer solstice (cos=1, max northward shift),
    //             phase π = northern winter solstice (cos=-1, max southward shift).
    let season_offset = (params.axial_tilt.to_radians() * 0.5 / (PI / 2.0)) * season_phase.cos();

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
            let abs_lat = (y as f64 - height as f64 / 2.0).abs() / (height as f64 / 2.0);
            let jitter = lat_jitter(x, y, width, height, &jitter_fbm);
            let w = westerly_weight(abs_lat + jitter, season_offset);
            let moisture = moisture_west[idx] * w + moisture_east[idx] * (1.0 - w);
            let band = lat_band_factor(abs_lat + jitter, season_offset);
            let moisture_capacity = (0.3 + 0.7 * temperature.data[idx]).clamp(0.3, 1.0);
            (band * (params.base_arid + moisture * (1.0 - params.base_arid)) * moisture_capacity).clamp(0.0, 1.0)
        })
        .collect();

    HeatMap { width, height, data }
}

/// Latitude precipitation factor based on Earth's general circulation bands.
/// Returns a [0, 1] multiplier applied before moisture weighting.
/// `season_offset` shifts all band latitudes (positive = ITCZ migrates north).
fn lat_band_factor(abs_lat: f64, season_offset: f64) -> f64 {
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
    // Shift abs_lat in the opposite direction: if ITCZ moves north (+offset),
    // a given cell effectively sits at a lower latitude relative to the band.
    let shifted = (abs_lat - season_offset).clamp(0.0, 1.0);
    for i in 0..stops.len() - 1 {
        let (ta, va) = stops[i];
        let (tb, vb) = stops[i + 1];
        if shifted <= tb {
            let t = (shifted - ta) / (tb - ta);
            return va + (vb - va) * t;
        }
    }
    stops.last().unwrap().1
}

/// Fraction of moisture contributed by the westerly sweep vs easterly sweep.
/// 1.0 = pure westerlies, 0.0 = pure easterlies.
/// `season_offset` shifts the wind belt latitudes with the season.
fn westerly_weight(abs_lat: f64, season_offset: f64) -> f64 {
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
    let shifted = (abs_lat - season_offset).clamp(0.0, 1.0);
    for i in 0..stops.len() - 1 {
        let (ta, va) = stops[i];
        let (tb, vb) = stops[i + 1];
        if shifted <= tb {
            let t = (shifted - ta) / (tb - ta);
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
