//! Basic geometry types. Units are millimetres, Y up, angles in degrees CCW.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn dist(self, o: Point) -> f64 {
        (self.x - o.x).hypot(self.y - o.y)
    }

    /// Rotate around the origin by `deg` degrees CCW.
    pub fn rotated(self, deg: f64) -> Point {
        if deg == 0.0 {
            return self;
        }
        let (s, c) = deg.to_radians().sin_cos();
        Point::new(self.x * c - self.y * s, self.x * s + self.y * c)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn add(self, o: Point) -> Point {
        Point::new(self.x + o.x, self.y + o.y)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn sub(self, o: Point) -> Point {
        Point::new(self.x - o.x, self.y - o.y)
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl std::ops::Add for Point {
    type Output = Point;
    fn add(self, o: Point) -> Point {
        Point::new(self.x + o.x, self.y + o.y)
    }
}

impl std::ops::Sub for Point {
    type Output = Point;
    fn sub(self, o: Point) -> Point {
        Point::new(self.x - o.x, self.y - o.y)
    }
}

impl std::ops::Mul<f64> for Point {
    type Output = Point;
    fn mul(self, k: f64) -> Point {
        Point::new(self.x * k, self.y * k)
    }
}

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rect {
    pub min: Point,
    pub max: Point,
}

impl Rect {
    pub fn width(&self) -> f64 {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> f64 {
        self.max.y - self.min.y
    }

    pub fn from_points(pts: impl IntoIterator<Item = Point>) -> Option<Rect> {
        let mut it = pts.into_iter();
        let first = it.next()?;
        let mut r = Rect {
            min: first,
            max: first,
        };
        for p in it {
            r.include(p);
        }
        Some(r)
    }

    pub fn include(&mut self, p: Point) {
        self.min.x = self.min.x.min(p.x);
        self.min.y = self.min.y.min(p.y);
        self.max.x = self.max.x.max(p.x);
        self.max.y = self.max.y.max(p.y);
    }

    pub fn union(&self, o: &Rect) -> Rect {
        let mut r = *self;
        r.include(o.min);
        r.include(o.max);
        r
    }
}

/// The edge from a vertex to the next one is an arc around `center`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ArcTo {
    pub center: Point,
    /// Counter-clockwise when true.
    pub ccw: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Vertex {
    pub x: f64,
    pub y: f64,
    /// When set, the edge to the next vertex is an arc. A contour of one
    /// vertex with an arc is a full circle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc: Option<ArcTo>,
}

impl Vertex {
    pub const fn line(x: f64, y: f64) -> Self {
        Self { x, y, arc: None }
    }

    pub fn point(&self) -> Point {
        Point::new(self.x, self.y)
    }
}

/// A closed outline made of lines and arcs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Contour {
    pub vertices: Vec<Vertex>,
}

impl Contour {
    pub fn rect(x: f64, y: f64, w: f64, h: f64) -> Contour {
        Contour {
            vertices: vec![
                Vertex::line(x, y),
                Vertex::line(x + w, y),
                Vertex::line(x + w, y + h),
                Vertex::line(x, y + h),
            ],
        }
    }

    /// Rectangle with rounded corners of radius `r`.
    pub fn rounded_rect(x: f64, y: f64, w: f64, h: f64, r: f64) -> Contour {
        if r <= 0.0 {
            return Self::rect(x, y, w, h);
        }
        let arc = |cx, cy| {
            Some(ArcTo {
                center: Point::new(cx, cy),
                ccw: true,
            })
        };
        let (x1, y1) = (x + w, y + h);
        Contour {
            vertices: vec![
                Vertex {
                    x: x + r,
                    y,
                    arc: None,
                },
                Vertex {
                    x: x1 - r,
                    y,
                    arc: arc(x1 - r, y + r),
                },
                Vertex {
                    x: x1,
                    y: y + r,
                    arc: None,
                },
                Vertex {
                    x: x1,
                    y: y1 - r,
                    arc: arc(x1 - r, y1 - r),
                },
                Vertex {
                    x: x1 - r,
                    y: y1,
                    arc: None,
                },
                Vertex {
                    x: x + r,
                    y: y1,
                    arc: arc(x + r, y1 - r),
                },
                Vertex {
                    x,
                    y: y1 - r,
                    arc: None,
                },
                Vertex {
                    x,
                    y: y + r,
                    arc: arc(x + r, y + r),
                },
            ],
        }
    }

    pub fn circle(center: Point, radius: f64) -> Contour {
        Contour {
            vertices: vec![Vertex {
                x: center.x + radius,
                y: center.y,
                arc: Some(ArcTo { center, ccw: true }),
            }],
        }
    }

    /// Check shape sanity. Returns a description of the first problem.
    pub fn check(&self) -> Result<(), String> {
        let n = self.vertices.len();
        if n == 0 {
            return Err("contour has no vertices".into());
        }
        for (i, v) in self.vertices.iter().enumerate() {
            if !v.point().is_finite() {
                return Err(format!("vertex {i} is not finite"));
            }
            if let Some(a) = v.arc {
                if !a.center.is_finite() {
                    return Err(format!("arc centre at vertex {i} is not finite"));
                }
                let next = self.vertices[(i + 1) % n].point();
                let r0 = v.point().dist(a.center);
                let r1 = next.dist(a.center);
                if r0 < 1e-6 {
                    return Err(format!("arc at vertex {i} has zero radius"));
                }
                if (r0 - r1).abs() > 1e-4_f64.max(r0 * 1e-6) {
                    return Err(format!(
                        "arc at vertex {i}: start radius {r0} and end radius {r1} differ"
                    ));
                }
            }
        }
        if n == 1 && self.vertices[0].arc.is_none() {
            return Err("a one-vertex contour must be a circle (set arc)".into());
        }
        if n == 2 && self.vertices.iter().all(|v| v.arc.is_none()) {
            return Err("contour needs at least 3 vertices".into());
        }
        Ok(())
    }

    /// Bounding box including arc bulges.
    pub fn bbox(&self) -> Option<Rect> {
        let pts = self.flatten(0.01);
        Rect::from_points(pts)
    }

    /// Polyline approximation with maximum chord error `tol` (mm).
    pub fn flatten(&self, tol: f64) -> Vec<Point> {
        let n = self.vertices.len();
        let mut out = Vec::new();
        for i in 0..n {
            let v = self.vertices[i];
            let p0 = v.point();
            out.push(p0);
            if let Some(a) = v.arc {
                let p1 = self.vertices[(i + 1) % n].point();
                out.extend(arc_points(p0, p1, a.center, a.ccw, tol, n == 1));
            }
        }
        out
    }

    /// The contour moved by (dx, dy).
    pub fn translated(&self, dx: f64, dy: f64) -> Contour {
        Contour {
            vertices: self
                .vertices
                .iter()
                .map(|v| Vertex {
                    x: v.x + dx,
                    y: v.y + dy,
                    arc: v.arc.map(|a| ArcTo {
                        center: Point::new(a.center.x + dx, a.center.y + dy),
                        ccw: a.ccw,
                    }),
                })
                .collect(),
        }
    }

    /// Exact signed area (positive when counter-clockwise).
    pub fn signed_area(&self) -> f64 {
        let n = self.vertices.len();
        let mut a = 0.0;
        for i in 0..n {
            let p = self.vertices[i].point();
            let q = self.vertices[(i + 1) % n].point();
            a += (p.x * q.y - q.x * p.y) / 2.0;
            if let Some(arc) = self.vertices[i].arc {
                let sweep = arc_sweep(p, q, arc.center, arc.ccw, n == 1);
                let r = p.dist(arc.center);
                // Circular segment between chord and arc.
                a += r * r / 2.0 * (sweep - sweep.sin());
            }
        }
        a
    }
}

/// Signed sweep angle in radians of an arc from `p0` to `p1`.
pub fn arc_sweep(p0: Point, p1: Point, c: Point, ccw: bool, full: bool) -> f64 {
    let tau = std::f64::consts::TAU;
    if full {
        return if ccw { tau } else { -tau };
    }
    let a0 = (p0.y - c.y).atan2(p0.x - c.x);
    let a1 = (p1.y - c.y).atan2(p1.x - c.x);
    if ccw {
        (a1 - a0).rem_euclid(tau)
    } else {
        -(a0 - a1).rem_euclid(tau)
    }
}

/// Intermediate points of an arc from `p0` to `p1` (both excluded).
pub fn arc_points(p0: Point, p1: Point, c: Point, ccw: bool, tol: f64, full: bool) -> Vec<Point> {
    let r = p0.dist(c);
    let a0 = (p0.y - c.y).atan2(p0.x - c.x);
    let sweep = arc_sweep(p0, p1, c, ccw, full);
    if r < 1e-9 || sweep.abs() < 1e-12 {
        return Vec::new();
    }
    let tol = tol.max(1e-6).min(r);
    let step = 2.0 * (1.0 - tol / r).clamp(-1.0, 1.0).acos();
    let segs = ((sweep.abs() / step.max(1e-3)).ceil() as usize).clamp(1, 4096);
    (1..segs)
        .map(|k| {
            let a = a0 + sweep * k as f64 / segs as f64;
            Point::new(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect()
}

/// A graphic primitive used for silkscreen, fab, courtyard and board graphics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Shape {
    Line {
        a: Point,
        b: Point,
    },
    /// Arc from `start` around `center`, sweeping `angle` degrees (CCW positive).
    Arc {
        center: Point,
        start: Point,
        angle: f64,
    },
    Circle {
        center: Point,
        radius: f64,
    },
    Rect {
        min: Point,
        max: Point,
    },
    Polygon {
        points: Vec<Point>,
    },
}

impl Shape {
    /// The shape moved by (dx, dy).
    pub fn translated(&self, dx: f64, dy: f64) -> Shape {
        let t = |p: &Point| Point::new(p.x + dx, p.y + dy);
        match self {
            Shape::Line { a, b } => Shape::Line { a: t(a), b: t(b) },
            Shape::Arc {
                center,
                start,
                angle,
            } => Shape::Arc {
                center: t(center),
                start: t(start),
                angle: *angle,
            },
            Shape::Circle { center, radius } => Shape::Circle {
                center: t(center),
                radius: *radius,
            },
            Shape::Rect { min, max } => Shape::Rect {
                min: t(min),
                max: t(max),
            },
            Shape::Polygon { points } => Shape::Polygon {
                points: points.iter().map(t).collect(),
            },
        }
    }

    pub fn points(&self) -> Vec<Point> {
        match self {
            Shape::Line { a, b } => vec![*a, *b],
            Shape::Arc {
                center,
                start,
                angle,
            } => {
                let end = start.sub(*center).rotated(*angle).add(*center);
                let mut v = vec![*start];
                v.extend(arc_points(*start, end, *center, *angle > 0.0, 0.01, false));
                v.push(end);
                v
            }
            Shape::Circle { center, radius } => {
                vec![
                    Point::new(center.x - radius, center.y - radius),
                    Point::new(center.x + radius, center.y + radius),
                ]
            }
            Shape::Rect { min, max } => vec![*min, *max],
            Shape::Polygon { points } => points.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_area_and_bbox() {
        let c = Contour::rect(0.0, 0.0, 70.0, 40.0);
        assert!(c.check().is_ok());
        assert!((c.signed_area() - 2800.0).abs() < 1e-9);
        let b = c.bbox().unwrap();
        assert_eq!(b.width(), 70.0);
        assert_eq!(b.height(), 40.0);
    }

    #[test]
    fn rounded_rect_area() {
        let r = 2.0;
        let c = Contour::rounded_rect(0.0, 0.0, 10.0, 10.0, r);
        c.check().unwrap();
        let expected = 100.0 - (4.0 - std::f64::consts::PI) * r * r;
        assert!(
            (c.signed_area() - expected).abs() < 1e-3,
            "{}",
            c.signed_area()
        );
        let b = c.bbox().unwrap();
        assert!((b.width() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn circle_contour() {
        let c = Contour::circle(Point::new(1.0, 1.0), 3.0);
        c.check().unwrap();
        let a = c.signed_area();
        assert!((a - std::f64::consts::PI * 9.0).abs() < 0.01, "{a}");
    }

    #[test]
    fn bad_arc_rejected() {
        let mut c = Contour::rect(0.0, 0.0, 1.0, 1.0);
        c.vertices[0].arc = Some(ArcTo {
            center: Point::new(5.0, 5.0),
            ccw: true,
        });
        assert!(c.check().is_err());
    }

    #[test]
    fn rotation() {
        let p = Point::new(1.0, 0.0).rotated(90.0);
        assert!((p.x).abs() < 1e-12 && (p.y - 1.0).abs() < 1e-12);
    }
}

// ---------------------------------------------------------------- outlines from edges

/// A loose edge of an outline being assembled from drawing primitives.
#[derive(Clone, Debug)]
pub enum Edge {
    Line(Point, Point),
    Arc {
        a: Point,
        b: Point,
        center: Point,
        ccw: bool,
    },
    /// A complete closed loop that needs no chaining.
    Loop(Contour),
}

/// Edges of a drawing shape, for [`chain_edges`].
pub fn edges_of(shape: &Shape) -> Vec<Edge> {
    match shape {
        Shape::Line { a, b } => vec![Edge::Line(*a, *b)],
        Shape::Arc {
            center,
            start,
            angle,
        } => {
            let end = start.sub(*center).rotated(*angle).add(*center);
            vec![Edge::Arc {
                a: *start,
                b: Point::new(crate::json::round6(end.x), crate::json::round6(end.y)),
                center: *center,
                ccw: *angle > 0.0,
            }]
        }
        Shape::Rect { min, max } => {
            let c = [
                *min,
                Point::new(max.x, min.y),
                *max,
                Point::new(min.x, max.y),
            ];
            (0..4).map(|i| Edge::Line(c[i], c[(i + 1) % 4])).collect()
        }
        Shape::Circle { center, radius } => vec![Edge::Loop(Contour::circle(*center, *radius))],
        Shape::Polygon { points } => contour_from_points(points.clone())
            .map(|c| vec![Edge::Loop(c)])
            .unwrap_or_default(),
    }
}

/// A counter-clockwise contour from a point list (no arcs); `None` when degenerate.
pub fn contour_from_points(mut pts: Vec<Point>) -> Option<Contour> {
    pts.dedup_by(|a, b| a.dist(*b) < 1e-6);
    if pts.len() > 2 && pts[0].dist(*pts.last().unwrap()) < 1e-6 {
        pts.pop();
    }
    if pts.len() < 3 {
        return None;
    }
    let mut c = Contour {
        vertices: pts.iter().map(|p| Vertex::line(p.x, p.y)).collect(),
    };
    if c.signed_area() < 0.0 {
        c.vertices.reverse();
    }
    Some(c)
}

/// Start, end and optional arc (centre, counter-clockwise) of a loose edge.
type Seg = (Point, Point, Option<(Point, bool)>);

/// Join loose edges into closed loops. Edges whose ends coincide (within
/// 5 µm) are joined first into paths; then the ends of open paths are
/// bridged, closest first, when they are less than `tol` (mm) apart. A
/// bridge becomes a short straight edge. Returns the loops and how many
/// edges could not be used.
pub fn chain_edges(edges: Vec<Edge>, tol: f64) -> (Vec<Contour>, usize) {
    let tol = tol.max(1e-6);
    let eps = tol.min(0.005);
    let mut loops = Vec::new();
    let mut pool: Vec<Seg> = Vec::new();
    let mut open = 0;
    for e in edges {
        match e {
            Edge::Loop(c) => loops.push(c),
            Edge::Line(a, b) => {
                if a.dist(b) > eps {
                    pool.push((a, b, None));
                }
            }
            Edge::Arc { a, b, center, ccw } => {
                if a.dist(b) > eps {
                    pool.push((a, b, Some((center, ccw))));
                }
            }
        }
    }
    let flip = |e: Seg| (e.1, e.0, e.2.map(|(c, ccw)| (c, !ccw)));
    // The pooled segment with an end nearest `p` (within `eps`), oriented
    // so that it starts there (`at_start`) or ends there.
    let take = |pool: &mut Vec<Seg>, p: Point, at_start: bool| -> Option<Seg> {
        let mut best: Option<(f64, usize, bool)> = None;
        for (i, e) in pool.iter().enumerate() {
            for (q, rev) in [(e.0, false), (e.1, true)] {
                let d = q.dist(p);
                if d < eps && best.is_none_or(|b| d < b.0) {
                    best = Some((d, i, rev));
                }
            }
        }
        let (_, i, rev) = best?;
        let e = pool.swap_remove(i);
        // `rev` means the matching end is the segment's end.
        Some(match (at_start, rev) {
            (true, false) | (false, true) => e,
            _ => flip(e),
        })
    };

    // Stage 1: paths through coinciding ends.
    let mut paths: Vec<std::collections::VecDeque<Seg>> = Vec::new();
    while let Some(first) = pool.pop() {
        let mut path = std::collections::VecDeque::from([first]);
        loop {
            let end = path.back().unwrap().1;
            if path.len() > 1 && end.dist(path.front().unwrap().0) < eps {
                break;
            }
            match take(&mut pool, end, true) {
                Some(e) => path.push_back(e),
                None => break,
            }
        }
        let closed = path.len() > 1 && path.back().unwrap().1.dist(path.front().unwrap().0) < eps;
        if !closed {
            while let Some(e) = take(&mut pool, path.front().unwrap().0, false) {
                path.push_front(e);
                if path.back().unwrap().1.dist(path.front().unwrap().0) < eps {
                    break;
                }
            }
        }
        paths.push(path);
    }

    // Stage 2: bridge the gaps between open paths, closest first.
    let is_closed = |p: &std::collections::VecDeque<Seg>| {
        p.len() > 1 && p.back().unwrap().1.dist(p.front().unwrap().0) < eps
    };
    let (mut done, mut todo): (Vec<_>, Vec<_>) = paths.into_iter().partition(is_closed);
    loop {
        // (distance, path, other path or itself, join to the other's end)
        let mut best: Option<(f64, usize, usize, bool)> = None;
        for (i, p) in todo.iter().enumerate() {
            let end = p.back().unwrap().1;
            for (j, q) in todo.iter().enumerate() {
                let start = q.front().unwrap().0;
                let d = end.dist(start);
                // Closing a single segment on itself is not a loop.
                if d < tol && !(i == j && p.len() < 2) && best.is_none_or(|b| d < b.0) {
                    best = Some((d, i, j, false));
                }
                if i != j {
                    let d = end.dist(q.back().unwrap().1);
                    if d < tol && best.is_none_or(|b| d < b.0) {
                        best = Some((d, i, j, true));
                    }
                }
            }
        }
        let Some((_, i, j, to_end)) = best else { break };
        if i == j {
            done.push(todo.swap_remove(i));
            continue;
        }
        let mut other = todo[j].clone();
        if to_end {
            other = other.into_iter().rev().map(flip).collect();
        }
        todo[i].extend(other);
        todo.swap_remove(j);
        let i = if i == todo.len() { j } else { i };
        if is_closed(&todo[i]) {
            done.push(todo.swap_remove(i));
        }
    }
    open += todo.iter().map(|p| p.len()).sum::<usize>();

    // Each edge starts a vertex; where an edge ends short of the next one,
    // its true end becomes a vertex too, so arcs keep their radius and a
    // short line bridges the gap.
    let build = |chain: &[Seg]| {
        let mut vertices = Vec::new();
        for (k, (a, b, arc)) in chain.iter().enumerate() {
            vertices.push(Vertex {
                x: a.x,
                y: a.y,
                arc: arc.map(|(center, ccw)| ArcTo { center, ccw }),
            });
            let next = chain[(k + 1) % chain.len()].0;
            if b.dist(next) > 1e-4 {
                vertices.push(Vertex::line(b.x, b.y));
            }
        }
        Contour { vertices }
    };
    for path in done {
        let chain: Vec<Seg> = path.into_iter().collect();
        let mut c = build(&chain);
        if c.signed_area() < 0.0 {
            let rev: Vec<Seg> = chain.iter().rev().map(|e| flip(*e)).collect();
            c = build(&rev);
        }
        loops.push(c);
    }
    (loops, open)
}
