use std::collections::VecDeque;

use noise::{Fbm, NoiseFn, Perlin};

use crate::heatmap::{neighbors_8, HeatMap};

/// Spherical FBM elevation generation with domain-warped noise. Sampled at
/// 3D sphere-surface coordinates so the field is seamless in x (longitude)
/// and y (latitude), with features naturally converging at the poles.
pub fn generate_elevation(width: usize, height: usize, seed: u32, warp_strength: f64) -> HeatMap {
    let fbm = Fbm::<Perlin>::new(seed);
    // Two decorrelated FBM fields warp the sample coordinates before the
    // main noise is read. This breaks up annular saddle features that FBM
    // occasionally produces, which otherwise manifest as ring-shaped trenches
    // that fill with circuit rivers. Spatial offsets (5.2, 1.3) decorrelate
    // the two warp axes from each other and from the main field.
    let warp_a = Fbm::<Perlin>::new(seed.wrapping_add(1));
    let warp_b = Fbm::<Perlin>::new(seed.wrapping_add(2));
    let warp_c = Fbm::<Perlin>::new(seed.wrapping_add(3));
    // Radius so that the equatorial circumference equals 3.5 — preserves
    // feature frequency at the equator. All three FBM fields are sampled at
    // the 3D sphere-surface point, making the noise seamless in both x and y
    // and causing features to converge naturally at the poles.
    let r = 3.5 / std::f64::consts::TAU;

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
            let dx = warp_a.get([sx, sy, sz]) * warp_strength;
            let dy = warp_b.get([sx + 5.2, sy + 1.3, sz + 3.7]) * warp_strength;
            let dz = warp_c.get([sx + 2.8, sy + 4.6, sz + 1.9]) * warp_strength;
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
