//! Coastline character support data. Render-only, like greening: nothing
//! here feeds back into climate or hydrology.

use crate::greening::bfs_falloff;
use crate::heatmap::HeatMap;
use crate::params::PlanetGenParams;

/// Per-cell river-mouth influence in [0, 1]: 1.0 at a river cell that
/// touches the ocean, falling off linearly to 0.0 at `shore_mouth_radius`.
/// Drives the delta-mud tint in the shore renderer — big rivers hitting
/// the sea deposit sediment, not clean sand.
pub fn compute_mouth_influence(
    hydro_map: &HeatMap,
    is_ocean: &[bool],
    params: &PlanetGenParams,
) -> HeatMap {
    let width = hydro_map.width;
    let height = hydro_map.height;
    let n = width * height;

    // Mouth cells: river band (0.0, 0.3] of the hydrology encoding, on
    // land, with at least one ocean 4-neighbor (x wraps, y clamps).
    let mouths: Vec<usize> = (0..n)
        .filter(|&idx| {
            if is_ocean[idx] || hydro_map.data[idx] <= 0.0 || hydro_map.data[idx] > 0.3 {
                return false;
            }
            let x = idx % width;
            let y = idx / width;
            let mut touches_ocean = is_ocean[y * width + (x + width - 1) % width]
                || is_ocean[y * width + (x + 1) % width];
            if y > 0 {
                touches_ocean = touches_ocean || is_ocean[(y - 1) * width + x];
            }
            if y + 1 < height {
                touches_ocean = touches_ocean || is_ocean[(y + 1) * width + x];
            }
            touches_ocean
        })
        .collect();

    let data = bfs_falloff(&mouths, is_ocean, width, height, params.shore_mouth_radius, 1.0);
    HeatMap { width, height, data }
}
