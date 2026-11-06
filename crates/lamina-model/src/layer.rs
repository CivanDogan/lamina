//! Board layers. They are serialized as short strings: `top`, `in1`, `bottom`,
//! `top.mask`, `bottom.silk`, `outline` and other names.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    #[default]
    Top,
    Bottom,
}

impl Side {
    pub fn flip(self) -> Side {
        match self {
            Side::Top => Side::Bottom,
            Side::Bottom => Side::Top,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Side::Top => "top",
            Side::Bottom => "bottom",
        }
    }
}

/// A copper layer. Inner layers are numbered from the top. The first inner layer is 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Copper {
    Top,
    Inner(u8),
    Bottom,
}

impl Copper {
    /// The position in the stack. 0 is the top.
    pub fn index(self, copper_layers: u8) -> u8 {
        match self {
            Copper::Top => 0,
            Copper::Inner(n) => n,
            Copper::Bottom => copper_layers.saturating_sub(1),
        }
    }

    pub fn from_index(i: u8, copper_layers: u8) -> Copper {
        if i == 0 {
            Copper::Top
        } else if i + 1 >= copper_layers {
            Copper::Bottom
        } else {
            Copper::Inner(i)
        }
    }

    pub fn exists_in(self, copper_layers: u8) -> bool {
        match self {
            Copper::Top => true,
            Copper::Bottom => copper_layers >= 2,
            Copper::Inner(n) => n >= 1 && n + 1 < copper_layers,
        }
    }

    pub fn side(self) -> Option<Side> {
        match self {
            Copper::Top => Some(Side::Top),
            Copper::Bottom => Some(Side::Bottom),
            Copper::Inner(_) => None,
        }
    }

    /// Mirrors through the board (for a footprint that is flipped).
    pub fn flip(self, copper_layers: u8) -> Copper {
        Copper::from_index(copper_layers - 1 - self.index(copper_layers), copper_layers)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Layer {
    Copper(Copper),
    Mask(Side),
    Paste(Side),
    Silk(Side),
    Courtyard(Side),
    Fab(Side),
    Outline,
}

impl Layer {
    pub const TOP: Layer = Layer::Copper(Copper::Top);
    pub const BOTTOM: Layer = Layer::Copper(Copper::Bottom);

    pub fn is_copper(self) -> bool {
        matches!(self, Layer::Copper(_))
    }

    pub fn copper(self) -> Option<Copper> {
        match self {
            Layer::Copper(c) => Some(c),
            _ => None,
        }
    }

    pub fn side(self) -> Option<Side> {
        match self {
            Layer::Copper(c) => c.side(),
            Layer::Mask(s)
            | Layer::Paste(s)
            | Layer::Silk(s)
            | Layer::Courtyard(s)
            | Layer::Fab(s) => Some(s),
            Layer::Outline => None,
        }
    }

    /// True if this layer exists on a board with `copper_layers` copper layers.
    pub fn exists_in(self, copper_layers: u8) -> bool {
        match self {
            Layer::Copper(c) => c.exists_in(copper_layers),
            _ => true,
        }
    }

    pub fn flip(self, copper_layers: u8) -> Layer {
        match self {
            Layer::Copper(c) => Layer::Copper(c.flip(copper_layers)),
            Layer::Mask(s) => Layer::Mask(s.flip()),
            Layer::Paste(s) => Layer::Paste(s.flip()),
            Layer::Silk(s) => Layer::Silk(s.flip()),
            Layer::Courtyard(s) => Layer::Courtyard(s.flip()),
            Layer::Fab(s) => Layer::Fab(s.flip()),
            Layer::Outline => Layer::Outline,
        }
    }

    /// All copper layers of a board, top first.
    pub fn all_copper(copper_layers: u8) -> Vec<Layer> {
        (0..copper_layers)
            .map(|i| Layer::Copper(Copper::from_index(i, copper_layers)))
            .collect()
    }
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (side, kind) = match self {
            Layer::Copper(Copper::Top) => return f.write_str("top"),
            Layer::Copper(Copper::Bottom) => return f.write_str("bottom"),
            Layer::Copper(Copper::Inner(n)) => return write!(f, "in{n}"),
            Layer::Outline => return f.write_str("outline"),
            Layer::Mask(s) => (s, "mask"),
            Layer::Paste(s) => (s, "paste"),
            Layer::Silk(s) => (s, "silk"),
            Layer::Courtyard(s) => (s, "courtyard"),
            Layer::Fab(s) => (s, "fab"),
        };
        write!(f, "{}.{}", side.as_str(), kind)
    }
}

impl FromStr for Layer {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "top" => return Ok(Layer::TOP),
            "bottom" => return Ok(Layer::BOTTOM),
            "outline" => return Ok(Layer::Outline),
            _ => {}
        }
        if let Some(n) = s.strip_prefix("in") {
            let n: u8 = n
                .parse()
                .map_err(|_| format!("The inner layer `{s}` is not valid."))?;
            if n == 0 || n > 30 {
                return Err(format!(
                    "The inner layer number in `{s}` is out of range. Use 1 to 30."
                ));
            }
            return Ok(Layer::Copper(Copper::Inner(n)));
        }
        let (side, kind) = s
            .split_once('.')
            .ok_or_else(|| format!("The layer `{s}` is unknown."))?;
        let side = match side {
            "top" => Side::Top,
            "bottom" => Side::Bottom,
            _ => {
                return Err(format!(
                    "The layer side in `{s}` is unknown. Use `top` or `bottom`."
                ));
            }
        };
        Ok(match kind {
            "mask" => Layer::Mask(side),
            "paste" => Layer::Paste(side),
            "silk" => Layer::Silk(side),
            "courtyard" => Layer::Courtyard(side),
            "fab" => Layer::Fab(side),
            _ => return Err(format!("The layer kind in `{s}` is unknown.")),
        })
    }
}

impl Serialize for Layer {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Layer {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = Cow::<str>::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for Layer {
    fn schema_name() -> Cow<'static, str> {
        "Layer".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "description": "Copper: `top`, `in1`..`in30`, `bottom`. Others: `top.mask`, `top.paste`, `top.silk`, `top.courtyard`, `top.fab` (and `bottom.*`), `outline`.",
            "pattern": "^(top|bottom|in([1-9]|[12][0-9]|30)|(top|bottom)\\.(mask|paste|silk|courtyard|fab)|outline)$"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_names() {
        for s in [
            "top",
            "bottom",
            "in1",
            "in12",
            "top.mask",
            "bottom.paste",
            "top.silk",
            "bottom.courtyard",
            "top.fab",
            "outline",
        ] {
            let l: Layer = s.parse().unwrap();
            assert_eq!(l.to_string(), s);
        }
        assert!("in0".parse::<Layer>().is_err());
        assert!("front".parse::<Layer>().is_err());
    }

    #[test]
    fn copper_indexing() {
        assert!(Copper::Inner(1).exists_in(4));
        assert!(Copper::Inner(2).exists_in(4));
        assert!(!Copper::Inner(3).exists_in(4));
        assert!(!Copper::Inner(1).exists_in(2));
        assert_eq!(Copper::Top.flip(4), Copper::Bottom);
        assert_eq!(Copper::Inner(1).flip(4), Copper::Inner(2));
        assert_eq!(Copper::from_index(3, 4), Copper::Bottom);
    }
}
