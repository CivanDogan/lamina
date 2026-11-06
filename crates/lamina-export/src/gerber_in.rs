//! General readers for Gerber (RS-274X with X2 attributes) and Excellon files.
//! They read the fab outputs of any CAD program.

use std::collections::HashMap;

use geo::{BooleanOps, ConvexHull, Coord, LineString, MultiPoint, MultiPolygon, Polygon};
use lamina_model::{Edge, Point, arc_points};

/// The tolerance in mm to flatten arcs and circles.
const TOL: f64 = 0.005;

/// One Gerber file read into geometry.
#[derive(Debug)]
pub struct GerberImage {
    /// Final dark image in mm, Y up.
    pub image: MultiPolygon<f64>,
    /// The centre lines of all draws, to read board outlines.
    pub edges: Vec<Edge>,
    /// The `%TF.FileFunction` value, for example `Copper,L1,Top`.
    pub function: Option<String>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug)]
enum Aperture {
    Circle(f64),
    /// All other shapes, as polygons at the origin, each with its exposure (dark or clear).
    Shapes(Vec<(bool, Vec<Point>)>),
}

impl Aperture {
    fn flash(&self, at: Point) -> MultiPolygon<f64> {
        match self {
            Aperture::Circle(d) => poly_mp(&circle(at, d / 2.0)),
            Aperture::Shapes(shapes) => {
                let mut acc = MultiPolygon::new(vec![]);
                for (dark, pts) in shapes {
                    let moved: Vec<Point> = pts.iter().map(|p| p.add(at)).collect();
                    let m = poly_mp(&moved);
                    acc = if *dark {
                        acc.union(&m)
                    } else {
                        acc.difference(&m)
                    };
                }
                acc
            }
        }
    }

    /// The convex outline of the aperture (for draws with a rectangle aperture).
    fn outline(&self) -> Vec<Point> {
        match self {
            Aperture::Circle(d) => circle(Point::new(0.0, 0.0), d / 2.0),
            Aperture::Shapes(shapes) => shapes
                .iter()
                .filter(|(dark, _)| *dark)
                .flat_map(|(_, p)| p.iter().copied())
                .collect(),
        }
    }
}

fn circle(c: Point, r: f64) -> Vec<Point> {
    if r <= 0.0 {
        return Vec::new();
    }
    let step = 2.0 * (1.0 - TOL.min(r) / r).clamp(-1.0, 1.0).acos();
    let n = ((std::f64::consts::TAU / step.max(1e-3)).ceil() as usize).clamp(12, 256);
    (0..n)
        .map(|k| {
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            Point::new(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect()
}

fn ring(pts: &[Point]) -> LineString<f64> {
    let mut v: Vec<Coord<f64>> = pts.iter().map(|p| Coord { x: p.x, y: p.y }).collect();
    if let Some(f) = v.first().copied() {
        v.push(f);
    }
    LineString::new(v)
}

fn poly_mp(pts: &[Point]) -> MultiPolygon<f64> {
    if pts.len() < 3 {
        return MultiPolygon::new(vec![]);
    }
    // A union with an empty polygon makes the orientation normal and repairs rings that
    // touch themselves.
    let p = MultiPolygon::new(vec![Polygon::new(ring(pts), vec![])]);
    p.union(&MultiPolygon::new(vec![]))
}

fn hull(pts: &[Point]) -> MultiPolygon<f64> {
    let mp: MultiPoint<f64> = pts.iter().map(|p| geo::Point::new(p.x, p.y)).collect();
    MultiPolygon::new(vec![mp.convex_hull()])
}

fn rect(w: f64, h: f64) -> Vec<Point> {
    vec![
        Point::new(-w / 2.0, -h / 2.0),
        Point::new(w / 2.0, -h / 2.0),
        Point::new(w / 2.0, h / 2.0),
        Point::new(-w / 2.0, h / 2.0),
    ]
}

fn obround(w: f64, h: f64) -> Vec<Point> {
    let r = w.min(h) / 2.0;
    let (dx, dy) = if w >= h {
        (w / 2.0 - r, 0.0)
    } else {
        (0.0, h / 2.0 - r)
    };
    let a = Point::new(-dx, -dy);
    let b = Point::new(dx, dy);
    lamina_geom::prim::capsule_points(a, b, r, TOL)
}

fn regular(d: f64, n: u32, rot: f64) -> Vec<Point> {
    let n = n.clamp(3, 64);
    (0..n)
        .map(|k| {
            let a = (rot + 360.0 * k as f64 / n as f64).to_radians();
            Point::new(d / 2.0 * a.cos(), d / 2.0 * a.sin())
        })
        .collect()
}

// ---------------------------------------------------------------- macros

#[derive(Clone, Debug)]
enum MacroItem {
    /// `$n=expr`
    Assign(usize, String),
    /// The primitive code and the expressions of its arguments.
    Prim(u32, Vec<String>),
}

fn eval(expr: &str, vars: &HashMap<usize, f64>) -> Result<f64, String> {
    let toks: Vec<char> = expr.chars().filter(|c| !c.is_whitespace()).collect();
    let mut i = 0;
    let v = eval_sum(&toks, &mut i, vars)?;
    if i != toks.len() {
        return Err(format!("The macro expression `{expr}` is not valid"));
    }
    Ok(v)
}

fn eval_sum(t: &[char], i: &mut usize, vars: &HashMap<usize, f64>) -> Result<f64, String> {
    let mut v = eval_prod(t, i, vars)?;
    while *i < t.len() && (t[*i] == '+' || t[*i] == '-') {
        let op = t[*i];
        *i += 1;
        let r = eval_prod(t, i, vars)?;
        v = if op == '+' { v + r } else { v - r };
    }
    Ok(v)
}

fn eval_prod(t: &[char], i: &mut usize, vars: &HashMap<usize, f64>) -> Result<f64, String> {
    let mut v = eval_unary(t, i, vars)?;
    while *i < t.len() && matches!(t[*i], 'x' | 'X' | '/') {
        let op = t[*i];
        *i += 1;
        let r = eval_unary(t, i, vars)?;
        v = if op == '/' { v / r } else { v * r };
    }
    Ok(v)
}

fn eval_unary(t: &[char], i: &mut usize, vars: &HashMap<usize, f64>) -> Result<f64, String> {
    match t.get(*i) {
        Some('-') => {
            *i += 1;
            Ok(-eval_unary(t, i, vars)?)
        }
        Some('+') => {
            *i += 1;
            eval_unary(t, i, vars)
        }
        Some('(') => {
            *i += 1;
            let v = eval_sum(t, i, vars)?;
            if t.get(*i) != Some(&')') {
                return Err("The macro expression has no closing `)`".into());
            }
            *i += 1;
            Ok(v)
        }
        Some('$') => {
            *i += 1;
            let s = *i;
            while *i < t.len() && t[*i].is_ascii_digit() {
                *i += 1;
            }
            let n: usize = t[s..*i]
                .iter()
                .collect::<String>()
                .parse()
                .map_err(|_| "The macro variable is not valid")?;
            Ok(vars.get(&n).copied().unwrap_or(0.0))
        }
        Some(_) => {
            let s = *i;
            while *i < t.len() && (t[*i].is_ascii_digit() || t[*i] == '.') {
                *i += 1;
            }
            let text: String = t[s..*i].iter().collect();
            text.parse()
                .map_err(|_| format!("The number `{text}` in the macro expression is not valid"))
        }
        None => Err("The macro expression is not complete".into()),
    }
}

fn rot(p: Point, deg: f64) -> Point {
    p.rotated(deg)
}

/// Calculate a macro with its parameters. The result is polygons, each with its
/// exposure (file units).
fn eval_macro(
    items: &[MacroItem],
    params: &[f64],
    notes: &mut Vec<String>,
) -> Result<Vec<(bool, Vec<Point>)>, String> {
    let mut vars: HashMap<usize, f64> = params
        .iter()
        .enumerate()
        .map(|(k, v)| (k + 1, *v))
        .collect();
    let mut out: Vec<(bool, Vec<Point>)> = Vec::new();
    for it in items {
        match it {
            MacroItem::Assign(n, e) => {
                let v = eval(e, &vars)?;
                vars.insert(*n, v);
            }
            MacroItem::Prim(code, args) => {
                let a: Vec<f64> = args
                    .iter()
                    .map(|e| eval(e, &vars))
                    .collect::<Result<_, _>>()?;
                let g = |k: usize| a.get(k).copied().unwrap_or(0.0);
                let exposure = |v: f64| v != 0.0;
                match code {
                    1 => {
                        let c = rot(Point::new(g(2), g(3)), g(4));
                        out.push((exposure(g(0)), circle(c, g(1) / 2.0)));
                    }
                    20 => {
                        let (p1, p2, w) = (Point::new(g(2), g(3)), Point::new(g(4), g(5)), g(1));
                        let d = p2.sub(p1);
                        let len = (d.x * d.x + d.y * d.y).sqrt().max(1e-12);
                        let nrm = Point::new(-d.y / len * w / 2.0, d.x / len * w / 2.0);
                        let pts = [p1.add(nrm), p2.add(nrm), p2.sub(nrm), p1.sub(nrm)];
                        out.push((exposure(g(0)), pts.iter().map(|p| rot(*p, g(6))).collect()));
                    }
                    21 => {
                        let c = Point::new(g(3), g(4));
                        let pts: Vec<Point> = rect(g(1), g(2))
                            .into_iter()
                            .map(|p| rot(p.add(c), g(5)))
                            .collect();
                        out.push((exposure(g(0)), pts));
                    }
                    22 => {
                        let ll = Point::new(g(3), g(4));
                        let pts: Vec<Point> = [
                            Point::new(0.0, 0.0),
                            Point::new(g(1), 0.0),
                            Point::new(g(1), g(2)),
                            Point::new(0.0, g(2)),
                        ]
                        .iter()
                        .map(|p| rot(p.add(ll), g(5)))
                        .collect();
                        out.push((exposure(g(0)), pts));
                    }
                    4 => {
                        let n = g(1) as usize;
                        let mut pts: Vec<Point> = (0..=n)
                            .map(|k| Point::new(g(2 + 2 * k), g(3 + 2 * k)))
                            .collect();
                        let r = g(4 + 2 * n);
                        if pts.len() > 1 && pts[0].dist(*pts.last().unwrap()) < 1e-9 {
                            pts.pop();
                        }
                        out.push((exposure(g(0)), pts.into_iter().map(|p| rot(p, r)).collect()));
                    }
                    5 => {
                        let c = Point::new(g(2), g(3));
                        let pts: Vec<Point> = regular(g(4), g(1) as u32, 0.0)
                            .into_iter()
                            .map(|p| rot(p.add(c), g(5)))
                            .collect();
                        out.push((exposure(g(0)), pts));
                    }
                    7 => {
                        // Thermal: a ring with four gaps on the axes.
                        let c = Point::new(g(0), g(1));
                        let (od, id, gap, r) = (g(2), g(3), g(4), g(5));
                        let mut m = poly_mp(&circle(Point::new(0.0, 0.0), od / 2.0))
                            .difference(&poly_mp(&circle(Point::new(0.0, 0.0), id / 2.0)));
                        m = m.difference(&poly_mp(&rect(od * 2.0, gap)));
                        m = m.difference(&poly_mp(&rect(gap, od * 2.0)));
                        for p in m.0 {
                            let pts: Vec<Point> = p
                                .exterior()
                                .coords()
                                .map(|q| rot(Point::new(q.x, q.y).add(c), r))
                                .collect();
                            out.push((true, pts));
                        }
                    }
                    6 => notes.push("Lamina skipped a moiré aperture primitive".into()),
                    other => notes.push(format!(
                        "Lamina skipped the aperture macro primitive {other}"
                    )),
                }
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- the reader

/// An open step-and-repeat block: the counts, the steps (mm) and its objects.
type StepRepeat = (u32, u32, f64, f64, Vec<(bool, MultiPolygon<f64>)>);

#[derive(Clone, Copy, PartialEq)]
enum Interp {
    Linear,
    Cw,
    Ccw,
}

struct Reader {
    xi: u32,
    xd: u32,
    yd: u32,
    trailing: bool,
    /// The scale from file units to mm.
    scale: f64,
    apertures: HashMap<u32, Aperture>,
    macros: HashMap<String, Vec<MacroItem>>,
    current: Option<u32>,
    pos: Point,
    interp: Interp,
    multi: bool,
    region: Option<Vec<Vec<Point>>>,
    dark: bool,
    last_op: u32,
    /// Objects of an open step-and-repeat block.
    sr: Option<StepRepeat>,
    acc: MultiPolygon<f64>,
    pending: Vec<MultiPolygon<f64>>,
    out: GerberImage,
}

fn union_tree(mut v: Vec<MultiPolygon<f64>>) -> MultiPolygon<f64> {
    while v.len() > 1 {
        let mut next = Vec::with_capacity(v.len().div_ceil(2));
        let mut it = v.into_iter();
        while let Some(a) = it.next() {
            match it.next() {
                Some(b) => next.push(a.union(&b)),
                None => next.push(a),
            }
        }
        v = next;
    }
    v.pop().unwrap_or_else(|| MultiPolygon::new(vec![]))
}

impl Reader {
    fn new() -> Self {
        Reader {
            xi: 3,
            xd: 6,
            yd: 6,
            trailing: false,
            scale: 1.0,
            apertures: HashMap::new(),
            macros: HashMap::new(),
            current: None,
            pos: Point::new(0.0, 0.0),
            interp: Interp::Linear,
            multi: true,
            region: None,
            dark: true,
            last_op: 1,
            sr: None,
            acc: MultiPolygon::new(vec![]),
            pending: Vec::new(),
            out: GerberImage {
                image: MultiPolygon::new(vec![]),
                edges: Vec::new(),
                function: None,
                notes: Vec::new(),
            },
        }
    }

    fn coord(&self, s: &str, dec: u32) -> Result<f64, String> {
        let v = if s.contains('.') {
            s.parse::<f64>()
                .map_err(|_| format!("The coordinate `{s}` is not valid"))?
        } else {
            let (neg, digits) = match s.strip_prefix('-') {
                Some(d) => (true, d),
                None => (false, s.strip_prefix('+').unwrap_or(s)),
            };
            let digits = if self.trailing {
                let total = (self.xi + dec) as usize;
                format!("{digits:0<total$}")
            } else {
                digits.to_string()
            };
            let n: f64 = digits
                .parse::<i64>()
                .map_err(|_| format!("The coordinate `{s}` is not valid"))?
                as f64;
            let v = n / 10f64.powi(dec as i32);
            if neg { -v } else { v }
        };
        Ok(v * self.scale)
    }

    fn emit(&mut self, dark: bool, m: MultiPolygon<f64>) {
        if let Some(sr) = self.sr.as_mut() {
            sr.4.push((dark, m));
            return;
        }
        self.add(dark, m);
    }

    fn add(&mut self, dark: bool, m: MultiPolygon<f64>) {
        if dark {
            self.pending.push(m);
            if self.pending.len() > 96 {
                let p = union_tree(std::mem::take(&mut self.pending));
                self.pending.push(p);
            }
        } else {
            self.flush();
            self.acc = self.acc.difference(&m);
        }
    }

    fn flush(&mut self) {
        if !self.pending.is_empty() {
            let p = union_tree(std::mem::take(&mut self.pending));
            self.acc = self.acc.union(&p);
        }
    }

    /// The points of a move from `a` to `b` (with flat arcs), and the edge that the move draws.
    fn path(&self, a: Point, b: Point, ij: (f64, f64), have_ij: bool) -> (Vec<Point>, Edge) {
        if self.interp == Interp::Linear || !have_ij {
            return (vec![a, b], Edge::Line(a, b));
        }
        let ccw = self.interp == Interp::Ccw;
        let center = if self.multi {
            Point::new(a.x + ij.0, a.y + ij.1)
        } else {
            // Single-quadrant: select the signs that give an arc of 90° or less.
            let mut best: Option<(f64, Point)> = None;
            for sx in [1.0, -1.0] {
                for sy in [1.0, -1.0] {
                    let c = Point::new(a.x + sx * ij.0.abs(), a.y + sy * ij.1.abs());
                    let sweep = lamina_model::arc_sweep(a, b, c, ccw, false).abs();
                    let err = (a.dist(c) - b.dist(c)).abs();
                    if sweep <= std::f64::consts::FRAC_PI_2 + 1e-6
                        && best.is_none_or(|(e, _)| err < e)
                    {
                        best = Some((err, c));
                    }
                }
            }
            best.map(|x| x.1)
                .unwrap_or(Point::new(a.x + ij.0, a.y + ij.1))
        };
        let full = self.multi && a.dist(b) < 1e-9;
        let mut pts = vec![a];
        pts.extend(arc_points(a, b, center, ccw, TOL, full));
        pts.push(b);
        let edge = if full {
            Edge::Loop(lamina_model::Contour::circle(center, a.dist(center)))
        } else {
            Edge::Arc { a, b, center, ccw }
        };
        (pts, edge)
    }

    fn draw(&mut self, pts: &[Point]) {
        let Some(ap) = self.current.and_then(|c| self.apertures.get(&c)).cloned() else {
            return;
        };
        let m = match &ap {
            Aperture::Circle(d) => {
                let r = d / 2.0;
                if r <= 0.0 {
                    return;
                }
                let parts: Vec<MultiPolygon<f64>> = pts
                    .windows(2)
                    .map(|w| poly_mp(&lamina_geom::prim::capsule_points(w[0], w[1], r, TOL)))
                    .collect();
                union_tree(parts)
            }
            Aperture::Shapes(_) => {
                let outline = ap.outline();
                let parts: Vec<MultiPolygon<f64>> = pts
                    .windows(2)
                    .map(|w| {
                        let mut all: Vec<Point> = outline.iter().map(|p| p.add(w[0])).collect();
                        all.extend(outline.iter().map(|p| p.add(w[1])));
                        hull(&all)
                    })
                    .collect();
                union_tree(parts)
            }
        };
        let dark = self.dark;
        self.emit(dark, m);
    }

    fn extended(&mut self, block: &str) -> Result<(), String> {
        let words: Vec<&str> = block
            .split('*')
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .collect();
        let Some(first) = words.first() else {
            return Ok(());
        };
        let first = first.replace(['\r', '\n'], "");
        if let Some(rest) = first.strip_prefix("FS") {
            // FSLAX46Y46 (also FSTAX.., and the rare incremental FSLIX..)
            self.trailing = rest.starts_with('T');
            if rest.chars().nth(1) == Some('I') {
                self.out
                    .notes
                    .push("Lamina does not support incremental coordinates".into());
            }
            let xi = rest.find('X').ok_or("The FS command is not valid")?;
            let yi = rest.find('Y').ok_or("The FS command is not valid")?;
            let xs: Vec<u32> = rest[xi + 1..yi]
                .chars()
                .filter_map(|c| c.to_digit(10))
                .collect();
            let ys: Vec<u32> = rest[yi + 1..]
                .chars()
                .filter_map(|c| c.to_digit(10))
                .collect();
            if xs.len() >= 2 && ys.len() >= 2 {
                self.xi = xs[0];
                self.xd = xs[1];
                self.yd = ys[1];
            }
        } else if first == "MOMM" {
            self.scale = 1.0;
        } else if first == "MOIN" {
            self.scale = 25.4;
        } else if first == "LPD" {
            self.dark = true;
        } else if first == "LPC" {
            self.dark = false;
        } else if let Some(rest) = first.strip_prefix("AM") {
            let mut items = Vec::new();
            for w in &words[1..] {
                let w = w.replace(['\r', '\n'], "");
                if w.starts_with('0') && (w.len() == 1 || w[1..].starts_with([' ', ','])) {
                    continue;
                }
                if let Some(assign) = w.strip_prefix('$') {
                    if let Some((n, e)) = assign.split_once('=') {
                        items.push(MacroItem::Assign(
                            n.trim()
                                .parse()
                                .map_err(|_| "The macro variable is not valid")?,
                            e.to_string(),
                        ));
                        continue;
                    }
                }
                let w: String = w.chars().filter(|c| !c.is_whitespace()).collect();
                let mut parts = w.split(',');
                let code: u32 = parts
                    .next()
                    .unwrap_or("")
                    .parse()
                    .map_err(|_| format!("The macro primitive `{w}` is not valid"))?;
                items.push(MacroItem::Prim(code, parts.map(str::to_string).collect()));
            }
            self.macros.insert(rest.to_string(), items);
        } else if let Some(rest) = first.strip_prefix("ADD") {
            let split = rest
                .find(|c: char| !c.is_ascii_digit())
                .ok_or("The AD command is not valid")?;
            let num: u32 = rest[..split]
                .parse()
                .map_err(|_| "The aperture number is not valid")?;
            let def = &rest[split..];
            let (kind, args) = def.split_once(',').unwrap_or((def, ""));
            let vals: Vec<f64> = args
                .split('X')
                .filter(|s| !s.is_empty())
                .map(|s| {
                    s.trim()
                        .parse::<f64>()
                        .map_err(|_| format!("The aperture value `{s}` is not valid"))
                })
                .collect::<Result<_, _>>()?;
            let s = self.scale;
            let g = |k: usize| vals.get(k).copied().unwrap_or(0.0);
            let with_hole = |shape: Vec<Point>, hole: f64| -> Aperture {
                let mut v = vec![(true, shape)];
                if hole > 0.0 {
                    v.push((false, circle(Point::new(0.0, 0.0), hole * s / 2.0)));
                }
                Aperture::Shapes(v)
            };
            let ap = match kind {
                "C" if g(1) <= 0.0 => Aperture::Circle(g(0) * s),
                "C" => with_hole(circle(Point::new(0.0, 0.0), g(0) * s / 2.0), g(1)),
                "R" => with_hole(rect(g(0) * s, g(1) * s), g(2)),
                "O" => with_hole(obround(g(0) * s, g(1) * s), g(2)),
                "P" => with_hole(regular(g(0) * s, g(1) as u32, g(2)), g(3)),
                name => {
                    let items = self
                        .macros
                        .get(name)
                        .ok_or_else(|| format!("The aperture macro `{name}` is not defined"))?
                        .clone();
                    let shapes = eval_macro(&items, &vals, &mut self.out.notes)?;
                    Aperture::Shapes(
                        shapes
                            .into_iter()
                            .map(|(d, pts)| {
                                (
                                    d,
                                    pts.into_iter()
                                        .map(|p| Point::new(p.x * s, p.y * s))
                                        .collect(),
                                )
                            })
                            .collect(),
                    )
                }
            };
            self.apertures.insert(num, ap);
        } else if let Some(rest) = first.strip_prefix("SR") {
            // Close the open block, if there is one. If the command gives counts, open a new block.
            if let Some((nx, ny, dx, dy, objs)) = self.sr.take() {
                for j in 0..ny {
                    for i in 0..nx {
                        let off = (i as f64 * dx, j as f64 * dy);
                        for (dark, m) in &objs {
                            let moved = MultiPolygon::new(
                                m.0.iter()
                                    .map(|p| {
                                        let t = |ls: &LineString<f64>| {
                                            LineString::new(
                                                ls.coords()
                                                    .map(|c| Coord {
                                                        x: c.x + off.0,
                                                        y: c.y + off.1,
                                                    })
                                                    .collect(),
                                            )
                                        };
                                        Polygon::new(
                                            t(p.exterior()),
                                            p.interiors().iter().map(t).collect(),
                                        )
                                    })
                                    .collect(),
                            );
                            self.add(*dark, moved);
                        }
                    }
                }
            }
            if !rest.is_empty() {
                let num = |key: char| -> f64 {
                    rest.find(key)
                        .map(|i| {
                            let t: String = rest[i + 1..]
                                .chars()
                                .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
                                .collect();
                            t.parse().unwrap_or(0.0)
                        })
                        .unwrap_or(0.0)
                };
                let (nx, ny) = (num('X').max(1.0) as u32, num('Y').max(1.0) as u32);
                if nx > 1 || ny > 1 {
                    self.sr = Some((
                        nx,
                        ny,
                        num('I') * self.scale,
                        num('J') * self.scale,
                        Vec::new(),
                    ));
                }
            }
        } else if let Some(rest) = first.strip_prefix("TF.FileFunction,") {
            self.out.function = Some(rest.to_string());
        } else if first.starts_with("IPNEG") {
            self.out
                .notes
                .push("Lamina does not support negative image polarity".into());
        } else if first.starts_with("AB") {
            self.out
                .notes
                .push("Lamina does not support block apertures".into());
        } else if first.starts_with("LM") || first.starts_with("LR") || first.starts_with("LS") {
            if !matches!(first.as_str(), "LMN" | "LR0" | "LS1") {
                self.out
                    .notes
                    .push(format!("Lamina ignores {first} (aperture transformation)"));
            }
        }
        Ok(())
    }

    fn word(&mut self, w: &str) -> Result<(), String> {
        let w: String = w.chars().filter(|c| !c.is_whitespace()).collect();
        if w.is_empty() {
            return Ok(());
        }
        if w.starts_with("G04") || (w.starts_with("G4") && !w.starts_with("G40")) {
            return Ok(());
        }
        // Split the word into pairs of a letter and a value.
        let mut fields: Vec<(char, String)> = Vec::new();
        let b: Vec<char> = w.chars().collect();
        let mut i = 0;
        while i < b.len() {
            let c = b[i];
            if !c.is_ascii_alphabetic() {
                return Err(format!("`{w}` contains the unexpected character `{c}`"));
            }
            i += 1;
            let s = i;
            while i < b.len() && (b[i].is_ascii_digit() || matches!(b[i], '-' | '+' | '.')) {
                i += 1;
            }
            fields.push((c.to_ascii_uppercase(), b[s..i].iter().collect()));
        }
        let mut x = None;
        let mut y = None;
        let (mut ii, mut jj, mut have_ij) = (0.0, 0.0, false);
        let mut op = None;
        for (c, v) in &fields {
            match c {
                'G' => match v.parse::<u32>().unwrap_or(999) {
                    1 => self.interp = Interp::Linear,
                    2 => self.interp = Interp::Cw,
                    3 => self.interp = Interp::Ccw,
                    36 => {
                        self.region = Some(Vec::new());
                    }
                    37 => {
                        if let Some(contours) = self.region.take() {
                            let dark = self.dark;
                            for c in contours {
                                if c.len() >= 3 {
                                    let m = poly_mp(&c);
                                    self.emit(dark, m);
                                }
                            }
                        }
                    }
                    74 => self.multi = false,
                    75 => self.multi = true,
                    70 => self.scale = 25.4,
                    71 => self.scale = 1.0,
                    90 | 54 | 55 => {}
                    91 => self
                        .out
                        .notes
                        .push("Lamina does not support incremental coordinates".into()),
                    _ => {}
                },
                'D' => {
                    let n: u32 = v
                        .parse()
                        .map_err(|_| format!("The D-code in `{w}` is not valid"))?;
                    if n >= 10 {
                        self.current = Some(n);
                    } else {
                        op = Some(n);
                    }
                }
                'M' => {}
                'X' => x = Some(self.coord(v, self.xd)?),
                'Y' => y = Some(self.coord(v, self.yd)?),
                'I' => {
                    ii = self.coord(v, self.xd)?;
                    have_ij = true;
                }
                'J' => {
                    jj = self.coord(v, self.yd)?;
                    have_ij = true;
                }
                _ => {}
            }
        }
        if x.is_none() && y.is_none() && op.is_none() {
            return Ok(());
        }
        let target = Point::new(x.unwrap_or(self.pos.x), y.unwrap_or(self.pos.y));
        let op = op.unwrap_or(self.last_op);
        self.last_op = op;
        match op {
            1 => {
                let (pts, edge) = self.path(self.pos, target, (ii, jj), have_ij);
                if let Some(contours) = self.region.as_mut() {
                    match contours.last_mut() {
                        Some(c) if !c.is_empty() => c.extend(pts.into_iter().skip(1)),
                        _ => contours.push(pts),
                    }
                } else {
                    self.out.edges.push(edge);
                    self.draw(&pts);
                }
            }
            2 => {
                if let Some(contours) = self.region.as_mut() {
                    contours.push(vec![target]);
                }
            }
            3 => {
                if let Some(ap) = self.current.and_then(|c| self.apertures.get(&c)).cloned() {
                    let m = ap.flash(target);
                    let dark = self.dark;
                    self.emit(dark, m);
                }
            }
            _ => {}
        }
        self.pos = target;
        Ok(())
    }
}

/// Read a Gerber file.
pub fn read(text: &str) -> Result<GerberImage, String> {
    let mut r = Reader::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let end = text[i + 1..]
                    .find('%')
                    .ok_or("An extended command has no closing `%`")?
                    + i
                    + 1;
                r.extended(&text[i + 1..end])?;
                i = end + 1;
            }
            b' ' | b'\r' | b'\n' | b'\t' => i += 1,
            _ => {
                let end = text[i..].find('*').map(|k| k + i).unwrap_or(b.len());
                let w = &text[i..end];
                if w.trim_start().starts_with("M02") || w.trim_start().starts_with("M00") {
                    break;
                }
                r.word(w).map_err(|e| format!("{e} (near byte {i})"))?;
                i = end + 1;
            }
        }
    }
    if r.sr.is_some() {
        r.extended("SR")?;
    }
    r.flush();
    r.out.image = std::mem::replace(&mut r.acc, MultiPolygon::new(vec![]));
    Ok(r.out)
}

// ---------------------------------------------------------------- Excellon

/// A drilled hole or slot: the diameter and the two ends (the same point for a round hole).
#[derive(Debug, Clone, PartialEq)]
pub struct DrillHit {
    pub diameter: f64,
    pub a: Point,
    pub b: Point,
    pub plated: Option<bool>,
}

#[derive(Debug, Default)]
pub struct DrillFile {
    pub hits: Vec<DrillHit>,
    /// The plating from the attributes of the file, if the file gives it.
    pub plated: Option<bool>,
    pub notes: Vec<String>,
}

pub fn read_drill(text: &str) -> Result<DrillFile, String> {
    let mut out = DrillFile::default();
    let mut scale = 1.0;
    let mut lz = true; // the file keeps leading zeros and removes trailing zeros
    let (mut int_digits, mut dec_digits) = (3usize, 3usize);
    let mut decimal_seen_fmt = false;
    let mut tools: HashMap<u32, (f64, Option<bool>)> = HashMap::new();
    let mut tool_plated: Option<bool> = None;
    let mut current: Option<u32> = None;
    let mut pos = Point::new(0.0, 0.0);
    let mut header = false;
    let mut route: Option<Vec<Point>> = None;
    let mut routing_mode = false;
    let coord = |s: &str, lz: bool, intd: usize, decd: usize, scale: f64| -> Option<f64> {
        if s.contains('.') {
            return s.parse::<f64>().ok().map(|v| v * scale);
        }
        let (neg, d) = match s.strip_prefix('-') {
            Some(d) => (true, d),
            None => (false, s.strip_prefix('+').unwrap_or(s)),
        };
        let v = if lz {
            // The file keeps leading zeros, so the first `intd` digits are the integer part.
            let total = intd + decd;
            let padded = format!("{d:0<total$}");
            padded.parse::<i64>().ok()? as f64 / 10f64.powi(decd as i32)
        } else {
            d.parse::<i64>().ok()? as f64 / 10f64.powi(decd as i32)
        };
        Some(if neg { -v } else { v } * scale)
    };
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(c) = line.strip_prefix(';') {
            let c = c.trim();
            if c.contains("TF.FileFunction") {
                if c.contains("NonPlated") {
                    out.plated = Some(false);
                } else if c.contains("Plated") {
                    out.plated = Some(true);
                }
            }
            if c.contains("TA.AperFunction") {
                tool_plated = if c.contains("NonPlated") {
                    Some(false)
                } else if c.contains("Plated") {
                    Some(true)
                } else {
                    None
                };
            }
            let up = c.to_ascii_uppercase();
            if up.starts_with("TYPE=NON_PLATED") {
                out.plated = Some(false);
            } else if up.starts_with("TYPE=PLATED") {
                out.plated = Some(true);
            }
            if up.starts_with("FORMAT={") {
                // KiCad: FORMAT={3:3/ absolute / metric / suppress trailing zeros}
                if let Some(f) = up.split('{').nth(1) {
                    let digits: Vec<usize> = f
                        .split(['/', '}'])
                        .next()
                        .unwrap_or("")
                        .split(':')
                        .filter_map(|v| v.trim().parse().ok())
                        .collect();
                    if digits.len() == 2 {
                        int_digits = digits[0];
                        dec_digits = digits[1];
                        decimal_seen_fmt = true;
                    }
                }
            }
            continue;
        }
        let up = line.to_ascii_uppercase();
        if up == "M48" {
            header = true;
            continue;
        }
        if header && (up == "%" || up == "M95") {
            header = false;
            continue;
        }
        if up.starts_with("METRIC") || up.starts_with("M71") {
            scale = 1.0;
            if up.contains("TZ") {
                lz = false;
            } else if up.contains("LZ") {
                lz = true;
            }
            if let Some(f) = up.split(',').find(|p| p.contains('.')) {
                let mut it = f.split('.');
                int_digits = it.next().map(str::len).unwrap_or(3);
                dec_digits = it.next().map(str::len).unwrap_or(3);
                decimal_seen_fmt = true;
            } else if !decimal_seen_fmt {
                int_digits = 3;
                dec_digits = 3;
            }
            continue;
        }
        if up.starts_with("INCH") || up.starts_with("M72") {
            scale = 25.4;
            if up.contains("TZ") {
                lz = false;
            } else if up.contains("LZ") {
                lz = true;
            }
            if let Some(f) = up.split(',').find(|p| p.contains('.')) {
                let mut it = f.split('.');
                int_digits = it.next().map(str::len).unwrap_or(2);
                dec_digits = it.next().map(str::len).unwrap_or(4);
                decimal_seen_fmt = true;
            } else if !decimal_seen_fmt {
                int_digits = 2;
                dec_digits = 4;
            }
            continue;
        }
        if let Some(rest) = up.strip_prefix('T') {
            let n_end = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            let Ok(n) = rest[..n_end].parse::<u32>() else {
                continue;
            };
            if let Some(ci) = rest.find('C') {
                let dstr: String = rest[ci + 1..]
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                if let Ok(d) = dstr.parse::<f64>() {
                    tools.insert(n, (d * scale, tool_plated.take()));
                }
                if !header {
                    current = Some(n);
                }
            } else {
                current = if n == 0 { None } else { Some(n) };
            }
            continue;
        }
        if header {
            continue;
        }
        match up.as_str() {
            "M15" => {
                route = Some(vec![pos]);
                continue;
            }
            "M16" | "M17" => {
                if let (Some(r), Some(t)) = (route.take(), current) {
                    if let Some((d, pl)) = tools.get(&t) {
                        for w in r.windows(2) {
                            out.hits.push(DrillHit {
                                diameter: *d,
                                a: w[0],
                                b: w[1],
                                plated: *pl,
                            });
                        }
                    }
                }
                continue;
            }
            "G05" | "G81" => {
                routing_mode = false;
                continue;
            }
            "M30" | "M00" => break,
            _ => {}
        }
        if up.starts_with("G00") {
            routing_mode = true;
        }
        // Coordinates: [G00|G01]X..Y..[G85X..Y..]
        let parse_xy = |s: &str, pos: Point| -> Option<Point> {
            let xi = s.find('X');
            let yi = s.find('Y');
            let take = |i: usize| -> String {
                s[i + 1..]
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
                    .collect()
            };
            let x = xi
                .map(take)
                .and_then(|v| coord(&v, lz, int_digits, dec_digits, scale))
                .unwrap_or(pos.x);
            let y = yi
                .map(take)
                .and_then(|v| coord(&v, lz, int_digits, dec_digits, scale))
                .unwrap_or(pos.y);
            (xi.is_some() || yi.is_some()).then_some(Point::new(x, y))
        };
        let body = up.trim_start_matches("G00").trim_start_matches("G01");
        if let Some((a, b)) = body.split_once("G85") {
            let (Some(pa), Some(t)) = (parse_xy(a, pos), current) else {
                continue;
            };
            let pb = parse_xy(b, pa).unwrap_or(pa);
            if let Some((d, pl)) = tools.get(&t) {
                out.hits.push(DrillHit {
                    diameter: *d,
                    a: pa,
                    b: pb,
                    plated: *pl,
                });
            }
            pos = pb;
            continue;
        }
        if let Some(p) = parse_xy(body, pos) {
            pos = p;
            if let Some(r) = route.as_mut() {
                r.push(p);
            } else if !routing_mode {
                if let Some(t) = current {
                    match tools.get(&t) {
                        Some((d, pl)) => out.hits.push(DrillHit {
                            diameter: *d,
                            a: p,
                            b: p,
                            plated: *pl,
                        }),
                        None => out
                            .notes
                            .push(format!("A hit uses the tool T{t}, which is not defined")),
                    }
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::Area;

    fn area(m: &MultiPolygon<f64>) -> f64 {
        m.unsigned_area()
    }

    #[test]
    fn kicad_style_file_with_macro_and_arcs() {
        let src = "%TF.FileFunction,Copper,L1,Top*%\n%FSLAX46Y46*%\n%MOMM*%\nG04 comment*\n%LPD*%\nG01*\n\
%AMRoundRect*\n0 Rectangle with rounded corners*\n0 $1 Rounding radius*\n0 $2 $3 $4 $5 $6 $7 $8 $9 X,Y pos of 4 corners*\n\
4,1,4,$2,$3,$4,$5,$6,$7,$8,$9,$2,$3,0*\n1,1,$1+$1,$2,$3*\n1,1,$1+$1,$4,$5*\n1,1,$1+$1,$6,$7*\n1,1,$1+$1,$8,$9*\n\
20,1,$1+$1,$2,$3,$4,$5,0*\n20,1,$1+$1,$4,$5,$6,$7,0*\n20,1,$1+$1,$6,$7,$8,$9,0*\n20,1,$1+$1,$8,$9,$2,$3,0*%\n\
%ADD10RoundRect,0.25X-1.000000X-0.500000X1.000000X-0.500000X1.000000X0.500000X-1.000000X0.500000*%\n\
%ADD11C,0.500000*%\n%ADD12R,2.000000X1.000000*%\n\
D10*\nX10000000Y10000000D03*\nD12*\nX20000000Y10000000D03*\nD11*\nX0Y0D02*\nX5000000Y0D01*\n\
X25000000Y20000000D02*\nG75*\nG03*\nX20000000Y25000000I-5000000J0D01*\nG01*\nG36*\nX30000000Y0D02*\nX32000000Y0D01*\nX32000000Y2000000D01*\nX30000000Y2000000D01*\nX30000000Y0D01*\nG37*\nM02*\n";
        let g = read(src).unwrap();
        assert_eq!(g.function.as_deref(), Some("Copper,L1,Top"));
        // Items: a rounded rect 2.5 x 1.5 with r 0.25, a rect 2 x 1, a region 2 x 2,
        // a line and a separate quarter arc. The line and the arc are 0.5 wide with
        // round ends.
        let cap = std::f64::consts::PI * 0.25 * 0.25;
        let rr = 2.5 * 1.5 - (4.0 - std::f64::consts::PI) * 0.25 * 0.25;
        let line = 5.0 * 0.5 + cap;
        let arc = std::f64::consts::FRAC_PI_2 * 5.0 * 0.5 + cap;
        let expect = rr + 2.0 + 4.0 + line + arc;
        assert!(
            (area(&g.image) - expect).abs() < 0.05,
            "{} vs {expect}",
            area(&g.image)
        );
        assert!(
            matches!(g.edges[1], Edge::Arc { ccw: true, .. }),
            "{:?}",
            g.edges[1]
        );
    }

    #[test]
    fn inch_trailing_zeros_and_clear_polarity() {
        let src = "%FSTAX24Y24*%\n%MOIN*%\n%ADD10C,0.1*%\n%ADD11R,0.04X0.04*%\nD10*\nX01Y01D03*\n%LPC*%\nD11*\nX01Y01D03*\n%LPD*%\nM02*\n";
        let g = read(src).unwrap();
        // The file removes trailing zeros, so "01" with format 2.4 is 01.0000 inch.
        let b = geo::BoundingRect::bounding_rect(&g.image).unwrap();
        assert!((b.center().x - 25.4).abs() < 1e-6, "{b:?}");
        let disc = std::f64::consts::PI * (1.27f64).powi(2);
        let hole = (0.04 * 25.4f64).powi(2);
        // Circles are flattened to a tolerance of 5 µm. Here, this removes about 0.03 mm².
        assert!(
            (area(&g.image) - (disc - hole)).abs() < 0.04,
            "{}",
            area(&g.image)
        );
    }

    #[test]
    fn step_and_repeat_and_single_quadrant() {
        let src = "%FSLAX33Y33*%\n%MOMM*%\n%ADD10C,1*%\n%SRX3Y2I5.0J4.0*%\nD10*\nX0Y0D03*\n%SR*%\n\
G74*\nX30000Y0D02*\nG02*\nX20000Y-10000I10000J0D01*\nM02*\n";
        let g = read(src).unwrap();
        let disc = std::f64::consts::PI * 0.25;
        let arc_len = std::f64::consts::FRAC_PI_2 * 10.0;
        let expect = 6.0 * disc + arc_len * 1.0 + disc;
        assert!(
            (area(&g.image) - expect).abs() < 0.1,
            "{} vs {expect}",
            area(&g.image)
        );
        match &g.edges[0] {
            Edge::Arc { center, ccw, .. } => {
                assert!(center.dist(Point::new(20.0, 0.0)) < 1e-9, "{center:?}");
                assert!(!ccw);
            }
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn excellon_variants() {
        let kicad = "M48\n; #@! TF.FileFunction,NonPlated,1,2,NPTH\nFMAT,2\nMETRIC\nT1C3.200\n%\nG90\nG05\nT1\nX4.0Y4.0\nX60.0Y4.0\nT0\nM30\n";
        let d = read_drill(kicad).unwrap();
        assert_eq!(d.plated, Some(false));
        assert_eq!(d.hits.len(), 2);
        assert_eq!(d.hits[1].a, Point::new(60.0, 4.0));
        assert!((d.hits[0].diameter - 3.2).abs() < 1e-9);

        // The file keeps trailing zeros, so each value is digits / 10^4 inch.
        let inch =
            "M48\nINCH,TZ\nT01C0.0400\n%\nT01\nX010000Y005\nX020000Y010000G85X030000Y010000\nM30\n";
        let d = read_drill(inch).unwrap();
        assert_eq!(d.hits.len(), 2);
        assert!(
            d.hits[0].a.dist(Point::new(25.4, 0.0127)) < 1e-6,
            "{:?}",
            d.hits[0].a
        );
        assert!(
            d.hits[1].a.dist(Point::new(50.8, 25.4)) < 1e-6
                && d.hits[1].b.dist(Point::new(76.2, 25.4)) < 1e-6,
            "{:?}",
            d.hits[1]
        );
        assert!((d.hits[0].diameter - 1.016).abs() < 1e-9);

        let lz = "M48\n;TYPE=PLATED\nMETRIC,LZ,000.000\nT1C0.300\n%\nT1\nX010000Y020000\nM30\n";
        let d = read_drill(lz).unwrap();
        assert_eq!(d.plated, Some(true));
        assert_eq!(d.hits[0].a, Point::new(10.0, 20.0));

        let routed = "M48\nMETRIC\nT2C1.0\n%\nT2\nG00X1.0Y1.0\nM15\nG01X3.0Y1.0\nM16\nG05\nM30\n";
        let d = read_drill(routed).unwrap();
        assert_eq!(d.hits.len(), 1);
        assert_eq!(
            (d.hits[0].a, d.hits[0].b),
            (Point::new(1.0, 1.0), Point::new(3.0, 1.0))
        );
    }
}
