use lamina_model::error::Result;
use lamina_model::*;
use lamina_render::{Snapshot, raster::Options2D};
use lamina_scene::SceneLayer;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::project::Project;

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RenderView {
    /// A technical view of the layers, in layer colours (default).
    #[default]
    Layers,
    /// The finished board from the top, with the mask, the finish and the silkscreen.
    Top,
    /// The finished board from the bottom (mirrored).
    Bottom,
    /// A perspective 3D view with the part models.
    #[serde(rename = "3d")]
    View3D,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct Render {
    #[serde(default)]
    pub view: RenderView,
    /// The layers for the `layers` view, for example
    /// `["top", "top.silk", "outline", "ratsnest"]`. Other valid names are `bottom`,
    /// `in1`, `*.mask`, `*.paste`, `*.courtyard`, `*.fab`, `drills`, `keepout` and `drc`.
    #[serde(default)]
    pub layers: Option<Vec<String>>,
    /// In 2D views, zoom to this box (mm).
    #[serde(default)]
    pub region: Option<Rect>,
    /// Image width in pixels (default 1400).
    #[serde(default)]
    pub width: Option<u32>,
    /// Image height in pixels (default 1000).
    #[serde(default)]
    pub height: Option<u32>,
    /// 3D camera angle around the board in degrees (default -62).
    #[serde(default)]
    pub yaw: Option<f32>,
    /// 3D camera elevation in degrees (default 38). At 90, the camera looks straight down.
    #[serde(default)]
    pub pitch: Option<f32>,
    /// The file name in cache/render. The default is the name of the view.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Rendered {
    pub path: String,
    pub width: u32,
    pub height: u32,
    #[serde(skip)]
    pub png: Vec<u8>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

pub fn parse_scene_layer(s: &str, copper_layers: u8) -> Option<SceneLayer> {
    match s {
        "outline" => return Some(SceneLayer::Outline),
        "drills" => return Some(SceneLayer::Drills),
        "ratsnest" => return Some(SceneLayer::Ratsnest),
        "drc" => return Some(SceneLayer::Drc),
        "keepout" => return Some(SceneLayer::Keepout),
        _ => {}
    }
    let l: Layer = s.parse().ok()?;
    Some(match l {
        Layer::Copper(c) => {
            if !c.exists_in(copper_layers) {
                return None;
            }
            SceneLayer::Copper(c.index(copper_layers))
        }
        Layer::Mask(sd) => SceneLayer::Mask(sd),
        Layer::Paste(sd) => SceneLayer::Paste(sd),
        Layer::Silk(sd) => SceneLayer::Silk(sd),
        Layer::Courtyard(sd) => SceneLayer::Courtyard(sd),
        Layer::Fab(sd) => SceneLayer::Fab(sd),
        Layer::Outline => SceneLayer::Outline,
    })
}

impl Project {
    pub fn render(&self, p: Render) -> Result<Rendered> {
        let w = p.width.unwrap_or(1400).clamp(64, 4096);
        let h = p.height.unwrap_or(1000).clamp(64, 4096);
        let (_, s2, s3) = lamina_scene::build(&self.data, self.root());
        let layers = match &p.layers {
            None => None,
            Some(names) => {
                let mut v = Vec::new();
                for n in names {
                    v.push(
                        parse_scene_layer(n, self.data.board.stackup.copper_layers).ok_or_else(
                            || Error::invalid(format!("The layer `{n}` is unknown.")),
                        )?,
                    );
                }
                Some(v)
            }
        };
        let kind = match p.view {
            RenderView::Layers => Snapshot::Layers,
            RenderView::Top => Snapshot::Side(Side::Top),
            RenderView::Bottom => Snapshot::Side(Side::Bottom),
            RenderView::View3D => Snapshot::View3D {
                yaw: p.yaw.unwrap_or(-62.0),
                pitch: p.pitch.unwrap_or(38.0).clamp(-89.9, 90.0),
            },
        };
        let opts = Options2D {
            width: w,
            height: h,
            layers,
            region: p.region,
        };
        let (png, problems) =
            lamina_render::snapshot(&s2, &s3, kind, w, h, Some(&opts)).map_err(Error::Invalid)?;
        let name = p.name.unwrap_or_else(|| {
            match p.view {
                RenderView::Layers => "layers",
                RenderView::Top => "top",
                RenderView::Bottom => "bottom",
                RenderView::View3D => "3d",
            }
            .to_string()
        });
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let path = self.root().join(RENDER_DIR).join(format!("{safe}.png"));
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| Error::io(&path, e))?;
        std::fs::write(&path, &png).map_err(|e| Error::io(&path, e))?;
        let mut notes = problems;
        if p.view == RenderView::View3D {
            let missing: Vec<&str> = s3
                .instances
                .iter()
                .filter(|i| i.model.is_none() && i.placeholder.is_some())
                .map(|i| i.refdes.as_str())
                .collect();
            if !missing.is_empty() {
                notes.push(format!(
                    "These parts have no 3D model, so they are shown as boxes: {}",
                    missing.join(" ")
                ));
            }
        }
        Ok(Rendered {
            path: path.display().to_string(),
            width: w,
            height: h,
            png,
            notes,
        })
    }
}
