//! Layout reuse operations: copy one block's layout onto others, and save
//! or place block layouts as files (see `crate::reuse`).

use std::path::PathBuf;

use lamina_model::error::Result;
use lamina_model::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::project::{OpResult, Project, Touched};
use crate::reuse::{self, Applied, ApplyOpts, Block, LayoutBlock};

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RepeatLayout {
    /// Block whose layout is copied: a group name, or a list of refdes.
    pub from: Block,
    /// Blocks that take the layout, each a group name or a list of refdes.
    /// Each must have the same footprints wired the same way as `from`.
    pub to: Vec<Block>,
    /// A part that stays where it is. In `from`: its counterpart stays put
    /// in every target. In a target: that part stays. Default: the largest
    /// part of `from`.
    #[serde(default)]
    pub anchor: Option<String>,
    /// Put the target's anchor part here instead (one target only).
    #[serde(default)]
    pub at: Option<Placement>,
    /// Copy tracks and vias too, replacing the target's old ones (default true).
    #[serde(default = "yes")]
    pub routing: bool,
    /// Also move locked parts.
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SaveLayoutBlock {
    /// The laid-out block: a group name, or a list of refdes.
    pub from: Block,
    /// File name without extension, e.g. `servo_channel`.
    pub name: String,
    /// Part that anchors the layout (default: the largest).
    #[serde(default)]
    pub anchor: Option<String>,
    /// Overwrite an existing layout of that name.
    #[serde(default)]
    pub replace: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PlaceLayoutBlock {
    /// Name of a saved layout (`layouts/<name>.json` in the board or a
    /// workspace library).
    pub name: String,
    /// Blocks that take the layout, each a group name or a list of refdes.
    pub to: Vec<Block>,
    /// A target part that stays where it is (default: the counterpart of
    /// the layout's anchor).
    #[serde(default)]
    pub anchor: Option<String>,
    /// Put the target's anchor part here instead (one target only).
    #[serde(default)]
    pub at: Option<Placement>,
    /// Copy tracks and vias too (default true).
    #[serde(default = "yes")]
    pub routing: bool,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LayoutSaved {
    pub path: String,
    pub anchor: String,
    pub parts: usize,
    pub tracks: usize,
    pub vias: usize,
}

fn summary(what: &str, done: &[(String, Applied)]) -> (String, Vec<String>) {
    let mut lines = Vec::new();
    let mut notes = Vec::new();
    for (label, a) in done {
        let mut s = format!("{label}: {} parts around {}", a.parts, a.anchor);
        if a.tracks + a.vias + a.removed > 0 {
            s.push_str(&format!(", {} tracks, {} vias", a.tracks, a.vias));
            if a.removed > 0 {
                s.push_str(&format!(", {} old pieces removed", a.removed));
            }
        }
        lines.push(s);
        notes.extend(a.notes.iter().map(|n| format!("{label}: {n}")));
    }
    (format!("{what}: {}", lines.join("; ")), notes)
}

impl Project {
    fn block_parts(&self, b: &Block) -> Result<Vec<String>> {
        b.parts(&self.data).map_err(Error::invalid)
    }

    fn commit_layout(
        &mut self,
        d: ProjectData,
        done: Vec<(String, Applied)>,
        routing: bool,
        strict: bool,
        what: &str,
    ) -> Result<OpResult> {
        let mut t = Touched::file(FileKind::Placement);
        if routing {
            t = t.and(FileKind::Routing);
        }
        let (s, notes) = summary(what, &done);
        let mut r = self.commit(d, t, strict, s)?;
        r.notes.extend(notes);
        Ok(r)
    }

    pub fn repeat_layout(&mut self, p: RepeatLayout) -> Result<OpResult> {
        let from = self.block_parts(&p.from)?;
        let to: Vec<Vec<String>> =
            p.to.iter()
                .map(|b| self.block_parts(b))
                .collect::<Result<_>>()?;
        let mut d = self.data.clone();
        let o = ApplyOpts {
            at: p.at,
            routing: p.routing,
            force: p.force,
            ..ApplyOpts::default()
        };
        let done =
            reuse::repeat(&mut d, &from, &to, p.anchor.as_deref(), &o).map_err(Error::invalid)?;
        let done = p.to.iter().map(Block::label).zip(done).collect();
        let what = format!("layout of {} copied", p.from.label());
        self.commit_layout(d, done, p.routing, p.strict, &what)
    }

    /// Where a new saved layout goes: the first workspace library, else the board.
    fn layout_save_dir(&self) -> PathBuf {
        let dirs = layout_dirs(self.root());
        dirs.get(1).unwrap_or(&dirs[0]).clone()
    }

    pub fn save_layout_block(&self, p: SaveLayoutBlock) -> Result<LayoutSaved> {
        if !valid_id(&p.name) {
            return Err(Error::invalid(format!(
                "bad layout name `{}`: letters, digits, . _ - only",
                p.name
            )));
        }
        let parts = self.block_parts(&p.from)?;
        let b = reuse::extract(&self.data, &parts, p.anchor.as_deref(), &p.name)
            .map_err(Error::invalid)?;
        let dir = self.layout_save_dir();
        let path = dir.join(format!("{}.json", p.name));
        if path.exists() && !p.replace {
            return Err(Error::invalid(format!(
                "{} exists; pass replace: true to overwrite it",
                path.display()
            )));
        }
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        let text = json::to_string(&b).map_err(|e| Error::Json {
            path: path.clone(),
            source: e,
        })?;
        write_atomic(&path, text.as_bytes())?;
        Ok(LayoutSaved {
            path: path.display().to_string(),
            anchor: b.anchor.clone(),
            parts: b.parts.len(),
            tracks: b.tracks.len(),
            vias: b.vias.len(),
        })
    }

    /// A saved layout by name, from the board's `layouts/` or a library's.
    pub fn load_layout_block(&self, name: &str) -> Result<LayoutBlock> {
        let dirs = layout_dirs(self.root());
        for d in &dirs {
            let f = d.join(format!("{name}.json"));
            if f.is_file() {
                let text = std::fs::read_to_string(&f).map_err(|e| Error::io(&f, e))?;
                return serde_json::from_str(&text)
                    .map_err(|e| Error::invalid(format!("{}: {e}", f.display())));
            }
        }
        let mut known: Vec<String> = dirs
            .iter()
            .filter_map(|d| std::fs::read_dir(d).ok())
            .flat_map(|rd| rd.flatten())
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n.strip_suffix(".json").map(str::to_string)
            })
            .collect();
        known.sort();
        known.dedup();
        Err(Error::invalid(if known.is_empty() {
            format!("no saved layout `{name}`; save one with save_layout_block")
        } else {
            format!(
                "no saved layout `{name}`; saved layouts: {}",
                known.join(", ")
            )
        }))
    }

    pub fn place_layout_block(&mut self, p: PlaceLayoutBlock) -> Result<OpResult> {
        let mut b = self.load_layout_block(&p.name)?;
        let to: Vec<Vec<String>> =
            p.to.iter()
                .map(|b| self.block_parts(b))
                .collect::<Result<_>>()?;
        if to.is_empty() {
            return Err(Error::invalid("no target block given"));
        }
        if p.at.is_some() && to.len() > 1 {
            return Err(Error::invalid("`at` needs exactly one target"));
        }
        if let Some(a) = &p.anchor {
            let t = to
                .iter()
                .find(|t| t.contains(a))
                .ok_or_else(|| Error::invalid(format!("anchor {a} is not in a target block")))?;
            let m = reuse::match_block(&self.data, &b, t).map_err(Error::invalid)?;
            let local = m
                .parts
                .iter()
                .find(|(_, r)| *r == a)
                .map(|(k, _)| k.clone())
                .ok_or_else(|| Error::invalid("anchor not paired"))?;
            b = b.rebase(&local).map_err(Error::invalid)?;
        }
        let o = ApplyOpts {
            at: p.at,
            routing: p.routing,
            force: p.force,
            ..ApplyOpts::default()
        };
        let mut d = self.data.clone();
        let mut done = Vec::new();
        for (blk, t) in p.to.iter().zip(&to) {
            let a = reuse::apply(&mut d, &b, t, &o).map_err(Error::invalid)?;
            done.push((blk.label(), a));
        }
        let what = format!("layout {} placed", p.name);
        self.commit_layout(d, done, p.routing, p.strict, &what)
    }
}
