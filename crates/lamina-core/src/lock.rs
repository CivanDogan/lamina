//! `.lamina.lock` keeps two engines from writing the same project.

use std::fs;
use std::path::{Path, PathBuf};

use lamina_model::error::{Error, Result};

pub const LOCK_FILE: &str = ".lamina.lock";

#[derive(Debug)]
pub struct Lock {
    path: PathBuf,
}

fn alive(pid: u32) -> bool {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let Ok(mut sys) = System::new() else {
        return false;
    };
    let p = Pid::from_u32(pid);
    sys.refresh_processes(ProcessesToUpdate::Some(&[p]), true);
    sys.process(p).is_some()
}

impl Lock {
    pub fn acquire(dir: &Path) -> Result<Lock> {
        let path = dir.join(LOCK_FILE);
        let me = std::process::id();
        if let Ok(text) = fs::read_to_string(&path) {
            if let Ok(pid) = text.trim().parse::<u32>() {
                if pid != me && alive(pid) {
                    return Err(Error::Locked(pid));
                }
            }
        }
        fs::write(&path, me.to_string()).map_err(|e| Error::io(&path, e))?;
        Ok(Lock { path })
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        if let Ok(text) = fs::read_to_string(&self.path) {
            if text.trim() == std::process::id().to_string() {
                let _ = fs::remove_file(&self.path);
            }
        }
    }
}
