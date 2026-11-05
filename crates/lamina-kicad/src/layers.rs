//! KiCad layer names and numbers.

use lamina_model::{Copper, FpLayer, Layer, Side};

/// Our layer for a KiCad board layer name, if we keep it.
pub fn layer_from_kicad(name: &str) -> Option<Layer> {
    let (side, kind) = match name {
        "F.Cu" => return Some(Layer::TOP),
        "B.Cu" => return Some(Layer::BOTTOM),
        "Edge.Cuts" => return Some(Layer::Outline),
        _ => {
            if let Some(n) = name.strip_prefix("In").and_then(|s| s.strip_suffix(".Cu")) {
                let n: u8 = n.parse().ok()?;
                return Some(Layer::Copper(Copper::Inner(n)));
            }
            let (s, k) = name.split_once('.')?;
            let side = match s {
                "F" => Side::Top,
                "B" => Side::Bottom,
                _ => return None,
            };
            (side, k)
        }
    };
    Some(match kind {
        "SilkS" | "Silkscreen" => Layer::Silk(side),
        "Mask" => Layer::Mask(side),
        "Paste" => Layer::Paste(side),
        "CrtYd" | "Courtyard" => Layer::Courtyard(side),
        "Fab" => Layer::Fab(side),
        _ => return None,
    })
}

pub fn layer_to_kicad(l: Layer) -> String {
    let side = |s: Side| if s == Side::Top { "F" } else { "B" };
    match l {
        Layer::Copper(Copper::Top) => "F.Cu".into(),
        Layer::Copper(Copper::Bottom) => "B.Cu".into(),
        Layer::Copper(Copper::Inner(n)) => format!("In{n}.Cu"),
        Layer::Outline => "Edge.Cuts".into(),
        Layer::Silk(s) => format!("{}.SilkS", side(s)),
        Layer::Mask(s) => format!("{}.Mask", side(s)),
        Layer::Paste(s) => format!("{}.Paste", side(s)),
        Layer::Courtyard(s) => format!("{}.CrtYd", side(s)),
        Layer::Fab(s) => format!("{}.Fab", side(s)),
    }
}

/// Where a KiCad layer of a footprint drawing ends up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FpLayerMap {
    Keep(FpLayer),
    /// User drawings and other layers we do not keep.
    Drop,
    /// A layer of the other side of the board.
    OtherSide,
}

/// Footprint-local layer for a KiCad layer name on a footprint of `side`.
pub fn fp_layer_from_kicad(name: &str, side: Side) -> FpLayerMap {
    let l = match layer_from_kicad(name) {
        Some(l) => l,
        None => return FpLayerMap::Drop,
    };
    let (ls, fl) = match l {
        Layer::Copper(Copper::Top) => (Side::Top, FpLayer::Copper),
        Layer::Copper(Copper::Bottom) => (Side::Bottom, FpLayer::Copper),
        Layer::Silk(s) => (s, FpLayer::Silk),
        Layer::Mask(s) => (s, FpLayer::Mask),
        Layer::Paste(s) => (s, FpLayer::Paste),
        Layer::Courtyard(s) => (s, FpLayer::Courtyard),
        Layer::Fab(s) => (s, FpLayer::Fab),
        Layer::Copper(Copper::Inner(_)) | Layer::Outline => return FpLayerMap::Drop,
    };
    if ls == side {
        FpLayerMap::Keep(fl)
    } else {
        FpLayerMap::OtherSide
    }
}

pub fn fp_layer_to_kicad(l: FpLayer, side: Side) -> String {
    let s = if side == Side::Top { "F" } else { "B" };
    let k = match l {
        FpLayer::Silk => "SilkS",
        FpLayer::Courtyard => "CrtYd",
        FpLayer::Fab => "Fab",
        FpLayer::Copper => "Cu",
        FpLayer::Mask => "Mask",
        FpLayer::Paste => "Paste",
    };
    format!("{s}.{k}")
}

/// The `(layers ...)` table of a KiCad 8 board for `copper` copper layers.
pub fn layer_table(copper: u8) -> Vec<(u32, &'static str, &'static str, Option<&'static str>)> {
    let mut v: Vec<(u32, &'static str, &'static str, Option<&'static str>)> =
        vec![(0, "F.Cu", "signal", None)];
    const INNER: [&str; 30] = [
        "In1.Cu", "In2.Cu", "In3.Cu", "In4.Cu", "In5.Cu", "In6.Cu", "In7.Cu", "In8.Cu", "In9.Cu",
        "In10.Cu", "In11.Cu", "In12.Cu", "In13.Cu", "In14.Cu", "In15.Cu", "In16.Cu", "In17.Cu",
        "In18.Cu", "In19.Cu", "In20.Cu", "In21.Cu", "In22.Cu", "In23.Cu", "In24.Cu", "In25.Cu",
        "In26.Cu", "In27.Cu", "In28.Cu", "In29.Cu", "In30.Cu",
    ];
    for (i, name) in INNER.iter().enumerate().take(copper.saturating_sub(2) as usize) {
        v.push((i as u32 + 1, name, "signal", None));
    }
    if copper >= 2 {
        v.push((31, "B.Cu", "signal", None));
    }
    v.extend([
        (32, "B.Adhes", "user", Some("B.Adhesive")),
        (33, "F.Adhes", "user", Some("F.Adhesive")),
        (34, "B.Paste", "user", None),
        (35, "F.Paste", "user", None),
        (36, "B.SilkS", "user", Some("B.Silkscreen")),
        (37, "F.SilkS", "user", Some("F.Silkscreen")),
        (38, "B.Mask", "user", None),
        (39, "F.Mask", "user", None),
        (40, "Dwgs.User", "user", Some("User.Drawings")),
        (41, "Cmts.User", "user", Some("User.Comments")),
        (42, "Eco1.User", "user", Some("User.Eco1")),
        (43, "Eco2.User", "user", Some("User.Eco2")),
        (44, "Edge.Cuts", "user", None),
        (45, "Margin", "user", None),
        (46, "B.CrtYd", "user", Some("B.Courtyard")),
        (47, "F.CrtYd", "user", Some("F.Courtyard")),
        (48, "B.Fab", "user", None),
        (49, "F.Fab", "user", None),
    ]);
    v
}
