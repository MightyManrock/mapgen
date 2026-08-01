// Diagnostic: the precipitation *ceiling* by latitude, versus the thresholds
// Region::character() needs to award each biome.
//
// precip = band(lat) * (base_arid + moisture*(1-base_arid)) * moisture_capacity
//
// so with moisture saturated at 1.0 the ceiling is band * capacity. If that
// ceiling sits below a biome's threshold, that biome is unreachable at that
// latitude no matter how wet the airmass gets — no amount of coastline or
// recycling can produce it.
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
const SEEDS: [u32; 4] = [42, 7, 123, 2024];
const PHASE: f64 = 0.25;

/// Mirror of `lat_band_factor` (private to climate.rs).
fn band(eff_lat: f64) -> f64 {
    let stops: &[(f64, f64)] = &[
        (0.00, 1.00), (0.17, 0.90), (0.33, 0.20), (0.50, 0.60),
        (0.65, 0.65), (0.78, 0.30), (1.00, 0.10),
    ];
    for i in 0..stops.len() - 1 {
        let (ta, va) = stops[i];
        let (tb, vb) = stops[i + 1];
        if eff_lat <= tb {
            return va + (vb - va) * (eff_lat - ta) / (tb - ta);
        }
    }
    stops.last().unwrap().1
}

fn cos_lat(y: usize) -> f64 {
    ((y as f64 / HEIGHT as f64 - 0.5) * std::f64::consts::PI).cos()
}

fn main() {
    let p = PlanetGenParams::earth_like();

    // Observed mean land temperature per latitude band, so the ceiling uses
    // realistic moisture_capacity rather than an assumed one.
    let mut t_sum = vec![0.0; 10];
    let mut t_cnt = vec![0.0; 10];
    let mut p_max = vec![0.0f64; 10];
    let mut p_sum = vec![0.0; 10];
    let mut p_cnt = vec![0.0; 10];
    let mut can_veg = vec![0.0; 10];
    let mut can_wet = vec![0.0; 10];

    for seed in SEEDS {
        let mut elev = elevation::generate_elevation(WIDTH, HEIGHT, seed, &p);
        elev.roughen_coastline(p.sea_level, seed.wrapping_add(10));
        let is_ocean = elevation::flood_fill_ocean(&elev.data, WIDTH, HEIGHT, p.sea_level);
        let base_t = climate::generate_temperature(&elev, &p, PHASE, seed);
        let t = climate::apply_ocean_currents(&base_t, &is_ocean, &p);
        let ice = climate::generate_sea_ice(&t, &is_ocean, p.sea_ice_temp_threshold);
        let precip = climate::generate_precipitation(&elev, &is_ocean, &t, &ice, &p, PHASE, seed);

        for y in 0..HEIGHT {
            let abs_lat = (y as f64 - HEIGHT as f64 / 2.0).abs() / (HEIGHT as f64 / 2.0);
            let b = ((abs_lat * 10.0) as usize).min(9);
            let w = cos_lat(y);
            for x in 0..WIDTH {
                let i = y * WIDTH + x;
                if is_ocean[i] {
                    continue;
                }
                // Per-cell ceiling: this cell's own band factor and capacity
                // with moisture saturated. Answers "could this cell ever be
                // forest", rather than "could the average cell at this latitude".
                let cap_i = (0.3 + 0.7 * t.data[i]).clamp(0.3, 1.0);
                let ceil_i = band(abs_lat) * cap_i;
                let (need_veg, need_wet) = if t.data[i] < 0.20 {
                    (0.20, f64::INFINITY)          // tundra
                } else if t.data[i] < 0.35 {
                    (0.20, f64::INFINITY)          // boreal forest
                } else if t.data[i] < 0.55 {
                    (0.35, 0.60)                   // temperate forest / rainforest
                } else if t.data[i] < 0.70 {
                    (0.45, f64::INFINITY)          // subtropical forest
                } else {
                    (0.45, 0.65)                   // tropical dry forest / rainforest
                };
                if ceil_i >= need_veg { can_veg[b] += w; }
                if ceil_i >= need_wet { can_wet[b] += w; }
                t_sum[b] += t.data[i] * w;
                t_cnt[b] += w;
                p_sum[b] += precip.data[i] * w;
                p_cnt[b] += w;
                p_max[b] = p_max[b].max(precip.data[i]);
            }
        }
    }

    println!("            |       |  mean |       | ceiling | observed | thresholds needed");
    println!(" |lat| band |  band |  land | capac | (band*  |  mean /  | steppe forest rainf");
    println!("            |       |     T |  -ity |  capac) |     max  | (at that temperature)");
    for b in 0..10 {
        if t_cnt[b] == 0.0 {
            continue;
        }
        let lat_mid = (b as f64 + 0.5) / 10.0;
        let mean_t = t_sum[b] / t_cnt[b];
        let cap = (0.3 + 0.7 * mean_t).clamp(0.3, 1.0);
        let bf = band(lat_mid);
        let ceiling = bf * cap;

        // Thresholds Region::character() applies at this mean temperature.
        let (dry, mid, wet) = if mean_t < 0.20 {
            (0.20, f64::NAN, f64::NAN)
        } else if mean_t < 0.35 {
            (0.20, f64::NAN, f64::NAN)
        } else if mean_t < 0.55 {
            (0.15, 0.35, 0.60)
        } else if mean_t < 0.70 {
            (0.20, 0.45, f64::NAN)
        } else {
            (0.20, 0.45, 0.65)
        };

        println!(
            "  {:.2}-{:.2} | {:5.2} | {:5.3} | {:5.3} |  {:6.3} | {:5.3} / {:5.3} | {:5.2} {:6.2} {:6.2}{}",
            b as f64 / 10.0, (b + 1) as f64 / 10.0,
            bf, mean_t, cap, ceiling,
            p_sum[b] / p_cnt[b], p_max[b],
            dry, mid, wet,
            format!("  | could-be-forest {:5.1}%  could-be-rainforest {:5.1}%",
                    can_veg[b] / t_cnt[b] * 100.0, can_wet[b] / t_cnt[b] * 100.0)
        );
    }
}
