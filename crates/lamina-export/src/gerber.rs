//! Gerber X2 (RS-274X with attributes) writer.

use std::collections::BTreeMap;
use std::fmt::Write;

use lamina_model::Point;
use lamina_scene::Geom;

/// The Gerber file function attribute, for example `Copper,L1,Top`.
pub struct Function {
    pub function: String,
    /// `Positive` or `Negative`.
    pub polarity: &'static str,
}

fn coord(v: f64) -> i64 {
    (v * 1e6).round() as i64
}

pub struct Writer {
    body: String,
    apertures: BTreeMap<i64, u32>,
    next: u32,
    current: Option<u32>,
    dark: bool,
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer {
    pub fn new() -> Writer {
        Writer {
            body: String::new(),
            apertures: BTreeMap::new(),
            next: 10,
            current: None,
            dark: true,
        }
    }

    fn circle(&mut self, d: f64) -> u32 {
        let key = coord(d.max(0.001));
        let next = &mut self.next;
        *self.apertures.entry(key).or_insert_with(|| {
            let n = *next;
            *next += 1;
            n
        })
    }

    fn select(&mut self, ap: u32) {
        if self.current != Some(ap) {
            let _ = writeln!(self.body, "D{ap}*");
            self.current = Some(ap);
        }
    }

    fn polarity(&mut self, dark: bool) {
        if self.dark != dark {
            self.body
                .push_str(if dark { "%LPD*%\n" } else { "%LPC*%\n" });
            self.dark = dark;
        }
    }

    fn mv(&mut self, p: Point, op: &str) {
        let _ = writeln!(self.body, "X{}Y{}{op}*", coord(p.x), coord(p.y));
    }

    fn region(&mut self, ring: &[Point]) {
        if ring.len() < 3 {
            return;
        }
        self.body.push_str("G36*\n");
        self.mv(ring[0], "D02");
        for p in &ring[1..] {
            self.mv(*p, "D01");
        }
        self.mv(ring[0], "D01");
        self.body.push_str("G37*\n");
    }

    /// Add a shape in dark polarity. Then clear the polygon holes.
    pub fn add(&mut self, g: &Geom) {
        match g {
            Geom::Disc { c, r } => {
                let ap = self.circle(2.0 * r);
                self.polarity(true);
                self.select(ap);
                self.mv(*c, "D03");
            }
            Geom::Stroke { pts, width, closed } => {
                if pts.is_empty() {
                    return;
                }
                let ap = self.circle(*width);
                self.polarity(true);
                self.select(ap);
                if pts.len() == 1 {
                    self.mv(pts[0], "D03");
                    return;
                }
                self.mv(pts[0], "D02");
                for p in &pts[1..] {
                    self.mv(*p, "D01");
                }
                if *closed {
                    self.mv(pts[0], "D01");
                }
            }
            Geom::Poly { outer, holes } => {
                self.polarity(true);
                self.body.push_str("G01*\n");
                self.region(outer);
                if !holes.is_empty() {
                    self.polarity(false);
                    for h in holes {
                        self.region(h);
                    }
                    self.polarity(true);
                }
            }
        }
    }

    pub fn finish(self, f: &Function) -> String {
        let mut out = String::new();
        out.push_str("%TF.GenerationSoftware,Lamina,lamina,");
        out.push_str(env!("CARGO_PKG_VERSION"));
        out.push_str("*%\n");
        out.push_str("%TF.SameCoordinates,Original*%\n");
        let _ = writeln!(out, "%TF.FileFunction,{}*%", f.function);
        let _ = writeln!(out, "%TF.FilePolarity,{}*%", f.polarity);
        out.push_str("%FSLAX46Y46*%\n%MOMM*%\n%LPD*%\nG01*\n");
        for (key, n) in &self.apertures {
            let _ = writeln!(out, "%ADD{n}C,{:.6}*%", *key as f64 / 1e6);
        }
        out.push_str(&self.body);
        out.push_str("M02*\n");
        out
    }
}
