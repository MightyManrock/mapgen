use noise::{Fbm, NoiseFn, Perlin};

/// A 2D float field over a normalized [0,1) x [0,1) coordinate plane.
/// x wraps (east-west); y clamps (poles do not connect).
pub struct HeatMap {
    pub width: usize,
    pub height: usize,
    pub data: Vec<f64>,
}

impl HeatMap {
    /// Sample by nearest-neighbor. x and y are in [0, 1).
    pub fn sample_nearest(&self, x: f64, y: f64) -> f64 {
        let px = (x.rem_euclid(1.0) * self.width as f64) as usize % self.width;
        let py = (y.clamp(0.0, 1.0 - f64::EPSILON) * self.height as f64) as usize;
        self.data[py * self.width + px]
    }

    /// Sample by bilinear interpolation. x and y are in [0, 1).
    pub fn sample(&self, x: f64, y: f64) -> f64 {
        let px = x.rem_euclid(1.0) * self.width as f64;
        let py = y.clamp(0.0, 1.0 - f64::EPSILON) * self.height as f64;

        let x0 = px.floor() as usize % self.width;
        let y0 = py.floor() as usize;
        let x1 = (x0 + 1) % self.width;
        let y1 = (y0 + 1).min(self.height - 1);

        let tx = px.fract();
        let ty = py.fract();

        let v00 = self.data[y0 * self.width + x0];
        let v10 = self.data[y0 * self.width + x1];
        let v01 = self.data[y1 * self.width + x0];
        let v11 = self.data[y1 * self.width + x1];

        let top = v00 + (v10 - v00) * tx;
        let bot = v01 + (v11 - v01) * tx;
        top + (bot - top) * ty
    }

    /// Iteratively blends cells toward their neighborhood mean, but only where
    /// local variance is below the threshold — i.e. flat plains and plateaus.
    /// High-variance areas (ridgelines, mountain peaks) are left untouched.
    pub fn smooth_low_variance(&mut self, passes: usize, variance_threshold: f64, blend: f64) {
        for _ in 0..passes {
            let prev = self.data.clone();
            for y in 0..self.height {
                for x in 0..self.width {
                    let idx = y * self.width + x;
                    let neighbors = neighbors_8(x, y, self.width, self.height);
                    let n = neighbors.len() as f64;
                    let vals: Vec<f64> =
                        neighbors.iter().map(|&(nx, ny)| prev[ny * self.width + nx]).collect();
                    let mean = vals.iter().sum::<f64>() / n;
                    let variance =
                        vals.iter().map(|&v| (v - mean).powi(2)).sum::<f64>() / n;
                    if variance < variance_threshold {
                        let neighborhood_mean = (mean * n + prev[idx]) / (n + 1.0);
                        self.data[idx] = prev[idx] * (1.0 - blend) + neighborhood_mean * blend;
                    }
                }
            }
        }
    }

    /// Adds high-frequency detail noise near sea level, producing jagged
    /// coastlines — cliffs, inlets, sea stacks — from cells just above or below
    /// the waterline being nudged across it. Uses spherical sampling so the
    /// detail is seamless. Must be called before flood_fill_ocean.
    pub fn roughen_coastline(&mut self, sea_level: f64, seed: u32) {
        let detail = Fbm::<Perlin>::new(seed);
        // 4× finer than the main terrain scale (3.5) for visible coastal detail.
        let r = 50.0 / std::f64::consts::TAU;
        const AMPLITUDE: f64 = 0.08;
        // Gaussian bandwidth: how far from sea level the effect reaches.
        const BANDWIDTH: f64 = 0.05;

        let width = self.width;
        let height = self.height;
        for y in 0..height {
            for x in 0..width {
                let idx = y * width + x;
                let elev = self.data[idx];
                let dist = elev - sea_level;
                let weight = (-(dist * dist) / (2.0 * BANDWIDTH * BANDWIDTH)).exp();
                if weight < 0.01 {
                    continue;
                }
                let lon = x as f64 / width as f64 * std::f64::consts::TAU;
                let lat = (y as f64 / height as f64 - 0.5) * std::f64::consts::PI;
                let cos_lat = lat.cos();
                let sx = r * cos_lat * lon.cos();
                let sy = r * cos_lat * lon.sin();
                let sz = r * lat.sin();
                let noise = detail.get([sx, sy, sz]);
                self.data[idx] = (elev + noise * AMPLITUDE * weight).clamp(0.0, 1.0);
            }
        }
    }
}

/// 8-connected neighbors with full spherical topology.
///
/// x wraps east-west as normal. At the poles, going off the top or bottom edge
/// wraps to the same pole row but offset by width/2 — the equirectangular
/// projection of crossing the pole and emerging on the opposite side of the
/// planet. Duplicates are suppressed (can occur when multiple dx values map to
/// the same cell near the poles).
pub(crate) fn neighbors_8(x: usize, y: usize, width: usize, height: usize) -> Vec<(usize, usize)> {
    let mut result = Vec::with_capacity(8);
    for dy in -1i32..=1 {
        for dx in -1i32..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let mut nx = (x as i32 + dx).rem_euclid(width as i32) as usize;
            let ny_raw = y as i32 + dy;
            let ny = if ny_raw < 0 {
                // Crossed the north pole: emerge on the opposite side, same row.
                nx = (nx + width / 2) % width;
                0
            } else if ny_raw >= height as i32 {
                // Crossed the south pole: emerge on the opposite side, same row.
                nx = (nx + width / 2) % width;
                height - 1
            } else {
                ny_raw as usize
            };
            if !result.contains(&(nx, ny)) {
                result.push((nx, ny));
            }
        }
    }
    result
}
