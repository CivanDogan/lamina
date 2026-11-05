//! Loading and saving a project folder.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::files::{BoardFile, NetsFile, NotesFile, PartsFile, PlacementFile, RoutingFile, SCHEMA};
use crate::footprint::{FOOTPRINT_SCHEMA, Footprint, valid_id};
use crate::json;

/// One of the top-level project files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FileKind {
    Board,
    Parts,
    Nets,
    Placement,
    Routing,
    Notes,
}

impl FileKind {
    pub const ALL: [FileKind; 6] = [
        FileKind::Board,
        FileKind::Parts,
        FileKind::Nets,
        FileKind::Placement,
        FileKind::Routing,
        FileKind::Notes,
    ];

    pub fn file_name(self) -> &'static str {
        match self {
            FileKind::Board => "board.json",
            FileKind::Parts => "parts.json",
            FileKind::Nets => "nets.json",
            FileKind::Placement => "placement.json",
            FileKind::Routing => "routing.json",
            FileKind::Notes => "notes.json",
        }
    }

    pub fn from_file_name(name: &str) -> Option<FileKind> {
        Self::ALL.into_iter().find(|k| k.file_name() == name)
    }
}

pub const FOOTPRINT_DIR: &str = "cache/footprints";
pub const MODEL_DIR: &str = "cache/models";
pub const PARTS_DIR: &str = "cache/parts";
pub const RENDER_DIR: &str = "cache/render";

/// Everything stored in a project folder.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectData {
    pub board: BoardFile,
    pub parts: PartsFile,
    pub nets: NetsFile,
    pub placement: PlacementFile,
    pub routing: RoutingFile,
    pub notes: NotesFile,
    /// Footprints keyed by id, from `cache/footprints/`.
    pub footprints: BTreeMap<String, Footprint>,
}

impl ProjectData {
    pub fn new(name: impl Into<String>) -> Self {
        ProjectData {
            board: BoardFile::new(name),
            parts: PartsFile {
                schema: SCHEMA,
                ..Default::default()
            },
            nets: NetsFile {
                schema: SCHEMA,
                ..Default::default()
            },
            placement: PlacementFile {
                schema: SCHEMA,
                ..Default::default()
            },
            routing: RoutingFile {
                schema: SCHEMA,
                ..Default::default()
            },
            notes: NotesFile {
                schema: SCHEMA,
                ..Default::default()
            },
            footprints: BTreeMap::new(),
        }
    }

    /// Whether `dir` looks like a Lamina project.
    pub fn is_project(dir: &Path) -> bool {
        dir.join(FileKind::Board.file_name()).is_file()
    }

    pub fn load(dir: &Path) -> Result<Self> {
        if !Self::is_project(dir) {
            return Err(Error::NotAProject(dir.to_path_buf()));
        }
        let mut footprints = BTreeMap::new();
        let fp_dir = dir.join(FOOTPRINT_DIR);
        if fp_dir.is_dir() {
            let mut entries: Vec<PathBuf> = fs::read_dir(&fp_dir)
                .map_err(|e| Error::io(&fp_dir, e))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .collect();
            entries.sort();
            for path in entries {
                let fp: Footprint = read_json(&path)?;
                check_schema(&path, fp.schema, FOOTPRINT_SCHEMA)?;
                footprints.insert(fp.id.clone(), fp);
            }
        }
        let data = ProjectData {
            board: read_kind(dir, FileKind::Board)?,
            parts: read_kind_or_default(dir, FileKind::Parts)?,
            nets: read_kind_or_default(dir, FileKind::Nets)?,
            placement: read_kind_or_default(dir, FileKind::Placement)?,
            routing: read_kind_or_default(dir, FileKind::Routing)?,
            notes: read_kind_or_default(dir, FileKind::Notes)?,
            footprints,
        };
        check_schema(&dir.join("board.json"), data.board.schema, SCHEMA)?;
        Ok(data)
    }

    /// Reload a single file in place (used by the viewer).
    pub fn reload(&mut self, dir: &Path, kind: FileKind) -> Result<()> {
        match kind {
            FileKind::Board => self.board = read_kind(dir, kind)?,
            FileKind::Parts => self.parts = read_kind_or_default(dir, kind)?,
            FileKind::Nets => self.nets = read_kind_or_default(dir, kind)?,
            FileKind::Placement => self.placement = read_kind_or_default(dir, kind)?,
            FileKind::Routing => self.routing = read_kind_or_default(dir, kind)?,
            FileKind::Notes => self.notes = read_kind_or_default(dir, kind)?,
        }
        Ok(())
    }

    pub fn reload_footprint(&mut self, path: &Path) -> Result<()> {
        if path.is_file() {
            let fp: Footprint = read_json(path)?;
            self.footprints.insert(fp.id.clone(), fp);
        } else if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            self.footprints.remove(stem);
        }
        Ok(())
    }

    pub fn save_all(&self, dir: &Path) -> Result<()> {
        for k in FileKind::ALL {
            self.save(dir, k)?;
        }
        for fp in self.footprints.values() {
            self.save_footprint(dir, &fp.id)?;
        }
        Ok(())
    }

    pub fn save(&self, dir: &Path, kind: FileKind) -> Result<()> {
        let text = match kind {
            FileKind::Board => json::to_string(&self.board),
            FileKind::Parts => json::to_string(&self.parts),
            FileKind::Nets => json::to_string(&self.nets),
            FileKind::Placement => json::to_string(&self.placement),
            FileKind::Routing => json::to_string(&self.routing),
            FileKind::Notes => json::to_string(&self.notes),
        }
        .map_err(|e| Error::Json {
            path: dir.join(kind.file_name()),
            source: e,
        })?;
        write_atomic(&dir.join(kind.file_name()), text.as_bytes())
    }

    pub fn save_footprint(&self, dir: &Path, id: &str) -> Result<()> {
        let fp = self
            .footprints
            .get(id)
            .ok_or_else(|| Error::Invalid(format!("no footprint `{id}`")))?;
        if !valid_id(&fp.id) {
            return Err(Error::Invalid(format!(
                "footprint id `{}` is not file-safe",
                fp.id
            )));
        }
        let path = footprint_path(dir, id);
        let text = json::to_string(fp).map_err(|e| Error::Json {
            path: path.clone(),
            source: e,
        })?;
        write_atomic(&path, text.as_bytes())
    }

    pub fn remove_footprint_file(dir: &Path, id: &str) -> Result<()> {
        let path = footprint_path(dir, id);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::io(&path, e)),
        }
    }
}

pub fn footprint_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(FOOTPRINT_DIR).join(format!("{id}.json"))
}

fn check_schema(path: &Path, found: u32, supported: u32) -> Result<()> {
    if found > supported {
        return Err(Error::Invalid(format!(
            "{} uses schema {found}, this build supports up to {supported}",
            path.display()
        )));
    }
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let text = read_retry(path)?;
    serde_json::from_str(&text).map_err(|e| Error::Json {
        path: path.to_path_buf(),
        source: e,
    })
}

fn read_kind<T: serde::de::DeserializeOwned>(dir: &Path, kind: FileKind) -> Result<T> {
    read_json(&dir.join(kind.file_name()))
}

fn read_kind_or_default<T: serde::de::DeserializeOwned + Default>(
    dir: &Path,
    kind: FileKind,
) -> Result<T> {
    let path = dir.join(kind.file_name());
    if !path.exists() {
        return Ok(T::default());
    }
    read_json(&path)
}

/// Windows can briefly refuse access while another process renames the file.
fn read_retry(path: &Path) -> Result<String> {
    let mut last = None;
    for attempt in 0..20 {
        match fs::read_to_string(path) {
            Ok(s) => return Ok(s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::io(path, e));
            }
            Err(e) => {
                last = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(5 + attempt * 5));
            }
        }
    }
    Err(Error::io(path, last.unwrap()))
}

/// Write to a temporary file in the same folder, then rename over the target,
/// so readers never see a half-written file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    // Skip the write when nothing changed, so watchers stay quiet.
    if fs::read(path).is_ok_and(|existing| existing == bytes) {
        return Ok(());
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    {
        let mut f = fs::File::create(&tmp).map_err(|e| Error::io(&tmp, e))?;
        f.write_all(bytes).map_err(|e| Error::io(&tmp, e))?;
        f.sync_all().map_err(|e| Error::io(&tmp, e))?;
    }
    let mut last = None;
    for attempt in 0..40 {
        match fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(5 + attempt * 5));
            }
        }
    }
    let _ = fs::remove_file(&tmp);
    Err(Error::io(path, last.unwrap()))
}
