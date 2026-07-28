use image::{ImageBuffer, Rgb};

use crate::heatmap::HeatMap;

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
