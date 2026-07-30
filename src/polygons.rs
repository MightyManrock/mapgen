//! Region boundary → polygon extraction for the Obsidian zoom-map draw
//! layer. Traces each land region's cell-grid outline into closed loops
//! and simplifies them to a draw-friendly vertex count.

use std::collections::{BTreeMap, VecDeque};

use crate::regions::{CellKind, Region};

pub struct RegionPolygons {
    pub region_id: u32,
    /// Closed outer loops (one per connected component), vertices in
    /// cell-corner coordinates (x in 0..=width, y in 0..=height).
    pub loops: Vec<Vec<(f64, f64)>>,
}

/// Left-hand direction of a unit heading in screen coordinates (y down).
fn left_of(d: (i64, i64)) -> (i64, i64) {
    (d.1, -d.0)
}

/// Walks the directed boundary edges of one connected component into
/// closed loops. Edges are emitted with the component interior on the
/// left; at pinch corners (diagonally-touching cells) the sharpest left
/// turn is preferred, which hugs the interior and keeps every loop
/// simple (non-self-crossing). Outer loops come out with negative
/// shoelace area in screen coordinates; holes come out positive and are
/// dropped by the caller.
fn trace_loops(edges: &BTreeMap<(i64, i64), Vec<(i64, i64)>>) -> Vec<Vec<(i64, i64)>> {
    let mut used: BTreeMap<((i64, i64), (i64, i64)), bool> = BTreeMap::new();
    for (&from, tos) in edges {
        for &to in tos {
            used.insert((from, to), false);
        }
    }

    let mut loops = Vec::new();
    let starts: Vec<((i64, i64), (i64, i64))> = used.keys().cloned().collect();
    for start_edge in starts {
        if used[&start_edge] {
            continue;
        }
        let (start, mut cur) = start_edge;
        used.insert(start_edge, true);
        let mut heading = (cur.0 - start.0, cur.1 - start.1);
        let mut lp = vec![start];
        while cur != start {
            lp.push(cur);
            // Candidate outgoing edges from cur, unused only.
            let candidates = edges.get(&cur).map(|v| v.as_slice()).unwrap_or(&[]);
            // Prefer sharpest left turn, then straight, then right.
            let prefs = [left_of(heading), heading, left_of(left_of(left_of(heading)))];
            let mut next = None;
            'pref: for want in prefs {
                for &to in candidates {
                    let d = (to.0 - cur.0, to.1 - cur.1);
                    if d == want && !used[&(cur, to)] {
                        next = Some(to);
                        break 'pref;
                    }
                }
            }
            let Some(to) = next else {
                // Boundary edge sets always close; if this fires the
                // emission logic is broken — drop the partial loop.
                debug_assert!(false, "open boundary loop");
                lp.clear();
                break;
            };
            used.insert((cur, to), true);
            heading = (to.0 - cur.0, to.1 - cur.1);
            cur = to;
        }
        if lp.len() >= 3 {
            loops.push(lp);
        }
    }
    loops
}

/// Twice the signed shoelace area (screen coordinates: negative = outer
/// loop under the interior-on-left emission convention).
fn shoelace2(lp: &[(i64, i64)]) -> i64 {
    let mut s = 0;
    for i in 0..lp.len() {
        let a = lp[i];
        let b = lp[(i + 1) % lp.len()];
        s += a.0 * b.1 - b.0 * a.1;
    }
    s
}

/// Removes vertices whose adjacent segments share a direction.
fn merge_collinear(lp: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let n = lp.len();
    let mut out = Vec::new();
    for i in 0..n {
        let prev = lp[(i + n - 1) % n];
        let cur = lp[i];
        let next = lp[(i + 1) % n];
        let d1 = (cur.0 - prev.0, cur.1 - prev.1);
        let d2 = (next.0 - cur.0, next.1 - cur.1);
        // Directions are axis-aligned units scaled by run length only
        // after merging, so compare normalized by cross product.
        if d1.0 * d2.1 - d1.1 * d2.0 != 0 {
            out.push(cur);
        }
    }
    out
}

/// Perpendicular distance from `p` to the line through `a`-`b`.
fn perp_dist(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-12 {
        let (ex, ey) = (p.0 - a.0, p.1 - a.1);
        return (ex * ex + ey * ey).sqrt();
    }
    ((p.0 - a.0) * dy - (p.1 - a.1) * dx).abs() / len
}

/// Douglas-Peucker on an open polyline (endpoints kept).
fn dp_simplify(pts: &[(f64, f64)], epsilon: f64, out: &mut Vec<(f64, f64)>) {
    if pts.len() < 3 {
        out.extend_from_slice(&pts[..pts.len().saturating_sub(1)]);
        return;
    }
    let a = pts[0];
    let b = pts[pts.len() - 1];
    let (mut max_d, mut max_i) = (0.0f64, 0usize);
    for (i, &p) in pts.iter().enumerate().skip(1).take(pts.len() - 2) {
        let d = perp_dist(p, a, b);
        if d > max_d {
            max_d = d;
            max_i = i;
        }
    }
    if max_d > epsilon {
        dp_simplify(&pts[..=max_i], epsilon, out);
        dp_simplify(&pts[max_i..], epsilon, out);
    } else {
        out.push(a);
    }
}

/// Simplifies a closed loop: split at vertex 0 and the vertex farthest
/// from it (stable anchors), Douglas-Peucker each arc.
fn simplify_loop(lp: &[(i64, i64)], epsilon: f64) -> Vec<(f64, f64)> {
    let pts: Vec<(f64, f64)> = lp.iter().map(|&(x, y)| (x as f64, y as f64)).collect();
    let far = pts
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            let da = (a.0 - pts[0].0).powi(2) + (a.1 - pts[0].1).powi(2);
            let db = (b.0 - pts[0].0).powi(2) + (b.1 - pts[0].1).powi(2);
            da.partial_cmp(&db).unwrap()
        })
        .map(|(i, _)| i)
        .unwrap();
    let arc1: Vec<(f64, f64)> = pts[..=far].to_vec();
    let mut arc2: Vec<(f64, f64)> = pts[far..].to_vec();
    arc2.push(pts[0]);
    let mut out = Vec::new();
    dp_simplify(&arc1, epsilon, &mut out);
    dp_simplify(&arc2, epsilon, &mut out);
    out
}

pub fn extract_region_polygons(
    region_map: &[u32],
    regions: &[Region],
    width: usize,
    height: usize,
    epsilon: f64,
) -> Vec<RegionPolygons> {
    // One pass: cell lists per region id (BTreeMap for deterministic
    // iteration; region_map carries a u32::MAX sentinel for unmapped
    // slivers, which no region id ever equals).
    let mut cells_by_region: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (idx, &rid) in region_map.iter().enumerate() {
        if rid != u32::MAX {
            cells_by_region.entry(rid).or_default().push(idx);
        }
    }

    let mut result = Vec::new();
    for r in regions {
        if r.kind != CellKind::Land {
            continue;
        }
        let Some(cells) = cells_by_region.get(&r.id) else { continue };

        // Connected components, 4-connected, deliberately NOT x-wrapped:
        // the antimeridian acts as a hard edge so seam-spanning regions
        // split into separate polygons (the plugin's coordinate space is
        // flat [0,1] with no wrap).
        let mut visited: BTreeMap<usize, bool> = cells.iter().map(|&c| (c, false)).collect();
        let mut loops_out = Vec::new();
        for &seed in cells {
            if visited[&seed] {
                continue;
            }
            let mut comp = Vec::new();
            let mut queue = VecDeque::from([seed]);
            visited.insert(seed, true);
            while let Some(idx) = queue.pop_front() {
                comp.push(idx);
                let x = idx % width;
                let y = idx / width;
                let mut push = |nidx: usize| {
                    if region_map[nidx] == r.id {
                        if let Some(v) = visited.get_mut(&nidx) {
                            if !*v {
                                *v = true;
                                queue.push_back(nidx);
                            }
                        }
                    }
                };
                if x > 0 { push(idx - 1); }
                if x + 1 < width { push(idx + 1); }
                if y > 0 { push(idx - width); }
                if y + 1 < height { push(idx + width); }
            }

            // Directed boundary edges, interior on the left (screen
            // coords, y down): top edge runs west, bottom east, west
            // side south, east side north. Component membership via
            // binary search over the sorted cell list.
            let mut comp_sorted = comp;
            comp_sorted.sort_unstable();
            let in_component = |cx: i64, cy: i64| -> bool {
                cx >= 0
                    && cy >= 0
                    && (cx as usize) < width
                    && (cy as usize) < height
                    && comp_sorted.binary_search(&(cy as usize * width + cx as usize)).is_ok()
            };

            let mut edges: BTreeMap<(i64, i64), Vec<(i64, i64)>> = BTreeMap::new();
            let mut add = |from: (i64, i64), to: (i64, i64)| {
                edges.entry(from).or_default().push(to);
            };
            for &idx in &comp_sorted {
                let x = (idx % width) as i64;
                let y = (idx / width) as i64;
                if !in_component(x, y - 1) {
                    add((x + 1, y), (x, y)); // top, westward
                }
                if !in_component(x, y + 1) {
                    add((x, y + 1), (x + 1, y + 1)); // bottom, eastward
                }
                if !in_component(x - 1, y) {
                    add((x, y), (x, y + 1)); // west side, southward
                }
                if !in_component(x + 1, y) {
                    add((x + 1, y + 1), (x + 1, y)); // east side, northward
                }
            }

            for lp in trace_loops(&edges) {
                if shoelace2(&lp) >= 0 {
                    continue; // hole (or degenerate) — dropped by design
                }
                let merged = merge_collinear(&lp);
                let simplified = simplify_loop(&merged, epsilon);
                if simplified.len() >= 3 {
                    loops_out.push(simplified);
                }
            }
        }

        if !loops_out.is_empty() {
            result.push(RegionPolygons { region_id: r.id, loops: loops_out });
        }
    }
    result
}
