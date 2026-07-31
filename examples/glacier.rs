// Temporary diagnostic: compare baseline vs. (normalization + lapse fix) on
// temperature, glacier cover, sea ice cover, and the precipitation that
// depends on them. Sea ice suppresses evaporation, so ice extent feeds back
// into the continental-dryness problem.
#[path = "../src/heatmap.rs"]
mod heatmap;
#[path = "../src/elevation.rs"]
mod elevation;
#[path = "../src/params.rs"]
mod params;
#[path = "../src/climate.rs"]
mod climate;

use heatmap::HeatMap;
use params::PlanetGenParams;

const WIDTH: usize = 1024;
const HEIGHT: usize = 512;
const TARGET: f64 = 0.30;
const SEEDS: [u32; 8] = [42, 7, 99, 123, 2024, 555, 8, 314];

fn cos_lat(y: usize) -> f64 {
    ((y as f64 / HEIGHT as f64 - 0.5) * std::f64::consts::PI).cos()
}

fn weighted_sea_level(data: &[f64], target_land: f64) -> f64 {
    let mut pairs: Vec<(f64, f64)> = (0..data.len())
        .map(|i| (data[i], cos_lat(i / WIDTH)))
        .collect();
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f64 = pairs.iter().map(|p| p.1).sum();
    let want = total * (1.0 - target_land);
    let mut acc = 0.0;
    for (v, w) in &pairs {
        acc += w;
        if acc >= want {
            return *v;
        }
    }
    1.0
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * q).round() as usize]
}

fn normalize_about(data: &mut [f64], sl: f64, lo: f64, hi: f64) {
    for v in data.iter_mut() {
        *v = if *v < sl {
            (0.5 * (*v - lo) / (sl - lo)).clamp(0.0, 0.5)
        } else {
            (0.5 + 0.5 * (*v - sl) / (hi - sl)).clamp(0.5, 1.0)
        };
    }
}

/// Temperature with a selectable lapse model.
/// `fixed_lapse == false` reproduces the current code (raw elevation).
/// `fixed_lapse == true` uses height above sea level.
fn temperature(elev: &HeatMap, p: &PlanetGenParams, seed: u32, fixed_lapse: bool) -> HeatMap {
    let base = climate::generate_temperature(elev, p, 0.0, seed);
    if !fixed_lapse {
        return base;
    }
    // Undo the raw-elevation lapse, reapply the above-sea-level form.
    let sl = p.sea_level;
    let data = (0..base.data.len())
        .map(|i| {
            let e = elev.data[i];
            let above = ((e - sl).max(0.0) / (1.0 - sl)) * p.lapse_factor;
            (base.data[i] + e * p.lapse_factor - above).clamp(0.0, 1.0)
        })
        .collect();
    HeatMap { width: base.width, height: base.height, data }
}

struct Row {
    land_pct: f64,
    mean_land_t: f64,
    glacier_pct: f64,
    seaice_pct: f64,
    mean_land_p: f64,
}

fn run(seed: u32, normalized: bool, fixed_lapse: bool) -> Row {
    run_lf(seed, normalized, fixed_lapse, PlanetGenParams::earth_like().lapse_factor)
}

fn run_lf(seed: u32, normalized: bool, fixed_lapse: bool, lapse_factor: f64) -> Row {
    let mut p = PlanetGenParams::earth_like();
    p.lapse_factor = lapse_factor;
    let mut elev = elevation::generate_elevation(WIDTH, HEIGHT, seed, p.warp_strength);

    if normalized {
        let sl = weighted_sea_level(&elev.data, TARGET);
        let mut sorted = elev.data.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let lo = quantile(&sorted, 0.001);
        let hi = quantile(&sorted, 0.999);
        normalize_about(&mut elev.data, sl, lo, hi);
        p.sea_level = 0.5;
        p.max_lake_fill = 0.062;
        p.slope_threshold = 0.023;
    }

    elev.roughen_coastline(p.sea_level, seed.wrapping_add(10));
    let is_ocean = elevation::flood_fill_ocean(&elev.data, WIDTH, HEIGHT, p.sea_level);

    let base_t = temperature(&elev, &p, seed, fixed_lapse);
    let t = climate::apply_ocean_currents(&base_t, &is_ocean, &p);
    let ice = climate::generate_sea_ice(&t, &is_ocean, p.sea_ice_temp_threshold);
    let glac = climate::generate_glacier(&t, &is_ocean, p.glacier_temp_threshold);
    let precip = climate::generate_precipitation(&elev, &is_ocean, &t, &ice, &p, 0.0, seed);

    let n = WIDTH * HEIGHT;
    let (mut land_a, mut ocean_a, mut tot_a) = (0.0, 0.0, 0.0);
    let (mut glac_a, mut ice_a) = (0.0, 0.0);
    let (mut t_sum, mut p_sum) = (0.0, 0.0);
    for y in 0..HEIGHT {
        let w = cos_lat(y);
        for x in 0..WIDTH {
            let i = y * WIDTH + x;
            tot_a += w;
            if is_ocean[i] {
                ocean_a += w;
                if ice[i] {
                    ice_a += w;
                }
            } else {
                land_a += w;
                t_sum += t.data[i] * w;
                p_sum += precip.data[i] * w;
                if glac[i] {
                    glac_a += w;
                }
            }
        }
    }
    Row {
        land_pct: land_a / tot_a * 100.0,
        mean_land_t: t_sum / land_a,
        glacier_pct: glac_a / land_a * 100.0,
        seaice_pct: ice_a / ocean_a * 100.0,
        mean_land_p: p_sum / land_a,
    }
}

fn main() {
    if std::env::args().any(|a| a == "sweep") {
        println!("lapse_factor | mean land T | glacier% of land | sea ice% of ocean | land precip");
        for lf in [0.30, 0.45, 0.60, 0.70, 0.80, 0.90, 1.00] {
            let (mut t, mut g, mut i, mut pr) = (0.0, 0.0, 0.0, 0.0);
            for seed in SEEDS {
                let r = run_lf(seed, true, true, lf);
                t += r.mean_land_t; g += r.glacier_pct; i += r.seaice_pct; pr += r.mean_land_p;
            }
            let n = SEEDS.len() as f64;
            println!("{:12.2} | {:11.3} | {:16.1} | {:17.1} | {:11.3}", lf, t/n, g/n, i/n, pr/n);
        }
        return;
    }
    for (label, normalized, fixed_lapse) in [
        ("BASELINE (current code)", false, false),
        ("NORMALIZED, raw lapse", true, false),
        ("NORMALIZED + lapse fix", true, true),
    ] {
        println!("\n=== {label} ===");
        println!("seed | land% | mean land T | glacier% of land | sea ice% of ocean | mean land precip");
        let (mut g, mut i, mut pr) = (0.0, 0.0, 0.0);
        for seed in SEEDS {
            let r = run(seed, normalized, fixed_lapse);
            g += r.glacier_pct;
            i += r.seaice_pct;
            pr += r.mean_land_p;
            println!(
                "{:5} | {:5.1} | {:11.3} | {:16.1} | {:17.1} | {:16.3}",
                seed, r.land_pct, r.mean_land_t, r.glacier_pct, r.seaice_pct, r.mean_land_p
            );
        }
        let n = SEEDS.len() as f64;
        println!("MEAN  |       |             | {:16.1} | {:17.1} | {:16.3}", g / n, i / n, pr / n);
    }
}
