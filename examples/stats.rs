// Measurement harness: land fraction, landmass size distribution, and
// continental moisture, across seeds. Compares the current configuration
// against the pre-change *elevation* params (see `baseline_params`).
//
//   cargo run --release --example stats
#[path = "../src/heatmap.rs"]
mod heatmap;
#[path = "../src/elevation.rs"]
mod elevation;
#[path = "../src/params.rs"]
mod params;
#[path = "../src/climate.rs"]
mod climate;

use std::collections::VecDeque;

use params::PlanetGenParams;

const WIDTH: usize = 1024;
const HEIGHT: usize = 512;
const SEEDS: [u32; 8] = [42, 7, 99, 123, 2024, 555, 8, 314];
const SEASON_PHASE: f64 = 0.25;

fn cos_lat(y: usize) -> f64 {
    ((y as f64 / HEIGHT as f64 - 0.5) * std::f64::consts::PI).cos()
}

/// Elevation/tuning params as they stood before the target-land-fraction
/// change. Reverts *parameters only* — the climate fixes in
/// `generate_temperature` cannot be toggled off, so this isolates the
/// elevation change rather than showing the full before/after.
fn baseline_params() -> PlanetGenParams {
    let mut p = PlanetGenParams::earth_like();
    p.target_land_fraction = None;
    p.sea_level = 0.525;
    p.lapse_factor = 0.3;
    p.slope_threshold = 0.015;
    p.max_lake_fill = 0.04;
    p
}

fn main() {
    for (label, p, phase) in [
        ("OLD ELEVATION PARAMS (climate fixes still applied)", baseline_params(), SEASON_PHASE),
        ("CURRENT", PlanetGenParams::earth_like(), SEASON_PHASE),
    ] {
        println!("\n=== {label} ===");
        println!(
            "seed | land% | landmasses>0.5% | largest% | top3% | polar-touching% | \
             mean precip | precip<0.10%"
        );
        for seed in SEEDS {
            let mut elev = elevation::generate_elevation(
                WIDTH,
                HEIGHT,
                seed,
                p.warp_strength,
                p.target_land_fraction,
            );
            elev.roughen_coastline(p.sea_level, seed.wrapping_add(10));
            let is_ocean = elevation::flood_fill_ocean(&elev.data, WIDTH, HEIGHT, p.sea_level);

            let n = WIDTH * HEIGHT;
            let mut area_land = 0.0;
            let mut area_tot = 0.0;
            for y in 0..HEIGHT {
                let w = cos_lat(y);
                for x in 0..WIDTH {
                    area_tot += w;
                    if !is_ocean[y * WIDTH + x] {
                        area_land += w;
                    }
                }
            }

            // Connected landmasses (area-weighted), 8-neighborhood, x wraps.
            // Landmasses touching a pole row are flagged: x wrapping links every
            // longitude there, so polar caps merge otherwise-separate continents.
            let mut comp = vec![usize::MAX; n];
            let mut sizes: Vec<(f64, bool)> = Vec::new();
            for start in 0..n {
                if is_ocean[start] || comp[start] != usize::MAX {
                    continue;
                }
                let id = sizes.len();
                let mut area = 0.0;
                let mut q = VecDeque::from([start]);
                comp[start] = id;
                while let Some(idx) = q.pop_front() {
                    area += cos_lat(idx / WIDTH);
                    for (nx, ny) in heatmap::neighbors_8(idx % WIDTH, idx / WIDTH, WIDTH, HEIGHT) {
                        let nidx = ny * WIDTH + nx;
                        if !is_ocean[nidx] && comp[nidx] == usize::MAX {
                            comp[nidx] = id;
                            q.push_back(nidx);
                        }
                    }
                }
                let polar = (0..WIDTH)
                    .any(|x| comp[x] == id || comp[(HEIGHT - 1) * WIDTH + x] == id);
                sizes.push((area, polar));
            }
            let polar_pct: f64 =
                sizes.iter().filter(|(_, p)| *p).map(|(a, _)| a).sum::<f64>() / area_land * 100.0;
            let mut areas: Vec<f64> = sizes.iter().map(|(a, _)| *a).collect();
            areas.sort_by(|a, b| b.total_cmp(a));
            let big = areas.iter().filter(|&&s| s / area_tot > 0.005).count();
            let top3: f64 = areas.iter().take(3).sum::<f64>() / area_land * 100.0;

            let base_t = climate::generate_temperature(&elev, &p, phase, seed);
            let t = climate::apply_ocean_currents(&base_t, &is_ocean, &p);
            let ice = climate::generate_sea_ice(&t, &is_ocean, p.sea_ice_temp_threshold);
            let precip =
                climate::generate_precipitation(&elev, &is_ocean, &t, &ice, &p, phase, seed);

            let mut p_sum = 0.0;
            let mut dry = 0.0;
            for y in 0..HEIGHT {
                let w = cos_lat(y);
                for x in 0..WIDTH {
                    let i = y * WIDTH + x;
                    if !is_ocean[i] {
                        p_sum += precip.data[i] * w;
                        if precip.data[i] < 0.10 {
                            dry += w;
                        }
                    }
                }
            }

            println!(
                "{:5} | {:5.1} | {:15} | {:8.1} | {:5.1} | {:15.1} | {:11.3} | {:11.1}",
                seed,
                area_land / area_tot * 100.0,
                big,
                areas[0] / area_tot * 100.0,
                top3,
                polar_pct,
                p_sum / area_land,
                dry / area_land * 100.0
            );
        }
    }
}
