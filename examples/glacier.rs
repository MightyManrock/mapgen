// Measurement harness: temperature, glacier cover, sea ice cover, and the
// precipitation that depends on them. Sea ice suppresses evaporation, so ice
// extent feeds back into continental dryness.
//
//   cargo run --release --example glacier          # baseline vs current
//   cargo run --release --example glacier seasons  # current, across the year
#[path = "../src/heatmap.rs"]
mod heatmap;
#[path = "../src/elevation.rs"]
mod elevation;
#[path = "../src/params.rs"]
mod params;
#[path = "../src/climate.rs"]
mod climate;

use params::PlanetGenParams;

const WIDTH: usize = 1024;
const HEIGHT: usize = 512;
const SEEDS: [u32; 8] = [42, 7, 99, 123, 2024, 555, 8, 314];
const SEASON_PHASE: f64 = 0.25;

fn cos_lat(y: usize) -> f64 {
    ((y as f64 / HEIGHT as f64 - 0.5) * std::f64::consts::PI).cos()
}

/// Elevation/tuning params as they stood before the target-land-fraction
/// change. NOTE this reverts *parameters only* — `generate_temperature` still
/// contains the signed-season and above-sea-level-lapse fixes, which cannot be
/// toggled off. So this arm isolates the elevation change, not the full
/// before/after. True pre-change climate figures (glacier 1.5% of land, sea ice
/// 0.3% of ocean, mean land precip 0.138) are recorded in
/// `docs/superpowers/specs/2026-07-31-target-land-fraction-design.md`.
fn baseline_params() -> PlanetGenParams {
    let mut p = PlanetGenParams::earth_like();
    p.target_land_fraction = None;
    p.sea_level = 0.525;
    p.lapse_factor = 0.3;
    p.slope_threshold = 0.015;
    p.max_lake_fill = 0.04;
    p
}

struct Row {
    land_pct: f64,
    mean_land_t: f64,
    glacier_pct: f64,
    seaice_pct: f64,
    mean_land_p: f64,
}

fn run(seed: u32, p: &PlanetGenParams, phase: f64) -> Row {
    let mut elev =
        elevation::generate_elevation(WIDTH, HEIGHT, seed, p.warp_strength, p.target_land_fraction);
    elev.roughen_coastline(p.sea_level, seed.wrapping_add(10));
    let is_ocean = elevation::flood_fill_ocean(&elev.data, WIDTH, HEIGHT, p.sea_level);

    let base_t = climate::generate_temperature(&elev, p, phase, seed);
    let t = climate::apply_ocean_currents(&base_t, &is_ocean, p);
    let ice = climate::generate_sea_ice(&t, &is_ocean, p.sea_ice_temp_threshold);
    let glac = climate::generate_glacier(&t, &is_ocean, p.glacier_temp_threshold);
    let precip = climate::generate_precipitation(&elev, &is_ocean, &t, &ice, p, phase, seed);

    let (mut land_a, mut ocean_a, mut tot_a) = (0.0, 0.0, 0.0);
    let (mut glac_a, mut ice_a, mut t_sum, mut p_sum) = (0.0, 0.0, 0.0, 0.0);
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

fn report(label: &str, p: &PlanetGenParams, phase: f64) {
    println!("\n=== {label} ===");
    println!("seed | land% | mean land T | glacier% of land | sea ice% of ocean | mean land precip");
    let (mut l, mut t, mut g, mut i, mut pr) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for seed in SEEDS {
        let r = run(seed, p, phase);
        l += r.land_pct;
        t += r.mean_land_t;
        g += r.glacier_pct;
        i += r.seaice_pct;
        pr += r.mean_land_p;
        println!(
            "{:5} | {:5.1} | {:11.3} | {:16.1} | {:17.1} | {:16.3}",
            seed, r.land_pct, r.mean_land_t, r.glacier_pct, r.seaice_pct, r.mean_land_p
        );
    }
    let n = SEEDS.len() as f64;
    println!(
        "MEAN  | {:5.1} | {:11.3} | {:16.1} | {:17.1} | {:16.3}",
        l / n,
        t / n,
        g / n,
        i / n,
        pr / n
    );
}

fn main() {
    if std::env::args().any(|a| a == "seasons") {
        let p = PlanetGenParams::earth_like();
        for (name, phase) in [("solstice N", 0.0), ("equinox", 0.25), ("solstice S", 0.5)] {
            report(name, &p, phase);
        }
        return;
    }
    // Same season on both arms, so the difference isolates the elevation change.
    report("OLD ELEVATION PARAMS (climate fixes still applied)", &baseline_params(), SEASON_PHASE);
    report("CURRENT", &PlanetGenParams::earth_like(), SEASON_PHASE);
}
