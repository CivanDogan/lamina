use lamina_model::error::Result;
use lamina_model::*;
use lamina_route::RouteOptions;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ops::place::Target;
use crate::project::{OpResult, Project, Touched, next_id};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RouteTrace {
    /// Net; inferred from the first pad when omitted.
    #[serde(default)]
    pub net: Option<String>,
    /// Copper layer, e.g. `top` or `bottom`.
    pub layer: Layer,
    /// Width in mm (default: the net class or board rule).
    #[serde(default)]
    pub width: Option<f64>,
    /// Points in order. Pad references (`U1.3`) snap to the pad centre.
    pub points: Vec<Target>,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AddVia {
    pub net: String,
    /// Position, or a pad reference to drop the via on.
    pub at: Target,
    #[serde(default)]
    pub diameter: Option<f64>,
    #[serde(default)]
    pub drill: Option<f64>,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AddZone {
    /// Net of the pour; omit for an unconnected island.
    #[serde(default)]
    pub net: Option<String>,
    pub layers: Vec<Layer>,
    /// Outline; defaults to the board outline.
    #[serde(default)]
    pub outline: Option<Contour>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub clearance: Option<f64>,
    #[serde(default)]
    pub min_width: Option<f64>,
    /// `thermal` (default), `solid` or `none`.
    #[serde(default)]
    pub pad_connection: PadConnection,
    #[serde(default)]
    pub thermal_gap: Option<f64>,
    #[serde(default)]
    pub thermal_spoke: Option<f64>,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AddKeepout {
    pub layers: Vec<Layer>,
    pub outline: Contour,
    #[serde(default)]
    pub forbid: Forbid,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct Ripup {
    /// Remove tracks and vias of this net.
    #[serde(default)]
    pub net: Option<String>,
    /// Remove these track or via ids.
    #[serde(default)]
    pub ids: Option<Vec<String>>,
    /// Only on this layer (tracks only).
    #[serde(default)]
    pub layer: Option<Layer>,
    /// Only items with a point inside this box.
    #[serde(default)]
    pub region: Option<Rect>,
    /// Remove every track and via.
    #[serde(default)]
    pub all: bool,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct DeleteItems {
    /// Ids of tracks, vias, zones, keep-outs, texts, graphics or holes.
    pub ids: Vec<String>,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Autoroute {
    #[serde(flatten)]
    pub options: RouteOptions,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AddText {
    /// `top.silk`, `bottom.silk`, a copper layer or `top.fab`.
    pub layer: Layer,
    pub text: String,
    pub at: Point,
    #[serde(default)]
    pub size: Option<f64>,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AddHole {
    pub at: Point,
    pub diameter: f64,
    /// Plated with a copper ring of this diameter.
    #[serde(default)]
    pub pad_diameter: Option<f64>,
    #[serde(default)]
    pub net: Option<String>,
    #[serde(default)]
    pub strict: bool,
}

fn check_copper(l: Layer, layers: u8) -> Result<()> {
    if l.is_copper() && l.exists_in(layers) {
        Ok(())
    } else {
        Err(Error::invalid(format!(
            "{l} is not a copper layer of this {layers}-layer board"
        )))
    }
}

impl Project {
    fn all_ids(&self) -> Vec<String> {
        let r = &self.data.routing;
        r.tracks
            .iter()
            .map(|t| t.id.clone())
            .chain(r.vias.iter().map(|v| v.id.clone()))
            .chain(r.zones.iter().map(|z| z.id.clone()))
            .chain(r.keepouts.iter().map(|k| k.id.clone()))
            .chain(r.texts.iter().map(|k| k.id.clone()))
            .chain(r.graphics.iter().map(|k| k.id.clone()))
            .chain(self.data.board.holes.iter().map(|h| h.id.clone()))
            .collect()
    }

    fn new_id(&self, prefix: &str) -> String {
        let ids = self.all_ids();
        next_id(prefix, ids.iter().map(|s| s.as_str()))
    }

    pub fn route_trace(&mut self, p: RouteTrace) -> Result<OpResult> {
        let layers = self.data.board.stackup.copper_layers;
        check_copper(p.layer, layers)?;
        if p.points.len() < 2 {
            return Err(Error::invalid("a trace needs at least two points"));
        }
        let li = p.layer.copper().unwrap().index(layers);
        let mut pts = Vec::new();
        let mut inferred: Option<String> = None;
        let pad_nets = self.data.nets.pad_map();
        for t in &p.points {
            match t {
                Target::Point(q) => {
                    if !q.is_finite() {
                        return Err(Error::invalid("trace point is not finite"));
                    }
                    pts.push(Point::new(json::round6(q.x), json::round6(q.y)));
                }
                Target::Name(s) => {
                    let pr: PadRef = s.parse().map_err(|e| {
                        Error::invalid(format!("`{s}`: {e}; trace points are pads or {{x, y}}"))
                    })?;
                    let (c, _, ls) = self.pad_position(&pr)?;
                    if !ls.contains(&li) {
                        return Err(Error::invalid(format!(
                            "{pr} has no copper on {}; route on its layer or add a via",
                            p.layer
                        )));
                    }
                    if inferred.is_none() {
                        inferred = pad_nets.get(&pr).cloned();
                    }
                    pts.push(Point::new(json::round6(c.x), json::round6(c.y)));
                }
            }
        }
        pts.dedup_by(|a, b| a.dist(*b) < 1e-9);
        if pts.len() < 2 {
            return Err(Error::invalid("trace points collapse to a single point"));
        }
        let net = p
            .net
            .or(inferred)
            .ok_or_else(|| Error::invalid("give `net`: none of the pads has a net"))?;
        if !self.data.nets.nets.contains_key(&net) {
            return Err(Error::invalid(format!("no net `{net}`")));
        }
        let class = self.data.nets.nets[&net].class.clone();
        let width = p
            .width
            .unwrap_or(self.data.board.rules.track_width_for(class.as_deref()));
        if !(width.is_finite() && width > 0.0) {
            return Err(Error::invalid("width must be positive"));
        }
        let id = self.new_id("t");
        let len: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
        let mut d = self.data.clone();
        d.routing.tracks.push(Track {
            id: id.clone(),
            net: net.clone(),
            layer: p.layer,
            width,
            points: pts,
        });
        self.commit(
            d,
            Touched::file(FileKind::Routing),
            p.strict,
            format!(
                "{id}: {net} on {}, {} mm long, {width} mm wide",
                p.layer,
                json::round6(len)
            ),
        )
    }

    pub fn add_via(&mut self, p: AddVia) -> Result<OpResult> {
        if !self.data.nets.nets.contains_key(&p.net) {
            return Err(Error::invalid(format!("no net `{}`", p.net)));
        }
        let at = match &p.at {
            Target::Point(q) => *q,
            Target::Name(s) => {
                let pr: PadRef = s.parse().map_err(Error::Invalid)?;
                self.pad_position(&pr)?.0
            }
        };
        let class = self.data.nets.nets[&p.net].class.clone();
        let nc = self.data.board.rules.class(class.as_deref());
        let diameter = p
            .diameter
            .or(nc.via_diameter)
            .unwrap_or(self.data.board.rules.via_diameter);
        let drill = p
            .drill
            .or(nc.via_drill)
            .unwrap_or(self.data.board.rules.via_drill);
        if !(drill > 0.0 && diameter > drill) {
            return Err(Error::invalid("via needs 0 < drill < diameter"));
        }
        let id = self.new_id("v");
        let mut d = self.data.clone();
        d.routing.vias.push(Via {
            id: id.clone(),
            net: p.net.clone(),
            at: Point::new(json::round6(at.x), json::round6(at.y)),
            diameter,
            drill,
            from: None,
            to: None,
        });
        self.commit(
            d,
            Touched::file(FileKind::Routing),
            p.strict,
            format!(
                "{id}: {} via at ({}, {})",
                p.net,
                json::round6(at.x),
                json::round6(at.y)
            ),
        )
    }

    pub fn add_zone(&mut self, p: AddZone) -> Result<OpResult> {
        let layers = self.data.board.stackup.copper_layers;
        if p.layers.is_empty() {
            return Err(Error::invalid("give at least one layer"));
        }
        for l in &p.layers {
            check_copper(*l, layers)?;
        }
        if let Some(n) = &p.net {
            if !self.data.nets.nets.contains_key(n) {
                return Err(Error::invalid(format!("no net `{n}`")));
            }
        }
        let outline = match p.outline {
            Some(c) => c,
            None => self.data.board.outline.clone().ok_or_else(|| {
                Error::invalid("no outline given and the board outline is not set")
            })?,
        };
        outline.check().map_err(Error::Invalid)?;
        let id = self.new_id("z");
        let mut d = self.data.clone();
        d.routing.zones.push(Zone {
            id: id.clone(),
            net: p.net.clone(),
            layers: p.layers.clone(),
            outline,
            priority: p.priority,
            clearance: p.clearance,
            min_width: p.min_width,
            pad_connection: p.pad_connection,
            thermal_gap: p.thermal_gap.unwrap_or(0.3),
            thermal_spoke: p.thermal_spoke.unwrap_or(0.3),
        });
        let names: Vec<String> = p.layers.iter().map(|l| l.to_string()).collect();
        self.commit(
            d,
            Touched::file(FileKind::Routing),
            p.strict,
            format!(
                "{id}: {} pour on {}",
                p.net.as_deref().unwrap_or("unconnected"),
                names.join(", ")
            ),
        )
    }

    pub fn add_keepout(&mut self, p: AddKeepout) -> Result<OpResult> {
        p.outline.check().map_err(Error::Invalid)?;
        if p.layers.is_empty() {
            return Err(Error::invalid("give at least one layer"));
        }
        let id = self.new_id("k");
        let mut d = self.data.clone();
        d.routing.keepouts.push(Keepout {
            id: id.clone(),
            layers: p.layers,
            outline: p.outline,
            forbid: p.forbid,
        });
        self.commit(
            d,
            Touched::file(FileKind::Routing),
            p.strict,
            format!("{id}: keep-out added"),
        )
    }

    pub fn ripup(&mut self, p: Ripup) -> Result<OpResult> {
        if !p.all && p.net.is_none() && p.ids.is_none() && p.region.is_none() {
            return Err(Error::invalid("give net, ids, region or all: true"));
        }
        let inside = |q: &Point| {
            p.region.as_ref().is_none_or(|r| {
                q.x >= r.min.x && q.x <= r.max.x && q.y >= r.min.y && q.y <= r.max.y
            })
        };
        let mut d = self.data.clone();
        let before = (d.routing.tracks.len(), d.routing.vias.len());
        d.routing.tracks.retain(|t| {
            let hit = (p.all || p.net.as_ref().is_none_or(|n| *n == t.net))
                && p.ids.as_ref().is_none_or(|ids| ids.contains(&t.id))
                && p.layer.is_none_or(|l| l == t.layer)
                && t.points.iter().any(&inside);
            !hit
        });
        if p.layer.is_none() {
            d.routing.vias.retain(|v| {
                let hit = (p.all || p.net.as_ref().is_none_or(|n| *n == v.net))
                    && p.ids.as_ref().is_none_or(|ids| ids.contains(&v.id))
                    && inside(&v.at);
                !hit
            });
        }
        let removed = (
            before.0 - d.routing.tracks.len(),
            before.1 - d.routing.vias.len(),
        );
        self.commit(
            d,
            Touched::file(FileKind::Routing),
            p.strict,
            format!("removed {} track(s) and {} via(s)", removed.0, removed.1),
        )
    }

    pub fn delete_items(&mut self, p: DeleteItems) -> Result<OpResult> {
        let mut d = self.data.clone();
        let mut missing = Vec::new();
        let mut touched = Touched::file(FileKind::Routing);
        for id in &p.ids {
            let r = &mut d.routing;
            let n = r.tracks.len()
                + r.vias.len()
                + r.zones.len()
                + r.keepouts.len()
                + r.texts.len()
                + r.graphics.len();
            r.tracks.retain(|x| &x.id != id);
            r.vias.retain(|x| &x.id != id);
            r.zones.retain(|x| &x.id != id);
            r.keepouts.retain(|x| &x.id != id);
            r.texts.retain(|x| &x.id != id);
            r.graphics.retain(|x| &x.id != id);
            let m = r.tracks.len()
                + r.vias.len()
                + r.zones.len()
                + r.keepouts.len()
                + r.texts.len()
                + r.graphics.len();
            let h = d.board.holes.len();
            d.board.holes.retain(|x| &x.id != id);
            if d.board.holes.len() != h {
                touched = touched.and(FileKind::Board);
            } else if m == n {
                missing.push(id.clone());
            }
        }
        if !missing.is_empty() {
            return Err(Error::invalid(format!(
                "no items with ids: {}",
                missing.join(", ")
            )));
        }
        self.commit(d, touched, p.strict, format!("deleted {}", p.ids.join(" ")))
    }

    pub fn autoroute(&mut self, p: Autoroute) -> Result<OpResult> {
        let out = lamina_route::autoroute(&self.data, &p.options).map_err(Error::Invalid)?;
        let mut d = self.data.clone();
        let n_t = out.tracks.len();
        let n_v = out.vias.len();
        d.routing.tracks.extend(out.tracks);
        d.routing.vias.extend(out.vias);
        let mut res = self.commit(
            d,
            Touched::file(FileKind::Routing),
            p.strict,
            format!(
                "routed {} connection(s) with {n_t} track(s) and {n_v} via(s); {} failed",
                out.routed.len(),
                out.failed.len()
            ),
        )?;
        res.notes = out
            .failed
            .iter()
            .map(|f| format!("failed {} {} -> {}: {}", f.net, f.from, f.to, f.reason))
            .collect();
        Ok(res)
    }

    pub fn add_text(&mut self, p: AddText) -> Result<OpResult> {
        if p.text.trim().is_empty() {
            return Err(Error::invalid("text is empty"));
        }
        let id = self.new_id("txt");
        let mut d = self.data.clone();
        d.routing.texts.push(BoardText {
            id: id.clone(),
            layer: p.layer,
            text: p.text.clone(),
            at: p.at,
            size: p.size.unwrap_or(1.0),
            rotation: p.rotation,
        });
        self.commit(
            d,
            Touched::file(FileKind::Routing),
            p.strict,
            format!("{id}: \"{}\" on {}", p.text, p.layer),
        )
    }

    pub fn add_hole(&mut self, p: AddHole) -> Result<OpResult> {
        if !p.diameter.is_finite() || p.diameter <= 0.0 {
            return Err(Error::invalid("diameter must be positive"));
        }
        if let Some(pd) = p.pad_diameter {
            if pd <= p.diameter {
                return Err(Error::invalid("pad_diameter must exceed the hole"));
            }
        }
        let id = self.new_id("h");
        let mut d = self.data.clone();
        d.board.holes.push(Hole {
            id: id.clone(),
            at: p.at,
            drill: Drill::Round {
                diameter: p.diameter,
            },
            plated: p.pad_diameter.is_some(),
            pad_diameter: p.pad_diameter,
            net: p.net,
            rotation: 0.0,
        });
        self.commit(
            d,
            Touched::file(FileKind::Board),
            p.strict,
            format!("{id}: {} mm hole", p.diameter),
        )
    }

    /// Missing connections as lines between pads.
    pub fn ratsnest(&self) -> Vec<lamina_drc::Ratsnest> {
        lamina_drc::analyze(&self.data).ratsnest
    }
}
