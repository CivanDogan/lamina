//! An open project and the commit step every operation goes through.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use lamina_model::error::Result;
use lamina_model::*;
use schemars::JsonSchema;
use serde::Serialize;

use crate::lock::Lock;

/// What an operation did. Returned as JSON to the agent.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct OpResult {
    pub summary: String,
    /// Files written.
    pub changed: Vec<String>,
    /// Violations this change introduced.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub new_violations: Vec<Violation>,
    /// Violations this change fixed (rule and items only).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fixed: Vec<String>,
    /// Total errors and warnings now on the board.
    pub errors: usize,
    pub warnings: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Files touched by a change.
#[derive(Debug, Default, Clone)]
pub struct Touched {
    pub files: BTreeSet<FileKind>,
    pub footprints: BTreeSet<String>,
    pub removed_footprints: BTreeSet<String>,
}

impl Touched {
    pub fn file(k: FileKind) -> Self {
        let mut t = Touched::default();
        t.files.insert(k);
        t
    }

    pub fn and(mut self, k: FileKind) -> Self {
        self.files.insert(k);
        self
    }

    pub fn footprint(mut self, id: impl Into<String>) -> Self {
        self.footprints.insert(id.into());
        self
    }
}

pub type Checker = fn(&ProjectData) -> Vec<Violation>;

pub struct Project {
    root: PathBuf,
    pub data: ProjectData,
    checkers: Vec<Checker>,
    source: std::sync::Arc<dyn lamina_parts::PartSource>,
    cached: Option<Vec<Violation>>,
    /// Size and modification time of every project file at the last
    /// load or save, to notice edits made by someone else.
    stamp: Vec<(String, u64, std::time::SystemTime)>,
    _lock: Option<Lock>,
}

impl std::fmt::Debug for Project {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Project").field("root", &self.root).finish()
    }
}

const PROJECT_GITIGNORE: &str = "cache/render/\n.lamina.lock\n*.tmp\n";

impl Project {
    /// Create a new project in `dir`, which must not contain a project.
    pub fn create(dir: &Path, name: &str) -> Result<Project> {
        if ProjectData::is_project(dir) {
            return Err(Error::invalid(format!(
                "{} already contains a project",
                dir.display()
            )));
        }
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        let lock = Lock::acquire(dir)?;
        let data = ProjectData::new(name);
        data.save_all(dir)?;
        for sub in [FOOTPRINT_DIR, MODEL_DIR, PARTS_DIR] {
            let p = dir.join(sub);
            std::fs::create_dir_all(&p).map_err(|e| Error::io(&p, e))?;
        }
        let gi = dir.join(".gitignore");
        if !gi.exists() {
            write_atomic(&gi, PROJECT_GITIGNORE.as_bytes())?;
        }
        Ok(Project {
            root: dir.to_path_buf(),
            data,
            checkers: Vec::new(),
            source: std::sync::Arc::new(lamina_parts::HttpSource::default()),
            cached: None,
            stamp: disk_stamp(dir),
            _lock: Some(lock),
        })
    }

    /// Open a project and take its lock, so no second engine writes it.
    pub fn open(dir: &Path) -> Result<Project> {
        let lock = Lock::acquire(dir)?;
        let mut p = Project::open_shared(dir)?;
        p._lock = Some(lock);
        Ok(p)
    }

    /// Open without taking the lock. The viewer edits this way next to an
    /// agent; both sides call [`Project::sync`] before each change.
    pub fn open_shared(dir: &Path) -> Result<Project> {
        let stamp = disk_stamp(dir);
        let data = ProjectData::load(dir)?;
        Ok(Project {
            root: dir.to_path_buf(),
            data,
            checkers: Vec::new(),
            source: std::sync::Arc::new(lamina_parts::HttpSource::default()),
            cached: None,
            stamp,
            _lock: None,
        })
    }

    /// Reload when the files changed on disk since the last load or save.
    /// Returns whether anything was reloaded.
    pub fn sync(&mut self) -> Result<bool> {
        let now = disk_stamp(&self.root);
        if now == self.stamp {
            return Ok(false);
        }
        self.data = ProjectData::load(&self.root)?;
        self.cached = None;
        self.stamp = now;
        Ok(true)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where part searches and imports go (HTTP by default).
    pub fn source(&self) -> &dyn lamina_parts::PartSource {
        self.source.as_ref()
    }

    pub fn set_source(&mut self, s: std::sync::Arc<dyn lamina_parts::PartSource>) {
        self.source = s;
    }

    /// Add a checker that runs after every change (DRC registers here).
    pub fn add_checker(&mut self, c: Checker) {
        self.cached = None;
        self.checkers.push(c);
    }

    /// All current violations.
    pub fn check(&self) -> Vec<Violation> {
        self.check_data(&self.data)
    }

    fn check_data(&self, d: &ProjectData) -> Vec<Violation> {
        let mut v = crate::validate::validate(d);
        v.extend(lamina_drc::check(d));
        for c in &self.checkers {
            v.extend(c(d));
        }
        v
    }

    /// Violations of the current state, cached between changes.
    fn current_violations(&mut self) -> Vec<Violation> {
        if let Some(v) = &self.cached {
            return v.clone();
        }
        let v = self.check_data(&self.data);
        self.cached = Some(v.clone());
        v
    }

    /// Re-read everything from disk (for example after the user edited files).
    pub fn reload(&mut self) -> Result<()> {
        self.stamp = disk_stamp(&self.root);
        self.data = ProjectData::load(&self.root)?;
        self.cached = None;
        Ok(())
    }

    /// Apply `new` as the project state, write the touched files and
    /// report. With `strict`, refuse when the change adds an error.
    pub fn commit(
        &mut self,
        new: ProjectData,
        touched: Touched,
        strict: bool,
        summary: impl Into<String>,
    ) -> Result<OpResult> {
        let before = self.current_violations();
        let after = self.check_data(&new);
        let before_keys: BTreeSet<String> = before.iter().map(|v| v.key()).collect();
        let after_keys: BTreeSet<String> = after.iter().map(|v| v.key()).collect();
        let new_violations: Vec<Violation> = after
            .iter()
            .filter(|v| !before_keys.contains(&v.key()))
            .cloned()
            .collect();
        if strict {
            let errs: Vec<&Violation> = new_violations
                .iter()
                .filter(|v| v.severity == Severity::Error)
                .collect();
            if !errs.is_empty() {
                let list: Vec<String> = errs
                    .iter()
                    .take(10)
                    .map(|v| format!("{}: {}", v.rule, v.message))
                    .collect();
                return Err(Error::Refused(format!(
                    "strict: change would add {} error(s): {}",
                    errs.len(),
                    list.join("; ")
                )));
            }
        }
        let fixed: Vec<String> = before
            .iter()
            .filter(|v| !after_keys.contains(&v.key()))
            .map(|v| format!("{}: {}", v.rule, v.message))
            .collect();

        for id in &touched.removed_footprints {
            ProjectData::remove_footprint_file(&self.root, id)?;
        }
        self.data = new;
        let mut changed = Vec::new();
        for id in &touched.footprints {
            self.data.save_footprint(&self.root, id)?;
            changed.push(format!("{FOOTPRINT_DIR}/{id}.json"));
        }
        for k in &touched.files {
            self.data.save(&self.root, *k)?;
            changed.push(k.file_name().to_string());
        }
        self.stamp = disk_stamp(&self.root);
        let errors = after
            .iter()
            .filter(|v| v.severity == Severity::Error)
            .count();
        let warnings = after.len() - errors;
        self.cached = Some(after);
        Ok(OpResult {
            summary: summary.into(),
            changed,
            new_violations,
            fixed,
            errors,
            warnings,
            notes: Vec::new(),
        })
    }

    /// Make `new` the project state, writing only the files that differ.
    pub fn apply(&mut self, new: ProjectData, summary: impl Into<String>) -> Result<OpResult> {
        let touched = touched_between(&self.data, &new);
        self.commit(new, touched, false, summary)
    }
}

/// Files that differ between two states.
pub fn touched_between(a: &ProjectData, b: &ProjectData) -> Touched {
    let mut t = Touched::default();
    if a.board != b.board {
        t.files.insert(FileKind::Board);
    }
    if a.parts != b.parts {
        t.files.insert(FileKind::Parts);
    }
    if a.nets != b.nets {
        t.files.insert(FileKind::Nets);
    }
    if a.placement != b.placement {
        t.files.insert(FileKind::Placement);
    }
    if a.routing != b.routing {
        t.files.insert(FileKind::Routing);
    }
    if a.notes != b.notes {
        t.files.insert(FileKind::Notes);
    }
    for (id, fp) in &b.footprints {
        if a.footprints.get(id) != Some(fp) {
            t.footprints.insert(id.clone());
        }
    }
    for id in a.footprints.keys() {
        if !b.footprints.contains_key(id) {
            t.removed_footprints.insert(id.clone());
        }
    }
    t
}

fn disk_stamp(root: &Path) -> Vec<(String, u64, std::time::SystemTime)> {
    let mut out = Vec::new();
    let mut add = |name: String, path: &Path| {
        if let Ok(m) = std::fs::metadata(path) {
            out.push((name, m.len(), m.modified().unwrap_or(std::time::UNIX_EPOCH)));
        }
    };
    for k in FileKind::ALL {
        add(k.file_name().to_string(), &root.join(k.file_name()));
    }
    if let Ok(rd) = std::fs::read_dir(root.join(FOOTPRINT_DIR)) {
        let mut files: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        files.sort();
        for f in files {
            if f.extension().is_some_and(|x| x == "json") {
                add(f.display().to_string(), &f);
            }
        }
    }
    out
}

/// Next free id like `t12` given existing ids with the same prefix.
pub fn next_id<'a>(prefix: &str, existing: impl IntoIterator<Item = &'a str>) -> String {
    let max = existing
        .into_iter()
        .filter_map(|s| s.strip_prefix(prefix)?.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    format!("{prefix}{}", max + 1)
}
