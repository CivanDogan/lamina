//! Grid autorouter with negotiated congestion (PathFinder).
//!
//! Every net is routed as a tree: each connection starts from the copper
//! the net already has and ends on the nearest copper of the part it joins.
//! Static obstacles (other nets' pads and existing copper, keep-outs, the
//! board edge) are hard. New wires of different nets may overlap at first;
//! each pass makes contested cells dearer (present and history costs) and
//! reroutes the nets that still overlap, until none do. Nets left in
//! conflict at the end are routed strictly around the others or reported.
//! Paths are straightened to 45-degree runs afterwards, keeping T-junctions
//! and vias where they are. A connection that cannot be made at the class
//! width is tried again at the minimum track width.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap};
use std::time::{Duration, Instant};

use lamina_geom::prim::{Prim, seg_point};
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
    /// Most rip-up-and-reroute passes (default 40).
    #[serde(default)]
    pub passes: Option<u32>,
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
    /// Rip-up-and-reroute passes used.
    pub passes: u32,
}

// ---------------------------------------------------------------- grid

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
    fn at(&self, cell: usize) -> Point {
        let r = cell % self.plane();
        self.center(r % self.nx, r / self.nx)
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
    /// Cells within `r` of `prim` on layer `l`.
    fn cover(&self, prim: &Prim, l: usize, r: f64, out: &mut Vec<u32>) {
        let (x0, y0, x1, y1) = self.range(&prim.bbox(), r);
        for iy in y0..=y1 {
            for ix in x0..=x1 {
                if prim.distance(&point_prim(self.center(ix, iy))) < r - 1e-9 {
                    out.push(self.idx(l, ix, iy) as u32);
                }
            }
        }
    }
}

/// Cells whose centres lie inside a polygon with holes (even-odd scanline).
fn raster(g: &Grid, outer: &[Point], holes: &[Vec<Point>], l: usize, out: &mut Vec<usize>) {
    let Some(bb) = Rect::from_points(outer.iter().copied()) else {
        return;
    };
    let (_, y0, _, y1) = g.range(&bb, 0.0);
    let mut xs: Vec<f64> = Vec::new();
    for iy in y0..=y1 {
        let y = g.y0 + iy as f64 * g.pitch;
        xs.clear();
        for ring in std::iter::once(outer).chain(holes.iter().map(|h| h.as_slice())) {
            let n = ring.len();
            for k in 0..n {
                let (a, b) = (ring[k], ring[(k + 1) % n]);
                if (a.y <= y) != (b.y <= y) {
                    xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        for pair in xs.as_chunks::<2>().0 {
            let lo = ((pair[0] - g.x0) / g.pitch).ceil().max(0.0) as usize;
            let hi = ((pair[1] - g.x0) / g.pitch).floor();
            if hi < 0.0 {
                continue;
            }
            let hi = (hi as usize).min(g.nx - 1);
            for ix in lo..=hi {
                out.push(g.idx(l, ix, iy));
            }
        }
    }
}

fn point_prim(p: Point) -> Prim {
    Prim::Circle { c: p, r: 0.0 }
}

const FREE: u32 = u32::MAX;
const HARD: u32 = u32::MAX - 1;
const MULTI: u32 = u32::MAX - 2;

/// Track and via sizes plus clearance a net is routed with.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Profile {
    hw: f64,
    cl: f64,
    via_r: f64,
    drill_r: f64,
}

impl Profile {
    fn key(&self) -> [i64; 4] {
        [self.hw, self.cl, self.via_r, self.drill_r].map(|v| (v * 1e6).round() as i64)
    }
}

/// Static obstacle map for one profile: who owns each cell.
struct Statics {
    p: Profile,
    /// Net index whose copper (with clearance) covers the cell, FREE, HARD or MULTI.
    owner: Vec<u32>,
    /// A via may be drilled here (holes, via keep-outs and the edge).
    drill_ok: Vec<bool>,
    /// Cell offsets within `via_r - hw`, for via copper checks.
    via_disk: Vec<(isize, isize)>,
    /// Other nets' new wires and vias covering the cell, for this profile.
    occ: Vec<u16>,
}

/// Geometry of a routed net, kept to stamp and unstamp it.
#[derive(Clone, Default)]
struct Routed {
    /// Paths as cell chains (layer-qualified cells).
    paths: Vec<Vec<usize>>,
    /// Via cells (plane cells) with their profile index.
    vias: Vec<(usize, usize)>,
    /// Profile index of each path.
    path_profile: Vec<usize>,
    /// Coverage per profile, filled on demand (geometry never changes
    /// after a `Routed` is built).
    cov: Vec<Vec<u32>>,
}

struct Ctx<'a> {
    d: &'a ProjectData,
    g: Grid,
    edge_dist: Vec<f32>,
    allowed: Vec<bool>,
    deadline: Instant,
    net_ids: BTreeMap<String, u32>,
    net_class: Vec<Option<String>>,
    geo: lamina_geom::BoardGeometry,
    profiles: Vec<Statics>,
    hist: Vec<f32>,
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
        let mut edges: Vec<(Point, Point)> = Vec::new();
        let mut rings = vec![outline.clone()];
        rings.extend(d.board.cutouts.iter().map(|c| c.flatten(0.01)));
        for r in &rings {
            for k in 0..r.len() {
                edges.push((r[k], r[(k + 1) % r.len()]));
            }
        }
        // Inside test by scanline, distance only within a band of the edge
        // (nothing needs more): O(cells + edges * band) instead of
        // O(cells * edges).
        let mut inside_cells = Vec::new();
        raster(&g, &rings[0], &rings[1..], 0, &mut inside_cells);
        // Wide enough for the largest edge rule plus a via.
        let band = (d.board.rules.copper_to_edge + 2.0) as f32;
        let mut edge_dist = vec![-band; g.plane()];
        for c in inside_cells {
            edge_dist[c] = band;
        }
        for (a, b) in &edges {
            let r = Rect::from_points([*a, *b]).unwrap();
            let (x0, y0, x1, y1) = g.range(&r, band as f64);
            for iy in y0..=y1 {
                for ix in x0..=x1 {
                    let k = iy * g.nx + ix;
                    let dd = seg_point(*a, *b, g.center(ix, iy)) as f32;
                    if dd < edge_dist[k].abs() {
                        edge_dist[k] = dd.copysign(edge_dist[k]);
                    }
                }
            }
        }
        let mut allowed = vec![true; nl];
        if let Some(ls) = &opt.layers {
            allowed = vec![false; nl];
            for l in ls {
                if let Some(c) = l.copper() {
                    let i = c.index(nl as u8) as usize;
                    if i < nl {
                        allowed[i] = true;
                    }
                }
            }
        }
        let net_ids: BTreeMap<String, u32> = d
            .nets
            .nets
            .keys()
            .enumerate()
            .map(|(i, n)| (n.clone(), i as u32))
            .collect();
        let net_class = d.nets.nets.values().map(|n| n.class.clone()).collect();
        let n = g.plane() * nl;
        Ok(Ctx {
            d,
            edge_dist,
            allowed,
            deadline: Instant::now()
                + Duration::from_secs_f64(opt.time_limit.unwrap_or(60.0).clamp(1.0, 3600.0)),
            net_ids,
            net_class,
            geo: lamina_geom::build(d),
            profiles: Vec::new(),
            hist: vec![0.0; n],
            g,
        })
    }

    fn class_of(&self, net: Option<&str>) -> Option<&str> {
        net.and_then(|n| self.net_ids.get(n))
            .and_then(|&i| self.net_class[i as usize].as_deref())
    }

    fn profile_for(&self, net: &str, narrow: bool) -> Profile {
        let r = &self.d.board.rules;
        let class = self.class_of(Some(net));
        let nc = r.class(class);
        let mut w = r.track_width_for(class);
        if narrow {
            w = w.min(r.min_track_width);
        }
        Profile {
            hw: w / 2.0,
            cl: r.clearance_for(class),
            via_r: nc.via_diameter.unwrap_or(r.via_diameter) / 2.0,
            drill_r: nc.via_drill.unwrap_or(r.via_drill) / 2.0,
        }
    }

    /// Index of the static map for `p`, built on first use. New wires
    /// already routed are stamped into it too.
    fn statics(&mut self, p: Profile, routed: &[Routed]) -> usize {
        if let Some(i) = self.profiles.iter().position(|s| s.p.key() == p.key()) {
            return i;
        }
        let g = &self.g;
        let rules = &self.d.board.rules;
        let n = g.plane() * g.nl;
        let mut owner = vec![FREE; n];
        let margin = g.pitch * 0.75;
        let edge_need = (rules.copper_to_edge + p.hw) as f32;
        // Layers the router may not use keep their real owners: a through
        // via still has copper there and must clear other nets.
        for l in 0..g.nl {
            for i in 0..g.plane() {
                if self.edge_dist[i] < edge_need {
                    owner[l * g.plane() + i] = HARD;
                }
            }
        }
        let mut cells = Vec::new();
        for it in &self.geo.copper {
            let id = it
                .net
                .as_deref()
                .and_then(|n| self.net_ids.get(n))
                .copied()
                .unwrap_or(HARD);
            let cl =
                p.cl.max(rules.clearance_for(self.class_of(it.net.as_deref())));
            cells.clear();
            g.cover(&it.prim, it.layer as usize, cl + p.hw + margin, &mut cells);
            for &c in &cells {
                let o = &mut owner[c as usize];
                *o = match *o {
                    FREE => id,
                    x if x == id => x,
                    HARD => HARD,
                    _ => {
                        if id == HARD {
                            HARD
                        } else {
                            MULTI
                        }
                    }
                };
            }
        }
        let layers = self.d.board.stackup.copper_layers;
        let mut keepouts: Vec<(Vec<u8>, Prim, Forbid)> = self
            .d
            .routing
            .keepouts
            .iter()
            .map(|k| {
                let ls = k
                    .layers
                    .iter()
                    .filter_map(|l| l.copper().map(|c| c.index(layers)))
                    .collect();
                (
                    ls,
                    lamina_geom::board::poly(k.outline.flatten(0.02)),
                    k.forbid,
                )
            })
            .collect();
        for (refdes, pl) in &self.d.placement.placements {
            let Some(fp) = self
                .d
                .parts
                .parts
                .get(refdes)
                .and_then(|p| self.d.footprints.get(&p.footprint))
            else {
                continue;
            };
            for (ls, pts, f) in lamina_geom::part_keepouts(fp, pl, layers) {
                keepouts.push((ls, lamina_geom::board::poly(pts), f));
            }
        }
        let mut drill_ok = vec![true; g.plane()];
        let via_edge = (rules.copper_to_edge + p.via_r) as f32;
        for (i, ok) in drill_ok.iter_mut().enumerate() {
            if self.edge_dist[i] < via_edge {
                *ok = false;
            }
        }
        for (ls, prim, f) in &keepouts {
            if f.tracks {
                for &l in ls {
                    cells.clear();
                    g.cover(prim, l as usize, p.hw + margin, &mut cells);
                    for &c in &cells {
                        owner[c as usize] = HARD;
                    }
                }
            }
            if f.vias {
                cells.clear();
                g.cover(prim, 0, p.via_r + margin, &mut cells);
                for &c in &cells {
                    drill_ok[c as usize] = false;
                }
            }
        }
        let need = rules.hole_to_hole + p.drill_r + margin;
        for dr in &self.geo.drills {
            cells.clear();
            g.cover(&dr.prim, 0, need, &mut cells);
            for &c in &cells {
                drill_ok[c as usize] = false;
            }
        }
        let reach = (p.via_r - p.hw).max(0.0) + margin;
        let k = (reach / g.pitch).ceil() as isize;
        let mut via_disk = Vec::new();
        for dy in -k..=k {
            for dx in -k..=k {
                if ((dx * dx + dy * dy) as f64).sqrt() * g.pitch <= reach + 1e-9 {
                    via_disk.push((dx, dy));
                }
            }
        }
        self.profiles.push(Statics {
            p,
            owner,
            drill_ok,
            via_disk,
            occ: vec![0; n],
        });
        let pi = self.profiles.len() - 1;
        // Stamp what is already routed for the new profile.
        for r in routed {
            if r.paths.is_empty() {
                continue;
            }
            let cov = self.coverage(r, pi);
            for &c in &cov {
                let o = &mut self.profiles[pi].occ[c as usize];
                *o = o.saturating_add(1);
            }
        }
        pi
    }

    /// Cells a net's geometry `r` covers for victims of profile `pi`.
    fn coverage(&self, r: &Routed, pi: usize) -> Vec<u32> {
        let g = &self.g;
        let victim = self.profiles[pi].p;
        let margin = g.pitch * 0.75;
        let mut cells = Vec::new();
        for (k, path) in r.paths.iter().enumerate() {
            let own = self.profiles[r.path_profile[k]].p;
            let rad = own.cl.max(victim.cl) + victim.hw + margin;
            for w in path.windows(2) {
                let (la, lb) = (w[0] / g.plane(), w[1] / g.plane());
                if la != lb {
                    continue;
                }
                let prim = Prim::Capsule {
                    a: g.at(w[0]),
                    b: g.at(w[1]),
                    r: own.hw,
                };
                g.cover(&prim, la, rad, &mut cells);
            }
            if path.len() == 1 {
                let prim = Prim::Circle {
                    c: g.at(path[0]),
                    r: own.hw,
                };
                g.cover(&prim, path[0] / g.plane(), rad, &mut cells);
            }
        }
        for &(cell, vp) in &r.vias {
            let own = self.profiles[vp].p;
            let rad = own.cl.max(victim.cl) + victim.hw + margin;
            let prim = Prim::Circle {
                c: g.at(cell),
                r: own.via_r,
            };
            for l in 0..g.nl {
                g.cover(&prim, l, rad, &mut cells);
            }
        }
        cells.sort_unstable();
        cells.dedup();
        cells
    }

    /// Fill the coverage cache up to the current number of profiles. A
    /// profile made after the net was stamped stamped it then with the same
    /// coverage, so adding and removing stay symmetric.
    fn ensure_cov(&self, r: &mut Routed) {
        while r.cov.len() < self.profiles.len() {
            let pi = r.cov.len();
            let c = self.coverage(r, pi);
            r.cov.push(c);
        }
    }

    /// Add (or remove) a net's geometry to every profile's occupancy.
    fn stamp(&mut self, r: &mut Routed, add: bool) {
        self.ensure_cov(r);
        for pi in 0..self.profiles.len() {
            let cov = &r.cov[pi];
            let occ = &mut self.profiles[pi].occ;
            for &c in cov {
                let o = &mut occ[c as usize];
                *o = if add {
                    o.saturating_add(1)
                } else {
                    o.saturating_sub(1)
                };
            }
        }
    }

    fn cells_in(&self, prim: &Prim, layers: &[u8]) -> Vec<usize> {
        let g = &self.g;
        let (x0, y0, x1, y1) = g.range(&prim.bbox(), 0.0);
        let mut out = Vec::new();
        for &l in layers {
            if (l as usize) >= g.nl || !self.allowed[l as usize] {
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
                if (l as usize) < g.nl && self.allowed[l as usize] {
                    out.push(g.idx(l as usize, cx, cy));
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------- search

#[derive(Copy, Clone, PartialEq)]
struct Node {
    f: f32,
    /// Path cost when pushed; an entry whose cost was since lowered is stale.
    g: f32,
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

/// Reusable A* buffers; `stamp_no` marks which entries are valid this search.
struct Search {
    cost: Vec<f32>,
    came: Vec<u32>,
    seen: Vec<u32>,
    target: Vec<u32>,
    stamp_no: u32,
}

impl Search {
    fn new(n: usize) -> Search {
        Search {
            cost: vec![0.0; n],
            came: vec![0; n],
            seen: vec![0; n],
            target: vec![0; n],
            stamp_no: 0,
        }
    }
}

/// How hard other nets' wires are: soft costs while negotiating, a wall
/// in the final strict pass.
#[derive(Clone, Copy)]
enum Mode {
    Soft { pres: f32 },
    Strict,
}

struct Query<'q> {
    net: u32,
    pi: usize,
    sources: &'q [usize],
    targets: &'q [usize],
    goals: &'q [Point],
    via_cost: f32,
    mode: Mode,
    /// New vias of all nets so far: (point, drill radius).
    vias: &'q [(Point, f64)],
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

fn astar(ctx: &Ctx, s: &mut Search, q: &Query) -> Option<Vec<usize>> {
    let g = &ctx.g;
    let st = &ctx.profiles[q.pi];
    s.stamp_no = s.stamp_no.wrapping_add(1);
    if s.stamp_no == 0 {
        s.seen.fill(0);
        s.target.fill(0);
        s.stamp_no = 1;
    }
    let stamp_no = s.stamp_no;
    for &t in q.targets {
        s.target[t] = stamp_no;
    }
    let is_target = |s: &Search, j: usize| s.target[j] == stamp_no;
    let free = |j: usize| {
        let o = st.owner[j];
        o == FREE || o == q.net
    };
    // Extra cost of a cell for this net; None = not allowed.
    let extra = |j: usize| -> Option<f32> {
        let occ = st.occ[j];
        match q.mode {
            Mode::Strict if occ > 0 => None,
            Mode::Strict => Some(0.0),
            Mode::Soft { pres } => Some(ctx.hist[j] + pres * occ as f32),
        }
    };
    let target_layers: Vec<usize> = {
        let mut v: Vec<usize> = q.targets.iter().map(|t| t / g.plane()).collect();
        v.sort();
        v.dedup();
        v
    };
    let goals: Vec<(f32, f32)> = q
        .goals
        .iter()
        .map(|p| {
            let (x, y) = g.cell(*p);
            (x as f32, y as f32)
        })
        .collect();
    let h = |idx: usize| -> f32 {
        let l = idx / g.plane();
        let r = idx % g.plane();
        let (ix, iy) = ((r % g.nx) as f32, (r / g.nx) as f32);
        let oct = goals
            .iter()
            .map(|(gx, gy)| {
                let dx = (ix - gx).abs();
                let dy = (iy - gy).abs();
                dx.max(dy) + (std::f32::consts::SQRT_2 - 1.0) * dx.min(dy)
            })
            .fold(f32::INFINITY, f32::min);
        // Goal cells are pad centres; target cells can lie a little closer.
        (oct - 3.0).max(0.0)
            + if target_layers.contains(&l) {
                0.0
            } else {
                q.via_cost
            }
    };
    let mut open = BinaryHeap::new();
    for &src in q.sources {
        s.cost[src] = 0.0;
        s.came[src] = u32::MAX;
        s.seen[src] = stamp_no;
        open.push(Node {
            f: h(src),
            g: 0.0,
            idx: src as u32,
        });
    }
    let mut expanded = 0usize;
    while let Some(Node { idx, g: pushed, .. }) = open.pop() {
        let idx = idx as usize;
        let base = s.cost[idx];
        if pushed > base {
            continue; // stale entry
        }
        if is_target(s, idx) {
            let mut path = vec![idx];
            let mut c = idx;
            while s.came[c] != u32::MAX {
                c = s.came[c] as usize;
                path.push(c);
            }
            path.reverse();
            return Some(path);
        }
        expanded += 1;
        if expanded.is_multiple_of(4096) && Instant::now() > ctx.deadline {
            return None;
        }
        let l = idx / g.plane();
        let r = idx % g.plane();
        let (ix, iy) = ((r % g.nx) as isize, (r / g.nx) as isize);
        let relax = |j: usize, step: f32, s: &mut Search, open: &mut BinaryHeap<Node>| {
            let c = base + step;
            if s.seen[j] != stamp_no || c < s.cost[j] {
                s.seen[j] = stamp_no;
                s.cost[j] = c;
                s.came[j] = idx as u32;
                open.push(Node {
                    f: c + h(j),
                    g: c,
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
            let tj = is_target(s, j);
            if !free(j) && !tj {
                continue;
            }
            if dx != 0 && dy != 0 {
                // No squeezing diagonally between two obstacles.
                let a = g.idx(l, (ix + dx) as usize, iy as usize);
                let b = g.idx(l, ix as usize, (iy + dy) as usize);
                if (!free(a) && !is_target(s, a)) || (!free(b) && !is_target(s, b)) {
                    continue;
                }
            }
            let Some(x) = (if tj { Some(0.0) } else { extra(j) }) else {
                continue;
            };
            relax(j, step * (1.0 + x), s, &mut open);
        }
        // Via to the other layers.
        if !st.drill_ok[r] {
            continue;
        }
        let at = g.at(r);
        if q.vias
            .iter()
            .any(|(p, dr)| p.dist(at) < dr + st.p.drill_r + ctx.d.board.rules.hole_to_hole)
        {
            continue;
        }
        let mut via_extra = 0.0f32;
        let mut ok = true;
        'disk: for &(dx, dy) in &st.via_disk {
            let (x, y) = (ix + dx, iy + dy);
            if x < 0 || y < 0 || x >= g.nx as isize || y >= g.ny as isize {
                ok = false;
                break;
            }
            for l2 in 0..g.nl {
                let j = g.idx(l2, x as usize, y as usize);
                let o = st.owner[j];
                // Via copper may not reach other nets on any layer.
                if !(o == FREE || o == q.net) {
                    ok = false;
                    break 'disk;
                }
                match extra(j) {
                    None => {
                        ok = false;
                        break 'disk;
                    }
                    Some(x) => via_extra = via_extra.max(x),
                }
            }
        }
        if !ok {
            continue;
        }
        for l2 in 0..g.nl {
            if l2 == l || !ctx.allowed[l2] {
                continue;
            }
            let j = l2 * g.plane() + r;
            if !free(j) && !is_target(s, j) {
                continue;
            }
            relax(j, q.via_cost * (1.0 + via_extra), s, &mut open);
        }
    }
    None
}

// ---------------------------------------------------------------- nets

/// One connection of a net, between two pads (ratsnest edge).
struct Conn {
    from: String,
    to: String,
}

struct NetJob {
    name: String,
    id: u32,
    conns: Vec<Conn>,
    /// Pad label to (cells, centre).
    pads: HashMap<String, (Vec<usize>, Point)>,
    /// Pad label to the cells of other copper already joined to the pad.
    islands: HashMap<String, Vec<usize>>,
    length: f64,
}

/// Route one net as a tree. Returns its geometry and the connections that
/// failed.
fn route_net(
    ctx: &mut Ctx,
    s: &mut Search,
    job: &NetJob,
    mode: Mode,
    via_cost: f32,
    all: &[Routed],
    other_vias: &[(Point, f64)],
) -> (Routed, Vec<(usize, String)>) {
    let mut r = Routed::default();
    let mut failed = Vec::new();
    // Groups of pads already joined (by copper outside the router, or now).
    let mut group: HashMap<&str, usize> = HashMap::new();
    let mut cells_of: Vec<Vec<usize>> = Vec::new();
    let mut goals_of: Vec<Vec<Point>> = Vec::new();
    for c in &job.conns {
        for label in [&c.from, &c.to] {
            if group.contains_key(label.as_str()) {
                continue;
            }
            let Some((cells, centre)) = job.pads.get(label) else {
                continue;
            };
            group.insert(label, cells_of.len());
            cells_of.push(cells.clone());
            goals_of.push(vec![*centre]);
        }
    }
    let normal = ctx.profile_for(&job.name, false);
    let narrow = ctx.profile_for(&job.name, true);
    // Island cells near other nets' copper would start a track too close.
    let npi = ctx.statics(normal, all);
    for (label, &gi) in &group {
        if let Some(cells) = job.islands.get(*label) {
            let st = &ctx.profiles[npi];
            cells_of[gi].extend(
                cells
                    .iter()
                    .copied()
                    .filter(|&c| st.owner[c] == FREE || st.owner[c] == job.id),
            );
        }
    }
    let mut vias: Vec<(Point, f64)> = other_vias.to_vec();
    for (ci, c) in job.conns.iter().enumerate() {
        let (Some(&ga), Some(&gb)) = (group.get(c.from.as_str()), group.get(c.to.as_str())) else {
            failed.push((ci, "pad not found".to_string()));
            continue;
        };
        if ga == gb {
            continue;
        }
        let mut found = None;
        for (k, p) in [normal, narrow].into_iter().enumerate() {
            if k == 1 && p.key() == normal.key() {
                break;
            }
            let pi = ctx.statics(p, all);
            let q = Query {
                net: job.id,
                pi,
                sources: &cells_of[ga],
                targets: &cells_of[gb],
                goals: &goals_of[gb],
                via_cost,
                mode,
                vias: &vias,
            };
            if let Some(path) = astar(ctx, s, &q) {
                found = Some((path, pi));
                break;
            }
        }
        let Some((path, pi)) = found else {
            failed.push((
                ci,
                if Instant::now() > ctx.deadline {
                    "time limit".to_string()
                } else {
                    "no path found".to_string()
                },
            ));
            continue;
        };
        // A start cell that is already a target: the groups touch, so they
        // merge without a wire.
        let wire = path.len() >= 2;
        let g = &ctx.g;
        for w in path.windows(2) {
            if w[0] / g.plane() != w[1] / g.plane() {
                let cell = w[0] % g.plane();
                r.vias.push((cell, pi));
                vias.push((g.at(cell), ctx.profiles[pi].p.drill_r));
            }
        }
        // Merge the two groups; the new wire joins them.
        let (keep, gone) = (ga.min(gb), ga.max(gb));
        let moved = std::mem::take(&mut cells_of[gone]);
        cells_of[keep].extend(moved);
        cells_of[keep].extend(path.iter().copied());
        let moved = std::mem::take(&mut goals_of[gone]);
        goals_of[keep].extend(moved);
        // Wire cells are targets too; a few of them guide the heuristic.
        goals_of[keep].extend(path.iter().step_by(8).map(|&c| g.at(c)));
        for v in group.values_mut() {
            if *v == gone {
                *v = keep;
            }
        }
        if wire {
            r.paths.push(path);
            r.path_profile.push(pi);
        }
    }
    (r, failed)
}

/// Cells where a stamped net's geometry sits on another net's: occupancy
/// above the net's own share.
fn in_conflict(ctx: &Ctx, r: &mut Routed) -> Vec<usize> {
    ctx.ensure_cov(r);
    let g = &ctx.g;
    let mut bad = Vec::new();
    let others = |pi: usize, c: usize| {
        let own = r.cov[pi].binary_search(&(c as u32)).is_ok() as u16;
        ctx.profiles[pi].occ[c] > own
    };
    for (k, path) in r.paths.iter().enumerate() {
        let pi = r.path_profile[k];
        bad.extend(path.iter().copied().filter(|&c| others(pi, c)));
    }
    for &(cell, pi) in &r.vias {
        let st = &ctx.profiles[pi];
        let (ix, iy) = ((cell % g.nx) as isize, (cell / g.nx) as isize);
        for &(dx, dy) in &st.via_disk {
            let (x, y) = (ix + dx, iy + dy);
            if x < 0 || y < 0 || x >= g.nx as isize || y >= g.ny as isize {
                continue;
            }
            for l in 0..g.nl {
                let j = g.idx(l, x as usize, y as usize);
                if others(pi, j) {
                    bad.push(j);
                }
            }
        }
    }
    bad
}

/// Route every unrouted connection that the options select. Connections
/// that fail are repaired: the new wires around them are taken up and
/// routed again together with them on a grid twice as fine.
pub fn autoroute(d: &ProjectData, opt: &RouteOptions) -> Result<RouteOutcome, String> {
    let start = Instant::now();
    let limit = opt.time_limit.unwrap_or(60.0).clamp(1.0, 3600.0);
    let left = || limit - start.elapsed().as_secs_f64();
    let mut out = route_once(d, opt)?;
    if !out.failed.is_empty() && left() > 2.0 {
        repair(d, &mut out, opt, left())?;
    }
    // New tracks cut pours into islands: route what the refilled pours now
    // leave open, then repair what that cannot reach.
    for _ in 0..5 {
        if left() < 2.0 {
            break;
        }
        let d2 = with_routes(d, &out);
        let failed: BTreeSet<(String, String, String)> = out
            .failed
            .iter()
            .map(|f| (f.net.clone(), f.from.clone(), f.to.clone()))
            .collect();
        let analysis = lamina_drc::analyze(&d2);
        let mut open: Vec<String> = analysis
            .ratsnest
            .iter()
            .filter(|e| {
                selected(opt, &e.net)
                    && !failed.contains(&(e.net.clone(), e.from.clone(), e.to.clone()))
            })
            .map(|e| e.net.clone())
            .collect();
        open.sort();
        open.dedup();
        if open.is_empty() {
            break;
        }
        let opt2 = RouteOptions {
            nets: Some(open),
            time_limit: Some(left()),
            ..opt.clone()
        };
        let more = route_once_with(&d2, &opt2, Some(analysis))?;
        let progress = !more.tracks.is_empty() || !more.vias.is_empty();
        out.tracks.extend(more.tracks);
        out.vias.extend(more.vias);
        out.routed.extend(more.routed);
        out.failed.extend(more.failed);
        out.passes += more.passes;
        if !out.failed.is_empty() && left() > 2.0 {
            repair(d, &mut out, opt, left())?;
        } else if !progress {
            break;
        }
    }
    Ok(out)
}

fn selected(opt: &RouteOptions, net: &str) -> bool {
    !opt.skip.iter().any(|s| s == net)
        && opt.nets.as_ref().is_none_or(|n| n.iter().any(|x| x == net))
}

fn with_routes(d: &ProjectData, out: &RouteOutcome) -> ProjectData {
    let mut d2 = d.clone();
    d2.routing.tracks.extend(out.tracks.iter().cloned());
    d2.routing.vias.extend(out.vias.iter().cloned());
    d2
}

/// Where a pad label (`U1.3`) is on the board.
fn pad_point(d: &ProjectData, label: &str) -> Option<Point> {
    let pr: PadRef = label.parse().ok()?;
    let part = d.parts.parts.get(&pr.part)?;
    let pl = d.placement.placements.get(&pr.part)?;
    let fp = d.footprints.get(&part.footprint)?;
    fp.pads
        .iter()
        .find(|p| p.number == pr.pad)
        .map(|p| pl.transform(p.at))
}

/// Take up the new wires around the failed connections and route those nets
/// again, together with the failed ones, on a grid twice as fine (unless a
/// grid was given). The failures in `out` are replaced by what still fails.
fn repair(
    d: &ProjectData,
    out: &mut RouteOutcome,
    opt: &RouteOptions,
    left: f64,
) -> Result<(), String> {
    let mut region: Option<Rect> = None;
    for f in &out.failed {
        for p in [pad_point(d, &f.from), pad_point(d, &f.to)]
            .into_iter()
            .flatten()
        {
            let r = Rect { min: p, max: p };
            region = Some(region.map_or(r, |q| q.union(&r)));
        }
    }
    let mut nets: BTreeSet<String> = out.failed.iter().map(|f| f.net.clone()).collect();
    if let Some(r) = region {
        let m = 3.0;
        let inside = |p: &Point| {
            p.x >= r.min.x - m && p.x <= r.max.x + m && p.y >= r.min.y - m && p.y <= r.max.y + m
        };
        for t in &out.tracks {
            if t.points.iter().any(inside) {
                nets.insert(t.net.clone());
            }
        }
        for v in &out.vias {
            if inside(&v.at) {
                nets.insert(v.net.clone());
            }
        }
    }
    nets.retain(|n| selected(opt, n));
    if nets.is_empty() {
        return Ok(());
    }
    out.tracks.retain(|t| !nets.contains(&t.net));
    out.vias.retain(|v| !nets.contains(&v.net));
    out.routed
        .retain(|r| r.split_once(':').is_none_or(|(n, _)| !nets.contains(n)));
    out.failed.retain(|f| !nets.contains(&f.net));
    let d2 = with_routes(d, out);
    let opt2 = RouteOptions {
        nets: Some(nets.into_iter().collect()),
        grid: Some(opt.grid.unwrap_or(0.05)),
        time_limit: Some(left),
        ..opt.clone()
    };
    let fine = route_once(&d2, &opt2)?;
    out.tracks.extend(fine.tracks);
    out.vias.extend(fine.vias);
    out.routed.extend(fine.routed);
    out.failed.extend(fine.failed);
    out.passes += fine.passes;
    Ok(())
}

fn route_once(d: &ProjectData, opt: &RouteOptions) -> Result<RouteOutcome, String> {
    route_once_with(d, opt, None)
}

/// One routing run; `analysis` is the DRC analysis of `d` when the caller
/// already has it.
fn route_once_with(
    d: &ProjectData,
    opt: &RouteOptions,
    analysis: Option<lamina_drc::Analysis>,
) -> Result<RouteOutcome, String> {
    let t0 = Instant::now();
    let dbg = std::env::var_os("LAMINA_ROUTE_DEBUG").is_some();
    let mut ctx = Ctx::new(d, opt)?;
    if dbg {
        eprintln!("ctx {:?} grid {}", t0.elapsed(), ctx.g.pitch);
    }
    let via_cost = opt.via_cost.unwrap_or(25.0) as f32;
    let analysis = analysis.unwrap_or_else(|| lamina_drc::analyze(d));
    if dbg {
        eprintln!("analyze {:?}", t0.elapsed());
    }
    let mut edges = analysis.ratsnest.clone();
    edges.retain(|e| {
        !opt.skip.contains(&e.net) && opt.nets.as_ref().is_none_or(|n| n.contains(&e.net))
    });

    // Jobs per net, with each pad's cells.
    let layers = d.board.stackup.copper_layers;
    let mut jobs: BTreeMap<String, NetJob> = BTreeMap::new();
    for e in &edges {
        let Some(&id) = ctx.net_ids.get(&e.net) else {
            continue;
        };
        let job = jobs.entry(e.net.clone()).or_insert_with(|| NetJob {
            name: e.net.clone(),
            id,
            conns: Vec::new(),
            pads: HashMap::new(),
            islands: HashMap::new(),
            length: 0.0,
        });
        job.length += e.a.dist(e.b);
        job.conns.push(Conn {
            from: e.from.clone(),
            to: e.to.clone(),
        });
    }
    // Copper already joined to a pad (pads, tracks, vias, pour pieces) is
    // part of it for routing: a connection may start or end anywhere on it.
    let copper = &analysis.geometry.copper;
    let pad_island: HashMap<(&str, &str), usize> = copper
        .iter()
        .enumerate()
        .filter_map(|(i, it)| match &it.owner {
            lamina_geom::Owner::Pad { part, pad, .. } => {
                Some(((part.as_str(), pad.as_str()), analysis.island[i]))
            }
            _ => None,
        })
        .collect();
    let mut island_cells: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut cells_of_island = |root: usize, ctx: &Ctx| -> Vec<usize> {
        island_cells
            .entry(root)
            .or_insert_with(|| {
                let mut out = Vec::new();
                for (i, it) in copper.iter().enumerate() {
                    if analysis.island[i] == root {
                        out.extend(ctx.cells_in(&it.prim, &[it.layer]));
                    }
                }
                for (f, roots) in analysis.fills.iter().zip(&analysis.fill_island) {
                    let Some(c) = f.layer.copper() else { continue };
                    let l = c.index(layers) as usize;
                    if l >= ctx.g.nl || !ctx.allowed[l] {
                        continue;
                    }
                    for (poly, &r) in f.polygons.iter().zip(roots) {
                        if r == root {
                            raster(&ctx.g, &poly.outer, &poly.holes, l, &mut out);
                        }
                    }
                }
                out.sort_unstable();
                out.dedup();
                out
            })
            .clone()
    };
    for job in jobs.values_mut() {
        job.conns
            .sort_by(|a, b| a.from.cmp(&b.from).then(a.to.cmp(&b.to)));
        let labels: BTreeSet<String> = job
            .conns
            .iter()
            .flat_map(|c| [c.from.clone(), c.to.clone()])
            .collect();
        for label in labels {
            let label_key = label.clone();
            let Ok(pr) = label.parse::<PadRef>() else {
                continue;
            };
            let (Some(part), Some(pl)) = (
                d.parts.parts.get(&pr.part),
                d.placement.placements.get(&pr.part),
            ) else {
                continue;
            };
            let Some(fp) = d.footprints.get(&part.footprint) else {
                continue;
            };
            if let Some((p, _, prim, ls)) = lamina_geom::placed_pads(fp, pl, layers)
                .into_iter()
                .find(|(p, ..)| p.number == pr.pad)
            {
                let cells = ctx.cells_in(&prim, &ls);
                let island = pad_island
                    .get(&(pr.part.as_str(), pr.pad.as_str()))
                    .map(|&root| cells_of_island(root, &ctx))
                    .unwrap_or_default();
                if !cells.is_empty() {
                    job.pads.insert(label, (cells, pl.transform(p.at)));
                    job.islands.insert(label_key, island);
                }
            }
        }
    }
    // Short nets first; nets with a pour last, since the pour joins most of
    // their pads and only what it leaves open is in the ratsnest.
    let poured: BTreeSet<&str> = d
        .routing
        .zones
        .iter()
        .filter_map(|z| z.net.as_deref())
        .collect();
    let mut order: Vec<String> = jobs.keys().cloned().collect();
    order.sort_by(|a, b| {
        poured
            .contains(a.as_str())
            .cmp(&poured.contains(b.as_str()))
            .then(jobs[a].length.total_cmp(&jobs[b].length))
    });
    let n_nets = ctx.net_ids.len();
    let mut routed: Vec<Routed> = vec![Routed::default(); n_nets];
    let mut failures: BTreeMap<String, Vec<(usize, String)>> = BTreeMap::new();
    let mut search = Search::new(ctx.g.plane() * ctx.g.nl);
    let passes = opt.passes.unwrap_or(40).max(1);
    let mut todo: Vec<String> = order.clone();
    let mut pres = 0.5f32;
    let mut used = 0;

    let new_vias = |routed: &[Routed], ctx: &Ctx, skip: u32| -> Vec<(Point, f64)> {
        routed
            .iter()
            .enumerate()
            .filter(|(i, _)| *i as u32 != skip)
            .flat_map(|(_, r)| {
                r.vias
                    .iter()
                    .map(|&(c, pi)| (ctx.g.at(c), ctx.profiles[pi].p.drill_r))
            })
            .collect()
    };

    if dbg {
        eprintln!("jobs {:?} ({} nets)", t0.elapsed(), order.len());
    }
    for pass in 0..passes {
        used = pass + 1;
        if dbg {
            eprintln!("pass {pass} at {:?}, {} nets", t0.elapsed(), todo.len());
        }
        for name in &todo {
            if Instant::now() > ctx.deadline {
                break;
            }
            let job = &jobs[name];
            let ni = job.id as usize;
            let mut old = std::mem::take(&mut routed[ni]);
            if !old.paths.is_empty() {
                ctx.stamp(&mut old, false);
            }
            let others = new_vias(&routed, &ctx, job.id);
            let (mut r, failed) = route_net(
                &mut ctx,
                &mut search,
                job,
                Mode::Soft { pres },
                via_cost,
                &routed,
                &others,
            );
            ctx.stamp(&mut r, true);
            routed[ni] = r;
            failures.insert(name.clone(), failed);
        }
        // Which nets still overlap another?
        let mut conflicted = Vec::new();
        for name in &order {
            let ni = jobs[name].id as usize;
            if routed[ni].paths.is_empty() {
                continue;
            }
            let bad = in_conflict(&ctx, &mut routed[ni]);
            if !bad.is_empty() {
                for c in bad {
                    ctx.hist[c] += 1.0;
                }
                conflicted.push(name.clone());
            }
        }
        if conflicted.is_empty() || Instant::now() > ctx.deadline {
            todo = conflicted;
            break;
        }
        todo = conflicted;
        // Capped: past this the soft costs are as good as walls anyway.
        pres = (pres * 1.6).min(1000.0);
    }

    if dbg {
        eprintln!("negotiated {:?}, {} left", t0.elapsed(), todo.len());
    }
    // Still overlapping: route those nets strictly around the rest.
    for name in &todo {
        let job = &jobs[name];
        let ni = job.id as usize;
        let mut old = std::mem::take(&mut routed[ni]);
        if !old.paths.is_empty() {
            ctx.stamp(&mut old, false);
        }
        let others = new_vias(&routed, &ctx, job.id);
        let (mut r, failed) = route_net(
            &mut ctx,
            &mut search,
            job,
            Mode::Strict,
            via_cost,
            &routed,
            &others,
        );
        ctx.stamp(&mut r, true);
        routed[ni] = r;
        failures.insert(name.clone(), failed);
    }

    // Straighten, net by net, against the final geometry of the others.
    let mut out = RouteOutcome {
        passes: used,
        ..Default::default()
    };
    let mut track_ids: Vec<String> = d.routing.tracks.iter().map(|t| t.id.clone()).collect();
    let mut via_ids: Vec<String> = d.routing.vias.iter().map(|v| v.id.clone()).collect();
    for name in &order {
        let job = &jobs[name];
        let ni = job.id as usize;
        let mut r = std::mem::take(&mut routed[ni]);
        if r.paths.is_empty() {
            routed[ni] = r;
            continue;
        }
        ctx.stamp(&mut r, false);
        let g = &ctx.g;
        // Path ends are junctions or pads: keep them as vertices.
        let pinned: BTreeSet<usize> = r
            .paths
            .iter()
            .flat_map(|p| [p[0], *p.last().unwrap()])
            .collect();
        let pad_at: HashMap<usize, Point> = job
            .pads
            .values()
            .flat_map(|(cells, c)| cells.iter().map(move |&k| (k, *c)))
            .collect();
        let mut straight = Routed {
            vias: r.vias.clone(),
            ..Default::default()
        };
        for (k, path) in r.paths.iter().enumerate() {
            let pi = r.path_profile[k];
            let st = &ctx.profiles[pi];
            let ok = |c: usize| {
                let o = st.owner[c];
                (o == FREE || o == job.id) && st.occ[c] == 0
            };
            let clear = |a: usize, b: usize| {
                let l = a / g.plane();
                let (ra, rb) = (a % g.plane(), b % g.plane());
                let (ax, ay) = ((ra % g.nx) as isize, (ra / g.nx) as isize);
                let (bx, by) = ((rb % g.nx) as isize, (rb / g.nx) as isize);
                let (dx, dy) = (bx - ax, by - ay);
                if !(dx == 0 || dy == 0 || dx.abs() == dy.abs()) {
                    return false;
                }
                let steps = dx.abs().max(dy.abs());
                // The ends are path cells already; check what lies between.
                (1..steps).all(|t| {
                    let x = ax + if steps == 0 { 0 } else { dx * t / steps };
                    let y = ay + if steps == 0 { 0 } else { dy * t / steps };
                    ok(g.idx(l, x as usize, y as usize))
                })
            };
            // Per-layer runs, each straightened.
            let mut runs: Vec<Vec<usize>> = Vec::new();
            for &c in path {
                match runs.last_mut() {
                    Some(run) if run[0] / g.plane() == c / g.plane() => run.push(c),
                    _ => runs.push(vec![c]),
                }
            }
            let n_runs = runs.len();
            let hw = st.p.hw;
            let mut kept_path = Vec::new();
            for (ri, cells) in runs.iter().enumerate() {
                let mut keep = vec![cells[0]];
                let mut i = 0;
                while i < cells.len() - 1 {
                    let limit = (i + 1..cells.len())
                        .find(|&t| pinned.contains(&cells[t]))
                        .unwrap_or(cells.len() - 1);
                    let mut j = limit;
                    while j > i + 1 && !clear(cells[i], cells[j]) {
                        j -= 1;
                    }
                    keep.push(cells[j]);
                    i = j;
                }
                let l = cells[0] / g.plane();
                let mut pts: Vec<Point> = keep
                    .iter()
                    .map(|&c| g.at(c))
                    .map(|p| Point::new(json::round6(p.x), json::round6(p.y)))
                    .collect();
                if ri == 0 {
                    if let Some(c) = pad_at.get(&cells[0]) {
                        pts.insert(0, *c);
                    }
                }
                if ri == n_runs - 1 {
                    if let Some(c) = pad_at.get(cells.last().unwrap()) {
                        pts.push(*c);
                    }
                }
                pts.dedup_by(|a, b| a.dist(*b) < 1e-9);
                // Expand kept vertices back into a cell chain for stamping.
                for w in keep.windows(2) {
                    let (ra, rb) = (w[0] % g.plane(), w[1] % g.plane());
                    let (ax, ay) = ((ra % g.nx) as isize, (ra / g.nx) as isize);
                    let (bx, by) = ((rb % g.nx) as isize, (rb / g.nx) as isize);
                    let steps = (bx - ax).abs().max((by - ay).abs()).max(1);
                    for t in 0..steps {
                        let x = ax + (bx - ax) * t / steps;
                        let y = ay + (by - ay) * t / steps;
                        kept_path.push(g.idx(l, x as usize, y as usize));
                    }
                }
                kept_path.push(*keep.last().unwrap());
                if pts.len() >= 2 {
                    let id = next_id_from("t", &track_ids);
                    track_ids.push(id.clone());
                    out.tracks.push(Track {
                        id,
                        net: job.name.clone(),
                        layer: Layer::Copper(Copper::from_index(l as u8, g.nl as u8)),
                        width: json::round6(hw * 2.0),
                        points: pts,
                    });
                }
            }
            straight.paths.push(kept_path);
            straight.path_profile.push(pi);
        }
        for &(cell, pi) in &r.vias {
            let p = ctx.g.at(cell);
            let st = ctx.profiles[pi].p;
            let id = next_id_from("v", &via_ids);
            via_ids.push(id.clone());
            out.vias.push(Via {
                id,
                net: job.name.clone(),
                at: Point::new(json::round6(p.x), json::round6(p.y)),
                diameter: json::round6(st.via_r * 2.0),
                drill: json::round6(st.drill_r * 2.0),
                from: None,
                to: None,
            });
        }
        ctx.stamp(&mut straight, true);
        routed[ni] = straight;
    }

    for name in &order {
        let job = &jobs[name];
        let failed = failures.get(name).cloned().unwrap_or_default();
        for (ci, c) in job.conns.iter().enumerate() {
            match failed.iter().find(|(i, _)| *i == ci) {
                Some((_, reason)) => out.failed.push(Failure {
                    net: name.clone(),
                    from: c.from.clone(),
                    to: c.to.clone(),
                    reason: reason.clone(),
                }),
                None => out.routed.push(format!("{}: {} -> {}", name, c.from, c.to)),
            }
        }
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
