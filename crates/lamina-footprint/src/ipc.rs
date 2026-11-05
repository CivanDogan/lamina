//! IPC-7351B land pattern equations.
//!
//! Given component tolerances and solder fillet goals, compute the pad
//! outer span Z, inner gap G and pad width X.

use serde::{Deserialize, Serialize};

/// A dimension with tolerance, in mm.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Dim {
    pub min: f64,
    pub max: f64,
}

impl Dim {
    pub const fn new(min: f64, max: f64) -> Dim {
        Dim { min, max }
    }

    /// Nominal with symmetric tolerance.
    pub fn nom(nom: f64, tol: f64) -> Dim {
        Dim::new(nom - tol, nom + tol)
    }

    pub fn tol(&self) -> f64 {
        self.max - self.min
    }

    pub fn nominal(&self) -> f64 {
        (self.min + self.max) / 2.0
    }

    pub fn check(&self, what: &str) -> Result<(), String> {
        if !(self.min.is_finite() && self.max.is_finite() && self.min > 0.0 && self.max >= self.min)
        {
            return Err(format!("{what}: need 0 < min <= max, got {:?}", self));
        }
        Ok(())
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
pub enum Density {
    /// Most material (level A).
    #[serde(rename = "A")]
    Most,
    /// Nominal (level B).
    #[default]
    #[serde(rename = "B")]
    Nominal,
    /// Least material (level C).
    #[serde(rename = "C")]
    Least,
}

impl Density {
    pub fn letter(self) -> &'static str {
        match self {
            Density::Most => "A",
            Density::Nominal => "B",
            Density::Least => "C",
        }
    }

    fn pick(self, a: f64, b: f64, c: f64) -> f64 {
        match self {
            Density::Most => a,
            Density::Nominal => b,
            Density::Least => c,
        }
    }

    pub fn courtyard_excess(self) -> f64 {
        self.pick(0.5, 0.25, 0.1)
    }
}

/// Solder fillet goals: toe, heel, side.
#[derive(Debug, Clone, Copy)]
pub struct Goals {
    pub toe: f64,
    pub heel: f64,
    pub side: f64,
}

/// Lead families with distinct fillet goals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Family {
    /// Rectangular chip, 0603 and larger.
    Chip,
    /// Rectangular chip smaller than 0603.
    SmallChip,
    /// Gull-wing leads with pitch above 0.625 mm.
    GullWing,
    /// Gull-wing leads with pitch 0.625 mm or less.
    FineGullWing,
    /// Bottom-only terminations (QFN, DFN, SON).
    NoLead,
    /// Flat leads (SOD-123F, SOT-89 tab and similar).
    FlatLead,
}

pub fn goals(f: Family, d: Density) -> Goals {
    let g = |toe, heel, side| Goals { toe, heel, side };
    match f {
        Family::Chip => g(d.pick(0.55, 0.35, 0.15), -0.05, d.pick(0.05, 0.0, -0.05)),
        Family::SmallChip => g(d.pick(0.30, 0.20, 0.10), -0.05, d.pick(0.05, 0.0, -0.05)),
        Family::GullWing => g(
            d.pick(0.55, 0.35, 0.15),
            d.pick(0.45, 0.35, 0.25),
            d.pick(0.05, 0.03, 0.01),
        ),
        Family::FineGullWing => g(
            d.pick(0.55, 0.35, 0.15),
            d.pick(0.45, 0.35, 0.25),
            d.pick(0.01, -0.02, -0.04),
        ),
        Family::NoLead => g(d.pick(0.40, 0.30, 0.20), 0.0, d.pick(-0.04, -0.04, -0.04)),
        Family::FlatLead => g(d.pick(0.30, 0.20, 0.10), 0.0, d.pick(0.05, 0.0, -0.05)),
    }
}

/// Fabrication and placement tolerances.
pub const FAB_TOL: f64 = 0.05;
pub const PLACE_TOL: f64 = 0.025;

/// Result of the land equations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Land {
    /// Outer edge to outer edge of the two pads.
    pub z: f64,
    /// Inner gap between the two pads.
    pub g: f64,
    /// Pad width (perpendicular to the lead axis).
    pub x: f64,
}

impl Land {
    pub fn pad_length(&self) -> f64 {
        (self.z - self.g) / 2.0
    }

    /// Centre-to-centre distance between opposite pads.
    pub fn span(&self) -> f64 {
        (self.z + self.g) / 2.0
    }
}

fn round_up(v: f64, step: f64) -> f64 {
    ((v / step) - 1e-9).ceil() * step
}

fn round_down(v: f64, step: f64) -> f64 {
    ((v / step) + 1e-9).floor() * step
}

/// IPC-7351B equations.
///
/// * `l` overall terminal-to-terminal length
/// * `t` terminal (foot) length
/// * `w` terminal width
pub fn land(l: Dim, t: Dim, w: Dim, goals: Goals) -> Land {
    let f2p2 = FAB_TOL * FAB_TOL + PLACE_TOL * PLACE_TOL;
    let s_min = l.min - 2.0 * t.max;
    let s_max = l.max - 2.0 * t.min;
    let s_tol = s_max - s_min;
    let s_tol_rms = (l.tol().powi(2) + 2.0 * t.tol().powi(2)).sqrt();
    let s_max_rms = s_max - (s_tol - s_tol_rms) / 2.0;

    let z = l.min + 2.0 * goals.toe + (l.tol().powi(2) + f2p2).sqrt();
    let g = s_max_rms - 2.0 * goals.heel - (s_tol_rms.powi(2) + f2p2).sqrt();
    let x = w.min + 2.0 * goals.side + (w.tol().powi(2) + f2p2).sqrt();
    Land {
        z: round_up(z, 0.01),
        g: round_down(g.max(0.0), 0.01),
        x: round_up(x, 0.01),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chip_0603_matches_known_land() {
        // 0603: L 1.6±0.15, W 0.8±0.15, T 0.3±0.15.
        let l = land(
            Dim::nom(1.6, 0.15),
            Dim::nom(0.3, 0.15),
            Dim::nom(0.8, 0.15),
            goals(Family::Chip, Density::Nominal),
        );
        // Widely used nominal land: 0.8 x 0.95 pads, 0.825 from centre.
        assert!((l.z - 2.46).abs() <= 0.02, "{l:?}");
        assert!((l.g - 0.83).abs() <= 0.03, "{l:?}");
        assert!((l.x - 0.96).abs() <= 0.02, "{l:?}");
    }

    #[test]
    fn denser_levels_are_smaller() {
        let args = (
            Dim::nom(1.6, 0.15),
            Dim::nom(0.3, 0.15),
            Dim::nom(0.8, 0.15),
        );
        let a = land(args.0, args.1, args.2, goals(Family::Chip, Density::Most));
        let b = land(
            args.0,
            args.1,
            args.2,
            goals(Family::Chip, Density::Nominal),
        );
        let c = land(args.0, args.1, args.2, goals(Family::Chip, Density::Least));
        assert!(a.z > b.z && b.z > c.z);
        assert!(a.x > b.x && b.x > c.x);
    }
}
