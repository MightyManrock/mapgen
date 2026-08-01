use std::collections::VecDeque;

use noise::{Fbm, NoiseFn, Perlin};

use crate::heatmap::{neighbors_8, HeatMap};
use crate::params::PlanetGenParams;

/// Fraction of the field remapped below sea level, and above it. Sea level
/// always lands at exactly `OCEAN_SPAN` after `normalize_about_sea_level`.
const OCEAN_SPAN: f64 = 0.5;

/// Quantiles used as the remap's tail anchors. The field's actual min and max
/// are each a single outlier cell (the deepest trench, the highest peak), so
/// anchoring on them would make the whole scale hostage to one sample — the
/// exact instability this normalization exists to remove.
const LOW_ANCHOR_Q: f64 = 0.001;
const HIGH_ANCHOR_Q: f64 = 0.999;

/// Cosine of the latitude of row `y`, matching `generate_elevation`'s mapping
/// of `y` to latitude. Used to weight cells by true surface area: an
/// equirectangular grid oversamples the poles badly, so an unweighted cell
/// count would treat a polar row as equal in area to an equatorial one.
fn row_area_weight(y: usize, height: usize) -> f64 {
    ((y as f64 / height as f64 - 0.5) * std::f64::consts::PI).cos()
}

/// Solves for the elevation threshold that yields `target_land` of the
/// planet's surface *area* as land. Returns the sea level to use.
///
/// Cells are weighted by `cos(lat)`, so the result is a true area fraction
/// rather than a cell-count fraction.
///
/// This is a single pass over the raw field. `roughen_coastline` and
/// `flood_fill_ocean` both run afterward and nudge the realized fraction —
/// measured at +0.1 to +1.9 points across seeds, nearly all of it from
/// `flood_fill_ocean` classifying disconnected below-sea-level basins as land
/// rather than ocean. `target_land` is therefore a target, not a guarantee.
pub fn weighted_sea_level(data: &[f64], width: usize, height: usize, target_land: f64) -> f64 {
    let mut pairs: Vec<(f64, f64)> = data
        .iter()
        .enumerate()
        .map(|(i, &v)| (v, row_area_weight(i / width, height)))
        .collect();
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));

    let total: f64 = pairs.iter().map(|(_, w)| w).sum();
    let want_below = total * (1.0 - target_land);
    let mut acc = 0.0;
    for (v, w) in &pairs {
        acc += w;
        if acc >= want_below {
            return *v;
        }
    }
    // Every cell accumulated without reaching the target: target_land was ~0.
    pairs.last().map_or(1.0, |(v, _)| *v)
}

/// Remaps the field so sea level sits at exactly 0.5, piecewise-linearly:
/// `[p0.1, sea_level] -> [0.0, 0.5]` and `[sea_level, p99.9] -> [0.5, 1.0]`,
/// with both tails clamped.
///
/// The point is to make absolute thresholds elsewhere in the pipeline mean the
/// same thing on every seed. Land relief natively spans `1 - sea_level`, which
/// varies from ~0.25 to ~0.40 depending on the seed, so a single tuned constant
/// (`slope_threshold`, `max_lake_fill`, `roughen_coastline`'s amplitude) lands
/// differently on each world. After this, land relief is always 0.5.
pub fn normalize_about_sea_level(data: &mut [f64], sea_level: f64) {
    let mut sorted = data.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let idx = |q: f64| sorted[((sorted.len() - 1) as f64 * q).round() as usize];
    let low = idx(LOW_ANCHOR_Q);
    let high = idx(HIGH_ANCHOR_Q);

    // Degenerate fields (all one value, or sea level outside the anchors)
    // would divide by ~zero; leave them untouched rather than emit NaN.
    if !(low < sea_level && sea_level < high) {
        return;
    }

    for v in data.iter_mut() {
        *v = if *v < sea_level {
            (OCEAN_SPAN * (*v - low) / (sea_level - low)).clamp(0.0, OCEAN_SPAN)
        } else {
            (OCEAN_SPAN + (1.0 - OCEAN_SPAN) * (*v - sea_level) / (high - sea_level))
                .clamp(OCEAN_SPAN, 1.0)
        };
    }
}

/// Spherical FBM elevation generation with domain-warped noise. Sampled at
/// 3D sphere-surface coordinates so the field is seamless in x (longitude)
/// and y (latitude), with features naturally converging at the poles.
///
/// When `params.target_land_fraction` is `Some`, the field is remapped so that
/// fraction of the surface area sits above 0.5, and the caller must use 0.5 as
/// sea level. When `None`, the raw min/max-normalized field is returned and the
/// caller's own `sea_level` applies.
///
/// Takes the whole params struct rather than individual fields, matching
/// `generate_temperature` and `generate_hydrology` — the argument list was
/// already at five and continent scale would have made six.
pub fn generate_elevation(
    width: usize,
    height: usize,
    seed: u32,
    params: &PlanetGenParams,
) -> HeatMap {
    let fbm = Fbm::<Perlin>::new(seed);
    // Two decorrelated FBM fields warp the sample coordinates before the
    // main noise is read. This breaks up annular saddle features that FBM
    // occasionally produces, which otherwise manifest as ring-shaped trenches
    // that fill with circuit rivers. Spatial offsets (5.2, 1.3) decorrelate
    // the two warp axes from each other and from the main field.
    let warp_a = Fbm::<Perlin>::new(seed.wrapping_add(1));
    let warp_b = Fbm::<Perlin>::new(seed.wrapping_add(2));
    let warp_c = Fbm::<Perlin>::new(seed.wrapping_add(3));
    // Radius so that the equatorial circumference equals
    // `continent_wavelengths` — preserves feature frequency at the equator,
    // and sets continent scale (see that param). All three FBM fields are
    // sampled at the 3D sphere-surface point, making the noise seamless in
    // both x and y and causing features to converge naturally at the poles.
    let r = params.continent_wavelengths / std::f64::consts::TAU;

    let mut data = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let lon = x as f64 / width as f64 * std::f64::consts::TAU;
            let lat = (y as f64 / height as f64 - 0.5) * std::f64::consts::PI;
            let cos_lat = lat.cos();
            let sx = r * cos_lat * lon.cos();
            let sy = r * cos_lat * lon.sin();
            let sz = r * lat.sin();

            // All three warp fields sampled at sphere-surface coords.
            let dx = warp_a.get([sx, sy, sz]) * params.warp_strength;
            let dy = warp_b.get([sx + 5.2, sy + 1.3, sz + 3.7]) * params.warp_strength;
            let dz = warp_c.get([sx + 2.8, sy + 4.6, sz + 1.9]) * params.warp_strength;
            data.push(fbm.get([sx + dx, sy + dy, sz + dz]));
        }
    }

    // Normalize to [0, 1] using actual min/max.
    let min = data.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = max - min;
    for v in &mut data {
        *v = (*v - min) / range;
    }

    let mut hm = HeatMap { width, height, data };
    hm.smooth_low_variance(6, 0.002, 0.25);

    // Smoothing shifts the distribution, so the sea level must be solved from
    // the final field, not the pre-smoothing one.
    if let Some(target) = params.target_land_fraction {
        let sea_level = weighted_sea_level(&hm.data, width, height, target);
        normalize_about_sea_level(&mut hm.data, sea_level);
    }

    hm
}

/// BFS flood-fill from the global elevation minimum, marking all connected
/// cells below sea_level as ocean. Disconnected below-sea-level areas are
/// inland basins (not ocean) and fall through to normal lake/endorheic logic.
pub fn flood_fill_ocean(data: &[f64], width: usize, height: usize, sea_level: f64) -> Vec<bool> {
    let n = width * height;
    let mut is_ocean = vec![false; n];

    let min_idx = (0..n).min_by(|&a, &b| data[a].total_cmp(&data[b])).unwrap();
    if data[min_idx] >= sea_level {
        return is_ocean; // entirely dry planet
    }

    let mut queue = VecDeque::new();
    is_ocean[min_idx] = true;
    queue.push_back(min_idx);

    while let Some(idx) = queue.pop_front() {
        let x = idx % width;
        let y = idx / width;
        for (nx, ny) in neighbors_8(x, y, width, height) {
            let nidx = ny * width + nx;
            if !is_ocean[nidx] && data[nidx] < sea_level {
                is_ocean[nidx] = true;
                queue.push_back(nidx);
            }
        }
    }

    is_ocean
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Area fraction above `sea_level`, cos-lat weighted — the quantity
    /// `weighted_sea_level` is solving for.
    fn land_area_fraction(data: &[f64], width: usize, height: usize, sea_level: f64) -> f64 {
        let mut land = 0.0;
        let mut total = 0.0;
        for (i, &v) in data.iter().enumerate() {
            let w = row_area_weight(i / width, height);
            total += w;
            if v >= sea_level {
                land += w;
            }
        }
        land / total
    }

    /// Guards against the classic threaded-but-ignored parameter: a knob that
    /// is accepted, documented, and never reaches the sampling radius would
    /// still compile and run, and every metric would silently stay put.
    #[test]
    fn continent_wavelengths_reaches_the_field() {
        let mut pangaea = PlanetGenParams::earth_like();
        pangaea.continent_wavelengths = 2.5;
        let mut archipelago = PlanetGenParams::earth_like();
        archipelago.continent_wavelengths = 11.0;

        let a = generate_elevation(96, 48, 7, &pangaea);
        let b = generate_elevation(96, 48, 7, &archipelago);

        let differing = a.data.iter().zip(&b.data).filter(|(x, y)| (*x - *y).abs() > 1e-6).count();
        assert!(
            differing > a.data.len() / 4,
            "continent_wavelengths barely changed the field ({differing} of {} cells)",
            a.data.len()
        );
    }

    #[test]
    fn generation_is_deterministic() {
        let params = PlanetGenParams::earth_like();
        let a = generate_elevation(96, 48, 11, &params);
        let b = generate_elevation(96, 48, 11, &params);
        assert_eq!(a.data, b.data, "same seed and params must give an identical field");
    }

    /// Continent scale and land fraction must stay orthogonal: sea level is
    /// solved *after* generation, so changing how continents are shaped must
    /// not change how much land there is.
    #[test]
    fn land_fraction_is_independent_of_continent_scale() {
        for wavelengths in [2.5, 5.5, 11.0, 18.0] {
            let mut params = PlanetGenParams::earth_like();
            params.continent_wavelengths = wavelengths;
            let (w, h) = (256, 128);
            let elev = generate_elevation(w, h, 3, &params);

            let mut land = 0.0;
            let mut total = 0.0;
            for (i, &v) in elev.data.iter().enumerate() {
                let weight = row_area_weight(i / w, h);
                total += weight;
                if v >= 0.5 {
                    land += weight;
                }
            }
            let frac = land / total;
            assert!(
                (frac - 0.30).abs() < 0.02,
                "wavelengths {wavelengths} gave land fraction {frac:.3}, expected ~0.30"
            );
        }
    }

    #[test]
    fn weighted_sea_level_hits_target_on_uniform_field() {
        // Elevation ramps 0..1 uniformly across x and is constant down each
        // column, so latitude weighting cancels and the answer is the plain
        // 0.70 quantile.
        let (width, height) = (100, 40);
        let data: Vec<f64> = (0..width * height)
            .map(|i| (i % width) as f64 / (width - 1) as f64)
            .collect();

        let sl = weighted_sea_level(&data, width, height, 0.30);

        assert!((sl - 0.70).abs() < 0.02, "expected ~0.70, got {sl}");
        let realized = land_area_fraction(&data, width, height, sl);
        assert!((realized - 0.30).abs() < 0.02, "expected ~0.30 land, got {realized}");
    }

    #[test]
    fn weighted_sea_level_weights_by_latitude() {
        // High near the poles, low at the equator. Polar rows hold most of the
        // *cells* but little of the *area*, so area weighting must yield a
        // markedly lower threshold than an unweighted quantile would.
        let (width, height) = (40, 100);
        let data: Vec<f64> = (0..width * height)
            .map(|i| {
                let y = i / width;
                ((y as f64 / height as f64) - 0.5).abs() * 2.0
            })
            .collect();

        let weighted = weighted_sea_level(&data, width, height, 0.30);

        let mut sorted = data.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let unweighted = sorted[((sorted.len() - 1) as f64 * 0.70).round() as usize];

        // Area below value V is sin(V*PI/2) for this field, so the weighted
        // answer is asin(0.70)/(PI/2) ~= 0.494 against an unweighted ~0.70.
        assert!(
            weighted < unweighted - 0.05,
            "area weighting should pull the threshold down: weighted {weighted}, \
             unweighted {unweighted}"
        );
        assert!((weighted - 0.494).abs() < 0.02, "expected ~0.494, got {weighted}");
        // And it should still hit the target in area terms.
        let realized = land_area_fraction(&data, width, height, weighted);
        assert!((realized - 0.30).abs() < 0.03, "expected ~0.30 land, got {realized}");
    }

    #[test]
    fn normalize_puts_sea_level_at_half_and_clamps_tails() {
        let mut data: Vec<f64> = (0..1000).map(|i| i as f64 / 999.0).collect();
        let sea_level = 0.62;
        normalize_about_sea_level(&mut data, sea_level);

        // Everything stays in range, and the split lands where it should.
        assert!(data.iter().all(|&v| (0.0..=1.0).contains(&v)));
        let below = data.iter().filter(|&&v| v < 0.5).count();
        let expected_below = (1000.0 * sea_level) as usize;
        assert!(
            below.abs_diff(expected_below) < 15,
            "expected ~{expected_below} cells below 0.5, got {below}"
        );
    }

    #[test]
    fn normalize_preserves_ordering() {
        let mut data = vec![0.05, 0.2, 0.45, 0.5, 0.61, 0.8, 0.95];
        normalize_about_sea_level(&mut data, 0.5);
        for pair in data.windows(2) {
            assert!(pair[0] <= pair[1], "ordering broken: {data:?}");
        }
    }

    #[test]
    fn normalize_leaves_degenerate_field_untouched() {
        // Sea level outside the anchor range would divide by ~zero.
        let mut data = vec![0.4; 50];
        normalize_about_sea_level(&mut data, 0.9);
        assert!(data.iter().all(|&v| v == 0.4), "degenerate field was modified");
    }
}
