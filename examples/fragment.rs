// Prototype: how do continent feature frequency and domain-warp strength
// affect continentality?
//
// The target is set by measurement, not taste: 18% of currently-generated land
// sits further from the ocean than Earth's most continental point (2,645 km).
// On Earth that figure is 0%. Bring it down without simply shredding the
// continents into islands.
#[path = "../src/heatmap.rs"]
mod heatmap;
#[path = "../src/elevation.rs"]
mod elevation;
#[path = "../src/params.rs"]
mod params;

use std::collections::VecDeque;

use noise::{Fbm, NoiseFn, Perlin};
use params::PlanetGenParams;

const WIDTH: usize = 1024;
const HEIGHT: usize = 512;
const SEEDS: [u32; 6] = [42, 7, 99, 123, 2024, 314];
const EARTH_MOST_CONTINENTAL_KM: f64 = 2645.0;

fn cos_lat(y: usize) -> f64 {
    ((y as f64 / HEIGHT as f64 - 0.5) * std::f64::consts::PI).cos()
}

/// `generate_elevation` with the two hardcoded constants exposed.
fn gen_elev(seed: u32, wavelengths: f64, warp: f64, target: Option<f64>) -> heatmap::HeatMap {
    let fbm = Fbm::<Perlin>::new(seed);
    let warp_a = Fbm::<Perlin>::new(seed.wrapping_add(1));
    let warp_b = Fbm::<Perlin>::new(seed.wrapping_add(2));
    let warp_c = Fbm::<Perlin>::new(seed.wrapping_add(3));
    let r = wavelengths / std::f64::consts::TAU;

    let mut data = Vec::with_capacity(WIDTH * HEIGHT);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let lon = x as f64 / WIDTH as f64 * std::f64::consts::TAU;
            let lat = (y as f64 / HEIGHT as f64 - 0.5) * std::f64::consts::PI;
            let cl = lat.cos();
            let (sx, sy, sz) = (r * cl * lon.cos(), r * cl * lon.sin(), r * lat.sin());
            let dx = warp_a.get([sx, sy, sz]) * warp;
            let dy = warp_b.get([sx + 5.2, sy + 1.3, sz + 3.7]) * warp;
            let dz = warp_c.get([sx + 2.8, sy + 4.6, sz + 1.9]) * warp;
            data.push(fbm.get([sx + dx, sy + dy, sz + dz]));
        }
    }
    let min = data.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    for v in &mut data {
        *v = (*v - min) / (max - min);
    }
    let mut hm = heatmap::HeatMap { width: WIDTH, height: HEIGHT, data };
    hm.smooth_low_variance(6, 0.002, 0.25);
    if let Some(t) = target {
        let sl = elevation::weighted_sea_level(&hm.data, WIDTH, HEIGHT, t);
        elevation::normalize_about_sea_level(&mut hm.data, sl);
    }
    hm
}

struct Metrics {
    grad_p95: f64,
    beyond_pct: f64,
    p50: f64,
    p90: f64,
    landmasses: f64,
    largest_pct: f64,
}

fn measure(p: &PlanetGenParams, wavelengths: f64, warp: f64) -> Metrics {
    let km_row = std::f64::consts::PI * p.radius_km / HEIGHT as f64;
    let km_col = |y: usize| std::f64::consts::TAU * p.radius_km * cos_lat(y) / WIDTH as f64;
    let (mut all, mut masses, mut largest, mut n) = (Vec::new(), 0.0, 0.0, 0.0);
    let mut grads: Vec<f64> = Vec::new();

    for seed in SEEDS {
        let mut elev = gen_elev(seed, wavelengths, warp, p.target_land_fraction);
        elev.roughen_coastline(p.sea_level, seed.wrapping_add(10));
        let is_ocean = elevation::flood_fill_ocean(&elev.data, WIDTH, HEIGHT, p.sea_level);

        let mut dist = vec![f64::INFINITY; WIDTH * HEIGHT];
        let mut q = VecDeque::new();
        for i in 0..WIDTH * HEIGHT {
            if is_ocean[i] { dist[i] = 0.0; q.push_back(i); }
        }
        while let Some(i) = q.pop_front() {
            let (x, y) = (i % WIDTH, i / WIDTH);
            let mut push = |nx: usize, ny: usize, step: f64, d: &mut Vec<f64>, q: &mut VecDeque<usize>| {
                let ni = ny * WIDTH + nx;
                if d[ni] > d[i] + step + 1e-9 { d[ni] = d[i] + step; q.push_back(ni); }
            };
            push((x + 1) % WIDTH, y, km_col(y), &mut dist, &mut q);
            push((x + WIDTH - 1) % WIDTH, y, km_col(y), &mut dist, &mut q);
            if y > 0 { push(x, y - 1, km_row, &mut dist, &mut q); }
            if y + 1 < HEIGHT { push(x, y + 1, km_row, &mut dist, &mut q); }
        }

        let (mut tot, mut land_area) = (0.0, 0.0);
        for y in 0..HEIGHT {
            let w = cos_lat(y);
            for x in 0..WIDTH {
                let i = y * WIDTH + x;
                tot += w;
                if !is_ocean[i] { all.push((dist[i], w)); land_area += w; }
            }
        }

        // Landmasses holding at least 0.5% of the globe, and the largest.
        let mut comp = vec![usize::MAX; WIDTH * HEIGHT];
        let mut sizes = Vec::new();
        for start in 0..WIDTH * HEIGHT {
            if is_ocean[start] || comp[start] != usize::MAX { continue; }
            let id = sizes.len();
            let mut area = 0.0;
            let mut qq = VecDeque::from([start]);
            comp[start] = id;
            while let Some(i) = qq.pop_front() {
                area += cos_lat(i / WIDTH);
                for (nx, ny) in heatmap::neighbors_8(i % WIDTH, i / WIDTH, WIDTH, HEIGHT) {
                    let ni = ny * WIDTH + nx;
                    if !is_ocean[ni] && comp[ni] == usize::MAX { comp[ni] = id; qq.push_back(ni); }
                }
            }
            sizes.push(area);
        }
        // Per-cell east-west land gradient: what the rain shadow's
        // slope_threshold (0.023) is compared against.
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let i = y * WIDTH + x;
                if is_ocean[i] { continue; }
                let up = y * WIDTH + (x + WIDTH - 1) % WIDTH;
                grads.push((elev.data[i] - elev.data[up]).abs());
            }
        }
        sizes.sort_by(|a, b| b.total_cmp(a));
        masses += sizes.iter().filter(|&&s| s / tot > 0.005).count() as f64;
        largest += sizes[0] / land_area * 100.0;
        n += 1.0;
    }

    all.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f64 = all.iter().map(|v| v.1).sum();
    let q_at = |f: f64| {
        let want = total * f;
        let mut acc = 0.0;
        for (d, w) in &all { acc += w; if acc >= want { return *d; } }
        all.last().unwrap().0
    };
    grads.sort_by(|a, b| a.total_cmp(b));
    Metrics {
        grad_p95: grads[(grads.len() as f64 * 0.95) as usize],
        beyond_pct: all.iter().filter(|(d, _)| *d > EARTH_MOST_CONTINENTAL_KM)
            .map(|(_, w)| w).sum::<f64>() / total * 100.0,
        p50: q_at(0.50),
        p90: q_at(0.90),
        landmasses: masses / n,
        largest_pct: largest / n,
    }
}

fn main() {
    if std::env::args().any(|a| a == "render") { render_candidates(); println!("rendered"); return; }
    let p = PlanetGenParams::earth_like();
    println!("Target: 'beyond' near 0% (Earth), while keeping real continents.\n");
    println!("wavelengths  warp | beyond |    p50 |    p90 | masses | largest% | p95 grad");
    for (wl, warp) in [
        (2.0, 0.15),
        (2.5, 0.20),
        (3.5, 0.20),
        (5.5, 0.65),
        (8.0, 0.65),
        (11.0, 0.65),
        (14.0, 0.70),
        (18.0, 0.70),
    ] {
        let m = measure(&p, wl, warp);
        println!(
            "{:11.1} {:5.2} | {:5.1}% | {:6.0} | {:6.0} | {:6.1} | {:8.1} | {:8.4}{}",
            wl, warp, m.beyond_pct, m.p50, m.p90, m.landmasses, m.largest_pct, m.grad_p95,
            if (wl - 3.5).abs() < 1e-9 && (warp - 0.2).abs() < 1e-9 { "   <- current" } else { "" }
        );
    }
}

// Renders candidate configurations for visual inspection: numbers can be
// right while the terrain reads as unnatural (strong domain warping can
// produce a swirled look that no metric here would catch).
#[allow(dead_code)]
fn render_candidates() {
    let p = PlanetGenParams::earth_like();
    for (wl, warp) in [(3.5, 0.20), (5.5, 0.50), (5.5, 0.65), (5.5, 0.80), (6.5, 0.65)] {
        let mut elev = gen_elev(42, wl, warp, p.target_land_fraction);
        elev.roughen_coastline(p.sea_level, 52);
        let is_ocean = elevation::flood_fill_ocean(&elev.data, WIDTH, HEIGHT, p.sea_level);
        let img = image::ImageBuffer::from_fn(WIDTH as u32, HEIGHT as u32, |x, y| {
            let i = y as usize * WIDTH + x as usize;
            if is_ocean[i] {
                let d = ((p.sea_level - elev.data[i]) / p.sea_level).clamp(0.0, 1.0);
                image::Rgb([(30.0 + 60.0 * (1.0 - d)) as u8, (70.0 + 80.0 * (1.0 - d)) as u8, 170])
            } else {
                let h = ((elev.data[i] - p.sea_level) / (1.0 - p.sea_level)).clamp(0.0, 1.0);
                image::Rgb([(120.0 + 110.0 * h) as u8, (140.0 + 80.0 * h) as u8, (90.0 + 70.0 * h) as u8])
            }
        });
        img.save(format!("output/cand_wl{wl}_warp{warp}.png")).unwrap();
    }
}
