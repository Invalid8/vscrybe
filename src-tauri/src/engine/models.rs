use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::thread;

use serde::Serialize;

use super::paths;

pub struct Model {
    pub name: &'static str,
    pub label: &'static str,
    files: &'static [(&'static str, &'static str)],
}

pub const DEFAULT: &str = "small";

pub const MODELS: &[Model] = &[
    Model {
        name: "small",
        label: "Fast",
        files: &[
            ("Systran/faster-whisper-small", "config.json"),
            ("Systran/faster-whisper-small", "tokenizer.json"),
            ("Systran/faster-whisper-small", "vocabulary.txt"),
            ("openai/whisper-small", "preprocessor_config.json"),
            ("Systran/faster-whisper-small", "model.bin"),
        ],
    },
    Model {
        name: "large-v3-turbo",
        label: "Accurate",
        files: &[
            ("dropbox-dash/faster-whisper-large-v3-turbo", "config.json"),
            ("dropbox-dash/faster-whisper-large-v3-turbo", "tokenizer.json"),
            ("dropbox-dash/faster-whisper-large-v3-turbo", "vocabulary.json"),
            ("dropbox-dash/faster-whisper-large-v3-turbo", "preprocessor_config.json"),
            ("dropbox-dash/faster-whisper-large-v3-turbo", "model.bin"),
        ],
    },
];

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub name: &'static str,
    pub label: &'static str,
    pub ready: bool,
    pub downloading: bool,
    pub error: Option<String>,
    pub progress: f64,
    pub total_mb: Option<u64>,
}

#[derive(Default)]
struct Download {
    total: u64,
    done: u64,
    error: Option<String>,
}

static DOWNLOADS: Mutex<Option<HashMap<&'static str, Download>>> = Mutex::new(None);
static ENSURE: Mutex<()> = Mutex::new(());

fn downloads() -> MutexGuard<'static, Option<HashMap<&'static str, Download>>> {
    DOWNLOADS.lock().unwrap_or_else(|e| e.into_inner())
}

fn update(name: &'static str, change: impl FnOnce(&mut Download)) {
    change(downloads().get_or_insert_default().entry(name).or_default());
}

pub fn find(name: &str) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.name == name)
}

pub fn label(name: &str) -> &'static str {
    find(name).map_or("", |m| m.label)
}

pub fn folder(name: &str) -> PathBuf {
    paths::cache_dir().join("models").join(name)
}

pub fn is_ready(name: &str) -> bool {
    find(name).is_some_and(|m| m.files.iter().all(|(_, file)| folder(name).join(file).is_file()))
}

pub fn status(name: &str) -> Status {
    let model = find(name).expect("a known model");
    let downloads = downloads();
    let download = downloads.as_ref().and_then(|d| d.get(model.name));
    Status {
        name: model.name,
        label: model.label,
        ready: is_ready(name),
        downloading: download.is_some_and(|d| d.error.is_none()),
        error: download.and_then(|d| d.error.clone()),
        progress: download.filter(|d| d.total > 0).map_or(0.0, |d| (d.done as f64 / d.total as f64).min(1.0)),
        total_mb: download.filter(|d| d.total > 0).map(|d| (d.total as f64 / 1e6).round() as u64),
    }
}

pub fn pending() -> Vec<Status> {
    let names: Vec<&str> = downloads().as_ref().map(|d| d.keys().copied().collect()).unwrap_or_default();
    MODELS.iter().filter(|m| names.contains(&m.name)).map(|m| status(m.name)).collect()
}

pub fn ensure(name: &str) -> Result<PathBuf, String> {
    let model = find(name).ok_or_else(|| format!("Unknown model “{name}”."))?;
    if is_ready(name) {
        return Ok(folder(name));
    }
    let _single = ENSURE.lock().unwrap_or_else(|e| e.into_inner());
    if is_ready(name) {
        return Ok(folder(name));
    }
    update(model.name, |d| *d = Download::default());
    match fetch(model) {
        Ok(()) => {
            downloads().get_or_insert_default().remove(model.name);
            Ok(folder(name))
        }
        Err(error) => {
            log::warn!("Model download failed for {name}: {error}");
            let message = format!(
                "Couldn't download the {} model. Check your internet connection, then press Retry.",
                model.label
            );
            update(model.name, |d| d.error = Some(message.clone()));
            Err(message)
        }
    }
}

pub fn download_in_background(name: &str) {
    if let Some(model) = find(name) {
        downloads().get_or_insert_default().remove(model.name);
        thread::spawn(move || {
            let _ = ensure(model.name);
        });
    }
}

fn url(repo: &str, file: &str) -> String {
    format!("https://huggingface.co/{repo}/resolve/main/{file}")
}

fn fetch(model: &'static Model) -> Result<(), Box<dyn std::error::Error>> {
    let dir = folder(model.name);
    fs::create_dir_all(&dir)?;
    let client = reqwest::blocking::Client::builder().user_agent("vscribe").timeout(None).build()?;
    let missing: Vec<_> = model.files.iter().filter(|(_, file)| !dir.join(file).is_file()).collect();
    let mut total = 0;
    for (repo, file) in &missing {
        total += client.head(url(repo, file)).send()?.error_for_status()?.content_length().unwrap_or(0);
    }
    update(model.name, |d| d.total = total);
    let mut buffer = vec![0; 1 << 16];
    for (repo, file) in missing {
        let mut response = client.get(url(repo, file)).send()?.error_for_status()?;
        let part = dir.join(format!("{file}.part"));
        let mut out = fs::File::create(&part)?;
        loop {
            let read = response.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            out.write_all(&buffer[..read])?;
            update(model.name, |d| d.done += read as u64);
        }
        out.sync_all()?;
        fs::rename(part, dir.join(file))?;
    }
    Ok(())
}
