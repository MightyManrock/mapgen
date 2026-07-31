// Diagnostic: how continental are the generated worlds, in kilometres?
//
// The biome mix can only be judged against the geology that produced it.
// Earth's most continental point (the Eurasian pole of inaccessibility, in
// Xinjiang) is ~2,645 km from open ocean, and that region is desert/steppe. If
// our *typical* land sits further inland than Earth's most extreme point, then
// a desert-heavy biome mix is the correct answer for this geology rather than
// a calibration error.
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
const SEEDS: [u32; 8] = [42, 7, 99, 123, 2024, 555, 8, 314];

/// Earth's pole of inaccessibility, km from ocean.
const EARTH_MOST_CONTINENTAL_KM: f64 = 2645.0;

fn cos_lat(y: usize) -> f64 {
    ((y as f64 / HEIGHT as f64 - 0.5) * std::f64::consts::PI).cos()
}

fn main() {
    let p = PlanetGenParams::earth_like();
    // Cell dimensions in km. Rows are a fixed height; columns shrink with cos(lat).
    let km_row = std::f64::consts::PI * p.radius_km / HEIGHT as f64;
    let km_col = |y: usize| std::f64::consts::TAU * p.radius_km * cos_lat(y) / WIDTH as f64;

    println!("Distance from land to the nearest ocean, km (area-weighted percentiles)");
    println!("seed |    p25 |    p50 |    p75 |    p90 |    max | % beyond Earth's extreme");
    let mut all: Vec<(f64, f64)> = Vec::new();

    for seed in SEEDS {
        let mut elev = elevation::generate_elevation(
            WIDTH, HEIGHT, seed, p.warp_strength, p.target_land_fraction,
        );
        elev.roughen_coastline(p.sea_level, seed.wrapping_add(10));
        let is_ocean = elevation::flood_fill_ocean(&elev.data, WIDTH, HEIGHT, p.sea_level);

        // Multi-source BFS outward from every ocean cell, accumulating physical
        // distance rather than cell counts (cells are not square, and shrink
        // toward the poles).
        let mut dist = vec![f64::INFINITY; WIDTH * HEIGHT];
        let mut q = VecDeque::new();
        for i in 0..WIDTH * HEIGHT {
            if is_ocean[i] {
                dist[i] = 0.0;
                q.push_back(i);
            }
        }
        while let Some(i) = q.pop_front() {
            let (x, y) = (i % WIDTH, i / WIDTH);
            let mut push = |nx: usize, ny: usize, step: f64, dist: &mut Vec<f64>, q: &mut VecDeque<usize>| {
                let ni = ny * WIDTH + nx;
                if dist[ni] > dist[i] + step + 1e-9 {
                    dist[ni] = dist[i] + step;
                    q.push_back(ni);
                }
            };
            push((x + 1) % WIDTH, y, km_col(y), &mut dist, &mut q);
            push((x + WIDTH - 1) % WIDTH, y, km_col(y), &mut dist, &mut q);
            if y > 0 { push(x, y - 1, km_row, &mut dist, &mut q); }
            if y + 1 < HEIGHT { push(x, y + 1, km_row, &mut dist, &mut q); }
        }

        let mut vals: Vec<(f64, f64)> = Vec::new();
        for y in 0..HEIGHT {
            let w = cos_lat(y);
            for x in 0..WIDTH {
                let i = y * WIDTH + x;
                if !is_ocean[i] && dist[i].is_finite() {
                    vals.push((dist[i], w));
                    all.push((dist[i], w));
                }
            }
        }
        vals.sort_by(|a, b| a.0.total_cmp(&b.0));
        let total: f64 = vals.iter().map(|v| v.1).sum();
        let q_at = |f: f64| {
            let want = total * f;
            let mut acc = 0.0;
            for (d, w) in &vals {
                acc += w;
                if acc >= want {
                    return *d;
                }
            }
            vals.last().unwrap().0
        };
        let beyond: f64 = vals.iter().filter(|(d, _)| *d > EARTH_MOST_CONTINENTAL_KM)
            .map(|(_, w)| w).sum::<f64>() / total * 100.0;
        println!(
            "{:4} | {:6.0} | {:6.0} | {:6.0} | {:6.0} | {:6.0} | {:6.1}%",
            seed, q_at(0.25), q_at(0.50), q_at(0.75), q_at(0.90),
            vals.last().unwrap().0, beyond
        );
    }

    all.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f64 = all.iter().map(|v| v.1).sum();
    let q_at = |f: f64| {
        let want = total * f;
        let mut acc = 0.0;
        for (d, w) in &all {
            acc += w;
            if acc >= want { return *d; }
        }
        all.last().unwrap().0
    };
    let beyond: f64 = all.iter().filter(|(d, _)| *d > EARTH_MOST_CONTINENTAL_KM)
        .map(|(_, w)| w).sum::<f64>() / total * 100.0;
    println!(
        "ALL  | {:6.0} | {:6.0} | {:6.0} | {:6.0} | {:6.0} | {:6.1}%",
        q_at(0.25), q_at(0.50), q_at(0.75), q_at(0.90), all.last().unwrap().0, beyond
    );
    println!("\nEarth's most continental point is {EARTH_MOST_CONTINENTAL_KM:.0} km from ocean (Xinjiang), and is desert.");
}
