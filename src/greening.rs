//! Freshwater greening: a render-only vegetation shift for land near
//! rivers, lakes, and aquifers. Deliberately NOT a climate change — a river
//! valley through a desert is still a desert climatically (the Nile case),
//! so region detection, aridity, and tooltips never see this field; only
//! the composite renderer does, where it pushes the biome color toward the
//! wet end of the gradient.

use std::collections::VecDeque;

use crate::heatmap::HeatMap;
use crate::params::PlanetGenParams;

/// Multi-source BFS over land cells (x wraps, y clamps, fixed W/E/N/S push
/// order — layer order makes the distances deterministic). Returns per-cell
/// `strength * (1 - d / radius)`, 0.0 beyond `radius` or on non-land.
/// Source cells themselves get full `strength`.
pub(crate) fn bfs_falloff(
    sources: &[usize],
    is_ocean: &[bool],
    width: usize,
    height: usize,
    radius: usize,
    strength: f64,
) -> Vec<f64> {
    let n = width * height;
    let mut dist = vec![usize::MAX; n];
    let mut queue = VecDeque::new();
    for &idx in sources {
        if dist[idx] == usize::MAX {
            dist[idx] = 0;
            queue.push_back(idx);
        }
    }
    while let Some(idx) = queue.pop_front() {
        let d = dist[idx];
        if d >= radius {
            continue;
        }
        let x = idx % width;
        let y = idx / width;
        let mut nbrs = [(0usize, 0usize); 4];
        let mut count = 0;
        nbrs[count] = ((x + width - 1) % width, y);
        count += 1;
        nbrs[count] = ((x + 1) % width, y);
        count += 1;
        if y > 0 {
            nbrs[count] = (x, y - 1);
            count += 1;
        }
        if y + 1 < height {
            nbrs[count] = (x, y + 1);
            count += 1;
        }
        for &(nx, ny) in &nbrs[..count] {
            let nidx = ny * width + nx;
            if !is_ocean[nidx] && dist[nidx] == usize::MAX {
                dist[nidx] = d + 1;
                queue.push_back(nidx);
            }
        }
    }
    dist.into_iter()
        .map(|d| {
            if d == usize::MAX {
                0.0
            } else {
                strength * (1.0 - d as f64 / radius as f64)
            }
        })
        .collect()
}

/// Builds the greening intensity field in [0, 1]:
///
/// 1. Surface water (river/lake cells in the hydrology map) greens at full
///    strength out to `greening_radius`; aquifer cells (groundwater in
///    endorheic basins — think oases tracing a buried river course) green
///    at `greening_aquifer_strength` over the same radius, so their reach
///    is shorter and weaker everywhere. The two passes combine by max.
/// 2. Dampened within `greening_ocean_damp_dist` of the ocean (coastal
///    moderation already covers those cells; keeps beaches beaches).
/// 3. Gated by temperature, fading in linearly from `greening_temp_floor`
///    to `greening_temp_full` — since temperature already includes the
///    lapse rate, this kills greening at high latitudes *and* altitudes
///    without a separate elevation gate.
pub fn compute_greening(
    hydro_map: &HeatMap,
    aquifer_zones: &[(usize, usize)],
    is_ocean: &[bool],
    temperature: &HeatMap,
    params: &PlanetGenParams,
) -> HeatMap {
    let width = hydro_map.width;
    let height = hydro_map.height;
    let n = width * height;

    // Rivers are (0.0, 0.3], lakes (0.3, 0.5] in the hydrology encoding;
    // both are land cells carrying surface water.
    let surface_sources: Vec<usize> = (0..n)
        .filter(|&i| !is_ocean[i] && hydro_map.data[i] > 0.0 && hydro_map.data[i] <= 0.5)
        .collect();
    let aquifer_sources: Vec<usize> = aquifer_zones.iter().map(|&(x, y)| y * width + x).collect();
    let ocean_sources: Vec<usize> = (0..n).filter(|&i| is_ocean[i]).collect();

    let surface = bfs_falloff(&surface_sources, is_ocean, width, height, params.greening_radius, 1.0);
    let aquifer = bfs_falloff(
        &aquifer_sources,
        is_ocean,
        width,
        height,
        params.greening_radius,
        params.greening_aquifer_strength,
    );
    // Ocean-distance BFS may cross any land cell (strength 1.0 at the
    // coast fading to 0.0 at damp_dist); greening scales by 1 - that.
    let coast = bfs_falloff(
        &ocean_sources,
        &vec![false; n],
        width,
        height,
        params.greening_ocean_damp_dist,
        1.0,
    );

    let data = (0..n)
        .map(|idx| {
            if is_ocean[idx] {
                return 0.0;
            }
            let base = surface[idx].max(aquifer[idx]);
            let ocean_damp = 1.0 - coast[idx];
            let temp = temperature.data[idx];
            let temp_gate = ((temp - params.greening_temp_floor)
                / (params.greening_temp_full - params.greening_temp_floor))
                .clamp(0.0, 1.0);
            base * ocean_damp * temp_gate
        })
        .collect();

    HeatMap { width, height, data }
}
