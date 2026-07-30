mod climate;
mod elevation;
mod export;
mod greening;
mod heatmap;
mod hydrology;
mod params;
mod polygons;
mod regions;
mod render;
mod shore;

use params::PlanetGenParams;

const SEED: u32 = 42;
const WIDTH: usize = 1024;
const HEIGHT: usize = 512;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all("output")?;
    let params = PlanetGenParams::earth_like();

    let mut elev = elevation::generate_elevation(WIDTH, HEIGHT, SEED, params.warp_strength);
    elev.roughen_coastline(params.sea_level, SEED.wrapping_add(10));
    let is_ocean = elevation::flood_fill_ocean(&elev.data, WIDTH, HEIGHT, params.sea_level);

    let base_temperature = climate::generate_temperature(&elev, &params, 0.0, SEED);
    let temperature = climate::apply_ocean_currents(&base_temperature, &is_ocean, &params);
    let is_sea_ice = climate::generate_sea_ice(&temperature, &is_ocean, params.sea_ice_temp_threshold);
    let precipitation =
        climate::generate_precipitation(&elev, &is_ocean, &temperature, &is_sea_ice, &params, 0.0, SEED);
    let is_glacier = climate::generate_glacier(&temperature, &is_ocean, params.glacier_temp_threshold);

    // Hydrology consumes the base precipitation; large lakes in its output
    // then feed a one-way climate adjustment (no feedback into hydrology).
    let hydro = hydrology::generate_hydrology(&elev, &is_ocean, &precipitation, &is_glacier, &params);
    let lake_halo = climate::compute_lake_halo(&hydro.map, &is_ocean, &params);
    let precipitation = climate::apply_lake_precip(&precipitation, &lake_halo, &params);

    let aridity = climate::generate_aridity(&temperature, &precipitation, params.et_factor);
    let diurnal_swing = climate::apply_lake_swing(
        &climate::generate_diurnal_swing(&temperature, &aridity, &params),
        &lake_halo,
        &params,
    );

    let (region_map, regions) = regions::detect_regions(
        &elev, &temperature, &diurnal_swing, &precipitation, &aridity,
        &is_ocean, &is_glacier, &is_sea_ice,
        params.land_threshold, params.ocean_threshold, params.region_min_size,
        params.island_coast_dist, params.island_arch_dist, params.lon_weight,
    );

    let green = greening::compute_greening(&hydro.map, &hydro.aquifer_zones, &is_ocean, &temperature, &params);
    let mouth_influence = shore::compute_mouth_influence(&hydro.map, &is_ocean, &params);

    render::save_elevation(&elev, "output/elevation.png")?;
    render::save_temperature(&temperature, "output/temperature.png")?;
    render::save_precipitation(&precipitation, "output/precipitation.png")?;
    render::save_aridity(&aridity, "output/aridity.png")?;
    render::save_diurnal_swing(&diurnal_swing, "output/diurnal_swing.png")?;
    render::save_hydrology(&hydro.map, "output/hydrology.png")?;
    render::save_glacier(&is_glacier, &temperature, params.glacier_temp_threshold, WIDTH, HEIGHT, "output/glacier.png")?;
    render::save_sea_ice(&is_sea_ice, &temperature, params.sea_ice_temp_threshold, WIDTH, HEIGHT, "output/sea_ice.png")?;
    render::save_ocean_currents(WIDTH, HEIGHT, &is_ocean, &params, "output/ocean_currents.png")?;
    render::save_greening(WIDTH, HEIGHT, &is_ocean, &green, "output/greening.png")?;
    render::save_shore(
        WIDTH, HEIGHT, &elev, &temperature, &precipitation, &mouth_influence, &is_ocean, &params,
        "output/shore.png",
    )?;
    render::save_composite(
        WIDTH, HEIGHT, &hydro.map, &elev, &temperature, &precipitation, &green, &mouth_influence,
        &is_ocean, &is_glacier, &is_sea_ice, &params, "output/composite.png",
    )?;
    render::save_regions(
        WIDTH, HEIGHT, &hydro.map, &elev, &temperature, &precipitation, &green, &mouth_influence,
        &is_ocean, &is_glacier, &is_sea_ice, &params, &region_map, &regions, "output/regions.png",
    )?;

    export::save_markers_json(
        WIDTH * render::RENDER_SCALE,
        HEIGHT * render::RENDER_SCALE,
        &params,
        "composite.png",
        &regions,
        WIDTH,
        HEIGHT,
        "output/composite.markers.json",
    )?;

    println!("Done — 13 layers written to output/");
    Ok(())
}
