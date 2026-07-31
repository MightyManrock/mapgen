// Temporary diagnostic: land fraction, landmass size distribution, and
// continental moisture statistics across seeds. Not part of the pipeline.
#[path = "../src/heatmap.rs"]
mod heatmap;
#[path = "../src/elevation.rs"]
mod elevation;
#[path = "../src/params.rs"]
mod params;
#[path = "../src/climate.rs"]
mod climate;

use std::collections::VecDeque;

const WIDTH: usize = 1024;
const HEIGHT: usize = 512;

fn cos_lat(y: usize) -> f64 {
    let lat = (y as f64 / HEIGHT as f64 - 0.5) * std::f64::consts::PI;
    lat.cos()
}

fn main() {
    let p = params::PlanetGenParams::earth_like();
    println!("seed | land% | area-land% | landmasses>0.5% | largest% | top3% | mean precip(land) | precip<0.10% | polar-touching%");
    for seed in [42u32, 7, 99, 123, 2024, 555, 8, 314] {
        let mut elev = elevation::generate_elevation(WIDTH, HEIGHT, seed, p.warp_strength);
        elev.roughen_coastline(p.sea_level, seed.wrapping_add(10));
        let is_ocean = elevation::flood_fill_ocean(&elev.data, WIDTH, HEIGHT, p.sea_level);

        let n = WIDTH * HEIGHT;
        let land_cells = (0..n).filter(|&i| !is_ocean[i]).count();
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
        let mut comp = vec![usize::MAX; n];
        let mut sizes: Vec<f64> = Vec::new();
        for start in 0..n {
            if is_ocean[start] || comp[start] != usize::MAX {
                continue;
            }
            let id = sizes.len();
            let mut area = 0.0;
            let mut q = VecDeque::from([start]);
            comp[start] = id;
            while let Some(idx) = q.pop_front() {
                let (x, y) = (idx % WIDTH, idx / WIDTH);
                area += cos_lat(y);
                for (nx, ny) in heatmap::neighbors_8(x, y, WIDTH, HEIGHT) {
                    let nidx = ny * WIDTH + nx;
                    if !is_ocean[nidx] && comp[nidx] == usize::MAX {
                        comp[nidx] = id;
                        q.push_back(nidx);
                    }
                }
            }
                let touches_pole = (0..WIDTH).any(|x| comp[x] == id || comp[(HEIGHT - 1) * WIDTH + x] == id);
            sizes.push(if touches_pole { -area } else { area });
        }
        let polar: f64 = sizes.iter().filter(|s| **s < 0.0).map(|s| -s).sum::<f64>() / area_land * 100.0;
        let mut sizes: Vec<f64> = sizes.iter().map(|s| s.abs()).collect();
        sizes.sort_by(|a, b| b.total_cmp(a));
        let big = sizes.iter().filter(|&&s| s / area_tot > 0.005).count();
        let largest = sizes[0] / area_tot * 100.0;
        let top3: f64 = sizes.iter().take(3).sum::<f64>() / area_land * 100.0;

        // Climate
        let base_t = climate::generate_temperature(&elev, &p, 0.0, seed);
        let t = climate::apply_ocean_currents(&base_t, &is_ocean, &p);
        let ice = climate::generate_sea_ice(&t, &is_ocean, p.sea_ice_temp_threshold);
        let precip = climate::generate_precipitation(&elev, &is_ocean, &t, &ice, &p, 0.0, seed);
        let mut sum = 0.0;
        let mut dry = 0usize;
        for i in 0..n {
            if !is_ocean[i] {
                sum += precip.data[i];
                if precip.data[i] < 0.10 {
                    dry += 1;
                }
            }
        }
        println!(
            "{:5} | {:5.1} | {:10.1} | {:15} | {:8.1} | {:5.1} | {:17.3} | {:11.1} | {:16.1}",
            seed,
            land_cells as f64 / n as f64 * 100.0,
            area_land / area_tot * 100.0,
            big,
            largest,
            top3,
            sum / land_cells as f64,
            dry as f64 / land_cells as f64 * 100.0,
            polar
        );
    }
}
