use image::{ImageBuffer, Rgb};

use crate::heatmap::HeatMap;
use crate::params::PlanetGenParams;

fn lerp_color(a: [u8; 3], b: [u8; 3], t: f64) -> [u8; 3] {
    [
        (a[0] as f64 + (b[0] as f64 - a[0] as f64) * t).round() as u8,
        (a[1] as f64 + (b[1] as f64 - a[1] as f64) * t).round() as u8,
        (a[2] as f64 + (b[2] as f64 - a[2] as f64) * t).round() as u8,
    ]
}

fn sample_gradient(t: f64, stops: &[([u8; 3], f64)]) -> [u8; 3] {
    for i in 0..stops.len() - 1 {
        let (ca, ta) = stops[i];
        let (cb, tb) = stops[i + 1];
        if t <= tb {
            let local_t = ((t - ta) / (tb - ta)).clamp(0.0, 1.0);
            return lerp_color(ca, cb, local_t);
        }
    }
    stops.last().unwrap().0
}

/// Raw elevation gradient: red (lowest) → yellow (mid) → green (highest).
fn elevation_color(t: f64) -> [u8; 3] {
    sample_gradient(
        t,
        &[
            ([255, 0, 0], 0.00),
            ([255, 255, 0], 0.50),
            ([0, 255, 0], 1.00),
        ],
    )
}

/// Water depth gradient covering rivers, lakes, and ocean.
///
/// Range mapping:
///   (0.0, 0.3] = rivers:  light cyan → medium blue
///   (0.3, 0.5] = lakes:   teal (distinct from ocean at the boundary)
///   (0.5, 1.0] = ocean:   medium blue → deep ocean
fn water_color(t: f64) -> [u8; 3] {
    sample_gradient(
        t,
        &[
            ([180, 230, 250], 0.01), // small stream
            ([80, 165, 220], 0.30),  // major river
            ([55, 175, 195], 0.30),  // lake edge (slight hue break from rivers)
            ([40, 130, 190], 0.50),  // deep lake / sea level
            ([30, 90, 180], 0.70),   // open ocean
            ([15, 40, 100], 1.00),   // deep ocean
        ],
    )
}

/// Temperature gradient: deep blue (coldest) → cyan → pale green → yellow → orange → deep red.
fn temperature_color(t: f64) -> [u8; 3] {
    sample_gradient(
        t,
        &[
            ([20, 20, 150], 0.00),   // arctic/polar
            ([70, 170, 230], 0.20),  // cold
            ([170, 220, 200], 0.40), // temperate cool
            ([240, 220, 130], 0.60), // warm
            ([230, 120, 30], 0.80),  // hot
            ([180, 20, 20], 1.00),   // extreme heat
        ],
    )
}

/// Precipitation gradient: tan (arid) → pale green → green → dark green → blue (extremely wet).
fn precipitation_color(t: f64) -> [u8; 3] {
    sample_gradient(
        t,
        &[
            ([210, 180, 130], 0.00), // hyperarid
            ([180, 200, 140], 0.20), // semi-arid
            ([100, 180, 100], 0.40), // moderate
            ([40, 130, 60], 0.60),   // wet
            ([20, 90, 130], 0.80),   // very wet
            ([10, 50, 180], 1.00),   // monsoon / extremely wet
        ],
    )
}

/// Sea ice color: flat white-grey, slightly more grey than land glacier to read
/// as a different surface. Dithers to ocean at its warm edge.
fn sea_ice_color(t: f64, threshold: f64) -> [u8; 3] {
    let normalized = (t / threshold).clamp(0.0, 1.0);
    sample_gradient(
        normalized,
        &[
            ([240, 245, 250], 0.00), // coldest — near-white
            ([195, 215, 230], 0.70), // mid — grey-blue
            ([160, 195, 220], 1.00), // warmest edge — more clearly blue-grey
        ],
    )
}

/// Glacier/ice color: pure white at coldest, pale blue at the warmer threshold edge.
/// t is the normalized temperature within the glaciated range [0, threshold].
fn glacier_color(t: f64, threshold: f64) -> [u8; 3] {
    let normalized = (t / threshold).clamp(0.0, 1.0);
    sample_gradient(
        normalized,
        &[
            ([250, 253, 255], 0.00), // pure cold white
            ([200, 228, 248], 0.60), // pale ice blue
            ([175, 212, 240], 1.00), // warmer glacier edge
        ],
    )
}

/// Effective moisture gradient: orange-tan (arid) → pale green → teal (humid).
fn aridity_color(t: f64) -> [u8; 3] {
    sample_gradient(
        t,
        &[
            ([210, 150, 80], 0.00),  // hyperarid
            ([220, 200, 130], 0.20), // arid
            ([190, 210, 150], 0.40), // semi-arid
            ([100, 175, 130], 0.60), // moderate
            ([50, 140, 120], 0.80),  // humid
            ([20, 90, 110], 1.00),   // very humid
        ],
    )
}

/// Land color from climate-space biome interpolation, elevation-shaded.
/// `temp_t`/`precip_t` are the raw (undithered) normalized [0,1] climate
/// values at this pixel — continuous by construction, no dithering needed
/// since there's no discrete category to break banding on. `land_t` is
/// the dithered, normalized elevation-above-sea-level value (same role as
/// the old `terrain_color`'s input).
fn biome_terrain_color(temp_t: f64, precip_t: f64, land_t: f64) -> [u8; 3] {
    const COLD_DRY: [u8; 3] = [150, 150, 120]; // tundra
    const COLD_WET: [u8; 3] = [40, 90, 70];    // taiga / boreal forest
    const HOT_DRY: [u8; 3]  = [210, 170, 100]; // desert
    const HOT_WET: [u8; 3]  = [30, 110, 40];   // tropical rainforest

    let low  = lerp_color(COLD_DRY, COLD_WET, precip_t);
    let high = lerp_color(HOT_DRY, HOT_WET, precip_t);
    let base = lerp_color(low, high, temp_t);
    let base_dark = [
        (base[0] as f64 * 0.75) as u8,
        (base[1] as f64 * 0.75) as u8,
        (base[2] as f64 * 0.75) as u8,
    ];

    sample_gradient(
        land_t,
        &[
            ([220, 200, 150], 0.00), // coastal sand (shared, unchanged)
            (base,            0.05), // climate-space biome color
            (base_dark,       0.55), // mild darkening toward "hills"
            ([150, 135, 110], 0.60), // highland — neutral rock-brown, no green cast
            ([140, 110, 80],  0.75), // rocky terrain (unchanged)
            ([170, 160, 150], 0.88), // grey rock (unchanged)
            ([230, 235, 240], 0.95), // snow line (unchanged)
            ([255, 255, 255], 1.00), // peak snow (unchanged)
        ],
    )
}

const BAYER_4X4: [[f64; 4]; 4] = [
    [ 0.0 / 16.0,  8.0 / 16.0,  2.0 / 16.0, 10.0 / 16.0],
    [12.0 / 16.0,  4.0 / 16.0, 14.0 / 16.0,  6.0 / 16.0],
    [ 3.0 / 16.0, 11.0 / 16.0,  1.0 / 16.0,  9.0 / 16.0],
    [15.0 / 16.0,  7.0 / 16.0, 13.0 / 16.0,  5.0 / 16.0],
];

/// Returns true if the elevation value crosses a contour boundary between
/// this pixel and either of its right/down neighbors.
fn is_contour(e: f64, e_right: f64, e_down: f64, n_contours: usize) -> bool {
    let level = |v: f64| (v * n_contours as f64).floor() as i64;
    level(e) != level(e_right) || level(e) != level(e_down)
}

/// Ordered dither: quantize t to n_levels steps, using Bayer threshold at
/// render pixel (rx, ry) to break ties at level boundaries.
fn bayer_dither(t: f64, rx: usize, ry: usize, n_levels: usize) -> f64 {
    let threshold = BAYER_4X4[ry % 4][rx % 4];
    let scaled = t * n_levels as f64;
    let lo = scaled.floor();
    let level = if scaled - lo > threshold { lo + 1.0 } else { lo };
    (level / n_levels as f64).clamp(0.0, 1.0)
}

fn save_sampled(
    heatmap: &HeatMap,
    color_fn: impl Fn(f64) -> [u8; 3],
    path: &str,
) -> Result<(), image::ImageError> {
    let width = heatmap.width as u32;
    let height = heatmap.height as u32;
    let img = ImageBuffer::from_fn(width, height, |x, y| {
        let nx = x as f64 / width as f64;
        let ny = y as f64 / height as f64;
        Rgb(color_fn(heatmap.sample(nx, ny)))
    });
    img.save(path)
}

pub fn save_elevation(elevation: &HeatMap, path: &str) -> Result<(), image::ImageError> {
    save_sampled(elevation, elevation_color, path)
}

pub fn save_temperature(temperature: &HeatMap, path: &str) -> Result<(), image::ImageError> {
    save_sampled(temperature, temperature_color, path)
}

pub fn save_precipitation(precipitation: &HeatMap, path: &str) -> Result<(), image::ImageError> {
    save_sampled(precipitation, precipitation_color, path)
}

pub fn save_aridity(aridity: &HeatMap, path: &str) -> Result<(), image::ImageError> {
    save_sampled(aridity, aridity_color, path)
}

/// Diurnal swing magnitude (day→night gap), scaled to Celsius, reusing the
/// temperature color ramp against a 40°C reference range.
pub fn save_diurnal_swing(diurnal_swing: &HeatMap, path: &str) -> Result<(), image::ImageError> {
    let width = diurnal_swing.width as u32;
    let height = diurnal_swing.height as u32;
    let img = ImageBuffer::from_fn(width, height, |x, y| {
        let i = y as usize * diurnal_swing.width + x as usize;
        let gap_c = diurnal_swing.data[i] * 70.0;
        Rgb(temperature_color((gap_c / 40.0).clamp(0.0, 1.0)))
    });
    img.save(path)
}

/// Raw hydrology: black for dry land, water gradient for wet cells.
/// `water_color(0.0)` is NOT black (the gradient extrapolates below its first
/// stop to pale cyan), so the `hydro > 0.0` guard is required.
pub fn save_hydrology(hydrology_map: &HeatMap, path: &str) -> Result<(), image::ImageError> {
    let width = hydrology_map.width as u32;
    let height = hydrology_map.height as u32;
    let img = ImageBuffer::from_fn(width, height, |x, y| {
        let nx = x as f64 / width as f64;
        let ny = y as f64 / height as f64;
        let hydro = hydrology_map.sample(nx, ny);
        let color = if hydro > 0.0 { water_color(hydro) } else { [0, 0, 0] };
        Rgb(color)
    });
    img.save(path)
}

/// Masked render (not a continuous field): pure black where the mask is
/// false, glacier-colored (by local temperature) where true.
pub fn save_glacier(
    is_glacier: &[bool],
    temperature: &HeatMap,
    threshold: f64,
    width: usize,
    height: usize,
    path: &str,
) -> Result<(), image::ImageError> {
    let img = ImageBuffer::from_fn(width as u32, height as u32, |x, y| {
        let idx = y as usize * width + x as usize;
        let nx = x as f64 / width as f64;
        let ny = y as f64 / height as f64;
        if is_glacier[idx] {
            Rgb(glacier_color(temperature.sample(nx, ny), threshold))
        } else {
            Rgb([0u8, 0, 0])
        }
    });
    img.save(path)
}

/// Masked render (not a continuous field): pure black where the mask is
/// false, sea-ice-colored (by local temperature) where true.
pub fn save_sea_ice(
    is_sea_ice: &[bool],
    temperature: &HeatMap,
    threshold: f64,
    width: usize,
    height: usize,
    path: &str,
) -> Result<(), image::ImageError> {
    let img = ImageBuffer::from_fn(width as u32, height as u32, |x, y| {
        let idx = y as usize * width + x as usize;
        let nx = x as f64 / width as f64;
        let ny = y as f64 / height as f64;
        if is_sea_ice[idx] {
            Rgb(sea_ice_color(temperature.sample(nx, ny), threshold))
        } else {
            Rgb([0u8, 0, 0])
        }
    });
    img.save(path)
}

/// Debug/verification render: plain dark land / ocean silhouette with a
/// red (warm) / blue (cool) overlay showing the current bias magnitude
/// and sign at each cell, independent of how it reads once blended into
/// the full composite/temperature renders.
pub fn save_ocean_currents(
    width: usize,
    height: usize,
    is_ocean: &[bool],
    params: &PlanetGenParams,
    path: &str,
) -> Result<(), image::ImageError> {
    let img = ImageBuffer::from_fn(width as u32, height as u32, |x, y| {
        let xi = x as usize;
        let yi = y as usize;
        let idx = yi * width + xi;
        let abs_lat = (yi as f64 - height as f64 / 2.0).abs() / (height as f64 / 2.0);
        let raw = crate::climate::current_bias_raw(xi, yi, width, is_ocean, params);
        let signed = raw * crate::climate::current_lat_envelope(abs_lat);
        let base: [u8; 3] = if is_ocean[idx] { [40, 40, 60] } else { [70, 65, 55] };
        let intensity = (signed.abs() * 255.0).clamp(0.0, 255.0) as u8;
        let color = if signed > 0.0 {
            [base[0].saturating_add(intensity), base[1], base[2]]
        } else if signed < 0.0 {
            [base[0], base[1], base[2].saturating_add(intensity)]
        } else {
            base
        };
        Rgb(color)
    });
    img.save(path)
}

/// Debug/verification render: plain land/ocean silhouette with the
/// freshwater greening intensity overlaid on the green channel.
pub fn save_greening(
    width: usize,
    height: usize,
    is_ocean: &[bool],
    greening: &HeatMap,
    path: &str,
) -> Result<(), image::ImageError> {
    let img = ImageBuffer::from_fn(width as u32, height as u32, |x, y| {
        let idx = y as usize * width + x as usize;
        let base: [u8; 3] = if is_ocean[idx] { [40, 40, 60] } else { [70, 65, 55] };
        let intensity = (greening.data[idx] * 255.0).clamp(0.0, 255.0) as u8;
        Rgb([base[0], base[1].saturating_add(intensity), base[2]])
    });
    img.save(path)
}

pub const RENDER_SCALE: usize = 3;
const N_DITHER_LEVELS: usize = 16;
const N_CONTOURS: usize = 40;
const CONTOUR_DARKEN: f64 = 0.90;
const CONTOUR_DARKEN_WATER: f64 = 0.95;

/// Naturalistic composite render: terrain colored by elevation band, water
/// blended in from the hydrology layer, contour lines. 3x supersampled with
/// Bayer ordered-dithering for smooth elevation-band transitions and
/// anti-aliased water/glacier/sea-ice edges. Ported from demiurge-rust's
/// render_composite_map, minus the salt-flat branch (mapgen has no salt-flat
/// generator).
fn composite_pixel_color(
    rx: u32,
    ry: u32,
    width: usize,
    height: usize,
    hydro_map: &HeatMap,
    elevation: &HeatMap,
    temperature: &HeatMap,
    precipitation: &HeatMap,
    greening: &HeatMap,
    is_ocean: &[bool],
    is_glacier: &[bool],
    is_sea_ice: &[bool],
    params: &PlanetGenParams,
) -> Rgb<u8> {
    let render_width = width * RENDER_SCALE;
    let render_height = height * RENDER_SCALE;
    let nx = rx as f64 / render_width as f64;
    let ny = ry as f64 / render_height as f64;
    let hydro_nearest = hydro_map.sample_nearest(nx, ny);
    let is_water = if hydro_nearest <= 0.0 {
        false
    } else if hydro_nearest <= 0.3 {
        true
    } else {
        const EDGE_COVERAGE: f64 = 0.0025;
        let dx = rx as usize / RENDER_SCALE;
        let dy = ry as usize / RENDER_SCALE;
        let off_x = rx as usize % RENDER_SCALE;
        let off_y = ry as usize % RENDER_SCALE;
        let neighbor = |ndx: i64, ndy: i64| -> f64 {
            let nnx = ndx.rem_euclid(width as i64) as usize;
            let nny = ndy.clamp(0, height as i64 - 1) as usize;
            hydro_map.data[nny * width + nnx]
        };
        let mut coverage = 1.0f64;
        if off_x == 0 && neighbor(dx as i64 - 1, dy as i64) <= 0.0 { coverage = EDGE_COVERAGE; }
        if off_x == 2 && neighbor(dx as i64 + 1, dy as i64) <= 0.0 { coverage = EDGE_COVERAGE; }
        if off_y == 0 && neighbor(dx as i64, dy as i64 - 1) <= 0.0 { coverage = EDGE_COVERAGE; }
        if off_y == 2 && neighbor(dx as i64, dy as i64 + 1) <= 0.0 { coverage = EDGE_COVERAGE; }
        BAYER_4X4[ry as usize % 4][rx as usize % 4] < coverage
    };
    let data_idx = (ry as usize / RENDER_SCALE) * width + (rx as usize / RENDER_SCALE);
    let dx = rx as usize / RENDER_SCALE;
    let dy = ry as usize / RENDER_SCALE;
    let off_x = rx as usize % RENDER_SCALE;
    let off_y = ry as usize % RENDER_SCALE;
    let mut color = if is_water && is_sea_ice[data_idx] {
        let sea_ice_neighbor = |ndx: i64, ndy: i64| -> bool {
            let nnx = ndx.rem_euclid(width as i64) as usize;
            let nny = ndy.clamp(0, height as i64 - 1) as usize;
            is_sea_ice[nny * width + nnx]
        };
        const SEA_ICE_EDGE: f64 = 0.05;
        let mut coverage = 1.0f64;
        if off_x == 0 && !sea_ice_neighbor(dx as i64 - 1, dy as i64) { coverage = SEA_ICE_EDGE; }
        if off_x == 2 && !sea_ice_neighbor(dx as i64 + 1, dy as i64) { coverage = SEA_ICE_EDGE; }
        if off_y == 0 && !sea_ice_neighbor(dx as i64, dy as i64 - 1) { coverage = SEA_ICE_EDGE; }
        if off_y == 2 && !sea_ice_neighbor(dx as i64, dy as i64 + 1) { coverage = SEA_ICE_EDGE; }
        if BAYER_4X4[ry as usize % 4][rx as usize % 4] < coverage {
            let t = temperature.sample(nx, ny);
            let d = bayer_dither(t / params.sea_ice_temp_threshold, rx as usize, ry as usize, N_DITHER_LEVELS);
            sea_ice_color(d, params.sea_ice_temp_threshold)
        } else {
            let d = bayer_dither(hydro_nearest, rx as usize, ry as usize, N_DITHER_LEVELS).max(0.01);
            water_color(d)
        }
    } else if is_water {
        let d = bayer_dither(hydro_nearest, rx as usize, ry as usize, N_DITHER_LEVELS).max(0.01);
        water_color(d)
    } else if is_glacier[data_idx] {
        let non_glacier_land = |ndx: i64, ndy: i64| -> bool {
            let nnx = ndx.rem_euclid(width as i64) as usize;
            let nny = ndy.clamp(0, height as i64 - 1) as usize;
            let nidx = nny * width + nnx;
            !is_glacier[nidx] && !is_ocean[nidx] && hydro_map.data[nidx] <= 0.0
        };
        const GLACIER_EDGE: f64 = 0.05;
        let mut coverage = 1.0f64;
        if off_x == 0 && non_glacier_land(dx as i64 - 1, dy as i64) { coverage = GLACIER_EDGE; }
        if off_x == 2 && non_glacier_land(dx as i64 + 1, dy as i64) { coverage = GLACIER_EDGE; }
        if off_y == 0 && non_glacier_land(dx as i64, dy as i64 - 1) { coverage = GLACIER_EDGE; }
        if off_y == 2 && non_glacier_land(dx as i64, dy as i64 + 1) { coverage = GLACIER_EDGE; }
        if BAYER_4X4[ry as usize % 4][rx as usize % 4] < coverage {
            let t = temperature.sample(nx, ny);
            let d = bayer_dither(t / params.glacier_temp_threshold, rx as usize, ry as usize, N_DITHER_LEVELS);
            glacier_color(d, params.glacier_temp_threshold)
        } else {
            let elev_t = elevation.sample(nx, ny);
            let land_t = ((elev_t - params.sea_level) / (1.0 - params.sea_level)).clamp(0.0, 1.0);
            let d = bayer_dither(land_t, rx as usize, ry as usize, N_DITHER_LEVELS);
            let temp_t = temperature.sample(nx, ny);
            let precip_t = (precipitation.sample(nx, ny)
                + greening.sample(nx, ny) * params.greening_strength).min(1.0);
            biome_terrain_color(temp_t, precip_t, d)
        }
    } else {
        let t = elevation.sample(nx, ny);
        let land_t = ((t - params.sea_level) / (1.0 - params.sea_level)).clamp(0.0, 1.0);
        let d = bayer_dither(land_t, rx as usize, ry as usize, N_DITHER_LEVELS);
        let temp_t = temperature.sample(nx, ny);
        let precip_t = (precipitation.sample(nx, ny)
            + greening.sample(nx, ny) * params.greening_strength).min(1.0);
        biome_terrain_color(temp_t, precip_t, d)
    };
    let nx_r = (rx as usize + 1) as f64 / render_width as f64;
    let ny_d = (ry as usize + 1) as f64 / render_height as f64;
    let e = elevation.sample(nx, ny);
    let e_r = elevation.sample(nx_r, ny);
    let e_d = elevation.sample(nx, ny_d);
    if is_contour(e, e_r, e_d, N_CONTOURS) {
        let factor = if is_water { CONTOUR_DARKEN_WATER } else { CONTOUR_DARKEN };
        color = [
            (color[0] as f64 * factor) as u8,
            (color[1] as f64 * factor) as u8,
            (color[2] as f64 * factor) as u8,
        ];
    }
    Rgb(color)
}

pub fn save_composite(
    width: usize,
    height: usize,
    hydro_map: &HeatMap,
    elevation: &HeatMap,
    temperature: &HeatMap,
    precipitation: &HeatMap,
    greening: &HeatMap,
    is_ocean: &[bool],
    is_glacier: &[bool],
    is_sea_ice: &[bool],
    params: &PlanetGenParams,
    path: &str,
) -> Result<(), image::ImageError> {
    let render_width = width * RENDER_SCALE;
    let render_height = height * RENDER_SCALE;
    let img = ImageBuffer::from_fn(render_width as u32, render_height as u32, |rx, ry| {
        composite_pixel_color(
            rx, ry, width, height, hydro_map, elevation, temperature, precipitation, greening,
            is_ocean, is_glacier, is_sea_ice, params,
        )
    });
    img.save(path)
}

/// Debug/verification render: same terrain as `save_composite`, but with
/// region boundaries overlaid in red and a small marker dot at each land
/// region's label point. Not a replacement for composite.png.
pub fn save_regions(
    width: usize,
    height: usize,
    hydro_map: &HeatMap,
    elevation: &HeatMap,
    temperature: &HeatMap,
    precipitation: &HeatMap,
    greening: &HeatMap,
    is_ocean: &[bool],
    is_glacier: &[bool],
    is_sea_ice: &[bool],
    params: &PlanetGenParams,
    region_map: &[u32],
    regions: &[crate::regions::Region],
    path: &str,
) -> Result<(), image::ImageError> {
    let render_width = width * RENDER_SCALE;
    let render_height = height * RENDER_SCALE;

    let mut img = ImageBuffer::from_fn(render_width as u32, render_height as u32, |rx, ry| {
        let dx = rx as usize / RENDER_SCALE;
        let dy = ry as usize / RENDER_SCALE;
        let own = region_map[dy * width + dx];

        let left = region_map[dy * width + (dx + width - 1) % width];
        let right = region_map[dy * width + (dx + 1) % width];
        let mut is_boundary = left != own || right != own;
        if dy > 0 {
            let up = region_map[(dy - 1) * width + dx];
            is_boundary = is_boundary || up != own;
        }
        if dy < height - 1 {
            let down = region_map[(dy + 1) * width + dx];
            is_boundary = is_boundary || down != own;
        }

        if is_boundary {
            Rgb([220u8, 30, 30])
        } else {
            composite_pixel_color(
                rx, ry, width, height, hydro_map, elevation, temperature, precipitation, greening,
                is_ocean, is_glacier, is_sea_ice, params,
            )
        }
    });

    for region in regions {
        if region.kind != crate::regions::CellKind::Land {
            continue;
        }
        let cx = (region.label_pos.0 * RENDER_SCALE + RENDER_SCALE / 2) as i64;
        let cy = (region.label_pos.1 * RENDER_SCALE + RENDER_SCALE / 2) as i64;
        for py in (cy - 4)..=(cy + 4) {
            if py < 0 || py >= render_height as i64 {
                continue;
            }
            for px in (cx - 4)..=(cx + 4) {
                let wrapped_px = px.rem_euclid(render_width as i64);
                let dist = (((px - cx).pow(2) + (py - cy).pow(2)) as f64).sqrt();
                if dist <= 3.0 {
                    img.put_pixel(wrapped_px as u32, py as u32, Rgb([255, 255, 255]));
                } else if dist <= 4.0 {
                    img.put_pixel(wrapped_px as u32, py as u32, Rgb([0, 0, 0]));
                }
            }
        }
    }

    img.save(path)
}
