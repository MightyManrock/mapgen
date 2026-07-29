use crate::heatmap::HeatMap;
use std::collections::{HashMap, VecDeque};

// ── Region detection ─────────────────────────────────────────────────────────

/// 4-connected neighbors for region detection. x wraps east-west; y clamps at poles.
fn neighbors_4(x: usize, y: usize, width: usize, height: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::with_capacity(4);
    out.push(((x + width - 1) % width, y));
    out.push(((x + 1) % width, y));
    if y > 0 { out.push((x, y - 1)); }
    if y < height - 1 { out.push((x, y + 1)); }
    out
}

/// Cell classification for region detection.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CellKind { Frozen, Ocean, Land }

fn cell_kind(idx: usize, is_ocean: &[bool], is_glacier: &[bool], is_sea_ice: &[bool]) -> CellKind {
    if is_sea_ice[idx] || is_glacier[idx] { CellKind::Frozen }
    else if is_ocean[idx]                  { CellKind::Ocean  }
    else                                   { CellKind::Land   }
}

/// Aggregate description of a detected geographic region.
pub struct Region {
    pub id:                u32,
    pub kind:               CellKind,
    pub size:              usize,
    pub mean_elev:         f64,
    pub mean_temp:         f64,
    pub mean_swing:        f64,
    pub mean_precip:       f64,
    pub mean_aridity:      f64,
    pub ocean_frac:        f64,
    pub glacier_frac:      f64,
    pub sea_ice_frac:      f64,
    pub island_components: usize, // 0 = continental, 1 = island, >1 = archipelago
    pub label_pos:         (usize, usize),
}

impl Region {
    pub fn temp_zone(&self) -> &'static str {
        if self.mean_temp < 0.20 { "Polar" }
        else if self.mean_temp < 0.35 { "Cold" }
        else if self.mean_temp < 0.55 { "Temperate" }
        else if self.mean_temp < 0.70 { "Hot" }
        else { "Tropical" }
    }

    pub fn climate_character(&self) -> &'static str {
        if self.sea_ice_frac > 0.5   { return "Sea Ice"; }
        if self.glacier_frac > 0.5   { return "Glacier / Ice Sheet"; }
        if self.ocean_frac > 0.5 {
            if self.mean_temp < 0.20 { return "Polar Ocean"; }
            if self.mean_temp < 0.50 { return "Cold Ocean"; }
            if self.mean_temp < 0.70 { return "Temperate Ocean"; }
            return "Tropical Ocean";
        }
        if self.mean_temp < 0.20 {
            if self.mean_precip < 0.20 { return "Polar Desert"; }
            return "Tundra";
        }
        if self.mean_temp < 0.35 {
            if self.mean_precip < 0.20 { return "Cold Desert"; }
            return "Boreal Forest";
        }
        if self.mean_temp < 0.55 {
            if self.mean_precip < 0.15 { return "Temperate Desert"; }
            if self.mean_precip < 0.35 { return "Steppe"; }
            if self.mean_precip < 0.60 { return "Temperate Forest"; }
            return "Temperate Rainforest";
        }
        if self.mean_temp < 0.70 {
            if self.mean_precip < 0.20 { return "Hot Desert"; }
            if self.mean_precip < 0.45 { return "Mediterranean"; }
            return "Subtropical Forest";
        }
        if self.mean_precip < 0.20 { return "Tropical Desert"; }
        if self.mean_precip < 0.45 { return "Savanna"; }
        if self.mean_precip < 0.65 { return "Tropical Dry Forest"; }
        "Tropical Rainforest"
    }

    pub fn character(&self) -> String {
        let suffix = match self.island_components {
            0 => "",
            1 => " Island",
            _ => " Archipelago",
        };
        format!("{}{suffix}", self.climate_character())
    }
}

/// Segment the map into geographic regions by multi-dimensional flood fill.
///
/// Three cell types are recognized: **Frozen** (sea ice or glacier), **Ocean**,
/// and **Land**. A BFS region may only expand into cells of its own type —
/// there is a hard barrier between open ocean and open land. Frozen cells form
/// their own pool and may freely mix sea ice and glaciated land.
///
/// `land_threshold` and `ocean_threshold` are the Euclidean similarity cutoffs
/// across (elevation, temperature, precipitation); ocean and frozen regions use
/// the laxer ocean threshold so the seas consolidate into fewer large regions.
///
/// Regions smaller than `min_size` cells are absorbed into their most-contacted
/// same-type neighbor.
///
/// Returns a flat region-ID map (one u32 per pixel) and a Vec<Region> sorted
/// largest-first.
pub fn detect_regions(
    elevation:       &HeatMap,
    temperature:     &HeatMap,
    diurnal_swing:   &HeatMap,
    precipitation:   &HeatMap,
    aridity:         &HeatMap,
    is_ocean:        &[bool],
    is_glacier:      &[bool],
    is_sea_ice:      &[bool],
    land_threshold:  f64,
    ocean_threshold: f64,
    min_size:        usize,
    coast_dist:      usize,
    arch_dist:       usize,
    lon_weight:      f64,
) -> (Vec<u32>, Vec<Region>) {
    let width  = elevation.width;
    let height = elevation.height;
    let n      = width * height;

    let mut region_map:   Vec<u32>       = vec![u32::MAX; n];
    let mut region_cells: Vec<Vec<usize>> = Vec::new();
    let mut region_kind:  Vec<CellKind>  = Vec::new();

    // Phase 1: BFS flood fill from each unvisited seed.
    // Similarity is checked against the SEED cell, not the frontier cell, to
    // prevent regions from drifting across gradual transitions.
    // Expansion is blocked across cell-kind boundaries (ocean ↔ land).
    for start in 0..n {
        if region_map[start] != u32::MAX { continue; }
        let id   = region_cells.len() as u32;
        let kind = cell_kind(start, is_ocean, is_glacier, is_sea_ice);
        let thr  = if kind == CellKind::Land { land_threshold } else { ocean_threshold };
        let mut cells = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back(start);
        region_map[start] = id;

        let se = elevation.data[start];
        let st = temperature.data[start];
        let sp = precipitation.data[start];
        let sx = start % width;
        let sy = start / width;
        // cos(latitude) — 1.0 at equator, 0.0 at poles. Scales lon_weight down
        // toward the poles where east-west cells are physically closer together.
        let lat_cos = (std::f64::consts::PI * (sy as f64 / height as f64 - 0.5)).cos();

        while let Some(idx) = queue.pop_front() {
            cells.push(idx);
            let x = idx % width;
            let y = idx / width;
            for (nx, ny) in neighbors_4(x, y, width, height) {
                let nidx = ny * width + nx;
                if region_map[nidx] != u32::MAX { continue; }
                if cell_kind(nidx, is_ocean, is_glacier, is_sea_ice) != kind { continue; }
                let de  = se - elevation.data[nidx];
                let dt  = st - temperature.data[nidx];
                let dp  = sp - precipitation.data[nidx];
                // Wrap-aware longitude distance from seed, normalised to [0, 0.5].
                let raw_dx = (sx as i64 - nx as i64).unsigned_abs() as usize;
                let ddx = raw_dx.min(width - raw_dx) as f64 / width as f64;
                let dl  = lon_weight * lat_cos * ddx;
                if (de * de + dt * dt + dp * dp + dl * dl).sqrt() <= thr {
                    region_map[nidx] = id;
                    queue.push_back(nidx);
                }
            }
        }
        region_cells.push(cells);
        region_kind.push(kind);
    }

    // Phase 2: Absorb regions below min_size into their most-contacted neighbor,
    // processing smallest-first so orphan slivers merge before their targets do.
    loop {
        let small = region_cells.iter().enumerate()
            .filter(|(_, c)| !c.is_empty() && c.len() < min_size)
            .min_by_key(|(_, c)| c.len())
            .map(|(i, _)| i);
        let Some(sid) = small else { break };

        let mut counts: HashMap<u32, usize> = HashMap::new();
        let skind = region_kind[sid];
        for &idx in &region_cells[sid] {
            let x = idx % width;
            let y = idx / width;
            for (nx, ny) in neighbors_4(x, y, width, height) {
                let nid = region_map[ny * width + nx];
                if nid != sid as u32 && region_kind[nid as usize] == skind {
                    *counts.entry(nid).or_default() += 1;
                }
            }
        }
        if let Some((&target, _)) = counts.iter().max_by_key(|&(_, &c)| c) {
            let cells = std::mem::take(&mut region_cells[sid]);
            for &idx in &cells { region_map[idx] = target; }
            region_cells[target as usize].extend(cells);
        } else {
            region_cells[sid].clear(); // isolated — discard
        }
    }

    // Phase 2.5: Island detection pass.
    // Land cells discarded by min_size merging are either absorbed into a nearby
    // continental region (within COAST_DIST ocean hops) or grouped into standalone
    // island / archipelago regions via ocean BFS (within ARCH_DIST ocean hops).
    let island_coast_dist = coast_dist;
    let island_arch_dist  = arch_dist;

    // Per-region island-component count (0 = continental).  New island regions are
    // appended to region_cells below; their counts are pushed in lock-step.
    let mut island_parts: Vec<usize> = vec![0; region_cells.len()];

    // Snapshot which cells are free land: their Phase-1 region was discarded in Phase 2.
    // Computed before any region_map mutations so absorbed cells are detectable later.
    let free_land: Vec<bool> = (0..n).map(|idx| {
        let rid = region_map[idx] as usize;
        region_cells[rid].is_empty() && region_kind[rid] == CellKind::Land
    }).collect();

    // Group free-land cells by original region ID (each ID = one connected component).
    let mut comp_map: HashMap<u32, Vec<usize>> = HashMap::new();
    for idx in 0..n {
        if free_land[idx] { comp_map.entry(region_map[idx]).or_default().push(idx); }
    }
    let island_components: Vec<(u32, Vec<usize>)> = comp_map.into_iter().collect();
    let n_comps = island_components.len();

    // Step B+C: BFS from each component outward through ocean/frozen cells.
    // If a continental land region is reachable within COAST_DIST hops, absorb there.
    let mut coast_targets: Vec<Option<u32>> = vec![None; n_comps];
    for (ii, (_, icells)) in island_components.iter().enumerate() {
        let mut dist: Vec<u8> = vec![u8::MAX; n];
        let mut queue = VecDeque::new();
        for &idx in icells { dist[idx] = 0; queue.push_back(idx); }
        'coast: while let Some(idx) = queue.pop_front() {
            let x = idx % width;
            let y = idx / width;
            for (nx, ny) in neighbors_4(x, y, width, height) {
                let nidx = ny * width + nx;
                if dist[nidx] != u8::MAX { continue; }
                // Non-free, non-ocean, non-frozen → must be a continental land cell.
                if !free_land[nidx] && !is_ocean[nidx] && !is_glacier[nidx] && !is_sea_ice[nidx] {
                    coast_targets[ii] = Some(region_map[nidx]);
                    break 'coast;
                }
                let nd = dist[idx].saturating_add(1);
                if (is_ocean[nidx] || is_glacier[nidx] || is_sea_ice[nidx])
                    && nd <= island_coast_dist as u8
                {
                    dist[nidx] = nd;
                    queue.push_back(nidx);
                }
            }
        }
    }

    // Apply coastal absorptions.
    for (ii, (_, icells)) in island_components.iter().enumerate() {
        if let Some(target) = coast_targets[ii] {
            for &idx in icells { region_map[idx] = target; }
            region_cells[target as usize].extend_from_slice(icells);
        }
    }

    // Step D: Group remaining (non-absorbed) components into islands/archipelagos.
    // BFS expands through ocean/frozen cells; reaching another remaining component
    // within ARCH_DIST hops merges it into the current group.
    let remaining: Vec<usize> = (0..n_comps).filter(|&ii| coast_targets[ii].is_none()).collect();
    let n_remaining = remaining.len();
    if n_remaining > 0 {
        // Map original region ID → index in remaining[].
        let rid_to_ri: HashMap<u32, usize> = remaining.iter().enumerate()
            .map(|(ri, &ci)| (island_components[ci].0, ri))
            .collect();

        let mut group_of: Vec<Option<usize>> = vec![None; n_remaining];
        let mut groups: Vec<Vec<usize>> = Vec::new();

        for start_ri in 0..n_remaining {
            if group_of[start_ri].is_some() { continue; }
            let gid = groups.len();
            groups.push(vec![start_ri]);
            group_of[start_ri] = Some(gid);

            let mut visited = vec![false; n];
            let mut queue: VecDeque<(usize, u8)> = VecDeque::new();
            for &idx in &island_components[remaining[start_ri]].1 {
                visited[idx] = true;
                queue.push_back((idx, 0));
            }

            while let Some((idx, d)) = queue.pop_front() {
                let x = idx % width;
                let y = idx / width;
                for (nx, ny) in neighbors_4(x, y, width, height) {
                    let nidx = ny * width + nx;
                    if visited[nidx] { continue; }
                    visited[nidx] = true;
                    if free_land[nidx] {
                        // free_land is a snapshot: absorbed cells may now have a
                        // continental region_map entry not in rid_to_ri — skip them.
                        if let Some(&ri) = rid_to_ri.get(&region_map[nidx]) {
                            if group_of[ri].is_none() {
                                group_of[ri] = Some(gid);
                                groups[gid].push(ri);
                                for &cidx in &island_components[remaining[ri]].1 {
                                    if !visited[cidx] {
                                        visited[cidx] = true;
                                        queue.push_back((cidx, 0));
                                    }
                                }
                            }
                        }
                        continue;
                    }
                    let nd = d.saturating_add(1);
                    if (is_ocean[nidx] || is_glacier[nidx] || is_sea_ice[nidx])
                        && nd <= island_arch_dist as u8
                    {
                        queue.push_back((nidx, nd));
                    }
                }
            }
        }

        // Step E: Assign new region IDs for each island group.
        for group in &groups {
            let new_rid = region_cells.len() as u32;
            let n_parts = group.len();
            let mut all_cells = Vec::new();
            for &ri in group {
                let (_, cells) = &island_components[remaining[ri]];
                for &idx in cells { region_map[idx] = new_rid; }
                all_cells.extend_from_slice(cells);
            }
            region_cells.push(all_cells);
            region_kind.push(CellKind::Land);
            island_parts.push(n_parts);
        }
    }

    // Phase 3: Compact IDs and build Region structs.
    let mut new_id = 0u32;
    let mut id_remap: Vec<u32> = vec![u32::MAX; region_cells.len()];
    for (i, cells) in region_cells.iter().enumerate() {
        if !cells.is_empty() { id_remap[i] = new_id; new_id += 1; }
    }
    for v in region_map.iter_mut() {
        if *v != u32::MAX { *v = id_remap[*v as usize]; }
    }

    let mut regions: Vec<Region> = region_cells.iter().enumerate()
        .filter(|(_, c)| !c.is_empty())
        .map(|(old_id, cells)| {
            let sf = cells.len() as f64;

            // Circular-mean centroid (x is circular to handle antimeridian-spanning
            // regions; y is a plain mean).
            let mut sin_sum = 0.0;
            let mut cos_sum = 0.0;
            let mut y_sum = 0u64;
            for &idx in cells {
                let x = idx % width;
                let y = idx / width;
                let angle = std::f64::consts::TAU * x as f64 / width as f64;
                sin_sum += angle.sin();
                cos_sum += angle.cos();
                y_sum += y as u64;
            }
            let cn = cells.len() as f64;
            let mean_angle = (sin_sum / cn).atan2(cos_sum / cn);
            let cx = (mean_angle / std::f64::consts::TAU * width as f64).rem_euclid(width as f64);
            let cy = y_sum as f64 / cells.len() as f64;

            // Snap to the nearest cell actually in this region (guarantees the
            // label point lands inside the region, unlike a raw centroid).
            let label_pos = cells.iter().copied().min_by(|&a, &b| {
                let dist = |idx: usize| -> f64 {
                    let x = (idx % width) as f64;
                    let y = (idx / width) as f64;
                    let raw_dx = (x - cx).abs();
                    let dx = raw_dx.min(width as f64 - raw_dx);
                    let dy = y - cy;
                    dx * dx + dy * dy
                };
                dist(a).partial_cmp(&dist(b)).unwrap()
            }).map(|idx| (idx % width, idx / width)).unwrap();

            Region {
                id:                id_remap[old_id],
                kind:              region_kind[old_id],
                size:              cells.len(),
                mean_elev:         cells.iter().map(|&i| elevation.data[i]).sum::<f64>()     / sf,
                mean_temp:         cells.iter().map(|&i| temperature.data[i]).sum::<f64>()   / sf,
                mean_swing:        cells.iter().map(|&i| diurnal_swing.data[i]).sum::<f64>() / sf,
                mean_precip:       cells.iter().map(|&i| precipitation.data[i]).sum::<f64>() / sf,
                mean_aridity:      cells.iter().map(|&i| aridity.data[i]).sum::<f64>()       / sf,
                ocean_frac:        cells.iter().filter(|&&i| is_ocean[i]).count()     as f64 / sf,
                glacier_frac:      cells.iter().filter(|&&i| is_glacier[i]).count()   as f64 / sf,
                sea_ice_frac:      cells.iter().filter(|&&i| is_sea_ice[i]).count()   as f64 / sf,
                island_components: island_parts[old_id],
                label_pos,
            }
        })
        .collect();

    regions.sort_unstable_by_key(|r| r.id);
    (region_map, regions)
}
