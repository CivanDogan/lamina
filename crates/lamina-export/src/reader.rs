//! Reader for the Gerber and Excellon subset Lamina writes, used to check
//! exports by reading them back.

use geo::{BooleanOps, MultiPolygon};
use lamina_geom::fill::inflate;
use lamina_geom::prim::Prim;
use lamina_model::Point;

fn mp_of(p: &Prim) -> MultiPolygon<f64> {
    inflate(p, 0.0)
}

fn ring_mp(pts: &[Point]) -> MultiPolygon<f64> {
    inflate(&lamina_geom::board::poly(pts.to_vec()), 0.0)
}

/// Read a Gerber file into dark geometry.
pub fn read_gerber(text: &str) -> Result<MultiPolygon<f64>, String> {
    let mut apertures: std::collections::HashMap<u32, f64> = Default::default();
    let mut ap: Option<f64> = None;
    let mut pos = Point::new(0.0, 0.0);
    let mut dark = true;
    let mut in_region = false;
    let mut region: Vec<Point> = Vec::new();
    let mut acc = MultiPolygon::new(vec![]);
    let mut pending: Vec<MultiPolygon<f64>> = Vec::new();
    let flush = |acc: &mut MultiPolygon<f64>, pending: &mut Vec<MultiPolygon<f64>>| {
        for p in pending.drain(..) {
            *acc = acc.union(&p);
        }
    };
    let num = |s: &str| -> Option<f64> { s.parse::<i64>().ok().map(|v| v as f64 / 1e6) };
    for raw in text.split('\n') {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("%ADD") {
            let rest = rest.trim_end_matches("*%");
            let (n, spec) = rest.split_at(rest.find('C').ok_or("unsupported aperture")?);
            let d: f64 = spec[2..]
                .parse()
                .map_err(|_| format!("bad aperture {line}"))?;
            apertures.insert(n.parse().map_err(|_| "bad aperture number")?, d);
            continue;
        }
        if line == "%LPD*%" || line == "%LPC*%" {
            flush(&mut acc, &mut pending);
            dark = line == "%LPD*%";
            continue;
        }
        if line.starts_with('%') || line.starts_with("G01") || line.starts_with("M02") {
            continue;
        }
        if line == "G36*" {
            in_region = true;
            region.clear();
            continue;
        }
        if line == "G37*" {
            in_region = false;
            if region.len() >= 3 {
                let m = ring_mp(&region);
                if dark {
                    pending.push(m);
                } else {
                    acc = acc.difference(&m);
                }
            }
            continue;
        }
        if let Some(n) = line.strip_prefix('D').and_then(|s| s.strip_suffix('*')) {
            let n: u32 = n.parse().map_err(|_| format!("bad line {line}"))?;
            ap = Some(
                *apertures
                    .get(&n)
                    .ok_or(format!("aperture D{n} undefined"))?,
            );
            continue;
        }
        if let Some(rest) = line.strip_prefix('X') {
            let yi = rest.find('Y').ok_or(format!("bad line {line}"))?;
            let di = rest.find('D').ok_or(format!("bad line {line}"))?;
            let x = num(&rest[..yi]).ok_or(format!("bad x in {line}"))?;
            let y = num(&rest[yi + 1..di]).ok_or(format!("bad y in {line}"))?;
            let op = &rest[di..rest.len() - 1];
            let p = Point::new(x, y);
            if in_region {
                if op == "D02" {
                    region.clear();
                }
                region.push(p);
            } else {
                let d = ap.ok_or("draw without aperture")?;
                let shape = match op {
                    "D03" => Some(Prim::Circle { c: p, r: d / 2.0 }),
                    "D01" => Some(Prim::Capsule {
                        a: pos,
                        b: p,
                        r: d / 2.0,
                    }),
                    _ => None,
                };
                if let Some(s) = shape {
                    let m = mp_of(&s);
                    if dark {
                        pending.push(m);
                    } else {
                        acc = acc.difference(&m);
                    }
                }
            }
            pos = p;
            if pending.len() > 64 {
                let mut tree = std::mem::take(&mut pending);
                while tree.len() > 1 {
                    let b = tree.pop().unwrap();
                    let a = tree.pop().unwrap();
                    tree.push(a.union(&b));
                }
                pending = tree;
            }
        }
    }
    flush(&mut acc, &mut pending);
    Ok(acc)
}

/// Read an Excellon file: (diameter, start, end) per hole; start == end for round holes.
pub fn read_excellon(text: &str) -> Result<Vec<(f64, Point, Point)>, String> {
    let mut tools: std::collections::HashMap<String, f64> = Default::default();
    let mut tool = None;
    let mut out = Vec::new();
    let parse_xy = |s: &str| -> Option<Point> {
        let s = s.strip_prefix('X')?;
        let yi = s.find('Y')?;
        Some(Point::new(s[..yi].parse().ok()?, s[yi + 1..].parse().ok()?))
    };
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('T') && line.contains('C') {
            let (t, d) = line.split_once('C').unwrap();
            tools.insert(
                t.to_string(),
                d.parse().map_err(|_| format!("bad tool {line}"))?,
            );
            continue;
        }
        if line.starts_with('T') && line.len() > 1 && line[1..].chars().all(|c| c.is_ascii_digit())
        {
            tool = tools.get(line).copied();
            continue;
        }
        if line.starts_with('X') {
            let d = tool.ok_or("hole without tool")?;
            if let Some((a, b)) = line.split_once("G85") {
                out.push((
                    d,
                    parse_xy(a).ok_or("bad slot")?,
                    parse_xy(b).ok_or("bad slot")?,
                ));
            } else {
                let p = parse_xy(line).ok_or(format!("bad hole {line}"))?;
                out.push((d, p, p));
            }
        }
    }
    Ok(out)
}
