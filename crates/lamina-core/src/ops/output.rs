use lamina_model::error::Result;
use lamina_model::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::project::Project;

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct Export {
    /// Output folder, relative to the project (default `output`).
    #[serde(default)]
    pub dir: Option<String>,
    /// Export even when DRC reports errors (default false).
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Exported {
    pub files: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct ListNotes {
    /// Include resolved notes.
    #[serde(default)]
    pub all: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AddNote {
    pub text: String,
    #[serde(default)]
    pub at: Option<Point>,
    #[serde(default)]
    pub refdes: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ResolveNotes {
    pub ids: Vec<String>,
}

impl Project {
    pub fn export(&self, p: Export) -> Result<Exported> {
        let errors: Vec<Violation> = self
            .check()
            .into_iter()
            .filter(|v| v.severity == Severity::Error)
            .collect();
        if !errors.is_empty() && !p.force {
            let list: Vec<String> = errors.iter().take(8).map(|v| v.message.clone()).collect();
            return Err(Error::Refused(format!(
                "{} DRC error(s); fix them or pass force: true. First: {}",
                errors.len(),
                list.join("; ")
            )));
        }
        let (a, s2, _) = lamina_scene::build(&self.data, self.root());
        let name: String = self
            .data
            .board
            .name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let mut fab = lamina_export::gerbers(&s2, &name);
        fab.extend(lamina_export::drills(&a.geometry, &name));
        let (asm, mut warnings) = lamina_export::assembly(&self.data, &name);
        let rel = p.dir.unwrap_or_else(|| "output".into());
        if rel.contains("..") {
            return Err(Error::invalid("dir must stay inside the project"));
        }
        let dir = self.root().join(rel);
        let files = lamina_export::write(&dir, &name, &fab, &asm).map_err(Error::Invalid)?;
        if !errors.is_empty() {
            warnings.insert(0, format!("exported with {} DRC error(s)", errors.len()));
        }
        let unrouted = a.ratsnest.len();
        if unrouted > 0 {
            warnings.push(format!("{unrouted} connection(s) are still unrouted"));
        }
        Ok(Exported {
            files: files.iter().map(|f| f.display().to_string()).collect(),
            warnings,
        })
    }

    fn notes_from_disk(&self) -> NotesFile {
        std::fs::read_to_string(self.root().join("notes.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn save_notes(&mut self, n: NotesFile) -> Result<()> {
        self.data.notes = n;
        self.data.notes.schema = SCHEMA;
        self.data.save(self.root(), FileKind::Notes)
    }

    pub fn list_notes(&mut self, p: ListNotes) -> Vec<Note> {
        let n = self.notes_from_disk();
        self.data.notes = n.clone();
        n.notes
            .into_iter()
            .filter(|x| p.all || !x.resolved)
            .collect()
    }

    pub fn add_note(&mut self, p: AddNote) -> Result<Note> {
        if p.text.trim().is_empty() {
            return Err(Error::invalid("text is empty"));
        }
        let mut n = self.notes_from_disk();
        let next = n
            .notes
            .iter()
            .filter_map(|x| x.id.strip_prefix('n')?.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        let note = Note {
            id: format!("n{next}"),
            text: p.text,
            at: p.at,
            refdes: p.refdes,
            author: "agent".into(),
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            resolved: false,
        };
        n.notes.push(note.clone());
        self.save_notes(n)?;
        Ok(note)
    }

    pub fn resolve_notes(&mut self, p: ResolveNotes) -> Result<usize> {
        let mut n = self.notes_from_disk();
        let mut k = 0;
        for x in n.notes.iter_mut() {
            if p.ids.contains(&x.id) && !x.resolved {
                x.resolved = true;
                k += 1;
            }
        }
        self.save_notes(n)?;
        Ok(k)
    }
}
