//! Grid autorouter: A* over copper layers with clearance-inflated
//! obstacles, vias at layer changes, and 45-degree path straightening.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::time::{Duration, Instant};

use lamina_geom::CopperItem;
use lamina_geom::prim::{Prim, ring_contains, seg_point};
use lamina_model::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RouteOptions {
    /// Nets to route (default: every net with unrouted connections).
    #[serde(default)]
    pub nets: Option<Vec<String>>,
    /// Nets to leave alone, e.g. `GND` when it will be poured.
    #[serde(default)]
    pub skip: Vec<String>,
    /// Copper layers the router may use (default all).
    #[serde(default)]
    pub layers: Option<Vec<Layer>>,
    /// Grid pitch in mm (default 0.1).
    #[serde(default)]
    pub grid: Option<f64>,
    /// Cost of a via in grid steps (default 25).
    #[serde(default)]
    pub via_cost: Option<f64>,
    /// Give up after this many seconds in total (default 60).
    #[serde(default)]
    pub time_limit: Option<f64>,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct Failure {
    pub net: String,
    pub from: String,
    pub to: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct RouteOutcome {
    pub tracks: Vec<Track>,
    pub vias: Vec<Via>,
    pub routed: Vec<String>,
    pub failed: Vec<Failure>,
}

struct Grid {
    x0: f64,
    y0: f64,
    pitch: f64,
    nx: usize,
    ny: usize,
    nl: usize,
}

impl Grid {
    fn center(&self, ix: usize, iy: usize) -> Point {
        Point::new(
            self.x0 + ix as f64 * self.pitch,
            self.y0 + iy as f64 * self.pitch,
        )
    }
    fn cell(&self, p: Point) -> (isize, isize) {
        (
            ((p.x - self.x0) / self.pitch).round() as isize,
            ((p.y - self.y0) / self.pitch).round() as isize,
        )
    }
    fn plane(&self) -> usize {
        self.nx * self.ny
    }
    fn idx(&self, l: usize, ix: usize, iy: usize) -> usize {
        l * self.plane() + iy * self.nx + ix
    }
    /// Cells whose centres fall in the rectangle grown by `m`.
    fn range(&self, r: &Rect, m: f64) -> (usize, usize, usize, usize) {
        let lo = self.cell(Point::new(r.min.x - m, r.min.y - m));
        let hi = self.cell(Point::new(r.max.x + m, r.max.y + m));
        let cx = |v: isize| v.clamp(0, self.nx as isize - 1) as usize;
        let cy = |v: isize| v.clamp(0, self.ny as isize - 1) as usize;
        (cx(lo.0), cy(lo.1), cx(hi.0), cy(hi.1))
    }
}

#[derive(Copy, Clone, PartialEq)]
struct Node {
    f: f32,
    idx: u32,
}
impl Eq for Node {}
impl Ord for Node {
    fn cmp(&self, o: &Self) -> Ordering {
        o.f.partial_cmp(&self.f)
            .unwrap_or(Ordering::Equal)
            .then(o.idx.cmp(&self.idx))
    }
}
impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

fn point_prim(p: Point) -> Prim {
    Prim::Circle { c: p, r: 0.0 }
}

struct Ctx<'a> {
    d: &'a ProjectData,
    g: Grid,
    /// Inside the board and far enough from its edge, per track radius.
    edge_dist: Vec<f32>,
    allowed_layers: Vec<bool>,
    deadline: Instant,
}

impl<'a> Ctx<'a> {
    fn new(d: &'a ProjectData, opt: &RouteOptions) -> Result<Ctx<'a>, String> {
        let outline = d
            .board
            .outline
            .as_ref()
            .ok_or("board outline is not set")?
            .flatten(0.01);
        let bb = Rect::from_points(outline.iter().copied()).ok_or("empty outline")?;
        let mut pitch = opt.grid.unwrap_or(0.1).clamp(0.02, 1.0);
        let nl = d.board.stackup.copper_layers as usize;
        while (bb.width() / pitch) * (bb.height() / pitch) * nl as f64 > 6.0e6 {
            pitch *= 1.25;
        }
        let g = Grid {
            x0: bb.min.x,
            y0: bb.min.y,
            pitch,
            nx: (bb.width() / pitch).ceil() as usize + 1,
            ny: (bb.height() / pitch).ceil() as usize + 1,
            nl,
        };
        // Distance from each cell to the board edge (negative outside).
        let mut edges: Vec<(Point, Point)> = Vec::new();
        let mut rings = vec![outline.clone()];
        rings.extend(d.board.cutouts.iter().map(|c| c.flatten(0.01)));
        for r in &rings {
            for k in 0..r.len() {
                edges.push((r[k], r[(k + 1) % r.len()]));
            }
        }
        let cutouts: Vec<Vec<Point>> = rings[1..].to_vec();
        let mut edge_dist = vec![0f32; g.plane()];
        for iy in 0..g.ny {
            for ix in 0..g.nx {
                let p = g.center(ix, iy);
                let inside =
                    ring_contains(&outline, p) && !cutouts.iter().any(|c| ring_contains(c, p));
                let dmin = edges
                    .iter()
                    .map(|(a, b)| seg_point(*a, *b, p))
                    .fold(f64::INFINITY, f64::min);
                edge_dist[iy * g.nx + ix] = if inside { dmin as f32 } else { -(dmin as f32) };
            }
        }
        let mut allowed_layers = vec![true; nl];
        if let Some(ls) = &opt.layers {
            allowed_layers = vec![false; nl];
            for l in ls {
                if let Some(c) = l.copper() {
                    let i = c.index(nl as u8) as usize;
                    if i < nl {
                        allowed_layers[i] = true;
                    }
                }
            }
        }
        Ok(Ctx {
            d,
            g,
            edge_dist,
            allowed_layers,
            deadline: Instant::now()
                + Duration::from_secs_f64(opt.time_limit.unwrap_or(60.0).clamp(1.0, 3600.0)),
        })
    }

    fn class(&self, net: &Option<String>) -> Option<String> {
        net.as_ref()
            .and_then(|n| self.d.nets.nets.get(n))
            .and_then(|n| n.class.clone())
    }

    /// Cells a track of `net` (half width `hw`) may not use, per layer.
    fn blocked(&self, d: &ProjectData, items: &[CopperItem], net: &str, hw: f64) -> Vec<bool> {
        let g = &self.g;
        let rules = &d.board.rules;
        let mut b = vec![false; g.plane() * g.nl];
        let edge_need = (rules.copper_to_edge + hw) as f32;
        for l in 0..g.nl {
            for i in 0..g.plane() {
                if !self.allowed_layers[l] || self.edge_dist[i] < edge_need {
                    b[l * g.plane() + i] = true;
                }
            }
        }
        let own = Some(net.to_string());
        let my_class = self.class(&own);
        for it in items {
            if it.net.as_deref() == Some(net) {
                continue;
            }
            let cl = rules
                .clearance_for(my_class.as_deref())
                .max(rules.clearance_for(self.class(&it.net).as_deref()));
            let r = cl + hw;
            self.stamp(&mut b, it.layer as usize, &it.prim, r);
        }
        let layers = d.board.stackup.copper_layers;
        for k in &d.routing.keepouts {
            if !k.forbid.tracks {
                continue;
            }
            let p = lamina_geom::board::poly(k.outline.flatten(0.02));
            for l in &k.layers {
                if let Some(c) = l.copper() {
                    self.stamp(&mut b, c.index(layers) as usize, &p, hw);
                }
            }
        }
        for (refdes, pl) in &d.placement.placements {
            let Some(fp) = d
                .parts
                .parts
                .get(refdes)
                .and_then(|p| d.footprints.get(&p.footprint))
            else {
                continue;
            };
            for (ls, pts, forbid) in lamina_geom::part_keepouts(fp, pl, layers) {
                if forbid.tracks {
                    let p = lamina_geom::board::poly(pts);
                    for l in ls {
                        self.stamp(&mut b, l as usize, &p, hw);
                    }
                }
            }
        }
        b
    }

    fn stamp(&self, b: &mut [bool], l: usize, prim: &Prim, r: f64) {
        let g = &self.g;
        if l >= g.nl {
            return;
        }
        let (x0, y0, x1, y1) = g.range(&prim.bbox(), r);
        for iy in y0..=y1 {
            for ix in x0..=x1 {
                let k = g.idx(l, ix, iy);
                if b[k] {
                    continue;
                }
                if prim.distance(&point_prim(g.center(ix, iy))) < r - 1e-9 {
                    b[k] = true;
                }
            }
        }
    }

    /// Cells where a via of radius `rv` may go.
    fn via_ok(
        &self,
        blocked: &[bool],
        drills: &[Prim],
        rv: f64,
        hw: f64,
        drill_r: f64,
    ) -> Vec<bool> {
        let g = &self.g;
        let rules = &self.d.board.rules;
        let reach = ((rv - hw).max(0.0) / g.pitch).ceil() as isize;
        let mut ok = vec![true; g.plane()];
        for iy in 0..g.ny {
            for ix in 0..g.nx {
                let mut good = true;
                'l: for l in 0..g.nl {
                    for dy in -reach..=reach {
                        for dx in -reach..=reach {
                            if ((dx * dx + dy * dy) as f64).sqrt() * g.pitch
                                > (rv - hw).max(0.0) + 1e-9
                            {
                                continue;
                            }
                            let (x, y) = (ix as isize + dx, iy as isize + dy);
                            if x < 0 || y < 0 || x >= g.nx as isize || y >= g.ny as isize {
                                good = false;
                                break 'l;
                            }
                            if blocked[g.idx(l, x as usize, y as usize)] {
                                good = false;
                                break 'l;
                            }
                        }
                    }
                }
                ok[iy * g.nx + ix] = good;
            }
        }
        let need = rules.hole_to_hole + drill_r;
        for dr in drills {
            let (x0, y0, x1, y1) = g.range(&dr.bbox(), need);
            for iy in y0..=y1 {
                for ix in x0..=x1 {
                    if dr.distance(&point_prim(g.center(ix, iy))) < need {
                        ok[iy * g.nx + ix] = false;
                    }
                }
            }
        }
        ok
    }

    fn cells_in(&self, prim: &Prim, layers: &[u8]) -> Vec<usize> {
        let g = &self.g;
        let (x0, y0, x1, y1) = g.range(&prim.bbox(), 0.0);
        let mut out = Vec::new();
        for &l in layers {
            if (l as usize) >= g.nl || !self.allowed_layers[l as usize] {
                continue;
            }
            for iy in y0..=y1 {
                for ix in x0..=x1 {
                    if prim.contains(g.center(ix, iy)) {
                        out.push(g.idx(l as usize, ix, iy));
                    }
                }
            }
        }
        if out.is_empty() {
            // Pad smaller than a cell: take the nearest cell.
            let (cx, cy) = g.cell(prim.anchor());
            let cx = cx.clamp(0, g.nx as isize - 1) as usize;
            let cy = cy.clamp(0, g.ny as isize - 1) as usize;
            for &l in layers {
                if (l as usize) < g.nl && self.allowed_layers[l as usize] {
                    out.push(g.idx(l as usize, cx, cy));
                }
            }
        }
        out
    }

    fn astar(
        &self,
        blocked: &[bool],
        via_ok: &[bool],
        sources: &[usize],
        targets: &[usize],
        goal: Point,
        via_cost: f32,
    ) -> Option<Vec<usize>> {
        let g = &self.g;
        let n = g.plane() * g.nl;
        let mut cost = vec![f32::INFINITY; n];
        let mut came = vec![u32::MAX; n];
        let mut is_target = vec![false; n];
        for &t in targets {
            is_target[t] = true;
        }
        let target_layers: Vec<usize> = {
            let mut v: Vec<usize> = targets.iter().map(|t| t / g.plane()).collect();
            v.sort();
            v.dedup();
            v
        };
        let (gx, gy) = g.cell(goal);
        let h = |idx: usize| -> f32 {
            let l = idx / g.plane();
            let r = idx % g.plane();
            let (ix, iy) = ((r % g.nx) as f32, (r / g.nx) as f32);
            let dx = (ix - gx as f32).abs();
            let dy = (iy - gy as f32).abs();
            let oct = dx.max(dy) + (std::f32::consts::SQRT_2 - 1.0) * dx.min(dy);
            oct + if target_layers.contains(&l) {
                0.0
            } else {
                via_cost
            }
        };
        let mut open = BinaryHeap::new();
        for &s in sources {
            cost[s] = 0.0;
            open.push(Node {
                f: h(s),
                idx: s as u32,
            });
        }
        const DIRS: [(isize, isize, f32); 8] = [
            (1, 0, 1.0),
            (-1, 0, 1.0),
            (0, 1, 1.0),
            (0, -1, 1.0),
            (1, 1, std::f32::consts::SQRT_2),
            (1, -1, std::f32::consts::SQRT_2),
            (-1, 1, std::f32::consts::SQRT_2),
            (-1, -1, std::f32::consts::SQRT_2),
        ];
        let mut expanded = 0usize;
        while let Some(Node { idx, .. }) = open.pop() {
            let idx = idx as usize;
            if is_target[idx] {
                let mut path = vec![idx];
                let mut c = idx;
                while came[c] != u32::MAX {
                    c = came[c] as usize;
                    path.push(c);
                }
                path.reverse();
                return Some(path);
            }
            expanded += 1;
            if expanded.is_multiple_of(4096) && Instant::now() > self.deadline {
                return None;
            }
            let base = cost[idx];
            let l = idx / g.plane();
            let r = idx % g.plane();
            let (ix, iy) = ((r % g.nx) as isize, (r / g.nx) as isize);
            let mut relax = |j: usize, step: f32, open: &mut BinaryHeap<Node>| {
                let c = base + step;
                if c < cost[j] {
                    cost[j] = c;
                    came[j] = idx as u32;
                    open.push(Node {
                        f: c + h(j),
                        idx: j as u32,
                    });
                }
            };
            for (dx, dy, step) in DIRS {
                let (x, y) = (ix + dx, iy + dy);
                if x < 0 || y < 0 || x >= g.nx as isize || y >= g.ny as isize {
                    continue;
                }
                let j = g.idx(l, x as usize, y as usize);
                if blocked[j] && !is_target[j] {
                    continue;
                }
                if dx != 0 && dy != 0 {
                    // No squeezing diagonally between two obstacles.
                    let a = g.idx(l, (ix + dx) as usize, iy as usize);
                    let b = g.idx(l, ix as usize, (iy + dy) as usize);
                    if (blocked[a] && !is_target[a]) || (blocked[b] && !is_target[b]) {
                        continue;
                    }
                }
                relax(j, step, &mut open);
            }
            if via_ok[r] {
                for l2 in 0..g.nl {
                    if l2 == l || !self.allowed_layers[l2] {
                        continue;
                    }
                    let j = l2 * g.plane() + r;
                    if blocked[j] && !is_target[j] {
                        continue;
                    }
                    relax(j, via_cost, &mut open);
                }
            }
        }
        None
    }

    /// Cells along a straight line are free on layer `l`.
    fn clear_line(&self, blocked: &[bool], l: usize, a: usize, b: usize) -> bool {
        let g = &self.g;
        let (ax, ay) = ((a % g.nx) as isize, (a / g.nx) as isize);
        let (bx, by) = ((b % g.nx) as isize, (b / g.nx) as isize);
        let (dx, dy) = (bx - ax, by - ay);
        let steps = dx.abs().max(dy.abs());
        for k in 0..=steps {
            let x = ax + if steps == 0 { 0 } else { dx * k / steps };
            let y = ay + if steps == 0 { 0 } else { dy * k / steps };
            if blocked[g.idx(l, x as usize, y as usize)] {
                return false;
            }
        }
        true
    }
}

fn octilinear(g: &Grid, a: usize, b: usize) -> bool {
    let (ax, ay) = ((a % g.nx) as isize, (a / g.nx) as isize);
    let (bx, by) = ((b % g.nx) as isize, (b / g.nx) as isize);
    let (dx, dy) = (bx - ax, by - ay);
    dx == 0 || dy == 0 || dx.abs() == dy.abs()
}

/// Route every unrouted connection that the options select.
pub fn autoroute(d: &ProjectData, opt: &RouteOptions) -> Result<RouteOutcome, String> {
    let ctx = Ctx::new(d, opt)?;
    let mut work = d.clone();
    let mut out = RouteOutcome::default();
    let via_cost = opt.via_cost.unwrap_or(25.0) as f32;
    let analysis = lamina_drc::analyze(d);
    let mut edges = analysis.ratsnest.clone();
    edges.retain(|e| {
        !opt.skip.contains(&e.net) && opt.nets.as_ref().is_none_or(|n| n.contains(&e.net))
    });
    edges.sort_by(|a, b| a.a.dist(a.b).total_cmp(&b.a.dist(b.b)));

    let rules = d.board.rules.clone();
    let mut track_ids: Vec<String> = work.routing.tracks.iter().map(|t| t.id.clone()).collect();
    let mut via_ids: Vec<String> = work.routing.vias.iter().map(|v| v.id.clone()).collect();

    for e in edges {
        if Instant::now() > ctx.deadline {
            out.failed.push(Failure {
                net: e.net.clone(),
                from: e.from.clone(),
                to: e.to.clone(),
                reason: "time limit".into(),
            });
            continue;
        }
        let class = work.nets.nets.get(&e.net).and_then(|n| n.class.clone());
        let nc = rules.class(class.as_deref());
        let width = rules.track_width_for(class.as_deref());
        let hw = width / 2.0;
        let via_d = nc.via_diameter.unwrap_or(rules.via_diameter);
        let via_drill = nc.via_drill.unwrap_or(rules.via_drill);

        let geo = lamina_geom::build(&work);
        let blocked = ctx.blocked(&work, &geo.copper, &e.net, hw);
        let drills: Vec<Prim> = geo.drills.iter().map(|x| x.prim.clone()).collect();
        let vok = ctx.via_ok(&blocked, &drills, via_d / 2.0, hw, via_drill / 2.0);

        let find_pad = |label: &str| -> Option<(Prim, Vec<u8>, Point)> {
            let pr: PadRef = label.parse().ok()?;
            let part = work.parts.parts.get(&pr.part)?;
            let pl = work.placement.placements.get(&pr.part)?;
            let fp = work.footprints.get(&part.footprint)?;
            lamina_geom::placed_pads(fp, pl, work.board.stackup.copper_layers)
                .into_iter()
                .find(|(p, ..)| p.number == pr.pad)
                .map(|(p, _, prim, ls)| (prim, ls, pl.transform(p.at)))
        };
        let (Some((pa, la, ca)), Some((pb, lb, cb))) = (find_pad(&e.from), find_pad(&e.to)) else {
            out.failed.push(Failure {
                net: e.net.clone(),
                from: e.from.clone(),
                to: e.to.clone(),
                reason: "pad not found".into(),
            });
            continue;
        };
        let sources = ctx.cells_in(&pa, &la);
        let targets = ctx.cells_in(&pb, &lb);
        if sources.is_empty() || targets.is_empty() {
            out.failed.push(Failure {
                net: e.net.clone(),
                from: e.from.clone(),
                to: e.to.clone(),
                reason: "pad is on a layer the router may not use".into(),
            });
            continue;
        }
        let Some(path) = ctx.astar(&blocked, &vok, &sources, &targets, cb, via_cost) else {
            out.failed.push(Failure {
                net: e.net.clone(),
                from: e.from.clone(),
                to: e.to.clone(),
                reason: "no path found".into(),
            });
            continue;
        };

        // Split into per-layer runs; straighten each run.
        let g = &ctx.g;
        let mut runs: Vec<(usize, Vec<usize>)> = Vec::new();
        for idx in path {
            let l = idx / g.plane();
            let r = idx % g.plane();
            match runs.last_mut() {
                Some((rl, cells)) if *rl == l => cells.push(r),
                _ => runs.push((l, vec![r])),
            }
        }
        let n_runs = runs.len();
        for (ri, (l, cells)) in runs.iter().enumerate() {
            let mut keep = vec![cells[0]];
            let mut i = 0;
            while i < cells.len() - 1 {
                let mut j = cells.len() - 1;
                while j > i + 1 {
                    if octilinear(g, cells[i], cells[j])
                        && ctx.clear_line(&blocked, *l, cells[i], cells[j])
                    {
                        break;
                    }
                    j -= 1;
                }
                keep.push(cells[j]);
                i = j;
            }
            let mut pts: Vec<Point> = keep
                .iter()
                .map(|r| g.center(r % g.nx, r / g.nx))
                .map(|p| Point::new(json::round6(p.x), json::round6(p.y)))
                .collect();
            if ri == 0 {
                pts.insert(0, ca);
            }
            if ri == n_runs - 1 {
                pts.push(cb);
            }
            pts.dedup_by(|a, b| a.dist(*b) < 1e-9);
            if pts.len() >= 2 {
                let id = next_id_from("t", &track_ids);
                track_ids.push(id.clone());
                let layer = Layer::Copper(Copper::from_index(*l as u8, g.nl as u8));
                let t = Track {
                    id,
                    net: e.net.clone(),
                    layer,
                    width,
                    points: pts,
                };
                work.routing.tracks.push(t.clone());
                out.tracks.push(t);
            }
            if ri + 1 < n_runs {
                let at = *cells.last().unwrap();
                let p = g.center(at % g.nx, at / g.nx);
                let id = next_id_from("v", &via_ids);
                via_ids.push(id.clone());
                let v = Via {
                    id,
                    net: e.net.clone(),
                    at: Point::new(json::round6(p.x), json::round6(p.y)),
                    diameter: via_d,
                    drill: via_drill,
                    from: None,
                    to: None,
                };
                work.routing.vias.push(v.clone());
                out.vias.push(v);
            }
        }
        out.routed
            .push(format!("{}: {} -> {}", e.net, e.from, e.to));
    }
    Ok(out)
}

fn next_id_from(prefix: &str, ids: &[String]) -> String {
    let max = ids
        .iter()
        .filter_map(|s| s.strip_prefix(prefix)?.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    format!("{prefix}{}", max + 1)
}
