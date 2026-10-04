use std::fs;
use std::path::{Path, PathBuf};

use crate::ID as APP;

const OLD_APPS: [&str; 2] = ["vscribe", "vn-transcribe"];

fn moved(base: Option<PathBuf>) -> PathBuf {
    let base = base.expect("a home directory");
    let current = base.join(APP);
    if !current.exists()
        && let Some(old) = OLD_APPS.iter().map(|name| base.join(name)).find(|old| old.is_dir())
        && let Err(error) = fs::rename(&old, &current)
    {
        log::warn!("Couldn't move {} to {}: {error}", old.display(), current.display());
    }
    current
}

pub fn data_dir() -> PathBuf {
    moved(dirs::data_dir())
}

pub fn cache_dir() -> PathBuf {
    moved(dirs::cache_dir())
}

pub fn log_dir() -> PathBuf {
    dirs::state_dir().or_else(dirs::cache_dir).expect("a state directory").join(APP).join("log")
}

pub fn log_file() -> PathBuf {
    log_dir().join(format!("{APP}.log"))
}

pub fn tilde(path: &Path) -> String {
    let home = dirs::home_dir().unwrap_or_default();
    match path.strip_prefix(&home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}
