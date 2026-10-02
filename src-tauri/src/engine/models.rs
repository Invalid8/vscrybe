use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use serde::{Deserialize, Serialize};

use super::paths;

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub name: String,
    pub label: String,
    pub builtin: bool,
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Source {
    Download { files: Vec<(String, String)> },
    Folder { path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Saved {
    name: String,
    label: String,
    source: Source,
}

pub const DEFAULT: &str = "small";

const REQUIRED: &[&str] = &["config.json", "model.bin", "tokenizer.json"];
const VOCABULARY: &[&str] = &["vocabulary.txt", "vocabulary.json"];
const PREPROCESSOR: &str = "preprocessor_config.json";

fn builtin(name: &str, label: &str, files: &[(&str, &str)]) -> Model {
    Model {
        name: name.into(),
        label: label.into(),
        builtin: true,
        source: Source::Download { files: files.iter().map(|(repo, file)| ((*repo).into(), (*file).into())).collect() },
    }
}

fn builtins() -> Vec<Model> {
    vec![
        builtin(
            "small",
            "Fast",
            &[
                ("Systran/faster-whisper-small", "config.json"),
                ("Systran/faster-whisper-small", "tokenizer.json"),
                ("Systran/faster-whisper-small", "vocabulary.txt"),
                ("openai/whisper-small", "preprocessor_config.json"),
                ("Systran/faster-whisper-small", "model.bin"),
            ],
        ),
        builtin(
            "large-v3-turbo",
            "Accurate",
            &[
                ("dropbox-dash/faster-whisper-large-v3-turbo", "config.json"),
                ("dropbox-dash/faster-whisper-large-v3-turbo", "tokenizer.json"),
                ("dropbox-dash/faster-whisper-large-v3-turbo", "vocabulary.json"),
                ("dropbox-dash/faster-whisper-large-v3-turbo", "preprocessor_config.json"),
                ("dropbox-dash/faster-whisper-large-v3-turbo", "model.bin"),
            ],
        ),
    ]
}

static REGISTRY: Mutex<Option<Vec<Saved>>> = Mutex::new(None);
static REVISION: AtomicU64 = AtomicU64::new(0);

pub fn revision() -> u64 {
    REVISION.load(Ordering::Acquire)
}

pub fn is_builtin(name: &str) -> bool {
    builtins().iter().any(|m| m.name == name)
}

fn registry_file() -> PathBuf {
    paths::data_dir().join("models.json")
}

fn registry() -> MutexGuard<'static, Option<Vec<Saved>>> {
    let mut registry = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    if registry.is_none() {
        *registry = Some(read_registry(&registry_file()));
    }
    registry
}

fn read_registry(file: &Path) -> Vec<Saved> {
    let Ok(text) = fs::read_to_string(file) else { return Vec::new() };
    serde_json::from_str(&text).unwrap_or_else(|error| {
        log::warn!("Ignoring {}: {error}", file.display());
        Vec::new()
    })
}

fn write_registry(models: &[Saved]) -> Result<(), String> {
    let file = registry_file();
    let failed = |e: &dyn std::fmt::Display| format!("Couldn't save {}: {e}", file.display());
    fs::create_dir_all(paths::data_dir()).map_err(|e| failed(&e))?;
    let text = serde_json::to_string_pretty(models).map_err(|e| failed(&e))?;
    fs::write(&file, text).map_err(|e| failed(&e))
}

pub fn all() -> Vec<Model> {
    let saved = registry().clone().unwrap_or_default();
    builtins()
        .into_iter()
        .chain(saved.into_iter().map(|s| Model { name: s.name, label: s.label, builtin: false, source: s.source }))
        .collect()
}

pub fn find(name: &str) -> Option<Model> {
    all().into_iter().find(|m| m.name == name)
}

pub fn label(name: &str) -> String {
    find(name).map_or_else(String::new, |m| m.label)
}

pub fn is_ready(name: &str) -> bool {
    find(name).is_some_and(|m| m.is_ready())
}

impl Model {
    pub fn folder(&self) -> PathBuf {
        match &self.source {
            Source::Download { .. } => paths::cache_dir().join("models").join(&self.name),
            Source::Folder { path } => path.clone(),
        }
    }

    pub fn is_ready(&self) -> bool {
        let folder = self.folder();
        match &self.source {
            Source::Download { files } => files.iter().all(|(_, file)| folder.join(file).is_file()),
            Source::Folder { .. } => gaps(|file| folder.join(file).is_file()).is_empty(),
        }
    }
}

fn gaps(has: impl Fn(&str) -> bool) -> Vec<String> {
    let mut missing: Vec<String> = REQUIRED.iter().filter(|f| !has(f)).map(|f| (*f).to_string()).collect();
    if !VOCABULARY.iter().any(|f| has(f)) {
        missing.push(VOCABULARY[0].into());
    }
    missing
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub name: String,
    pub label: String,
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

static DOWNLOADS: Mutex<Option<HashMap<String, Download>>> = Mutex::new(None);
static ENSURING: Mutex<Option<HashMap<String, Arc<Mutex<()>>>>> = Mutex::new(None);

fn downloads() -> MutexGuard<'static, Option<HashMap<String, Download>>> {
    DOWNLOADS.lock().unwrap_or_else(|e| e.into_inner())
}

fn update(name: &str, change: impl FnOnce(&mut Download)) {
    change(downloads().get_or_insert_default().entry(name.to_string()).or_default());
}

pub fn status(model: &Model) -> Status {
    let downloads = downloads();
    let download = downloads.as_ref().and_then(|d| d.get(&model.name));
    Status {
        name: model.name.clone(),
        label: model.label.clone(),
        ready: model.is_ready(),
        downloading: download.is_some_and(|d| d.error.is_none()),
        error: download.and_then(|d| d.error.clone()),
        progress: download.filter(|d| d.total > 0).map_or(0.0, |d| (d.done as f64 / d.total as f64).min(1.0)),
        total_mb: download.filter(|d| d.total > 0).map(|d| (d.total as f64 / 1e6).round() as u64),
    }
}

pub fn pending() -> Vec<Status> {
    let names: Vec<String> = downloads().as_ref().map(|d| d.keys().cloned().collect()).unwrap_or_default();
    all().iter().filter(|m| names.contains(&m.name)).map(status).collect()
}

pub fn ensure(name: &str) -> Result<PathBuf, String> {
    let model = find(name).ok_or_else(|| format!("Unknown model “{name}”."))?;
    if model.is_ready() {
        return Ok(model.folder());
    }
    let Source::Download { files } = &model.source else {
        let folder = model.folder();
        let missing = gaps(|file| folder.join(file).is_file()).join(", ");
        return Err(format!("The {} model's folder {} is missing {missing}.", model.label, paths::tilde(&folder)));
    };
    let single = Arc::clone(
        ENSURING.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_default().entry(model.name.clone()).or_default(),
    );
    let _single = single.lock().unwrap_or_else(|e| e.into_inner());
    if model.is_ready() {
        return Ok(model.folder());
    }
    update(&model.name, |d| *d = Download::default());
    match fetch(&model.name, &model.folder(), files) {
        Ok(()) => {
            downloads().get_or_insert_default().remove(&model.name);
            Ok(model.folder())
        }
        Err(error) => {
            log::warn!("Model download failed for {name}: {error}");
            let message = download_problem(error.as_ref(), &model.label);
            update(&model.name, |d| d.error = Some(message.clone()));
            Err(message)
        }
    }
}

fn download_problem(error: &(dyn std::error::Error + 'static), label: &str) -> String {
    let status = error.downcast_ref::<reqwest::Error>().and_then(reqwest::Error::status);
    match status {
        Some(reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN) => format!(
            "Hugging Face refused to send the {label} model: it needs a signed-in account that has accepted its terms. \
             Download it in your browser instead, then add the folder."
        ),
        Some(status) => format!("Couldn't download the {label} model: Hugging Face answered {status}. Press Retry to try again."),
        None => format!("Couldn't download the {label} model. Check your internet connection, then press Retry."),
    }
}

pub fn download_in_background(name: &str) {
    let Some(model) = find(name).filter(|m| !m.is_ready()) else { return };
    downloads().get_or_insert_default().insert(model.name.clone(), Download::default());
    thread::spawn(move || {
        let _ = ensure(&model.name);
    });
}

fn url(repo: &str, file: &str) -> String {
    format!("https://huggingface.co/{repo}/resolve/main/{file}")
}

fn client() -> Result<reqwest::blocking::Client, reqwest::Error> {
    reqwest::blocking::Client::builder().user_agent("vscribe").timeout(None).build()
}

fn fetch(name: &str, dir: &Path, files: &[(String, String)]) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(dir)?;
    let client = client()?;
    let missing: Vec<_> = files.iter().filter(|(_, file)| !dir.join(file).is_file()).collect();
    let mut total = 0;
    for (repo, file) in &missing {
        let head = client.head(url(repo, file)).send()?.error_for_status()?;
        total += head
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok()?.parse::<u64>().ok())
            .unwrap_or(0);
    }
    update(name, |d| d.total = total);
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
            update(name, |d| d.done += read as u64);
        }
        out.sync_all()?;
        fs::rename(part, dir.join(file))?;
    }
    Ok(())
}

pub enum Adding {
    Repo(String),
    Folder(PathBuf),
}

pub fn add(adding: Adding, label: &str) -> Result<Model, String> {
    let label = label.trim();
    if label.is_empty() {
        return Err("Give the model a name to show in the picker.".into());
    }
    if all().iter().any(|m| m.label.eq_ignore_ascii_case(label)) {
        return Err(format!("There is already a model called “{label}”."));
    }
    let source = match adding {
        Adding::Folder(path) => {
            let missing = gaps(|file| path.join(file).is_file());
            if !missing.is_empty() {
                return Err(not_ctranslate2(&paths::tilde(&path), &missing));
            }
            Source::Folder { path }
        }
        Adding::Repo(repo) => Source::Download { files: repo_files(repo.trim())? },
    };
    let mut registry = registry();
    let saved = registry.get_or_insert_default();
    let name = free_name(label, saved);
    let mut next = saved.clone();
    next.push(Saved { name: name.clone(), label: label.into(), source: source.clone() });
    write_registry(&next)?;
    *saved = next;
    REVISION.fetch_add(1, Ordering::Release);
    Ok(Model { name, label: label.into(), builtin: false, source })
}

pub fn remove(name: &str) -> Result<(), String> {
    let model = find(name).ok_or_else(|| format!("Unknown model “{name}”."))?;
    if model.builtin {
        return Err(format!("The {} model is built in and can't be removed.", model.label));
    }
    if let Source::Download { .. } = model.source {
        let folder = model.folder();
        if folder.exists() {
            fs::remove_dir_all(&folder).map_err(|e| format!("Couldn't delete {}: {e}", folder.display()))?;
        }
    }
    let mut registry = registry();
    let saved = registry.get_or_insert_default();
    let next: Vec<Saved> = saved.iter().filter(|s| s.name != name).cloned().collect();
    write_registry(&next)?;
    *saved = next;
    REVISION.fetch_add(1, Ordering::Release);
    Ok(())
}

fn not_ctranslate2(place: &str, missing: &[String]) -> String {
    format!(
        "{place} isn't a Whisper model in CTranslate2 (faster-whisper) format: it has no {}. \
         A Transformers model can be converted with ct2-transformers-converter.",
        missing.join(", ")
    )
}

const CATALOGUE: &[(&str, &str)] = &[
    ("Systran/faster-whisper-tiny", "Quickest and least accurate"),
    ("Systran/faster-whisper-base", "Between Tiny and Fast"),
    ("Systran/faster-distil-whisper-small.en", "Quicker than Fast"),
    ("Systran/faster-whisper-medium", "More accurate than Fast, slower"),
    ("Systran/faster-distil-whisper-large-v3", "Close to Accurate, quicker"),
];

#[derive(Deserialize)]
struct RepoInfo {
    id: String,
    #[serde(default)]
    siblings: Vec<Sibling>,
    #[serde(default)]
    gated: serde_json::Value,
    #[serde(default, rename = "cardData")]
    card: Card,
}

#[derive(Deserialize, Default)]
struct Card {
    #[serde(default)]
    language: serde_json::Value,
}

#[derive(Deserialize)]
struct Sibling {
    rfilename: String,
    size: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Offer {
    pub repo: String,
    pub note: String,
    pub size_mb: Option<u64>,
    pub languages: Option<String>,
    pub problem: Option<String>,
    pub added: bool,
}

#[derive(Clone)]
struct Checked {
    offer: Offer,
    files: Vec<String>,
}

static INSPECTED: Mutex<Option<HashMap<String, Checked>>> = Mutex::new(None);

fn hub(path: &str, query: &[(&str, &str)]) -> Result<String, String> {
    let response = client()
        .and_then(|c| c.get(format!("https://huggingface.co/api/{path}")).query(query).send())
        .map_err(|e| format!("Couldn't reach Hugging Face: {e}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err("Hugging Face has no public model by that name.".into());
    }
    response.error_for_status().and_then(|r| r.text()).map_err(|e| format!("Couldn't read from Hugging Face: {e}"))
}

fn check(info: &RepoInfo) -> Checked {
    let present: Vec<&str> = info.siblings.iter().map(|s| s.rfilename.as_str()).collect();
    let (files, size_mb, problem) = match chosen_files(&present) {
        Ok(files) => {
            let bytes: Option<u64> =
                info.siblings.iter().filter(|s| files.contains(&s.rfilename)).map(|s| s.size).sum();
            (files, bytes.map(|b| (b as f64 / 1e6).round() as u64), None)
        }
        Err(missing) => (Vec::new(), None, Some(format!("Not in CTranslate2 format: no {}", missing.join(", ")))),
    };
    let gated = !matches!(info.gated, serde_json::Value::Null | serde_json::Value::Bool(false));
    let problem = problem.or_else(|| gated.then(|| "Needs a Hugging Face account that accepted its terms".to_string()));
    let languages = languages(&info.card.language);
    Checked { offer: Offer { repo: info.id.clone(), note: String::new(), size_mb, languages, problem, added: false }, files }
}

fn languages(card: &serde_json::Value) -> Option<String> {
    let codes: Vec<&str> = match card {
        serde_json::Value::String(code) => vec![code.as_str()],
        serde_json::Value::Array(codes) => codes.iter().filter_map(serde_json::Value::as_str).collect(),
        _ => return None,
    };
    match codes.as_slice() {
        [] => None,
        ["en"] => Some("English only".into()),
        [code] => Some(format!("One language ({code})")),
        many => Some(format!("{} languages", many.len())),
    }
}

fn inspect(repo: &str) -> Result<Checked, String> {
    if let Some(known) = INSPECTED.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|c| c.get(repo)) {
        return Ok(known.clone());
    }
    let info: RepoInfo = serde_json::from_str(&hub(&format!("models/{repo}"), &[("blobs", "true")])?)
        .map_err(|e| format!("Couldn't read {repo} from Hugging Face: {e}"))?;
    let checked = check(&info);
    INSPECTED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_default().insert(repo.into(), checked.clone());
    Ok(checked)
}

fn mark_added(mut offers: Vec<Offer>) -> Vec<Offer> {
    let repos: Vec<String> = all()
        .into_iter()
        .filter_map(|m| match m.source {
            Source::Download { files } => files.into_iter().find(|(_, f)| f == "model.bin").map(|(repo, _)| repo),
            Source::Folder { .. } => None,
        })
        .collect();
    for offer in &mut offers {
        offer.added = repos.contains(&offer.repo);
    }
    offers
}

pub fn catalogue() -> Vec<Offer> {
    let offers = thread::scope(|scope| {
        let looking: Vec<_> =
            CATALOGUE.iter().map(|(repo, note)| (repo, note, scope.spawn(move || inspect(repo)))).collect();
        looking
            .into_iter()
            .map(|(repo, note, handle)| {
                let inspected = handle.join().unwrap_or_else(|_| Err("Couldn't check this model.".into()));
                let offer = inspected.map(|checked| checked.offer).unwrap_or_else(|problem| Offer {
                    repo: (*repo).into(),
                    note: String::new(),
                    size_mb: None,
                    languages: None,
                    problem: Some(problem),
                    added: false,
                });
                Offer { note: (*note).into(), ..offer }
            })
            .collect()
    });
    mark_added(offers)
}

pub fn search(query: &str) -> Result<Vec<Offer>, String> {
    let text = hub(
        "models",
        &[
            ("search", query.trim()),
            ("filter", "ctranslate2"),
            ("filter", "automatic-speech-recognition"),
            ("sort", "downloads"),
            ("direction", "-1"),
            ("limit", "20"),
            ("full", "true"),
            ("cardData", "true"),
        ],
    )?;
    let found: Vec<RepoInfo> =
        serde_json::from_str(&text).map_err(|e| format!("Couldn't read the search from Hugging Face: {e}"))?;
    Ok(mark_added(found.iter().map(|info| check(info).offer).collect()))
}

fn repo_files(repo: &str) -> Result<Vec<(String, String)>, String> {
    let checked = inspect(repo)?;
    match checked.offer.problem {
        Some(problem) => Err(format!("{repo}: {problem}.")),
        None => Ok(checked.files.into_iter().map(|file| (repo.to_string(), file)).collect()),
    }
}

fn chosen_files(present: &[&str]) -> Result<Vec<String>, Vec<String>> {
    let has = |file: &str| present.contains(&file);
    let missing = gaps(has);
    if !missing.is_empty() {
        return Err(missing);
    }
    let mut files: Vec<String> =
        VOCABULARY.iter().chain(&[PREPROCESSOR]).chain(REQUIRED).filter(|f| has(f)).map(|f| (*f).to_string()).collect();
    files.sort_by_key(|f| f == "model.bin");
    Ok(files)
}

fn free_name(label: &str, taken: &[Saved]) -> String {
    let words: Vec<String> = label
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(String::from)
        .collect();
    let base = if words.is_empty() { "custom".to_string() } else { format!("custom-{}", words.join("-")) };
    let used = |name: &str| taken.iter().any(|s| s.name == name) || is_builtin(name);
    (1..).map(|n| if n == 1 { base.clone() } else { format!("{base}-{n}") }).find(|n| !used(n)).expect("a free name")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repo_without_model_bin_is_not_a_ctranslate2_model() {
        let missing = chosen_files(&["config.json", "tokenizer.json", "pytorch_model.bin"]).unwrap_err();
        assert_eq!(missing, vec!["model.bin".to_string(), "vocabulary.txt".to_string()]);
    }

    #[test]
    fn a_ctranslate2_repo_downloads_what_it_has_with_the_weights_last() {
        let files = chosen_files(&[
            "README.md",
            "model.bin",
            "config.json",
            "tokenizer.json",
            "vocabulary.json",
            "preprocessor_config.json",
        ])
        .unwrap();
        assert_eq!(files.last().map(String::as_str), Some("model.bin"));
        assert!(files.contains(&"vocabulary.json".to_string()));
        assert!(files.contains(&"preprocessor_config.json".to_string()));
        assert!(!files.contains(&"README.md".to_string()));
    }

    #[test]
    fn an_offer_counts_only_the_files_that_would_download() {
        let info: RepoInfo = serde_json::from_str(
            r#"{"id": "owner/whisper", "siblings": [
                {"rfilename": "README.md", "size": 9000000},
                {"rfilename": "config.json", "size": 2000},
                {"rfilename": "tokenizer.json", "size": 2000000},
                {"rfilename": "vocabulary.txt", "size": 500000},
                {"rfilename": "model.bin", "size": 145000000}
            ]}"#,
        )
        .unwrap();
        let checked = check(&info);
        assert_eq!(checked.offer.size_mb, Some(148));
        assert_eq!(checked.offer.problem, None);
        assert_eq!(checked.files.last().map(String::as_str), Some("model.bin"));
    }

    #[test]
    fn a_transformers_repo_is_offered_with_what_it_lacks() {
        let info: RepoInfo = serde_json::from_str(
            r#"{"id": "owner/whisper", "siblings": [{"rfilename": "config.json"}, {"rfilename": "model.safetensors"}]}"#,
        )
        .unwrap();
        let checked = check(&info);
        assert!(checked.offer.problem.as_deref().is_some_and(|p| p.contains("model.bin")));
        assert_eq!(checked.offer.languages, None);
        assert!(checked.files.is_empty());
    }

    #[test]
    fn a_gated_repo_is_offered_but_cannot_be_chosen() {
        let info: RepoInfo = serde_json::from_str(
            r#"{"id": "owner/whisper", "gated": "auto", "siblings": [
                {"rfilename": "config.json"}, {"rfilename": "tokenizer.json"},
                {"rfilename": "vocabulary.json"}, {"rfilename": "model.bin"}
            ]}"#,
        )
        .unwrap();
        assert!(check(&info).offer.problem.as_deref().is_some_and(|p| p.contains("accepted its terms")));
    }

    #[test]
    fn languages_are_summarised_from_the_model_card() {
        assert_eq!(languages(&serde_json::json!(["en"])).as_deref(), Some("English only"));
        assert_eq!(languages(&serde_json::json!("yo")).as_deref(), Some("One language (yo)"));
        assert_eq!(languages(&serde_json::json!(["en", "fr", "yo"])).as_deref(), Some("3 languages"));
        assert_eq!(languages(&serde_json::Value::Null), None);
    }

    #[test]
    fn a_folder_missing_its_weights_is_refused_by_name() {
        let folder = std::env::temp_dir().join(format!("vscribe-model-test-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        for file in ["config.json", "tokenizer.json", "vocabulary.txt"] {
            fs::write(folder.join(file), "{}").unwrap();
        }
        assert_eq!(gaps(|file| folder.join(file).is_file()), vec!["model.bin".to_string()]);
        fs::write(folder.join("model.bin"), "").unwrap();
        assert!(gaps(|file| folder.join(file).is_file()).is_empty());
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn names_never_reuse_a_taken_or_built_in_one() {
        let taken = vec![Saved {
            name: "custom-my-model".into(),
            label: "My Model".into(),
            source: Source::Folder { path: PathBuf::from("/tmp") },
        }];
        assert_eq!(free_name("My Model!", &taken), "custom-my-model-2");
        assert_eq!(free_name("  ", &[]), "custom");
    }

    #[test]
    fn an_unreadable_registry_is_ignored_rather_than_fatal() {
        let file = std::env::temp_dir().join(format!("vscribe-models-test-{}.json", std::process::id()));
        fs::write(&file, "not json").unwrap();
        assert!(read_registry(&file).is_empty());
        fs::remove_file(&file).unwrap();
    }
}
