// Measurement harness: how continent scale affects continentality.
//
// The target is set by measurement, not taste. Earth's most continental point
// (the Eurasian pole of inaccessibility, 2,645 km inland) is desert; no land on
// Earth lies beyond it. Land further inland than that is more continental than
// anywhere real, and will generate as desert for correct reasons.
//
//   cargo run --release --example fragment           # sweep the range
//   cargo run --release --example fragment render    # write candidate terrains
#[path = "../src/heatmap.rs"]
mod heatmap;
#[path = "../src/elevation.rs"]
mod elevation;
#[path = "../src/params.rs"]
mod params;

use std::collections::VecDeque;

use params::PlanetGenParams;

const WIDTH: usize = 1024;
const HEIGHT: usize = 512;
const SEEDS: [u32; 6] = [42, 7, 99, 123, 2024, 314];
const EARTH_MOST_CONTINENTAL_KM: f64 = 2645.0;

fn cos_lat(y: usize) -> f64 {
    ((y as f64 / HEIGHT as f64 - 0.5) * std::f64::consts::PI).cos()
}

fn configured(wavelengths: f64, warp: f64) -> PlanetGenParams {
    let mut p = PlanetGenParams::earth_like();
    p.continent_wavelengths = wavelengths;
    p.warp_strength = warp;
    p
}

struct Metrics {
    beyond_pct: f64,
    p50: f64,
    p90: f64,
    landmasses: f64,
    largest_pct: f64,
    grad_p95: f64,
}

fn measure(p: &PlanetGenParams) -> Metrics {
    let km_row = std::f64::consts::PI * p.radius_km / HEIGHT as f64;
    let km_col = |y: usize| std::f64::consts::TAU * p.radius_km * cos_lat(y) / WIDTH as f64;
    let (mut all, mut masses, mut largest, mut n) = (Vec::new(), 0.0, 0.0, 0.0);
    let mut grads: Vec<f64> = Vec::new();

    for seed in SEEDS {
        let mut elev = elevation::generate_elevation(WIDTH, HEIGHT, seed, p);
        elev.roughen_coastline(p.sea_level, seed.wrapping_add(10));
        let is_ocean = elevation::flood_fill_ocean(&elev.data, WIDTH, HEIGHT, p.sea_level);

        // Multi-source BFS from every ocean cell, accumulating physical
        // distance: cells are not square and shrink toward the poles.
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
                if !is_ocean[i] {
                    all.push((dist[i], w));
                    land_area += w;
                    let up = y * WIDTH + (x + WIDTH - 1) % WIDTH;
                    grads.push((elev.data[i] - elev.data[up]).abs());
                }
            }
        }

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
        sizes.sort_by(|a, b| b.total_cmp(a));
        masses += sizes.iter().filter(|&&s| s / tot > 0.005).count() as f64;
        largest += sizes[0] / land_area * 100.0;
        n += 1.0;
    }

    all.sort_by(|a, b| a.0.total_cmp(&b.0));
    grads.sort_by(|a, b| a.total_cmp(b));
    let total: f64 = all.iter().map(|v| v.1).sum();
    let q_at = |f: f64| {
        let want = total * f;
        let mut acc = 0.0;
        for (d, w) in &all { acc += w; if acc >= want { return *d; } }
        all.last().unwrap().0
    };
    Metrics {
        beyond_pct: (all.iter().filter(|(d, _)| *d > EARTH_MOST_CONTINENTAL_KM)
            .map(|(_, w)| w).sum::<f64>() / total * 100.0).max(0.0),
        p50: q_at(0.50),
        p90: q_at(0.90),
        landmasses: masses / n,
        largest_pct: largest / n,
        grad_p95: grads[(grads.len() as f64 * 0.95) as usize],
    }
}

/// Writes candidate terrains for visual inspection — metrics cannot catch a
/// terrain that simply reads as unnatural.
fn render_candidates() {
    for (wl, warp) in [(2.5, 0.20), (5.5, 0.65), (11.0, 0.65), (18.0, 0.70)] {
        let p = configured(wl, warp);
        let mut elev = elevation::generate_elevation(WIDTH, HEIGHT, 42, &p);
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
        img.save(format!("output/continents_wl{wl}_warp{warp}.png")).unwrap();
    }
}

fn main() {
    if std::env::args().any(|a| a == "render") {
        render_candidates();
        println!("wrote output/continents_*.png");
        return;
    }
    let default = PlanetGenParams::earth_like();
    println!("Earth: ~5 major landmasses, largest ~57% of land, 0% beyond 2,645 km.\n");
    println!("wavelengths  warp | beyond |    p50 |    p90 | masses | largest% | p95 grad");
    for (wl, warp) in [
        (2.5, 0.20),
        (3.5, 0.20),
        (5.5, 0.65),
        (8.0, 0.65),
        (11.0, 0.65),
        (18.0, 0.70),
    ] {
        let m = measure(&configured(wl, warp));
        let is_default = (wl - default.continent_wavelengths).abs() < 1e-9
            && (warp - default.warp_strength).abs() < 1e-9;
        println!(
            "{:11.1} {:5.2} | {:5.1}% | {:6.0} | {:6.0} | {:6.1} | {:8.1} | {:8.4}{}",
            wl, warp, m.beyond_pct, m.p50, m.p90, m.landmasses, m.largest_pct, m.grad_p95,
            if is_default { "   <- default" } else { "" }
        );
    }
}
