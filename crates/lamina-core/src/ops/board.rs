use lamina_model::error::Result;
use lamina_model::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::project::{OpResult, Project, Touched};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RectSpec {
    /// Lower-left corner X in mm.
    #[serde(default)]
    pub x: f64,
    /// Lower-left corner Y in mm.
    #[serde(default)]
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Corner radius in mm (0 for sharp corners).
    #[serde(default)]
    pub corner_radius: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SetOutline {
    /// Rectangular board. Give either `rect` or `contour`.
    #[serde(default)]
    pub rect: Option<RectSpec>,
    /// Arbitrary outline of lines and arcs.
    #[serde(default)]
    pub contour: Option<Contour>,
    /// Internal cut-outs; replaces existing ones when given.
    #[serde(default)]
    pub cutouts: Option<Vec<Contour>>,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct SetStackup {
    /// 1, or an even number 2..32.
    pub copper_layers: Option<u8>,
    /// Finished thickness in mm.
    pub thickness: Option<f64>,
    pub outer_copper_oz: Option<f64>,
    pub inner_copper_oz: Option<f64>,
    pub finish: Option<String>,
    pub mask_color: Option<String>,
    pub silk_color: Option<String>,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct SetRules {
    /// Partial rules: only the fields given are changed. Net classes given
    /// here replace classes of the same name.
    #[serde(default)]
    pub rules: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub strict: bool,
}

impl Project {
    pub fn set_outline(&mut self, p: SetOutline) -> Result<OpResult> {
        let contour = match (p.rect, p.contour) {
            (Some(r), None) => {
                for (v, n) in [(r.width, "width"), (r.height, "height")] {
                    if !(v.is_finite() && v > 0.0) {
                        return Err(Error::invalid(format!("rect {n} must be positive")));
                    }
                }
                if r.corner_radius < 0.0 || r.corner_radius * 2.0 > r.width.min(r.height) {
                    return Err(Error::invalid("corner_radius out of range"));
                }
                Contour::rounded_rect(r.x, r.y, r.width, r.height, r.corner_radius)
            }
            (None, Some(c)) => c,
            (None, None) => return Err(Error::invalid("give `rect` or `contour`")),
            (Some(_), Some(_)) => {
                return Err(Error::invalid("give only one of `rect` and `contour`"));
            }
        };
        contour.check().map_err(Error::Invalid)?;
        let mut d = self.data.clone();
        if let Some(cuts) = p.cutouts {
            for c in &cuts {
                c.check()
                    .map_err(|e| Error::invalid(format!("cutout: {e}")))?;
            }
            d.board.cutouts = cuts;
        }
        let bb = contour.bbox().expect("checked contour has points");
        d.board.outline = Some(contour);
        self.commit(
            d,
            Touched::file(FileKind::Board),
            p.strict,
            format!("outline set, {:.3} x {:.3} mm", bb.width(), bb.height()),
        )
    }

    pub fn set_stackup(&mut self, p: SetStackup) -> Result<OpResult> {
        let mut d = self.data.clone();
        let s = &mut d.board.stackup;
        if let Some(n) = p.copper_layers {
            s.copper_layers = n;
        }
        if let Some(t) = p.thickness {
            s.thickness = t;
        }
        if let Some(v) = p.outer_copper_oz {
            s.outer_copper_oz = v;
        }
        if let Some(v) = p.inner_copper_oz {
            s.inner_copper_oz = v;
        }
        if let Some(v) = p.finish {
            s.finish = v;
        }
        if let Some(v) = p.mask_color {
            s.mask_color = v;
        }
        if let Some(v) = p.silk_color {
            s.silk_color = v;
        }
        s.check().map_err(Error::Invalid)?;
        let n = s.copper_layers;
        // Removing layers that still carry copper would leave dangling data.
        let stranded: Vec<String> = d
            .routing
            .tracks
            .iter()
            .filter(|t| !t.layer.exists_in(n))
            .map(|t| t.id.clone())
            .chain(
                d.routing
                    .zones
                    .iter()
                    .filter(|z| z.layers.iter().any(|l| !l.exists_in(n)))
                    .map(|z| z.id.clone()),
            )
            .collect();
        if !stranded.is_empty() {
            return Err(Error::Refused(format!(
                "these items are on layers a {n}-layer board does not have: {}",
                stranded.join(", ")
            )));
        }
        let summary = format!(
            "stackup: {n} copper layers, {} mm",
            d.board.stackup.thickness
        );
        self.commit(d, Touched::file(FileKind::Board), p.strict, summary)
    }

    pub fn set_rules(&mut self, p: SetRules) -> Result<OpResult> {
        let mut d = self.data.clone();
        let mut current =
            serde_json::to_value(&d.board.rules).map_err(|e| Error::invalid(e.to_string()))?;
        let obj = current
            .as_object_mut()
            .expect("rules serialize to an object");
        for (k, v) in p.rules {
            if k == "net_classes" {
                let classes = obj
                    .entry("net_classes")
                    .or_insert_with(|| serde_json::Value::Object(Default::default()));
                let (Some(dst), Some(src)) = (classes.as_object_mut(), v.as_object()) else {
                    return Err(Error::invalid("net_classes must be an object"));
                };
                for (name, c) in src {
                    dst.insert(name.clone(), c.clone());
                }
            } else if obj.contains_key(&k) || k == "preset" {
                obj.insert(k, v);
            } else {
                return Err(Error::invalid(format!("unknown rule `{k}`")));
            }
        }
        let rules: Rules =
            serde_json::from_value(current).map_err(|e| Error::invalid(format!("rules: {e}")))?;
        for (name, v) in [
            ("clearance", rules.clearance),
            ("track_width", rules.track_width),
            ("via_diameter", rules.via_diameter),
            ("via_drill", rules.via_drill),
        ] {
            if !(v.is_finite() && v > 0.0) {
                return Err(Error::invalid(format!("{name} must be positive")));
            }
        }
        if rules.via_drill >= rules.via_diameter {
            return Err(Error::invalid(
                "via_drill must be smaller than via_diameter",
            ));
        }
        d.board.rules = rules;
        self.commit(d, Touched::file(FileKind::Board), p.strict, "rules updated")
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct UseFabPreset {
    /// `jlcpcb-2layer`, `jlcpcb-4layer` or `relaxed`.
    pub name: String,
    #[serde(default)]
    pub strict: bool,
}

impl Project {
    pub fn use_fab_preset(&mut self, p: UseFabPreset) -> Result<OpResult> {
        let mut rules = lamina_drc::presets::get(&p.name).ok_or_else(|| {
            let names: Vec<&str> = lamina_drc::presets::PRESETS
                .iter()
                .map(|x| x.name)
                .collect();
            Error::invalid(format!(
                "unknown preset `{}`; use one of {}",
                p.name,
                names.join(", ")
            ))
        })?;
        let mut d = self.data.clone();
        rules.net_classes = d.board.rules.net_classes.clone();
        d.board.rules = rules;
        self.commit(
            d,
            Touched::file(FileKind::Board),
            p.strict,
            format!("rules from preset {}", p.name),
        )
    }
}
