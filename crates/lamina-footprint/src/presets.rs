//! Named packages with JEDEC / manufacturer-typical dimensions.
//!
//! Names: `R_0603`, `C_0805`, `LED_0603`, `D_SOD-123`, `SOT-23`, `SOT-23-5`,
//! `SOT-23-6`, `SOT-223`, `SOIC-8`, `SOIC-16W`, `TSSOP-28`, `SSOP-20`,
//! `MSOP-10`, `QFN-32_5x5_P0.5[_EP3.3x3.3]`, `LQFP-48_7x7_P0.5`,
//! `PinHeader_1x03_P2.54`, `BGA-64_8x8_P0.8_5x5`.

use crate::build::PackageSpec;
use crate::ipc::Dim;

fn chip_dims(code: &str) -> Option<(Dim, Dim, Dim, f64)> {
    // (length, width, terminal, height)
    Some(match code {
        "0201" => (
            Dim::nom(0.6, 0.03),
            Dim::nom(0.3, 0.03),
            Dim::nom(0.15, 0.05),
            0.26,
        ),
        "0402" => (
            Dim::nom(1.0, 0.1),
            Dim::nom(0.5, 0.1),
            Dim::nom(0.25, 0.1),
            0.35,
        ),
        "0603" => (
            Dim::nom(1.6, 0.15),
            Dim::nom(0.8, 0.15),
            Dim::nom(0.3, 0.15),
            0.45,
        ),
        "0805" => (
            Dim::nom(2.0, 0.2),
            Dim::nom(1.25, 0.2),
            Dim::nom(0.4, 0.2),
            0.6,
        ),
        "1206" => (
            Dim::nom(3.2, 0.2),
            Dim::nom(1.6, 0.2),
            Dim::nom(0.5, 0.25),
            0.7,
        ),
        "1210" => (
            Dim::nom(3.2, 0.2),
            Dim::nom(2.5, 0.2),
            Dim::nom(0.5, 0.25),
            0.7,
        ),
        "2010" => (
            Dim::nom(5.0, 0.2),
            Dim::nom(2.5, 0.2),
            Dim::nom(0.6, 0.25),
            0.7,
        ),
        "2512" => (
            Dim::nom(6.3, 0.2),
            Dim::nom(3.2, 0.2),
            Dim::nom(0.6, 0.25),
            0.7,
        ),
        _ => return None,
    })
}

fn num(s: &str) -> Option<f64> {
    s.parse().ok()
}

fn wxh(s: &str) -> Option<(f64, f64)> {
    let (a, b) = s.split_once('x')?;
    Some((num(a)?, num(b)?))
}

fn lead_width_for_pitch(p: f64) -> Dim {
    if p <= 0.4 {
        Dim::new(0.13, 0.23)
    } else if p <= 0.5 {
        Dim::new(0.17, 0.27)
    } else if p <= 0.65 {
        Dim::new(0.22, 0.38)
    } else {
        Dim::new(0.3, 0.45)
    }
}

/// Resolve a package name to a spec.
pub fn resolve(name: &str) -> Result<PackageSpec, String> {
    let up = name.trim();
    let unknown =
        || format!("unknown package `{name}`; see footprint presets in the tool description");

    // Two-terminal chips.
    for (prefix, polarized) in [
        ("R_", false),
        ("C_", false),
        ("L_", false),
        ("LED_", true),
        ("D_", true),
    ] {
        if let Some(code) = up.strip_prefix(prefix) {
            if let Some((length, width, terminal, height)) = chip_dims(code) {
                let height = if prefix == "C_" { height * 1.6 } else { height };
                return Ok(PackageSpec::Chip {
                    length,
                    width,
                    terminal,
                    height,
                    polarized,
                });
            }
        }
    }

    match up {
        "D_SOD-123" => {
            return Ok(PackageSpec::Dual {
                pins: 2,
                pitch: 1.0,
                span: Dim::new(3.55, 3.85),
                lead_length: Dim::new(0.25, 0.5),
                lead_width: Dim::new(0.5, 0.7),
                body_width: 2.7,
                body_length: 1.6,
                height: 1.25,
                flat_leads: true,
                exposed_pad: None,
                layout: Some(vec![(-1, 0.0), (1, 0.0)]),
                wide_leads: vec![],
            });
        }
        "D_SOD-323" => {
            return Ok(PackageSpec::Dual {
                pins: 2,
                pitch: 1.0,
                span: Dim::new(2.3, 2.7),
                lead_length: Dim::new(0.2, 0.4),
                lead_width: Dim::new(0.25, 0.4),
                body_width: 1.7,
                body_length: 1.25,
                height: 1.0,
                flat_leads: true,
                exposed_pad: None,
                layout: Some(vec![(-1, 0.0), (1, 0.0)]),
                wide_leads: vec![],
            });
        }
        "SOT-23" | "SOT-23-3" => {
            return Ok(PackageSpec::Dual {
                pins: 3,
                pitch: 0.95,
                span: Dim::new(2.1, 2.64),
                lead_length: Dim::new(0.3, 0.6),
                lead_width: Dim::new(0.3, 0.5),
                body_width: 1.3,
                body_length: 2.9,
                height: 1.1,
                flat_leads: false,
                exposed_pad: None,
                layout: Some(vec![(-1, 1.0), (-1, -1.0), (1, 0.0)]),
                wide_leads: vec![],
            });
        }
        "SOT-23-5" => {
            return Ok(PackageSpec::Dual {
                pins: 5,
                pitch: 0.95,
                span: Dim::new(2.6, 3.0),
                lead_length: Dim::new(0.3, 0.6),
                lead_width: Dim::new(0.3, 0.5),
                body_width: 1.6,
                body_length: 2.9,
                height: 1.45,
                flat_leads: false,
                exposed_pad: None,
                layout: Some(vec![(-1, 1.0), (-1, 0.0), (-1, -1.0), (1, -1.0), (1, 1.0)]),
                wide_leads: vec![],
            });
        }
        "SOT-23-6" => {
            return Ok(PackageSpec::Dual {
                pins: 6,
                pitch: 0.95,
                span: Dim::new(2.6, 3.0),
                lead_length: Dim::new(0.3, 0.6),
                lead_width: Dim::new(0.3, 0.5),
                body_width: 1.6,
                body_length: 2.9,
                height: 1.45,
                flat_leads: false,
                exposed_pad: None,
                layout: None,
                wide_leads: vec![],
            });
        }
        "SOT-223" => {
            return Ok(PackageSpec::Dual {
                pins: 4,
                pitch: 2.3,
                span: Dim::new(6.7, 7.3),
                lead_length: Dim::new(0.75, 1.15),
                lead_width: Dim::new(0.66, 0.84),
                body_width: 3.5,
                body_length: 6.5,
                height: 1.8,
                flat_leads: false,
                exposed_pad: None,
                layout: Some(vec![(-1, 1.0), (-1, 0.0), (-1, -1.0), (1, 0.0)]),
                wide_leads: vec![(4, Dim::new(2.9, 3.15))],
            });
        }
        _ => {}
    }

    // SOIC / TSSOP / SSOP / MSOP with pin count.
    let dual = |fam: &str, rest: &str| -> Option<PackageSpec> {
        let wide = rest.ends_with('W');
        let pins: u32 = rest.trim_end_matches('W').parse().ok()?;
        if pins < 4 || !pins.is_multiple_of(2) {
            return None;
        }
        let half = (pins / 2) as f64;
        let (pitch, span, lead_length, lead_width, body_width, height) = match (fam, wide) {
            ("SOIC", false) => (
                1.27,
                Dim::new(5.8, 6.2),
                Dim::new(0.4, 1.27),
                Dim::new(0.31, 0.51),
                3.9,
                1.75,
            ),
            ("SOIC", true) => (
                1.27,
                Dim::new(10.0, 10.65),
                Dim::new(0.4, 1.27),
                Dim::new(0.31, 0.51),
                7.5,
                2.65,
            ),
            ("TSSOP", false) => (
                0.65,
                Dim::new(6.2, 6.6),
                Dim::new(0.45, 0.75),
                Dim::new(0.19, 0.3),
                4.4,
                1.2,
            ),
            ("SSOP", false) => (
                0.65,
                Dim::new(7.4, 8.2),
                Dim::new(0.55, 0.95),
                Dim::new(0.22, 0.38),
                5.3,
                2.0,
            ),
            ("MSOP", false) => {
                let p = if pins > 8 { 0.5 } else { 0.65 };
                (
                    p,
                    Dim::new(4.75, 5.05),
                    Dim::new(0.4, 0.7),
                    lead_width_for_pitch(p),
                    3.0,
                    1.1,
                )
            }
            _ => return None,
        };
        let body_length = match (fam, pins) {
            ("SOIC", 8) => 4.9,
            ("MSOP", _) => 3.0,
            _ => half * pitch + if fam == "SOIC" { 0.0 } else { 0.4 },
        };
        Some(PackageSpec::Dual {
            pins,
            pitch,
            span,
            lead_length,
            lead_width,
            body_width,
            body_length,
            height,
            flat_leads: false,
            exposed_pad: None,
            layout: None,
            wide_leads: vec![],
        })
    };
    for fam in ["SOIC", "TSSOP", "SSOP", "MSOP"] {
        if let Some(rest) = up.strip_prefix(&format!("{fam}-")) {
            return dual(fam, rest).ok_or_else(unknown);
        }
    }

    // QFN-32_5x5_P0.5[_EP3.3x3.3]
    if let Some(rest) = up.strip_prefix("QFN-").or_else(|| up.strip_prefix("DFN-")) {
        let is_dfn = up.starts_with("DFN");
        let parts: Vec<&str> = rest.split('_').collect();
        let pins: u32 = parts
            .first()
            .and_then(|s| s.parse().ok())
            .ok_or_else(unknown)?;
        let (bx, by) = parts.get(1).and_then(|s| wxh(s)).ok_or_else(unknown)?;
        let pitch = parts
            .get(2)
            .and_then(|s| s.strip_prefix('P'))
            .and_then(num)
            .ok_or_else(unknown)?;
        let ep = parts
            .iter()
            .find_map(|s| s.strip_prefix("EP"))
            .and_then(wxh);
        let (pv, ph) = if is_dfn {
            (pins / 2, 0)
        } else {
            if !pins.is_multiple_of(4) {
                return Err(format!("QFN pin count {pins} must be a multiple of 4"));
            }
            (pins / 4, pins / 4)
        };
        let lw = if pitch <= 0.4 {
            Dim::new(0.15, 0.25)
        } else if pitch <= 0.5 {
            Dim::new(0.18, 0.30)
        } else {
            Dim::new(0.25, 0.35)
        };
        return Ok(PackageSpec::Quad {
            pins_vertical: pv,
            pins_horizontal: ph,
            pitch,
            span_x: Dim::nom(bx, 0.05),
            span_y: Dim::nom(by, 0.05),
            lead_length: Dim::new(0.3, 0.5),
            lead_width: lw,
            body_x: bx,
            body_y: by,
            height: 0.9,
            no_lead: true,
            exposed_pad: ep,
        });
    }

    // LQFP-48_7x7_P0.5 / TQFP
    if let Some(rest) = up
        .strip_prefix("LQFP-")
        .or_else(|| up.strip_prefix("TQFP-"))
    {
        let thin = up.starts_with("TQFP");
        let parts: Vec<&str> = rest.split('_').collect();
        let pins: u32 = parts
            .first()
            .and_then(|s| s.parse().ok())
            .ok_or_else(unknown)?;
        let (bx, by) = parts.get(1).and_then(|s| wxh(s)).ok_or_else(unknown)?;
        let pitch = parts
            .get(2)
            .and_then(|s| s.strip_prefix('P'))
            .and_then(num)
            .ok_or_else(unknown)?;
        if !pins.is_multiple_of(4) {
            return Err(format!("QFP pin count {pins} must be a multiple of 4"));
        }
        return Ok(PackageSpec::Quad {
            pins_vertical: pins / 4,
            pins_horizontal: pins / 4,
            pitch,
            span_x: Dim::nom(bx + 2.0, 0.2),
            span_y: Dim::nom(by + 2.0, 0.2),
            lead_length: Dim::new(0.45, 0.75),
            lead_width: lead_width_for_pitch(pitch),
            body_x: bx,
            body_y: by,
            height: if thin { 1.2 } else { 1.6 },
            no_lead: false,
            exposed_pad: None,
        });
    }

    // PinHeader_1x03_P2.54
    if let Some(rest) = up
        .strip_prefix("PinHeader_")
        .or_else(|| up.strip_prefix("PinSocket_"))
    {
        let parts: Vec<&str> = rest.split('_').collect();
        let (rows_s, cols_s) = parts
            .first()
            .and_then(|s| s.split_once('x'))
            .ok_or_else(unknown)?;
        let a: u32 = rows_s.parse().map_err(|_| unknown())?;
        let b: u32 = cols_s.parse().map_err(|_| unknown())?;
        let pitch = parts
            .get(1)
            .and_then(|s| s.strip_prefix('P'))
            .and_then(num)
            .unwrap_or(2.54);
        let (drill, pad) = if pitch >= 2.5 {
            (1.0, 1.7)
        } else if pitch >= 2.0 {
            (0.8, 1.35)
        } else {
            (0.65, 1.0)
        };
        // `1x03` means 1 column of 3 pins, like the common naming.
        return Ok(PackageSpec::PinHeader {
            rows: b,
            cols: a,
            pitch,
            drill,
            pad,
            height: if up.starts_with("PinSocket") {
                8.5
            } else {
                8.5 + 2.5
            },
        });
    }

    // BGA-64_8x8_P0.8_5x5 : pins, grid, pitch, body
    if let Some(rest) = up.strip_prefix("BGA-") {
        let parts: Vec<&str> = rest.split('_').collect();
        let (c, r) = parts.get(1).and_then(|s| wxh(s)).ok_or_else(unknown)?;
        let pitch = parts
            .get(2)
            .and_then(|s| s.strip_prefix('P'))
            .and_then(num)
            .ok_or_else(unknown)?;
        let (bx, by) = parts.get(3).and_then(|s| wxh(s)).ok_or_else(unknown)?;
        return Ok(PackageSpec::Bga {
            rows: r as u32,
            cols: c as u32,
            pitch,
            ball: pitch * 0.5,
            body_x: bx,
            body_y: by,
            height: 1.2,
        });
    }

    Err(unknown())
}

/// Package name examples, shown to agents.
pub const EXAMPLES: &[&str] = &[
    "R_0402",
    "R_0603",
    "C_0805",
    "LED_0603",
    "D_SOD-123",
    "SOT-23",
    "SOT-23-5",
    "SOT-23-6",
    "SOT-223",
    "SOIC-8",
    "SOIC-16W",
    "TSSOP-28",
    "SSOP-20",
    "MSOP-10",
    "QFN-32_5x5_P0.5_EP3.3x3.3",
    "DFN-8_3x3_P0.65",
    "LQFP-48_7x7_P0.5",
    "PinHeader_1x03_P2.54",
    "PinHeader_2x05_P1.27",
    "BGA-64_8x8_P0.8_5x5",
];
