//! Triangulation of 2D scene shapes and of the board slab.

use lamina_model::{Point, Rect};
use lamina_scene::Geom;

/// Triangles in board millimetres.
#[derive(Debug, Default, Clone)]
pub struct Tris {
    pub pos: Vec<[f32; 2]>,
    pub idx: Vec<u32>,
}

impl Tris {
    fn fan(&mut self, c: Point, r: f64, segs: usize) {
        let base = self.pos.len() as u32;
        self.pos.push([c.x as f32, c.y as f32]);
        for i in 0..segs {
            let a = std::f64::consts::TAU * i as f64 / segs as f64;
            self.pos
                .push([(c.x + r * a.cos()) as f32, (c.y + r * a.sin()) as f32]);
        }
        for i in 0..segs as u32 {
            self.idx
                .extend_from_slice(&[base, base + 1 + i, base + 1 + (i + 1) % segs as u32]);
        }
    }

    fn quad(&mut self, a: Point, b: Point, hw: f64) {
        let d = b.sub(a);
        let len = (d.x * d.x + d.y * d.y).sqrt();
        if len < 1e-12 {
            return;
        }
        let n = Point::new(-d.y / len * hw, d.x / len * hw);
        let base = self.pos.len() as u32;
        for p in [a.add(n), b.add(n), b.sub(n), a.sub(n)] {
            self.pos.push([p.x as f32, p.y as f32]);
        }
        self.idx
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    pub fn polygon(&mut self, outer: &[Point], holes: &[Vec<Point>]) {
        if outer.len() < 3 {
            return;
        }
        let mut coords: Vec<f64> =
            Vec::with_capacity((outer.len() + holes.iter().map(|h| h.len()).sum::<usize>()) * 2);
        let mut hole_idx = Vec::new();
        for p in outer {
            coords.push(p.x);
            coords.push(p.y);
        }
        for h in holes {
            if h.len() < 3 {
                continue;
            }
            hole_idx.push(coords.len() / 2);
            for p in h {
                coords.push(p.x);
                coords.push(p.y);
            }
        }
        let Ok(ids) = earcutr::earcut(&coords, &hole_idx, 2) else {
            return;
        };
        let base = self.pos.len() as u32;
        for c in coords.chunks(2) {
            self.pos.push([c[0] as f32, c[1] as f32]);
        }
        self.idx.extend(ids.into_iter().map(|i| base + i as u32));
    }

    pub fn add(&mut self, g: &Geom, px_per_mm: f64) {
        let segs = |r: f64| ((r * px_per_mm * 0.8).ceil() as usize).clamp(8, 48);
        match g {
            Geom::Poly { outer, holes } => self.polygon(outer, holes),
            Geom::Disc { c, r } => self.fan(*c, *r, segs(*r)),
            Geom::Stroke { pts, width, closed } => {
                let hw = width / 2.0;
                let n = pts.len();
                for i in 0..n {
                    self.fan(pts[i], hw, segs(hw));
                    if i + 1 < n {
                        self.quad(pts[i], pts[i + 1], hw);
                    }
                }
                if *closed && n > 2 {
                    self.quad(pts[n - 1], pts[0], hw);
                }
            }
        }
    }
}

/// Vertex of the board slab: position, normal, texture coordinate, face
/// (0 top, 1 bottom, 2 edge).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BoardVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub face: f32,
}

fn ring_area(r: &[Point]) -> f64 {
    let n = r.len();
    (0..n)
        .map(|i| {
            let (p, q) = (r[i], r[(i + 1) % n]);
            p.x * q.y - q.x * p.y
        })
        .sum::<f64>()
        / 2.0
}

/// Extrude the board outline (with holes) from z = 0 to z = `t`.
pub fn board_slab(
    outline: &[Point],
    holes: &[Vec<Point>],
    t: f64,
    bbox: Rect,
) -> (Vec<BoardVertex>, Vec<u32>) {
    let mut tris = Tris::default();
    tris.polygon(outline, holes);
    let uv = |x: f32, y: f32| {
        [
            ((x as f64 - bbox.min.x) / bbox.width()) as f32,
            ((bbox.max.y - y as f64) / bbox.height()) as f32,
        ]
    };
    let mut v = Vec::new();
    let mut idx = Vec::new();
    // Top and bottom faces.
    for (z, nz, face) in [(t as f32, 1.0f32, 0.0f32), (0.0, -1.0, 1.0)] {
        let base = v.len() as u32;
        for p in &tris.pos {
            v.push(BoardVertex {
                pos: [p[0], p[1], z],
                normal: [0.0, 0.0, nz],
                uv: uv(p[0], p[1]),
                face,
            });
        }
        for t3 in tris.idx.chunks(3) {
            if nz > 0.0 {
                idx.extend_from_slice(&[base + t3[0], base + t3[1], base + t3[2]]);
            } else {
                idx.extend_from_slice(&[base + t3[0], base + t3[2], base + t3[1]]);
            }
        }
    }
    // Edges: outline walls face out, hole walls face into the hole.
    let mut wall = |ring: &[Point], outward: bool| {
        let ccw = ring_area(ring) > 0.0;
        let n = ring.len();
        for i in 0..n {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            let d = b.sub(a);
            let len = (d.x * d.x + d.y * d.y).sqrt();
            if len < 1e-9 {
                continue;
            }
            // Right-hand normal of a CCW ring points outward.
            let mut nrm = [(d.y / len) as f32, (-d.x / len) as f32, 0.0];
            if ccw != outward {
                nrm = [-nrm[0], -nrm[1], 0.0];
            }
            let base = v.len() as u32;
            for (p, z) in [(a, 0.0f32), (b, 0.0), (b, t as f32), (a, t as f32)] {
                v.push(BoardVertex {
                    pos: [p.x as f32, p.y as f32, z],
                    normal: nrm,
                    uv: [0.0, 0.0],
                    face: 2.0,
                });
            }
            idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    };
    wall(outline, true);
    for h in holes {
        wall(h, false);
    }
    (v, idx)
}
