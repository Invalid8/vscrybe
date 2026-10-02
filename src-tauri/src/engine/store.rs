use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::transcribe::{DEFAULT_LANGUAGE, Engine};
use super::transcript::{Transcript, round2};
use super::{models, paths, probe_duration};

pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

fn default_language() -> String {
    DEFAULT_LANGUAGE.into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub name: String,
    pub audio: String,
    pub model: String,
    pub created: f64,
    #[serde(default)]
    pub session: String,
    #[serde(default = "queued")]
    pub status: String,
    #[serde(default)]
    pub progress: f64,
    #[serde(default)]
    pub elapsed: Option<f64>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub transcript: Option<Transcript>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default = "default_language")]
    pub language: String,
}

fn queued() -> String {
    "queued".into()
}

impl Job {
    pub fn active(&self) -> bool {
        matches!(self.status.as_str(), "queued" | "running")
    }

    pub fn folder(&self, root: &Path) -> PathBuf {
        root.join("notes").join(&self.id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub created: f64,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, Default)]
pub struct State {
    pub jobs: HashMap<String, Job>,
    pub sessions: HashMap<String, Session>,
    pub batch_total: usize,
    pub batch_done: usize,
    pub batch_failed: usize,
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct Batch {
    pub total: usize,
    pub failed: usize,
}

type BatchListener = Box<dyn Fn(Batch) + Send + Sync>;

impl State {
    pub fn active(&self) -> Vec<&Job> {
        let mut active: Vec<_> = self.jobs.values().filter(|j| j.active()).collect();
        active.sort_by(|a, b| a.created.total_cmp(&b.created));
        active
    }

    pub fn batch_progress(&self) -> f64 {
        if self.batch_total == 0 {
            return 0.0;
        }
        let running: f64 = self.jobs.values().filter(|j| j.status == "running").map(|j| j.progress).sum();
        (self.batch_done as f64 + running) / self.batch_total as f64
    }

    pub fn notes(&self, session: &str) -> Vec<&Job> {
        let mut notes: Vec<_> = self.jobs.values().filter(|j| j.session == session).collect();
        notes.sort_by(|a, b| a.created.total_cmp(&b.created));
        notes
    }

    pub fn title(&self, session: &Session) -> String {
        if !session.title.is_empty() {
            return session.title.clone();
        }
        let notes = self.notes(&session.id);
        notes
            .iter()
            .filter_map(|j| j.transcript.as_ref().map(Transcript::text))
            .find(|t| !t.is_empty())
            .or_else(|| notes.first().map(|j| j.name.clone()))
            .unwrap_or_else(|| "New session".into())
    }

    pub fn last_activity(&self, session: &Session) -> f64 {
        self.notes(&session.id).iter().map(|j| j.created).fold(session.created, f64::max)
    }

    pub fn listed(&self, query: &str) -> Vec<Session> {
        let mut sessions: Vec<_> = self.sessions.values().collect();
        sessions.sort_by(|a, b| self.last_activity(b).total_cmp(&self.last_activity(a)));
        let query = query.trim().to_lowercase();
        sessions
            .into_iter()
            .filter(|s| {
                query.is_empty()
                    || s.title.to_lowercase().contains(&query)
                    || self.notes(&s.id).iter().any(|j| {
                        j.name.to_lowercase().contains(&query)
                            || j.transcript.as_ref().is_some_and(|t| t.text().to_lowercase().contains(&query))
                    })
            })
            .cloned()
            .collect()
    }
}

#[derive(Clone)]
pub struct Store {
    root: PathBuf,
    state: Arc<Mutex<State>>,
    queue: Sender<String>,
    batch_listener: Arc<OnceLock<BatchListener>>,
}

impl Store {
    pub fn open() -> io::Result<Self> {
        Self::open_at(paths::data_dir())
    }

    pub fn open_at(root: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(root.join("notes"))?;
        fs::create_dir_all(root.join("sessions"))?;
        let _ = fs::remove_dir_all(root.join("incoming"));
        fs::create_dir_all(root.join("incoming"))?;
        let (queue, pending) = channel();
        let store = Self { root, state: Arc::default(), queue, batch_listener: Arc::default() };
        let mut resume = Vec::new();
        {
            let mut state = store.lock();
            for session in load_all::<Session>(&store.root.join("sessions"), |p| p.extension().is_some_and(|e| e == "json")) {
                state.sessions.insert(session.id.clone(), session);
            }
            let mut jobs = load_all::<Job>(&store.root.join("notes"), |p| p.is_dir());
            jobs.sort_by(|a, b| a.created.total_cmp(&b.created));
            for mut job in jobs {
                if !state.sessions.contains_key(&job.session) {
                    let session = store.create_session_in(&mut state, Some(job.created))?;
                    job.session = session.id;
                    store.save_job(&job)?;
                }
                if job.active() {
                    job.status = "queued".into();
                    job.progress = 0.0;
                    resume.push(job.id.clone());
                }
                state.jobs.insert(job.id.clone(), job);
            }
        }
        for id in resume {
            store.enqueue(&mut store.lock(), &id);
        }
        let worker = store.clone();
        thread::spawn(move || worker.work(pending));
        Ok(store)
    }

    pub fn on_batch_finished(&self, listener: impl Fn(Batch) + Send + Sync + 'static) {
        let _ = self.batch_listener.set(Box::new(listener));
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn incoming(&self) -> PathBuf {
        self.root.join("incoming").join(new_id())
    }

    pub fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn snapshot(&self) -> State {
        self.lock().clone()
    }

    pub fn save_job(&self, job: &Job) -> io::Result<()> {
        fs::write(job.folder(&self.root).join("job.json"), serde_json::to_vec(job)?)
    }

    pub fn save_session(&self, session: &Session) -> io::Result<()> {
        fs::write(self.root.join("sessions").join(format!("{}.json", session.id)), serde_json::to_vec(session)?)
    }

    fn create_session_in(&self, state: &mut State, created: Option<f64>) -> io::Result<Session> {
        let session = Session { id: new_id(), created: created.unwrap_or_else(now), title: String::new() };
        self.save_session(&session)?;
        state.sessions.insert(session.id.clone(), session.clone());
        Ok(session)
    }

    pub fn create_session(&self) -> io::Result<Session> {
        self.create_session_in(&mut self.lock(), None)
    }

    fn enqueue(&self, state: &mut State, id: &str) {
        if !state.jobs.values().any(|j| j.active() && j.id != id) {
            state.batch_total = 0;
            state.batch_done = 0;
            state.batch_failed = 0;
        }
        state.batch_total += 1;
        let _ = self.queue.send(id.into());
    }

    pub fn add(&self, name: &str, staged: &Path, model: &str, language: &str, session: &str) -> io::Result<Job> {
        let name = Path::new(name).file_name().and_then(|n| n.to_str()).unwrap_or("voice-note").to_string();
        let extension = Path::new(&name).extension().and_then(|e| e.to_str()).map(|e| format!(".{}", e.to_lowercase()));
        let mut job = Job {
            id: new_id(),
            audio: format!("audio{}", extension.unwrap_or_default()),
            name,
            model: model.into(),
            created: now(),
            session: session.into(),
            status: "queued".into(),
            progress: 0.0,
            elapsed: None,
            error: None,
            transcript: None,
            duration: None,
            language: language.into(),
        };
        let folder = job.folder(&self.root);
        let saved = (|| {
            fs::create_dir_all(&folder)?;
            fs::rename(staged, folder.join(&job.audio))?;
            job.duration = probe_duration(&folder.join(&job.audio)).map(round2);
            self.save_job(&job)
        })();
        if let Err(error) = saved {
            let _ = fs::remove_dir_all(&folder);
            return Err(error);
        }
        let mut state = self.lock();
        state.jobs.insert(job.id.clone(), job.clone());
        self.enqueue(&mut state, &job.id);
        Ok(job)
    }

    pub fn retry(&self, id: &str, model: &str, language: &str) -> io::Result<Option<Job>> {
        let mut state = self.lock();
        let Some(job) = state.jobs.get_mut(id) else {
            return Ok(None);
        };
        job.model = model.into();
        job.language = language.into();
        job.status = "queued".into();
        job.progress = 0.0;
        job.error = None;
        job.transcript = None;
        let job = job.clone();
        self.save_job(&job)?;
        self.enqueue(&mut state, id);
        Ok(Some(job))
    }

    pub fn rename(&self, id: &str, title: &str) -> io::Result<Option<Session>> {
        let mut state = self.lock();
        let Some(session) = state.sessions.get_mut(id) else {
            return Ok(None);
        };
        session.title = title.trim().chars().take(80).collect();
        let session = session.clone();
        self.save_session(&session)?;
        Ok(Some(session))
    }

    pub fn delete_job(&self, id: &str) {
        let mut state = self.lock();
        if let Some(job) = state.jobs.remove(id) {
            if job.status == "queued" {
                state.batch_done += 1;
            }
            let _ = fs::remove_dir_all(job.folder(&self.root));
        }
    }

    pub fn delete_session(&self, id: &str) {
        let notes: Vec<String> = self.lock().notes(id).iter().map(|j| j.id.clone()).collect();
        for note in notes {
            self.delete_job(&note);
        }
        self.lock().sessions.remove(id);
        let _ = fs::remove_file(self.root.join("sessions").join(format!("{id}.json")));
    }

    pub fn take_failed(&self) -> Vec<String> {
        std::mem::take(&mut self.lock().failed)
    }

    fn work(self, pending: Receiver<String>) {
        let mut engines: HashMap<String, Engine> = HashMap::new();
        let mut revision = models::revision();
        match Engine::load(models::DEFAULT) {
            Ok(engine) => {
                engines.insert(models::DEFAULT.into(), engine);
            }
            Err(error) => log::error!("Couldn't load the {} model: {error}", models::DEFAULT),
        }
        for id in pending {
            let Some(job) = self.lock().jobs.get(&id).cloned() else {
                continue;
            };
            if models::revision() != revision {
                revision = models::revision();
                engines.retain(|name, _| models::is_builtin(name));
            }
            let started = now();
            let result = (|| {
                if !engines.contains_key(&job.model) {
                    let engine = Engine::load(&job.model)?;
                    engines.insert(job.model.clone(), engine);
                }
                self.update(&id, |j| j.status = "running".into());
                let mut progress = |value: f64| self.update(&id, |j| j.progress = value);
                engines[&job.model]
                    .transcribe(&job.folder(&self.root).join(&job.audio), &job.language, &mut progress)
                    .map_err(|e| e.to_string())
            })();
            let mut state = self.lock();
            state.batch_done += 1;
            let failed = result.as_ref().err().map(|_| job.name.clone());
            let Some(saved) = state.jobs.get_mut(&id) else {
                continue;
            };
            match result {
                Ok(transcript) => {
                    saved.transcript = Some(transcript);
                    saved.status = "done".into();
                    saved.progress = 1.0;
                }
                Err(error) => {
                    log::warn!("Transcription failed for {}: {error}", job.name);
                    saved.status = "failed".into();
                    saved.error = Some(error);
                }
            }
            saved.elapsed = Some(((now() - started) * 10.0).round() / 10.0);
            let saved = saved.clone();
            state.batch_failed += usize::from(failed.is_some());
            state.failed.extend(failed);
            let finished = state.jobs.values().all(|j| !j.active()).then_some(Batch { total: state.batch_total, failed: state.batch_failed });
            drop(state);
            if let (Some(batch), Some(listener)) = (finished, self.batch_listener.get()) {
                listener(batch);
            }
            if let Err(error) = self.save_job(&saved) {
                log::error!("Couldn't save the transcript for {}: {error}", saved.name);
            }
        }
    }

    fn update(&self, id: &str, change: impl FnOnce(&mut Job)) {
        if let Some(job) = self.lock().jobs.get_mut(id) {
            change(job);
        }
    }
}

fn load_all<T: DeserializeOwned>(dir: &Path, wanted: impl Fn(&Path) -> bool) -> Vec<T> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| wanted(p))
        .map(|p| if p.is_dir() { p.join("job.json") } else { p })
        .filter_map(|path| {
            let parsed = fs::read(&path).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string()));
            parsed.map_err(|error| log::warn!("Skipping unreadable {}: {error}", path.display())).ok()
        })
        .collect()
}
