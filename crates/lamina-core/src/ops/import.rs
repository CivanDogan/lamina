//! Open a board file from another program: a KiCad board, an EasyEDA board
//! or a set of Gerbers is imported into a Lamina project folder.

use std::path::{Path, PathBuf};

use lamina_model::error::Result;
use lamina_model::*;

use crate::ops::{ImportEasyedaBoard, ImportGerbers, ImportKicadBoard};
use crate::project::{OpResult, Project};

/// Kinds of board files that can be opened directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardFile {
    Kicad,
    Easyeda,
    Gerbers,
}

const GERBER_EXT: &[&str] = &[
    "gbr", "ger", "gtl", "gbl", "gto", "gbo", "gts", "gbs", "gtp", "gbp", "gko", "gm1", "drl",
    "xln", "exc", "art", "pho",
];

fn ext(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// What `p` is, if it can be opened: a `.kicad_pcb`, an EasyEDA `.json`,
/// a Gerber zip or a folder (or file) of Gerbers.
pub fn board_file_kind(p: &Path) -> Option<BoardFile> {
    if p.is_dir() {
        let has_gerbers = std::fs::read_dir(p)
            .ok()?
            .flatten()
            .any(|e| GERBER_EXT.contains(&ext(&e.path()).as_str()));
        return has_gerbers.then_some(BoardFile::Gerbers);
    }
    match ext(p).as_str() {
        "kicad_pcb" => Some(BoardFile::Kicad),
        "json" if !ProjectData::is_project(p.parent()?) => Some(BoardFile::Easyeda),
        "zip" => Some(BoardFile::Gerbers),
        e if GERBER_EXT.contains(&e) => Some(BoardFile::Gerbers),
        _ => None,
    }
}

/// 64-bit FNV-1a: stable across builds, unlike the std hasher.
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir)
        .join("lamina")
}

/// The project folder an opened board file is imported into: one per
/// source path, under the user's local data folder.
pub fn import_dir_for(file: &Path) -> PathBuf {
    let abs = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
    // A single Gerber stands for its whole folder.
    let src = if !abs.is_dir()
        && matches!(board_file_kind(&abs), Some(BoardFile::Gerbers))
        && ext(&abs) != "zip"
    {
        abs.parent().map(Path::to_path_buf).unwrap_or(abs)
    } else {
        abs
    };
    let stem: String = src
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("board")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let key = src.to_string_lossy().to_lowercase();
    data_dir()
        .join("imports")
        .join(format!("{stem}-{:08x}", fnv(&key) as u32))
}

fn modified(p: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Import `file` into `dir`, replacing a board already there.
pub fn import_board_file(file: &Path, dir: &Path) -> Result<OpResult> {
    let kind = board_file_kind(file).ok_or_else(|| {
        Error::invalid(format!(
            "{} is not a board file Lamina can open (KiCad .kicad_pcb, EasyEDA .json, Gerbers)",
            file.display()
        ))
    })?;
    let name = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("board")
        .to_string();
    let mut p = if ProjectData::is_project(dir) {
        Project::open(dir)?
    } else {
        Project::create(dir, &name)?
    };
    let path = if kind == BoardFile::Gerbers && !file.is_dir() && ext(file) != "zip" {
        file.parent().unwrap_or(file).to_string_lossy().into_owned()
    } else {
        file.to_string_lossy().into_owned()
    };
    match kind {
        BoardFile::Kicad => p.import_kicad_board(ImportKicadBoard {
            path,
            models: true,
            online: true,
            replace: true,
            strict: false,
        }),
        BoardFile::Easyeda => p.import_easyeda_board(ImportEasyedaBoard {
            path,
            models: true,
            replace: true,
            strict: false,
        }),
        BoardFile::Gerbers => p.import_gerbers(ImportGerbers {
            path,
            replace: true,
        }),
    }
}

/// The Lamina project for `path`: the folder itself when it is one, else
/// the import folder of a board file, imported again when the source is
/// newer than the import. Returns the folder and the import result when an
/// import ran.
pub fn open_board_path(path: &Path) -> Result<(PathBuf, Option<OpResult>)> {
    if ProjectData::is_project(path) {
        return Ok((path.to_path_buf(), None));
    }
    if let Some(dir) = path.parent().filter(|d| ProjectData::is_project(d)) {
        if ext(path) == "json" {
            return Ok((dir.to_path_buf(), None));
        }
    }
    let dir = import_dir_for(path);
    let stamp = dir.join(".imported");
    let fresh = match (modified(path), modified(&stamp)) {
        (Some(src), Some(done)) => done >= src && ProjectData::is_project(&dir),
        _ => false,
    };
    if fresh {
        return Ok((dir, None));
    }
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
    let r = import_board_file(path, &dir)?;
    let _ = std::fs::write(&stamp, path.display().to_string());
    Ok((dir, Some(r)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_import_dirs() {
        let t = tempfile::tempdir().unwrap();
        let k = t.path().join("b.kicad_pcb");
        std::fs::write(&k, "").unwrap();
        assert_eq!(board_file_kind(&k), Some(BoardFile::Kicad));
        let g = t.path().join("gerbers");
        std::fs::create_dir_all(&g).unwrap();
        assert_eq!(board_file_kind(&g), None);
        std::fs::write(g.join("b.GTL"), "").unwrap();
        assert_eq!(board_file_kind(&g), Some(BoardFile::Gerbers));
        assert_eq!(board_file_kind(&g.join("b.GTL")), Some(BoardFile::Gerbers));
        // A single Gerber maps to its folder's import.
        assert_eq!(import_dir_for(&g.join("b.GTL")), import_dir_for(&g));
        assert_ne!(import_dir_for(&k), import_dir_for(&g));
        assert_eq!(board_file_kind(&t.path().join("x.txt")), None);
        assert_eq!(fnv("a"), 0xaf63_dc4c_8601_ec8c);
    }
}
