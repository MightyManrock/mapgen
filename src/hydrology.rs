use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use crate::heatmap::{neighbors_8, HeatMap};
use crate::params::PlanetGenParams;

pub struct HydrologyResult {
    pub map: HeatMap,
    /// Cells where rivers sink underground rather than running dry.
    pub aquifer_zones: Vec<(usize, usize)>,
    /// Endorheic basin cells: water accumulates here but never reaches the ocean.
    pub is_endorheic: Vec<bool>,
    /// D8 flow direction: each land cell's downstream neighbour index.
    pub flow_to: Vec<Option<usize>>,
    /// Land cells sorted highest-elevation-first (topological upstream→downstream order).
    pub topo_order: Vec<usize>,
    /// Per-cell flow accumulation (precipitation-weighted, normalised by mean land precip).
    pub accumulation: Vec<f64>,
}

/// Convert a [0, 1] float to an integer heap key for min-heap ordering.
fn float_key(v: f64) -> u64 {
    (v.clamp(0.0, 1.0) * 1_000_000.0) as u64
}

/// Deterministic per-cell pseudo-random value in [0, 1).
/// Used to assign aquifer vs terminal outcome for endorheic basins.
fn cell_hash(x: usize, y: usize) -> f64 {
    let mut h = (x as u64)
        .wrapping_mul(2654435761)
        .wrapping_add((y as u64).wrapping_mul(2246822519));
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51afd7ed558ccd);
    h ^= h >> 33;
    (h & 0xFFFF) as f64 / 65535.0
}

pub fn generate_hydrology(
    elevation: &HeatMap,
    is_ocean: &[bool],
    precipitation: &HeatMap,
    is_glacier: &[bool],
    params: &PlanetGenParams,
) -> HydrologyResult {
    let width = elevation.width;
    let height = elevation.height;
    let n = width * height;

    // ── Phase 1: Priority-flood depression filling ───────────────────────
    //
    // Seeds the flood from all ocean cells and pole edges. Each cell's
    // filled elevation is raised to at least its natural elevation, ensuring
    // every land cell has a monotonic downhill path to the ocean. Without
    // this, FBM pits would trap flow accumulation and produce disconnected
    // puddles instead of rivers that reach the sea.
    //
    // Cells are processed lowest-first (min-heap) so fill propagates from
    // the ocean upward, never raising a cell above the height needed to
    // just reach its outlet.

    let mut filled = elevation.data.clone();
    let mut in_queue = vec![false; n];
    // (Reverse(key), index): min-heap by filled elevation.
    let mut heap: BinaryHeap<(Reverse<u64>, usize)> = BinaryHeap::new();

    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let is_pole = y == 0 || y == height - 1;
            if is_pole || is_ocean[idx] {
                in_queue[idx] = true;
                heap.push((Reverse(float_key(filled[idx])), idx));
            }
        }
    }

    while let Some((_, idx)) = heap.pop() {
        let cx = idx % width;
        let cy = idx / width;
        for (nx, ny) in neighbors_8(cx, cy, width, height) {
            let nidx = ny * width + nx;
            if in_queue[nidx] {
                continue;
            }
            in_queue[nidx] = true;
            filled[nidx] = f64::max(elevation.data[nidx], filled[idx]);
            heap.push((Reverse(float_key(filled[nidx])), nidx));
        }
    }

    // ── Phase 2: D8 flow directions on filled terrain ───────────────────
    //
    // Each land cell points to the neighbor with the steepest downhill
    // gradient. For neighbors inside filled (lake) regions, natural elevation
    // is used instead of filled elevation — otherwise the entire flat lake
    // surface has zero slope and D8 can't route water to the deep center,
    // leaving endorheic cells with no incoming accumulation.

    let is_lake_cell = |i: usize| filled[i] > elevation.data[i] + 1e-6;

    let mut flow_to: Vec<Option<usize>> = vec![None; n];

    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            if is_ocean[idx] {
                continue;
            }
            let h = filled[idx];
            let mut best = None;
            let mut steepest = 0.0f64;

            for (nx, ny) in neighbors_8(x, y, width, height) {
                let nidx = ny * width + nx;
                // Within flat lake regions use natural elevation so flow
                // routes toward the deep center rather than stalling.
                let nh = if is_lake_cell(nidx) { elevation.data[nidx] } else { filled[nidx] };
                // Correct dx for x-axis wrap.
                let raw_dx = nx as i64 - x as i64;
                let dx = if raw_dx.abs() > 1 { -raw_dx.signum() } else { raw_dx };
                let dy = ny as i64 - y as i64;
                let dist = ((dx * dx + dy * dy) as f64).sqrt();
                let slope = (h - nh) / dist;
                if slope > steepest {
                    steepest = slope;
                    best = Some(nidx);
                }
            }
            flow_to[idx] = best;
        }
    }

    // ── Phase 3: Per-basin classification ──────────────────────────────
    //
    // BFS over connected components of lake cells (filled > natural elev).
    // Each basin is classified as a unit before outlet routing and flow
    // accumulation run, so both can use the correct endorheic flags.

    let mut basin_id: Vec<Option<usize>> = vec![None; n];
    let mut basins: Vec<Vec<usize>> = Vec::new();

    for start in 0..n {
        if !is_lake_cell(start) || basin_id[start].is_some() || is_ocean[start] {
            continue;
        }
        let id = basins.len();
        basins.push(Vec::new());
        let mut bfs = VecDeque::new();
        bfs.push_back(start);
        basin_id[start] = Some(id);
        while let Some(idx) = bfs.pop_front() {
            basins[id].push(idx);
            let cx = idx % width;
            let cy = idx / width;
            for (nx, ny) in neighbors_8(cx, cy, width, height) {
                let nidx = ny * width + nx;
                if is_lake_cell(nidx) && basin_id[nidx].is_none() && !is_ocean[nidx] {
                    basin_id[nidx] = Some(id);
                    bfs.push_back(nidx);
                }
            }
        }
    }

    let basin_endorheic: Vec<bool> = basins
        .iter()
        .map(|cells| {
            cells
                .iter()
                .map(|&i| filled[i] - elevation.data[i])
                .fold(0.0f64, f64::max)
                > params.max_lake_fill
        })
        .collect();

    // ── Phase 4: Outlet routing ─────────────────────────────────────────
    //
    // For each non-endorheic basin, find the single lowest rim cell (the
    // natural spill point) and BFS outward from it through the lake, forcing
    // every lake cell's flow_to toward the outlet. Without this, multiple
    // rim cells at nearly equal elevation all act as outlets simultaneously,
    // producing diffuse shore seepage rather than one clean river exit.

    let mut outlet_visited = vec![false; n];

    for (id, cells) in basins.iter().enumerate() {
        if basin_endorheic[id] {
            continue;
        }

        // Find the non-lake, non-ocean neighbor with the lowest natural
        // elevation adjacent to any cell in this basin — the spill point.
        let mut outlet_lake_cell = usize::MAX;
        let mut rim_cell = usize::MAX;
        let mut best_rim_elev = f64::INFINITY;

        for &idx in cells {
            let cx = idx % width;
            let cy = idx / width;
            for (nx, ny) in neighbors_8(cx, cy, width, height) {
                let nidx = ny * width + nx;
                if !is_lake_cell(nidx) && !is_ocean[nidx] {
                    let e = elevation.data[nidx];
                    if e < best_rim_elev {
                        best_rim_elev = e;
                        outlet_lake_cell = idx;
                        rim_cell = nidx;
                    }
                }
            }
        }

        if rim_cell == usize::MAX {
            continue;
        }

        // Route the outlet lake cell directly to the rim.
        flow_to[outlet_lake_cell] = Some(rim_cell);

        // BFS outward from the outlet lake cell; each reached cell flows
        // toward the cell it was reached from (i.e., toward the outlet).
        outlet_visited[outlet_lake_cell] = true;
        let mut bfs = VecDeque::new();
        bfs.push_back(outlet_lake_cell);
        while let Some(idx) = bfs.pop_front() {
            let cx = idx % width;
            let cy = idx / width;
            for (nx, ny) in neighbors_8(cx, cy, width, height) {
                let nidx = ny * width + nx;
                if basin_id[nidx] == Some(id) && !outlet_visited[nidx] {
                    outlet_visited[nidx] = true;
                    flow_to[nidx] = Some(idx);
                    bfs.push_back(nidx);
                }
            }
        }

        // Reset visited flags using the known cell list (O(basin_size)).
        for &idx in cells {
            outlet_visited[idx] = false;
        }
    }

    // ── Phase 5: Flow accumulation ──────────────────────────────────────
    //
    // Process land cells highest-first. Each cell adds its precipitation-
    // weighted contribution to its downstream neighbor, now using the
    // outlet-corrected flow_to graph.

    let mut land_order: Vec<usize> = (0..n).filter(|&i| !is_ocean[i]).collect();
    land_order.sort_unstable_by(|&a, &b| filled[b].total_cmp(&filled[a]));

    let land_count = land_order.len();
    let mean_land_precip = if land_count > 0 {
        land_order.iter().map(|&i| precipitation.data[i]).sum::<f64>() / land_count as f64
    } else {
        1.0
    };
    // Glacier cells contribute extra flow representing meltwater. The bonus
    // is multiplicative so high-precip glaciers (wet snowfields) feed larger rivers.
    let mut accumulation: Vec<f64> = (0..n)
        .map(|i| {
            if is_ocean[i] { return 0.0; }
            let base = precipitation.data[i] / mean_land_precip;
            if is_glacier[i] { base * params.glacier_melt_factor } else { base }
        })
        .collect();
    for &idx in &land_order {
        if let Some(ds) = flow_to[idx] {
            accumulation[ds] += accumulation[idx];
        }
    }

    // ── Phase 6: Aquifer zone identification ────────────────────────────

    let endorheic: Vec<bool> = (0..n)
        .map(|i| match basin_id[i] {
            Some(id) => basin_endorheic[id],
            None => false,
        })
        .collect();

    let mut aquifer_zones = Vec::new();
    for idx in 0..n {
        if endorheic[idx] && accumulation[idx] >= params.river_threshold {
            if cell_hash(idx % width, idx / width) < params.aquifer_probability {
                aquifer_zones.push((idx % width, idx / width));
            }
        }
    }

    // ── Phase 7: Encode into hydrology HeatMap ──────────────────────────
    //
    // Value ranges:
    //   0.0        = dry land (no water present)
    //   (0.0, 0.3] = river, proportional to log flow accumulation
    //   (0.3, 0.5] = lake, proportional to fill depth
    //   (0.5, 1.0] = ocean, proportional to depth below sea level

    let max_accum = accumulation.iter().cloned().fold(1.0f64, f64::max);
    let log_max = max_accum.ln().max(1.0);
    let mut data = vec![0.0f64; n];

    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let elev = elevation.data[idx];
            let fill_depth = filled[idx] - elev;

            data[idx] = if is_ocean[idx] {
                let depth = (params.sea_level - elev) / params.sea_level;
                0.5 + depth.clamp(0.0, 1.0) * 0.5
            } else if endorheic[idx] {
                0.0 // water disappears — dry basin floor
            } else if fill_depth > 1e-6 {
                let depth_norm = (fill_depth / params.max_lake_fill).clamp(0.0, 1.0);
                0.3 + depth_norm * 0.2
            } else if accumulation[idx] >= params.river_threshold {
                let norm = accumulation[idx].ln() / log_max;
                0.01 + norm * 0.29
            } else {
                0.0
            };
        }
    }

    HydrologyResult {
        map: HeatMap { width, height, data },
        aquifer_zones,
        is_endorheic: endorheic,
        flow_to,
        topo_order: land_order,
        accumulation,
    }
}
