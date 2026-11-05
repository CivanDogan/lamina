//! Specctra DSN out and SES in, for external autorouters (Freerouting).

use std::path::{Path, PathBuf};

use lamina_model::error::Result;
use lamina_model::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::project::{OpResult, Project, Touched, next_id};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ExportSpecctra {
    /// Output file (default `output/<board name>.dsn` in the project).
    #[serde(default)]
    pub path: Option<String>,
    /// Leave existing tracks and vias out so the router starts from scratch
    /// (default: they are exported as protected wiring).
    #[serde(default)]
    pub reroute_all: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SpecctraExported {
    pub path: String,
    pub nets: usize,
    pub parts: usize,
    /// How to route it with Freerouting.
    pub hint: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ImportSpecctraSession {
    /// Path of the `.ses` file the router wrote.
    pub path: String,
    /// Replace the existing tracks and vias of the routed nets (default
    /// true); otherwise the session's routes are added to them.
    #[serde(default = "yes")]
    pub replace: bool,
    #[serde(default)]
    pub strict: bool,
}

fn yes() -> bool {
    true
}

impl Project {
    pub fn export_specctra(&self, p: ExportSpecctra) -> Result<SpecctraExported> {
        let path = match p.path {
            Some(s) => PathBuf::from(s),
            None => self
                .root()
                .join("output")
                .join(format!("{}.dsn", self.data.board.name)),
        };
        if self.data.board.outline.is_none() {
            return Err(Error::invalid(
                "set the board outline before exporting a DSN",
            ));
        }
        let text = lamina_specctra::export_dsn(&self.data, &self.data.board.name, p.reroute_all);
        write_atomic(&path, text.as_bytes())?;
        Ok(SpecctraExported {
            path: path.display().to_string(),
            nets: self.data.nets.nets.len(),
            parts: self.data.placement.placements.len(),
            hint: "route it with Freerouting (java -jar freerouting.jar -de <file.dsn> -do <file.ses>), \
                   then import_specctra_session with the .ses"
                .into(),
        })
    }

    pub fn import_specctra_session(&mut self, p: ImportSpecctraSession) -> Result<OpResult> {
        let path = Path::new(&p.path);
        let text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        let session = lamina_specctra::import_ses(&text, &self.data).map_err(Error::Invalid)?;
        let mut d = self.data.clone();
        let mut removed = 0;
        if p.replace {
            let before = d.routing.tracks.len() + d.routing.vias.len();
            d.routing.tracks.retain(|t| !session.nets.contains(&t.net));
            d.routing.vias.retain(|v| !session.nets.contains(&v.net));
            removed = before - d.routing.tracks.len() - d.routing.vias.len();
        }
        let mut ids: Vec<String> = d
            .routing
            .tracks
            .iter()
            .map(|t| t.id.clone())
            .chain(d.routing.vias.iter().map(|v| v.id.clone()))
            .collect();
        let (nt, nv) = (session.tracks.len(), session.vias.len());
        let min_w = d.board.rules.min_track_width;
        let mut widened = 0;
        for mut t in session.tracks {
            // Routers hand back widths a rounding step under the minimum.
            if t.width < min_w && t.width >= min_w * 0.9 {
                t.width = min_w;
                widened += 1;
            }
            t.id = next_id("t", ids.iter().map(String::as_str));
            ids.push(t.id.clone());
            d.routing.tracks.push(t);
        }
        for mut v in session.vias {
            v.id = next_id("v", ids.iter().map(String::as_str));
            ids.push(v.id.clone());
            d.routing.vias.push(v);
        }
        let mut res = self.commit(
            d,
            Touched::file(FileKind::Routing),
            p.strict,
            format!(
                "session routed {} net(s): {nt} track(s) and {nv} via(s) added, {removed} replaced",
                session.nets.len()
            ),
        )?;
        res.notes = session.notes;
        if widened > 0 {
            res.notes.push(format!(
                "{widened} track(s) came back just under the minimum width and were widened to {min_w} mm"
            ));
        }
        Ok(res)
    }
}
