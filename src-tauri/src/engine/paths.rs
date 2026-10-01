use std::fs;
use std::path::{Path, PathBuf};

const APP: &str = "vscribe";
const OLD_APP: &str = "vn-transcribe";

fn moved(base: Option<PathBuf>) -> PathBuf {
    let base = base.expect("a home directory");
    let current = base.join(APP);
    let old = base.join(OLD_APP);
    if !current.exists() && old.is_dir() {
        if let Err(error) = fs::rename(&old, &current) {
            log::warn!("Couldn't move {} to {}: {error}", old.display(), current.display());
        }
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

pub fn tilde(path: &Path) -> String {
    let home = dirs::home_dir().unwrap_or_default();
    match path.strip_prefix(&home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}
