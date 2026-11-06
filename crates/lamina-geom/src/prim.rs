//! Copper primitives and exact distances between them.

use lamina_model::{Point, Rect};

/// A copper shape. Tracks and oval pads are capsules (a segment with round
/// ends). Rectangular pads, custom pads and pour fills are polygons.
#[derive(Debug, Clone, PartialEq)]
pub enum Prim {
    Circle {
        c: Point,
        r: f64,
    },
    Capsule {
        a: Point,
        b: Point,
        r: f64,
    },
    /// The outer ring is counter-clockwise. Holes are clockwise. Rings are not
    /// closed (the last point is not the same as the first).
    Poly {
        outer: Vec<Point>,
        holes: Vec<Vec<Point>>,
    },
}

impl Prim {
    pub fn bbox(&self) -> Rect {
        match self {
            Prim::Circle { c, r } => Rect {
                min: Point::new(c.x - r, c.y - r),
                max: Point::new(c.x + r, c.y + r),
            },
            Prim::Capsule { a, b, r } => Rect {
                min: Point::new(a.x.min(b.x) - r, a.y.min(b.y) - r),
                max: Point::new(a.x.max(b.x) + r, a.y.max(b.y) + r),
            },
            Prim::Poly { outer, .. } => Rect::from_points(outer.iter().copied()).unwrap_or(Rect {
                min: Point::new(0.0, 0.0),
                max: Point::new(0.0, 0.0),
            }),
        }
    }

    /// A point inside the shape.
    pub fn anchor(&self) -> Point {
        match self {
            Prim::Circle { c, .. } => *c,
            Prim::Capsule { a, b, .. } => Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0),
            Prim::Poly { outer, .. } => {
                let b = self.bbox();
                let c = Point::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0);
                if self.contains(c) {
                    c
                } else {
                    outer.first().copied().unwrap_or(c)
                }
            }
        }
    }

    pub fn contains(&self, p: Point) -> bool {
        match self {
            Prim::Circle { c, r } => c.dist(p) <= *r,
            Prim::Capsule { a, b, r } => seg_point(*a, *b, p) <= *r,
            Prim::Poly { outer, holes } => {
                ring_contains(outer, p) && !holes.iter().any(|h| ring_contains(h, p))
            }
        }
    }

    /// The edge-to-edge distance. It is 0 if the shapes touch or overlap.
    pub fn distance(&self, o: &Prim) -> f64 {
        use Prim::*;
        let d = match (self, o) {
            (Circle { c: c1, r: r1 }, Circle { c: c2, r: r2 }) => c1.dist(*c2) - r1 - r2,
            (Circle { c, r: r1 }, Capsule { a, b, r: r2 })
            | (Capsule { a, b, r: r2 }, Circle { c, r: r1 }) => seg_point(*a, *b, *c) - r1 - r2,
            (
                Capsule {
                    a: a1,
                    b: b1,
                    r: r1,
                },
                Capsule {
                    a: a2,
                    b: b2,
                    r: r2,
                },
            ) => seg_seg(*a1, *b1, *a2, *b2) - r1 - r2,
            (Poly { .. }, Circle { c, r }) | (Circle { c, r }, Poly { .. }) => {
                let poly = if matches!(self, Poly { .. }) { self } else { o };
                if poly.contains(*c) {
                    0.0
                } else {
                    poly_core_dist(poly, |a, b| seg_point(a, b, *c)) - r
                }
            }
            (Poly { .. }, Capsule { a, b, r }) | (Capsule { a, b, r }, Poly { .. }) => {
                let poly = if matches!(self, Poly { .. }) { self } else { o };
                if poly.contains(*a) || poly.contains(*b) {
                    0.0
                } else {
                    poly_core_dist(poly, |p, q| seg_seg(p, q, *a, *b)) - r
                }
            }
            (Poly { outer: oa, .. }, Poly { outer: ob, .. }) => {
                if oa.first().is_some_and(|p| o.contains(*p))
                    || ob.first().is_some_and(|p| self.contains(*p))
                {
                    0.0
                } else {
                    let mut best = f64::INFINITY;
                    for_edges(self, |p, q| {
                        for_edges(o, |s, t| {
                            best = best.min(seg_seg(p, q, s, t));
                        });
                    });
                    best
                }
            }
        };
        d.max(0.0)
    }

    pub fn translate(&mut self, v: Point) {
        match self {
            Prim::Circle { c, .. } => *c = c.add(v),
            Prim::Capsule { a, b, .. } => {
                *a = a.add(v);
                *b = b.add(v);
            }
            Prim::Poly { outer, holes } => {
                for p in outer.iter_mut().chain(holes.iter_mut().flatten()) {
                    *p = p.add(v);
                }
            }
        }
    }

    /// The polygon outline (circles and capsules are flattened), for drawings and fills.
    pub fn outline(&self, tol: f64) -> Vec<Point> {
        match self {
            Prim::Circle { c, r } => circle_points(*c, *r, tol),
            Prim::Capsule { a, b, r } => capsule_points(*a, *b, *r, tol),
            Prim::Poly { outer, .. } => outer.clone(),
        }
    }

    pub fn area(&self) -> f64 {
        match self {
            Prim::Circle { r, .. } => std::f64::consts::PI * r * r,
            Prim::Capsule { a, b, r } => std::f64::consts::PI * r * r + 2.0 * r * a.dist(*b),
            Prim::Poly { outer, holes } => {
                ring_area(outer).abs() - holes.iter().map(|h| ring_area(h).abs()).sum::<f64>()
            }
        }
    }
}

fn for_edges(p: &Prim, mut f: impl FnMut(Point, Point)) {
    if let Prim::Poly { outer, holes } = p {
        for ring in std::iter::once(outer).chain(holes.iter()) {
            let n = ring.len();
            for i in 0..n {
                f(ring[i], ring[(i + 1) % n]);
            }
        }
    }
}

fn poly_core_dist(poly: &Prim, mut d: impl FnMut(Point, Point) -> f64) -> f64 {
    let mut best = f64::INFINITY;
    for_edges(poly, |a, b| best = best.min(d(a, b)));
    best
}

pub fn ring_area(r: &[Point]) -> f64 {
    let n = r.len();
    let mut a = 0.0;
    for i in 0..n {
        let p = r[i];
        let q = r[(i + 1) % n];
        a += p.x * q.y - q.x * p.y;
    }
    a / 2.0
}

pub fn ring_contains(r: &[Point], p: Point) -> bool {
    let n = r.len();
    let mut inside = false;
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (r[i], r[j]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x;
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// The distance from point `p` to segment `a`-`b`.
pub fn seg_point(a: Point, b: Point, p: Point) -> f64 {
    let ab = b.sub(a);
    let len2 = ab.x * ab.x + ab.y * ab.y;
    if len2 < 1e-24 {
        return a.dist(p);
    }
    let t = (((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / len2).clamp(0.0, 1.0);
    Point::new(a.x + ab.x * t, a.y + ab.y * t).dist(p)
}

fn orient(a: Point, b: Point, c: Point) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

pub fn segments_intersect(a: Point, b: Point, c: Point, d: Point) -> bool {
    let d1 = orient(c, d, a);
    let d2 = orient(c, d, b);
    let d3 = orient(a, b, c);
    let d4 = orient(a, b, d);
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

/// The distance between segments. It is 0 if they cross.
pub fn seg_seg(a: Point, b: Point, c: Point, d: Point) -> f64 {
    if segments_intersect(a, b, c, d) {
        return 0.0;
    }
    seg_point(a, b, c)
        .min(seg_point(a, b, d))
        .min(seg_point(c, d, a))
        .min(seg_point(c, d, b))
}

pub fn circle_points(c: Point, r: f64, tol: f64) -> Vec<Point> {
    let n = segments_for(r, tol, std::f64::consts::TAU);
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            Point::new(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect()
}

fn segments_for(r: f64, tol: f64, sweep: f64) -> usize {
    if r <= tol {
        return 8;
    }
    let step = 2.0 * (1.0 - tol / r).acos();
    ((sweep / step).ceil() as usize).clamp(8, 256)
}

/// The capsule outline, counter-clockwise.
pub fn capsule_points(a: Point, b: Point, r: f64, tol: f64) -> Vec<Point> {
    if a.dist(b) < 1e-12 {
        return circle_points(a, r, tol);
    }
    let ang = (b.y - a.y).atan2(b.x - a.x);
    let n = segments_for(r, tol, std::f64::consts::PI).max(4);
    let mut out = Vec::with_capacity(2 * n + 2);
    // A half circle around b from -90 to +90 degrees, then around a.
    for i in 0..=n {
        let t = ang - std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * i as f64 / n as f64;
        out.push(Point::new(b.x + r * t.cos(), b.y + r * t.sin()));
    }
    for i in 0..=n {
        let t = ang + std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * i as f64 / n as f64;
        out.push(Point::new(a.x + r * t.cos(), a.y + r * t.sin()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(x: f64, y: f64, s: f64) -> Prim {
        Prim::Poly {
            outer: vec![
                Point::new(x, y),
                Point::new(x + s, y),
                Point::new(x + s, y + s),
                Point::new(x, y + s),
            ],
            holes: vec![],
        }
    }

    #[test]
    fn distances() {
        let a = Prim::Circle {
            c: Point::new(0.0, 0.0),
            r: 1.0,
        };
        let b = Prim::Capsule {
            a: Point::new(3.0, -5.0),
            b: Point::new(3.0, 5.0),
            r: 0.5,
        };
        assert!((a.distance(&b) - 1.5).abs() < 1e-12);
        let s = sq(2.0, -1.0, 2.0);
        assert!((a.distance(&s) - 1.0).abs() < 1e-12);
        assert_eq!(b.distance(&s), 0.0);
        let s2 = sq(10.0, 0.0, 1.0);
        assert!((s.distance(&s2) - 6.0).abs() < 1e-12);
        // A capsule that crosses a square, with no endpoints in the square.
        let c = Prim::Capsule {
            a: Point::new(1.0, 0.0),
            b: Point::new(5.0, 0.0),
            r: 0.1,
        };
        assert_eq!(c.distance(&s), 0.0);
    }

    #[test]
    fn holes() {
        let ring = Prim::Poly {
            outer: vec![
                Point::new(0.0, 0.0),
                Point::new(10.0, 0.0),
                Point::new(10.0, 10.0),
                Point::new(0.0, 10.0),
            ],
            holes: vec![vec![
                Point::new(3.0, 3.0),
                Point::new(3.0, 7.0),
                Point::new(7.0, 7.0),
                Point::new(7.0, 3.0),
            ]],
        };
        let dot = Prim::Circle {
            c: Point::new(5.0, 5.0),
            r: 0.5,
        };
        assert!((ring.distance(&dot) - 1.5).abs() < 1e-12);
        assert!((ring.area() - 84.0).abs() < 1e-12);
    }

    #[test]
    fn outlines() {
        let c = capsule_points(Point::new(0.0, 0.0), Point::new(2.0, 0.0), 0.5, 0.01);
        let a = ring_area(&c);
        assert!(a > 0.0);
        let exact = std::f64::consts::PI * 0.25 + 2.0;
        assert!((a - exact).abs() < 0.05 && a < exact, "{a}");
    }
}

/// A polygon with an index of its edges, for fast inside and touch tests
/// with small shapes (for example, a pour piece with thousands of edges,
/// tested against pads and tracks).
pub struct IndexedPoly {
    edges: Vec<(Point, Point)>,
    index: crate::Index,
    bbox: Rect,
}

impl IndexedPoly {
    pub fn new(outer: &[Point], holes: &[Vec<Point>]) -> IndexedPoly {
        let mut edges = Vec::new();
        for ring in std::iter::once(outer).chain(holes.iter().map(|h| h.as_slice())) {
            let n = ring.len();
            for k in 0..n {
                edges.push((ring[k], ring[(k + 1) % n]));
            }
        }
        let index = crate::Index::new(
            edges
                .iter()
                .map(|(a, b)| Rect::from_points([*a, *b]).unwrap()),
        );
        let bbox = Rect::from_points(outer.iter().copied()).unwrap_or(Rect {
            min: Point::new(0.0, 0.0),
            max: Point::new(0.0, 0.0),
        });
        IndexedPoly { edges, index, bbox }
    }

    /// An even-odd inside test (holes are not included). It counts only the edges
    /// that a ray to +x can cross.
    pub fn contains(&self, p: Point) -> bool {
        if p.x < self.bbox.min.x
            || p.x > self.bbox.max.x
            || p.y < self.bbox.min.y
            || p.y > self.bbox.max.y
        {
            return false;
        }
        let ray = Rect {
            min: p,
            max: Point::new(self.bbox.max.x, p.y),
        };
        let mut inside = false;
        for k in self.index.near(&ray, 0.0) {
            let (a, b) = self.edges[k];
            if (a.y <= p.y) != (b.y <= p.y) {
                let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if x > p.x {
                    inside = !inside;
                }
            }
        }
        inside
    }

    /// True if two indexed polygons overlap or come within `eps`.
    pub fn touches_poly(&self, o: &IndexedPoly, eps: f64) -> bool {
        let (a, b) = (&self.bbox, &o.bbox);
        if a.max.x + eps < b.min.x
            || b.max.x + eps < a.min.x
            || a.max.y + eps < b.min.y
            || b.max.y + eps < a.min.y
        {
            return false;
        }
        if o.edges.first().is_some_and(|e| self.contains(e.0))
            || self.edges.first().is_some_and(|e| o.contains(e.0))
        {
            return true;
        }
        o.edges.iter().any(|&(p, q)| {
            let r = Rect::from_points([p, q]).unwrap();
            self.index
                .near(&r, eps)
                .any(|k| seg_seg(self.edges[k].0, self.edges[k].1, p, q) <= eps)
        })
    }

    /// True if `prim` overlaps the polygon or comes within `eps` of it.
    pub fn touches(&self, prim: &Prim, eps: f64) -> bool {
        let bb = prim.bbox();
        let near = |r: f64| self.index.near(&bb, r + eps);
        match prim {
            Prim::Circle { c, r } => {
                self.contains(*c)
                    || near(0.0).any(|k| seg_point(self.edges[k].0, self.edges[k].1, *c) <= r + eps)
            }
            Prim::Capsule { a, b, r } => {
                self.contains(*a)
                    || self.contains(*b)
                    || near(0.0)
                        .any(|k| seg_seg(self.edges[k].0, self.edges[k].1, *a, *b) <= r + eps)
            }
            Prim::Poly { outer, .. } => {
                if outer.iter().any(|p| self.contains(*p)) {
                    return true;
                }
                let n = outer.len();
                near(0.0).any(|k| {
                    let (a, b) = self.edges[k];
                    // The pour edge goes through the inside of the pad, or the edges meet.
                    prim.contains(a)
                        || (0..n).any(|i| seg_seg(a, b, outer[i], outer[(i + 1) % n]) <= eps)
                })
            }
        }
    }
}

#[cfg(test)]
mod indexed_tests {
    use super::*;

    #[test]
    fn indexed_poly_matches_plain_tests() {
        let outer = vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        let hole = vec![
            Point::new(4.0, 4.0),
            Point::new(4.0, 6.0),
            Point::new(6.0, 6.0),
            Point::new(6.0, 4.0),
        ];
        let ip = IndexedPoly::new(&outer, std::slice::from_ref(&hole));
        let plain = Prim::Poly {
            outer: outer.clone(),
            holes: vec![hole.clone()],
        };
        for (x, y) in [
            (1.0, 1.0),
            (5.0, 5.0),
            (11.0, 5.0),
            (4.5, 7.0),
            (-1.0, -1.0),
        ] {
            let p = Point::new(x, y);
            assert_eq!(ip.contains(p), plain.contains(p), "{p:?}");
        }
        // A track that ends in the hole but crosses the pour touches it.
        let t = Prim::Capsule {
            a: Point::new(5.0, 5.0),
            b: Point::new(5.0, 3.0),
            r: 0.1,
        };
        assert!(ip.touches(&t, 1e-6));
        // A via fully in the hole, clear of its edges, does not touch it.
        let v = Prim::Circle {
            c: Point::new(5.0, 5.0),
            r: 0.3,
        };
        assert!(!ip.touches(&v, 1e-6));
        // A pad that overlaps the outer edge touches it.
        let pad = Prim::Poly {
            outer: vec![
                Point::new(9.5, 2.0),
                Point::new(11.0, 2.0),
                Point::new(11.0, 3.0),
                Point::new(9.5, 3.0),
            ],
            holes: vec![],
        };
        assert!(ip.touches(&pad, 1e-6));
        let away = Prim::Circle {
            c: Point::new(12.0, 12.0),
            r: 0.5,
        };
        assert!(!ip.touches(&away, 1e-6));
    }
}
