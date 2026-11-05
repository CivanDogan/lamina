//! Footprint builders for standard package families.

use lamina_model::*;
use serde::{Deserialize, Serialize};

use crate::ipc::{self, Density, Dim, Family, land};

/// Body and lead dimensions of a package family. All mm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "family", rename_all = "snake_case")]
pub enum PackageSpec {
    /// Two-terminal rectangular chip (resistor, capacitor, LED, diode).
    Chip {
        /// Overall length, terminal to terminal.
        length: Dim,
        width: Dim,
        /// Terminal length.
        terminal: Dim,
        height: f64,
        /// Pin 1 is the cathode / polarity mark.
        #[serde(default)]
        polarized: bool,
    },
    /// Two rows of gull-wing or flat leads (SOIC, SSOP, TSSOP, SOT, SOD).
    Dual {
        pins: u32,
        pitch: f64,
        /// Lead tip to lead tip across the rows.
        span: Dim,
        /// Foot length.
        lead_length: Dim,
        lead_width: Dim,
        body_width: f64,
        body_length: f64,
        height: f64,
        #[serde(default)]
        flat_leads: bool,
        /// Exposed pad size, numbered pins + 1.
        #[serde(default)]
        exposed_pad: Option<(f64, f64)>,
        /// Explicit pad layout for SOT parts: per pin, (column -1|1, row offset in pitches).
        #[serde(default)]
        layout: Option<Vec<(i8, f64)>>,
        /// Override lead width per pin (for tabs), pin number to width.
        #[serde(default)]
        wide_leads: Vec<(u32, Dim)>,
    },
    /// Leads on four sides (QFP) or terminals on four sides (QFN).
    Quad {
        /// Pins on the left and right sides each.
        pins_vertical: u32,
        /// Pins on the top and bottom sides each.
        pins_horizontal: u32,
        pitch: f64,
        /// Tip-to-tip across the left/right leads (QFP) or body width (QFN).
        span_x: Dim,
        span_y: Dim,
        lead_length: Dim,
        lead_width: Dim,
        body_x: f64,
        body_y: f64,
        height: f64,
        /// true for QFN/DFN style bottom terminations.
        no_lead: bool,
        #[serde(default)]
        exposed_pad: Option<(f64, f64)>,
    },
    /// Ball grid array, NSMD pads.
    Bga {
        rows: u32,
        cols: u32,
        pitch: f64,
        ball: f64,
        body_x: f64,
        body_y: f64,
        height: f64,
    },
    /// Through-hole pin header grid.
    PinHeader {
        rows: u32,
        cols: u32,
        pitch: f64,
        drill: f64,
        pad: f64,
        height: f64,
    },
}

#[derive(Debug, Clone)]
pub struct Options {
    pub density: Density,
    pub silk_width: f64,
    pub silk_to_pad: f64,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            density: Density::Nominal,
            silk_width: 0.15,
            silk_to_pad: 0.15,
        }
    }
}

fn r2(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn pad_shape(w: f64, h: f64) -> PadShape {
    // IPC-7351C recommends rounded corners at 25 % of the short side.
    let r = (w.min(h) * 0.25).min(0.25);
    PadShape::RoundRect {
        width: r2(w),
        height: r2(h),
        radius: r2(r),
    }
}

/// Build a footprint from a package spec.
pub fn generate(
    id: &str,
    package: &str,
    spec: &PackageSpec,
    opt: &Options,
) -> Result<Footprint, String> {
    let mut fp = Footprint::new(
        id,
        Provenance::Ipc7351 {
            package: package.to_string(),
            density: opt.density.letter().into(),
            inputs: serde_json::to_value(spec).map_err(|e| e.to_string())?,
        },
    );
    let d = opt.density;
    let (body, height, pin1): (Rect, f64, Option<Pin1>) = match spec {
        PackageSpec::Chip {
            length,
            width,
            terminal,
            height,
            polarized,
        } => {
            length.check("length")?;
            width.check("width")?;
            terminal.check("terminal")?;
            let fam = if length.nominal() < 1.5 {
                Family::SmallChip
            } else {
                Family::Chip
            };
            let l = land(*length, *terminal, *width, ipc::goals(fam, d));
            let c = l.span() / 2.0;
            for (n, x) in [("1", -c), ("2", c)] {
                fp.pads.push(Pad::smd(
                    n,
                    Point::new(r2(x), 0.0),
                    pad_shape(l.pad_length(), l.x),
                ));
            }
            let body = centered(length.nominal(), width.nominal());
            let p1 = polarized.then_some(Pin1::Side(Point::new(-1.0, 0.0)));
            (body, *height, p1)
        }
        PackageSpec::Dual {
            pins,
            pitch,
            span,
            lead_length,
            lead_width,
            body_width,
            body_length,
            height,
            flat_leads,
            exposed_pad,
            layout,
            wide_leads,
        } => {
            span.check("span")?;
            lead_length.check("lead_length")?;
            lead_width.check("lead_width")?;
            if *pins < 2 {
                return Err("dual package needs at least 2 pins".into());
            }
            let fam = if *flat_leads {
                Family::FlatLead
            } else if *pitch <= 0.625 {
                Family::FineGullWing
            } else {
                Family::GullWing
            };
            let goals = ipc::goals(fam, d);
            let base = land(*span, *lead_length, *lead_width, goals);
            let max_w = pitch - 0.2; // keep at least 0.2 mm copper gap
            let c = base.span() / 2.0;
            let positions: Vec<(i8, f64)> = match layout {
                Some(l) => {
                    if l.len() != *pins as usize {
                        return Err(format!("layout has {} entries for {pins} pins", l.len()));
                    }
                    l.clone()
                }
                None => {
                    if pins % 2 != 0 {
                        return Err("dual package without layout needs an even pin count".into());
                    }
                    let half = pins / 2;
                    let top = (half as f64 - 1.0) / 2.0;
                    (0..*pins)
                        .map(|i| {
                            if i < half {
                                (-1, top - i as f64)
                            } else {
                                (1, -top + (i - half) as f64)
                            }
                        })
                        .collect()
                }
            };
            for (i, (col, row)) in positions.iter().enumerate() {
                let num = i as u32 + 1;
                let lw = wide_leads.iter().find(|(n, _)| *n == num).map(|(_, w)| *w);
                let l = match lw {
                    Some(w) => land(*span, *lead_length, w, goals),
                    None => base,
                };
                let x_size = if lw.is_some() { l.x } else { l.x.min(max_w) };
                fp.pads.push(Pad::smd(
                    num.to_string(),
                    Point::new(r2(*col as f64 * c), r2(row * pitch)),
                    pad_shape(base.pad_length(), x_size),
                ));
            }
            if let Some((w, h)) = exposed_pad {
                let mut ep = Pad::smd(
                    (pins + 1).to_string(),
                    Point::new(0.0, 0.0),
                    pad_shape(*w, *h),
                );
                ep.paste_margin = Some(-(w.min(*h) * 0.1).min(0.2));
                fp.pads.push(ep);
            }
            let p1 = fp
                .pads
                .first()
                .map(|p| Pin1::Pad(p.at, Point::new(-1.0, 0.0)));
            (centered(*body_width, *body_length), *height, p1)
        }
        PackageSpec::Quad {
            pins_vertical,
            pins_horizontal,
            pitch,
            span_x,
            span_y,
            lead_length,
            lead_width,
            body_x,
            body_y,
            height,
            no_lead,
            exposed_pad,
        } => {
            span_x.check("span_x")?;
            span_y.check("span_y")?;
            let fam = if *no_lead {
                Family::NoLead
            } else if *pitch <= 0.625 {
                Family::FineGullWing
            } else {
                Family::GullWing
            };
            let goals = ipc::goals(fam, d);
            let lx = land(*span_x, *lead_length, *lead_width, goals);
            let ly = land(*span_y, *lead_length, *lead_width, goals);
            let wmax = pitch - 0.2_f64.min(pitch * 0.4);
            let (nv, nh) = (*pins_vertical as f64, *pins_horizontal as f64);
            let mut num = 1u32;
            let mut push = |fp: &mut Footprint, at: Point, w: f64, h: f64| {
                fp.pads.push(Pad::smd(num.to_string(), at, pad_shape(w, h)));
                num += 1;
            };
            // Left side, top to bottom.
            for i in 0..*pins_vertical {
                let y = ((nv - 1.0) / 2.0 - i as f64) * pitch;
                push(
                    &mut fp,
                    Point::new(r2(-lx.span() / 2.0), r2(y)),
                    lx.pad_length(),
                    lx.x.min(wmax),
                );
            }
            // Bottom, left to right.
            for i in 0..*pins_horizontal {
                let x = (-(nh - 1.0) / 2.0 + i as f64) * pitch;
                push(
                    &mut fp,
                    Point::new(r2(x), r2(-ly.span() / 2.0)),
                    ly.x.min(wmax),
                    ly.pad_length(),
                );
            }
            // Right, bottom to top.
            for i in 0..*pins_vertical {
                let y = (-(nv - 1.0) / 2.0 + i as f64) * pitch;
                push(
                    &mut fp,
                    Point::new(r2(lx.span() / 2.0), r2(y)),
                    lx.pad_length(),
                    lx.x.min(wmax),
                );
            }
            // Top, right to left.
            for i in 0..*pins_horizontal {
                let x = ((nh - 1.0) / 2.0 - i as f64) * pitch;
                push(
                    &mut fp,
                    Point::new(r2(x), r2(ly.span() / 2.0)),
                    ly.x.min(wmax),
                    ly.pad_length(),
                );
            }
            if let Some((w, h)) = exposed_pad {
                let total = 2 * (pins_vertical + pins_horizontal) + 1;
                let mut ep = Pad::smd(total.to_string(), Point::new(0.0, 0.0), pad_shape(*w, *h));
                ep.paste_margin = Some(-(w.min(*h) * 0.15).min(0.3));
                fp.pads.push(ep);
            }
            let p1 = fp
                .pads
                .first()
                .map(|p| Pin1::Pad(p.at, Point::new(-1.0, 0.0)));
            (centered(*body_x, *body_y), *height, p1)
        }
        PackageSpec::Bga {
            rows,
            cols,
            pitch,
            ball,
            body_x,
            body_y,
            height,
        } => {
            let pad = r2(ball * 0.8);
            let letters = bga_rows();
            if *rows as usize > letters.len() {
                return Err("too many BGA rows".into());
            }
            for r in 0..*rows {
                for c in 0..*cols {
                    let x = (c as f64 - (*cols as f64 - 1.0) / 2.0) * pitch;
                    let y = ((*rows as f64 - 1.0) / 2.0 - r as f64) * pitch;
                    let mut p = Pad::smd(
                        format!("{}{}", letters[r as usize], c + 1),
                        Point::new(r2(x), r2(y)),
                        PadShape::Circle { diameter: pad },
                    );
                    p.mask_margin = Some(0.05);
                    fp.pads.push(p);
                }
            }
            let p1 = fp.pads.first().map(|p| Pin1::Corner(p.at));
            (centered(*body_x, *body_y), *height, p1)
        }
        PackageSpec::PinHeader {
            rows,
            cols,
            pitch,
            drill,
            pad,
            height,
        } => {
            if *rows == 0 || *cols == 0 {
                return Err("pin header needs rows and cols".into());
            }
            if drill >= pad {
                return Err("drill must be smaller than pad".into());
            }
            // Pins numbered along each row of `cols`, rows top to bottom
            // (1x3 is one vertical column of 3 when cols == 1).
            let mut n = 1;
            for r in 0..*rows {
                for c in 0..*cols {
                    let x = (c as f64 - (*cols as f64 - 1.0) / 2.0) * pitch;
                    let y = ((*rows as f64 - 1.0) / 2.0 - r as f64) * pitch;
                    let shape = if n == 1 {
                        PadShape::RoundRect {
                            width: *pad,
                            height: *pad,
                            radius: r2(pad * 0.1),
                        }
                    } else {
                        PadShape::Circle { diameter: *pad }
                    };
                    fp.pads.push(Pad::through(
                        n.to_string(),
                        Point::new(r2(x), r2(y)),
                        shape,
                        Drill::Round { diameter: *drill },
                    ));
                    n += 1;
                }
            }
            let w = *cols as f64 * pitch;
            let h = *rows as f64 * pitch;
            let p1 = fp.pads.first().map(|p| Pin1::Corner(p.at));
            (centered(w, h), *height, p1)
        }
    };
    decorate(&mut fp, body, height, pin1, opt);
    Ok(fp)
}

fn bga_rows() -> Vec<String> {
    let mut v = Vec::new();
    let letters: Vec<char> = ('A'..='Z')
        .filter(|c| !matches!(c, 'I' | 'O' | 'Q' | 'S' | 'X' | 'Z'))
        .collect();
    for c in &letters {
        v.push(c.to_string());
    }
    for a in &letters {
        for b in &letters {
            v.push(format!("{a}{b}"));
        }
    }
    v
}

fn centered(w: f64, h: f64) -> Rect {
    Rect {
        min: Point::new(-w / 2.0, -h / 2.0),
        max: Point::new(w / 2.0, h / 2.0),
    }
}

/// Where to put the pin-1 mark.
pub enum Pin1 {
    /// Polarized two-terminal part: mark the side towards `dir`.
    Side(Point),
    /// Mark beyond this pad in direction `dir`.
    Pad(Point, Point),
    /// Mark at the body corner nearest this pad.
    Corner(Point),
}

fn snap_out(v: f64, step: f64) -> f64 {
    if v >= 0.0 {
        (v / step - 1e-9).ceil() * step
    } else {
        (v / step + 1e-9).floor() * step
    }
}

/// Add courtyard, fabrication outline, silkscreen and reference texts.
pub fn decorate(fp: &mut Footprint, body: Rect, height: f64, pin1: Option<Pin1>, opt: &Options) {
    let pads = fp.pads_bbox().unwrap_or(body);
    let all = pads.union(&body);
    let ex = opt.density.courtyard_excess();
    let cy = Rect {
        min: Point::new(
            snap_out(all.min.x - ex, 0.05),
            snap_out(all.min.y - ex, 0.05),
        ),
        max: Point::new(
            snap_out(all.max.x + ex, 0.05),
            snap_out(all.max.y + ex, 0.05),
        ),
    };
    fp.graphics.push(FpGraphic {
        layer: FpLayer::Courtyard,
        shape: Shape::Rect {
            min: cy.min,
            max: cy.max,
        },
        width: 0.05,
        filled: false,
    });

    // Fabrication outline with a chamfered pin-1 corner.
    let ch = (body.width().min(body.height()) * 0.25).min(1.0);
    let fab = vec![
        Point::new(body.min.x + ch, body.max.y),
        Point::new(body.max.x, body.max.y),
        Point::new(body.max.x, body.min.y),
        Point::new(body.min.x, body.min.y),
        Point::new(body.min.x, body.max.y - ch),
    ];
    fp.graphics.push(FpGraphic {
        layer: FpLayer::Fab,
        shape: Shape::Polygon { points: fab },
        width: 0.1,
        filled: false,
    });

    // Silkscreen: body outline just outside the body, clipped clear of pads.
    let sw = opt.silk_width;
    let o = sw / 2.0 + 0.05;
    let sb = Rect {
        min: Point::new(body.min.x - o, body.min.y - o),
        max: Point::new(body.max.x + o, body.max.y + o),
    };
    let edges = [
        (sb.min, Point::new(sb.max.x, sb.min.y)),
        (Point::new(sb.max.x, sb.min.y), sb.max),
        (sb.max, Point::new(sb.min.x, sb.max.y)),
        (Point::new(sb.min.x, sb.max.y), sb.min),
    ];
    let keep = opt.silk_to_pad + sw / 2.0;
    let blocks: Vec<Rect> = fp
        .pads
        .iter()
        .map(|p| {
            let b = p.bbox();
            Rect {
                min: Point::new(b.min.x - keep, b.min.y - keep),
                max: Point::new(b.max.x + keep, b.max.y + keep),
            }
        })
        .collect();
    for (a, b) in edges {
        for (s, e) in clip_segment(a, b, &blocks) {
            if s.dist(e) >= 0.2 {
                fp.graphics.push(FpGraphic {
                    layer: FpLayer::Silk,
                    shape: Shape::Line { a: s, b: e },
                    width: sw,
                    filled: false,
                });
            }
        }
    }

    // Pin-1 mark.
    let mark = match pin1 {
        Some(Pin1::Side(dir)) => {
            // A bar outside pad 1's outer end.
            let p1 = fp.pads.first().map(|p| p.bbox());
            p1.map(|b| {
                let x = if dir.x < 0.0 {
                    b.min.x - keep
                } else {
                    b.max.x + keep
                };
                Shape::Line {
                    a: Point::new(x, b.min.y),
                    b: Point::new(x, b.max.y),
                }
            })
        }
        Some(Pin1::Pad(at, dir)) => {
            let b = fp.pads.first().map(|p| p.bbox()).unwrap_or(body);
            let x = if dir.x < 0.0 {
                b.min.x - keep - 0.15
            } else {
                b.max.x + keep + 0.15
            };
            Some(Shape::Circle {
                center: Point::new(x, at.y),
                radius: 0.15,
            })
        }
        Some(Pin1::Corner(at)) => {
            let c = Point::new(sb.min.x - 0.3, sb.max.y + 0.3);
            let _ = at;
            Some(Shape::Circle {
                center: c,
                radius: 0.15,
            })
        }
        None => None,
    };
    if let Some(shape) = mark {
        let filled = matches!(shape, Shape::Circle { .. });
        fp.graphics.push(FpGraphic {
            layer: FpLayer::Silk,
            shape,
            width: if filled { 0.0 } else { sw },
            filled,
        });
    }

    fp.texts.push(FpText {
        layer: FpLayer::Silk,
        text: "${ref}".into(),
        at: Point::new(0.0, cy.max.y + 0.6),
        size: 0.8,
        rotation: 0.0,
    });
    fp.texts.push(FpText {
        layer: FpLayer::Fab,
        text: "${value}".into(),
        at: Point::new(0.0, 0.0),
        size: (body.height().min(body.width()) * 0.4).clamp(0.3, 1.0),
        rotation: 0.0,
    });
    fp.height = Some(height);
}

/// Parts of the axis-aligned segment a-b outside every rectangle.
fn clip_segment(a: Point, b: Point, blocks: &[Rect]) -> Vec<(Point, Point)> {
    let horizontal = (a.y - b.y).abs() < 1e-9;
    let (lo, hi, fixed) = if horizontal {
        (a.x.min(b.x), a.x.max(b.x), a.y)
    } else {
        (a.y.min(b.y), a.y.max(b.y), a.x)
    };
    let mut cuts: Vec<(f64, f64)> = blocks
        .iter()
        .filter_map(|r| {
            let (rlo, rhi, flo, fhi) = if horizontal {
                (r.min.x, r.max.x, r.min.y, r.max.y)
            } else {
                (r.min.y, r.max.y, r.min.x, r.max.x)
            };
            (fixed >= flo && fixed <= fhi && rhi > lo && rlo < hi).then_some((rlo, rhi))
        })
        .collect();
    cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut out = Vec::new();
    let mut cur = lo;
    for (s, e) in cuts {
        if s > cur {
            out.push((cur, s.min(hi)));
        }
        cur = cur.max(e);
    }
    if cur < hi {
        out.push((cur, hi));
    }
    out.into_iter()
        .map(|(s, e)| {
            if horizontal {
                (Point::new(s, fixed), Point::new(e, fixed))
            } else {
                (Point::new(fixed, s), Point::new(fixed, e))
            }
        })
        .collect()
}
