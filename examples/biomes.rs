// Measurement harness: land biome mix, and precipitation within the
// subtropical desert belt specifically.
//
// Mean precipitation can rise while the climate zones stay correct, or while
// the deserts quietly disappear — those look identical in an average. This
// classifies every land cell with the same thresholds `Region::character()`
// uses, so the zone structure itself is checked.
//
//   cargo run --release --example biomes
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

/// The subtropical dry belt, in normalized |latitude| — the horse latitudes,
/// where the circulation model puts its deserts (`lat_band_factor` bottoms out
/// at 0.33). If deserts survive anywhere they must survive here.
const SUBTROPICS: (f64, f64) = (0.28, 0.40);

fn cos_lat(y: usize) -> f64 {
    ((y as f64 / HEIGHT as f64 - 0.5) * std::f64::consts::PI).cos()
}

/// Per-km decay but no recycling. This isolates the *recycling* half of the
/// change; the per-km half cannot be disabled by parameters, since `land_decay`
/// is now in per-1000-km units by definition. (0.68 there is the equivalent of
/// the old per-cell 0.985 at the equator.)
fn no_recycling() -> PlanetGenParams {
    let mut p = PlanetGenParams::earth_like();
    p.land_recycle_floor = 0.0;
    p
}

/// Proposed thresholds, derived from annual rainfall at 1.0 == 3000 mm/yr.
///   desert       <250mm  -> 0.083
///   steppe    250-500mm  -> 0.167
///   savanna  500-1000mm  -> 0.333
///   forest top   2000mm  -> 0.667
///   boreal        300mm  -> 0.100
///   tundra        200mm  -> 0.067
fn classify_new(temp: f64, precip: f64) -> usize {
    if temp < 0.20 {
        return if precip < 0.067 { 0 } else { 4 };
    }
    if temp < 0.35 {
        return if precip < 0.100 { 0 } else { 2 };
    }
    if temp < 0.55 {
        if precip < 0.083 { return 0; }
        if precip < 0.167 { return 1; }
        if precip < 0.667 { return 2; }
        return 3;
    }
    if temp < 0.70 {
        if precip < 0.083 { return 0; }
        if precip < 0.300 { return 1; }
        return 2;
    }
    if precip < 0.083 { return 0; }
    if precip < 0.333 { return 1; }
    if precip < 0.667 { return 2; }
    3
}

/// Same thresholds as `Region::character()` in regions.rs.
fn classify(temp: f64, precip: f64) -> usize {
    if temp < 0.20 {
        return if precip < 0.20 { 0 } else { 4 }; // polar desert / tundra
    }
    if temp < 0.35 {
        return if precip < 0.20 { 0 } else { 2 }; // cold desert / boreal
    }
    if temp < 0.55 {
        if precip < 0.15 { return 0; }
        if precip < 0.35 { return 1; }
        if precip < 0.60 { return 2; }
        return 3;
    }
    if temp < 0.70 {
        if precip < 0.20 { return 0; }
        if precip < 0.45 { return 1; }
        return 2;
    }
    if precip < 0.20 { return 0; }
    if precip < 0.45 { return 1; }
    if precip < 0.65 { return 2; }
    3
}

const NAMES: [&str; 5] = ["desert", "steppe/savanna", "forest", "rainforest", "tundra"];

fn main() {
    for (label, p) in [
        ("PER-KM DECAY ONLY (recycling off)", no_recycling()),
        ("PER-KM DECAY + RECYCLING (shipping)", PlanetGenParams::earth_like()),
    ] {
        let mut mix = [0.0f64; 5];
        let mut land_area = 0.0;
        let mut precip_sum = 0.0;
        // Subtropical belt, land only.
        let mut sub_precip = 0.0;
        let mut sub_area = 0.0;
        let mut sub_desert = 0.0;
        let mut land_precip: Vec<f64> = Vec::new();
        let mut mix_new = [0.0f64; 5];
        let mut sub_desert_new = 0.0;

        for seed in SEEDS {
            let mut elev = elevation::generate_elevation(
                WIDTH, HEIGHT, seed, p.warp_strength, p.target_land_fraction,
            );
            elev.roughen_coastline(p.sea_level, seed.wrapping_add(10));
            let is_ocean = elevation::flood_fill_ocean(&elev.data, WIDTH, HEIGHT, p.sea_level);
            let base_t = climate::generate_temperature(&elev, &p, SEASON_PHASE, seed);
            let t = climate::apply_ocean_currents(&base_t, &is_ocean, &p);
            let ice = climate::generate_sea_ice(&t, &is_ocean, p.sea_ice_temp_threshold);
            let precip =
                climate::generate_precipitation(&elev, &is_ocean, &t, &ice, &p, SEASON_PHASE, seed);

            for y in 0..HEIGHT {
                let w = cos_lat(y);
                let abs_lat = (y as f64 - HEIGHT as f64 / 2.0).abs() / (HEIGHT as f64 / 2.0);
                for x in 0..WIDTH {
                    let i = y * WIDTH + x;
                    if is_ocean[i] {
                        continue;
                    }
                    let c = classify(t.data[i], precip.data[i]);
                    mix[c] += w;
                    mix_new[classify_new(t.data[i], precip.data[i])] += w;
                    land_area += w;
                    precip_sum += precip.data[i] * w;
                    land_precip.push(precip.data[i]);
                    if abs_lat >= SUBTROPICS.0 && abs_lat <= SUBTROPICS.1 {
                        sub_area += w;
                        sub_precip += precip.data[i] * w;
                        if c == 0 {
                            sub_desert += w;
                        }
                        if classify_new(t.data[i], precip.data[i]) == 0 {
                            sub_desert_new += w;
                        }
                    }
                }
            }
        }

        println!("\n=== {label} ===");
        println!("  mean land precipitation : {:.3}", precip_sum / land_area);
        print!("  biome mix               :");
        for c in 0..5 {
            print!("  {} {:.1}%", NAMES[c], mix[c] / land_area * 100.0);
        }
        println!();
        // Where land precipitation actually sits on the [0,1] scale the biome
        // colour ramp lerps over. If the mass sits in the bottom third, the
        // render reads desert regardless of how the cells classify.
        land_precip.sort_by(|a, b| a.total_cmp(b));
        let q = |f: f64| land_precip[((land_precip.len() - 1) as f64 * f) as usize];
        print!("  biome mix (RECALIBRATED)  :");
        for c in 0..5 {
            print!("  {} {:.1}%", NAMES[c], mix_new[c] / land_area * 100.0);
        }
        println!();
        println!(
            "  land precip percentiles : p10 {:.3}  p50 {:.3}  p90 {:.3}  p99 {:.3}",
            q(0.10), q(0.50), q(0.90), q(0.99)
        );
        println!(
            "  subtropical belt (|lat| {:.2}-{:.2}): mean precip {:.3}, {:.1}% classified desert",
            SUBTROPICS.0,
            SUBTROPICS.1,
            sub_precip / sub_area,
            sub_desert / sub_area * 100.0
        );
        println!(
            "  subtropical belt, RECALIBRATED: {:.1}% classified desert",
            sub_desert_new / sub_area * 100.0
        );
    }
}
