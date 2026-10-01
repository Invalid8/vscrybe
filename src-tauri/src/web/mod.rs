mod view;

use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Form, Multipart, Path as UrlPath, Query, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use chrono::{Local, TimeZone};
use minijinja::{Environment, Value};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use rust_embed::RustEmbed;
use serde::Deserialize;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::engine::store::{Job, Session, Store};
use crate::engine::transcribe::{DEFAULT_LANGUAGE, LANGUAGES, is_language};
use crate::engine::transcript::clock;
use crate::engine::{models, paths};

#[derive(RustEmbed)]
#[folder = "../src/"]
struct Ui;

pub const AUDIO_EXTENSIONS: &[&str] =
    &["opus", "ogg", "oga", "m4a", "mp3", "wav", "aac", "amr", "flac", "webm", "mp4", "3gp"];

#[derive(Clone)]
struct App {
    store: Store,
    templates: Arc<Environment<'static>>,
}

pub fn router(store: Store) -> Router {
    let app = App { store, templates: Arc::new(templates()) };
    Router::new()
        .route("/", get(index))
        .route("/new", get(new_session))
        .route("/sessions", get(sessions))
        .route("/upload", post(upload))
        .route("/sessions/{id}", get(session_view).delete(delete_session))
        .route("/sessions/{id}/row", get(session_row))
        .route("/sessions/{id}/title", post(rename))
        .route("/sessions/{id}/download", get(session_download))
        .route("/queue", get(queue))
        .route("/models/banner", get(model_banner))
        .route("/models/onboard", get(model_onboard))
        .route("/models/{name}/download", post(model_download))
        .route("/export.zip", get(export))
        .route("/notes/{id}", get(note).delete(delete_note))
        .route("/notes/{id}/status", get(note_status))
        .route("/notes/{id}/audio", get(audio))
        .route("/notes/{id}/download", get(note_download))
        .route("/notes/{id}/retry", post(retry))
        .route("/static/{*path}", get(asset))
        .layer(DefaultBodyLimit::disable())
        .with_state(app)
}

fn templates() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_loader(|name| {
        Ok(Ui::get(&format!("templates/{name}")).map(|f| String::from_utf8_lossy(&f.data).into_owned()))
    });
    env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback);
    env.add_filter("clock", |seconds: f64| clock(seconds));
    env.add_filter("when", when);
    env.add_filter("truncate", truncate);
    env.add_function("model_status", |name: String| Value::from_serialize(models::status(&name)));
    env
}

fn when(timestamp: f64) -> String {
    Local.timestamp_opt(timestamp as i64, 0).single().map_or_else(String::new, |t| t.format("%-d %b, %H:%M").to_string())
}

fn local(timestamp: f64, format: &str) -> String {
    Local.timestamp_opt(timestamp as i64, 0).single().unwrap_or_else(Local::now).format(format).to_string()
}

fn truncate(value: String, length: usize) -> String {
    if value.chars().count() <= length + 5 {
        return value;
    }
    let kept: Vec<char> = value.chars().take(length - 3).collect();
    let cut = kept.iter().rposition(|c| *c == ' ').unwrap_or(kept.len());
    format!("{}...", kept[..cut].iter().collect::<String>())
}

struct Failure(StatusCode, String);

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let body = serde_json::json!({ "detail": self.1 }).to_string();
        (self.0, [(header::CONTENT_TYPE, "application/json")], body).into_response()
    }
}

type Reply = Result<Response, Failure>;

fn missing_session() -> Failure {
    Failure(StatusCode::NOT_FOUND, "That session no longer exists. It may have been deleted in another tab.".into())
}

fn missing_note() -> Failure {
    Failure(StatusCode::NOT_FOUND, "That voice note no longer exists. It may have been deleted in another tab.".into())
}

fn server_error(error: impl std::fmt::Display) -> Failure {
    log::error!("{error}");
    Failure(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

struct Page {
    template: &'static str,
    context: BTreeMap<&'static str, Value>,
    headers: Vec<(HeaderName, String)>,
}

impl Page {
    fn new(template: &'static str) -> Self {
        Self { template, context: BTreeMap::new(), headers: Vec::new() }
    }

    fn with(mut self, key: &'static str, value: impl Into<Value>) -> Self {
        self.context.insert(key, value.into());
        self
    }

    fn toast(self, message: &str) -> Self {
        self.toast_kind(message, "success")
    }

    fn toast_kind(mut self, message: &str, kind: &str) -> Self {
        let json = serde_json::json!({ "toast": { "message": message, "kind": kind } }).to_string();
        let ascii = json.chars().fold(String::new(), |mut out, c| {
            if c.is_ascii() {
                out.push(c);
            } else {
                let mut units = [0; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            out
        });
        self.headers.push((HeaderName::from_static("hx-trigger"), ascii));
        self
    }

    fn push_url(mut self, url: String) -> Self {
        self.headers.push((HeaderName::from_static("hx-push-url"), url));
        self
    }

    fn render(self, app: &App, request: &HeaderMap) -> Reply {
        let snapshot = app.store.snapshot();
        let host = request.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("127.0.0.1");
        let mut context: BTreeMap<&str, Value> = BTreeMap::from([
            ("models", Value::from_iter(models::MODELS.iter().map(|m| (m.name, m.label)))),
            ("languages", Value::from_iter(LANGUAGES.iter().map(|(code, label)| (*code, *label)))),
            ("model_downloads", Value::from_serialize(models::pending())),
            ("store", Value::from_object(view::StoreView(snapshot))),
            ("partial", Value::from(request.get("hx-request").is_some_and(|v| v == "true"))),
            ("base_url", Value::from(format!("http://{host}"))),
            ("version", Value::from(env!("CARGO_PKG_VERSION"))),
            ("data_dir", Value::from(paths::tilde(app.store.root()))),
            ("log_file", Value::from(paths::tilde(&paths::log_dir().join("vscribe.log")))),
        ]);
        context.extend(self.context);
        let html = app
            .templates
            .get_template(self.template)
            .and_then(|t| t.render(&context))
            .map_err(|e| server_error(format!("Couldn't render {}: {e:#}", self.template)))?;
        let mut response = Html(html).into_response();
        for (name, value) in self.headers {
            response.headers_mut().insert(name, HeaderValue::from_str(&value).map_err(server_error)?);
        }
        Ok(response)
    }
}

fn session_value(session: &Session) -> Value {
    Value::from_serialize(session)
}

fn sessions_value(app: &App, query: &str) -> Value {
    Value::from_serialize(app.store.lock().listed(query))
}

fn find_session(app: &App, id: &str) -> Result<Session, Failure> {
    app.store.lock().sessions.get(id).cloned().ok_or_else(missing_session)
}

fn find_job(app: &App, id: &str) -> Result<Job, Failure> {
    app.store.lock().jobs.get(id).cloned().ok_or_else(missing_note)
}

fn plural(count: usize, word: &str) -> String {
    format!("{count} {word}{}", if count == 1 { "" } else { "s" })
}

const QUOTE: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~').remove(b'/');

fn attachment(filename: &str, content_type: &str, body: impl Into<Body>) -> Response {
    let fallback: String =
        filename.chars().map(|c| if c.is_ascii() && c != '?' { if c == '"' { '\'' } else { c } } else { '_' }).collect();
    let disposition = format!("attachment; filename=\"{fallback}\"; filename*=UTF-8''{}", utf8_percent_encode(filename, QUOTE));
    ([(header::CONTENT_TYPE, content_type.to_string()), (header::CONTENT_DISPOSITION, disposition)], body.into())
        .into_response()
}

fn flag(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(|v| matches!(v.to_lowercase().as_str(), "true" | "1" | "yes" | "on"))
}

#[derive(Deserialize)]
struct SessionQuery {
    #[serde(default)]
    s: String,
}

async fn index(State(app): State<App>, headers: HeaderMap, Query(q): Query<SessionQuery>) -> Reply {
    let current = app.store.lock().sessions.get(&q.s).cloned();
    Page::new("index.html")
        .with("sessions", sessions_value(&app, ""))
        .with("current", current.as_ref().map(session_value).unwrap_or_default())
        .render(&app, &headers)
}

async fn new_session(State(app): State<App>, headers: HeaderMap) -> Reply {
    Page::new("_empty.html").render(&app, &headers)
}

#[derive(Deserialize)]
struct Search {
    #[serde(default)]
    q: String,
}

async fn sessions(State(app): State<App>, headers: HeaderMap, Query(search): Query<Search>) -> Reply {
    Page::new("_sessions.html")
        .with("sessions", sessions_value(&app, &search.q))
        .with("query", search.q)
        .render(&app, &headers)
}

fn is_audio(name: &str, content_type: &str) -> bool {
    let extension = Path::new(name).extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    AUDIO_EXTENSIONS.contains(&extension.as_str()) || content_type.starts_with("audio/")
}

async fn upload(State(app): State<App>, headers: HeaderMap, mut form: Multipart) -> Reply {
    let bad = |e: axum::extract::multipart::MultipartError| Failure(StatusCode::BAD_REQUEST, e.body_text());
    let (mut model, mut language, mut session) = (models::DEFAULT.to_string(), DEFAULT_LANGUAGE.to_string(), String::new());
    let mut files = Vec::new();
    let mut skipped = 0;
    while let Some(field) = form.next_field().await.map_err(bad)? {
        match field.name().unwrap_or_default() {
            "model" => model = field.text().await.map_err(bad)?,
            "language" => language = field.text().await.map_err(bad)?,
            "session" => session = field.text().await.map_err(bad)?,
            "files" => {
                let name = field.file_name().unwrap_or_default().to_string();
                let content_type = field.content_type().unwrap_or_default().to_string();
                if is_audio(&name, &content_type) {
                    files.push((name, field.bytes().await.map_err(bad)?));
                } else if !name.is_empty() {
                    skipped += 1;
                }
            }
            _ => {}
        }
    }
    if models::find(&model).is_none() {
        return Err(Failure(StatusCode::BAD_REQUEST, format!("Unknown model “{model}”.")));
    }
    if !is_language(&language) {
        return Err(Failure(StatusCode::BAD_REQUEST, format!("Unknown language “{language}”.")));
    }
    if files.is_empty() {
        return Err(Failure(
            StatusCode::BAD_REQUEST,
            "None of those files are audio. Try .opus, .ogg, .m4a, .mp3, .wav or .amr.".into(),
        ));
    }
    let existing = app.store.lock().sessions.get(&session).cloned();
    let current = match existing {
        Some(current) => current,
        None => app.store.create_session().map_err(server_error)?,
    };
    let count = files.len();
    for (name, data) in files {
        let store = app.store.clone();
        let (model, language, session) = (model.clone(), language.clone(), current.id.clone());
        tokio::task::spawn_blocking(move || store.add(&name, &data, &model, &language, &session))
            .await
            .map_err(server_error)?
            .map_err(|e| server_error(format!("Couldn't save the voice note: {e}")))?;
    }
    let mut message = format!("Added {}", plural(count, "voice note"));
    if skipped > 0 {
        let verb = if skipped == 1 { "isn’t" } else { "aren’t" };
        message += &format!(" · skipped {} that {verb} audio", plural(skipped, "file"));
    }
    Page::new("_session.html")
        .push_url(format!("/?s={}", current.id))
        .toast(&message)
        .with("current", session_value(&current))
        .with("sessions", sessions_value(&app, ""))
        .with("refresh_side", true)
        .render(&app, &headers)
}

async fn session_view(State(app): State<App>, headers: HeaderMap, UrlPath(id): UrlPath<String>) -> Reply {
    let current = find_session(&app, &id)?;
    Page::new("_session.html").with("current", session_value(&current)).render(&app, &headers)
}

async fn session_row(State(app): State<App>, headers: HeaderMap, UrlPath(id): UrlPath<String>) -> Reply {
    let session = find_session(&app, &id)?;
    Page::new("_session_row.html").with("session", session_value(&session)).render(&app, &headers)
}

#[derive(Deserialize)]
struct Title {
    #[serde(default)]
    title: String,
}

async fn rename(State(app): State<App>, headers: HeaderMap, UrlPath(id): UrlPath<String>, Form(form): Form<Title>) -> Reply {
    let session = app.store.rename(&id, &form.title).map_err(server_error)?.ok_or_else(missing_session)?;
    let message = if session.title.is_empty() { "Name cleared · using the transcript as the title" } else { "Session renamed" };
    Page::new("_renamed.html").toast(message).with("session", session_value(&session)).render(&app, &headers)
}

#[derive(Deserialize)]
struct Timestamps {
    timestamps: Option<String>,
    #[serde(default)]
    q: String,
}

fn transcript_text(job: &Job, timestamps: bool) -> Option<String> {
    job.transcript.as_ref().map(|t| if timestamps { t.timestamped() } else { t.text() })
}

async fn session_download(State(app): State<App>, UrlPath(id): UrlPath<String>, Query(q): Query<Timestamps>) -> Reply {
    let session = find_session(&app, &id)?;
    let timestamps = flag(&q.timestamps);
    let text = {
        let state = app.store.lock();
        state
            .notes(&id)
            .into_iter()
            .filter_map(|job| {
                let body = transcript_text(job, timestamps)?;
                Some(format!("{} ({})\n{body}", job.name, clock(job.transcript.as_ref()?.duration)))
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    let filename = local(session.created, "session-%Y-%m-%d-%H%M.txt");
    Ok(attachment(&filename, "text/plain; charset=utf-8", text + "\n"))
}

async fn delete_session(State(app): State<App>, headers: HeaderMap, UrlPath(id): UrlPath<String>) -> Reply {
    find_session(&app, &id)?;
    app.store.delete_session(&id);
    Page::new("_empty.html")
        .push_url("/".into())
        .toast("Session deleted")
        .with("sessions", sessions_value(&app, ""))
        .with("refresh_side", true)
        .render(&app, &headers)
}

async fn queue(State(app): State<App>, headers: HeaderMap) -> Reply {
    let failed = app.store.take_failed();
    let page = Page::new("_queue.html");
    let page = match failed.as_slice() {
        [] => page,
        [name] => page.toast_kind(&format!("Couldn't transcribe {name}. Open it to see why."), "error"),
        many => page.toast_kind(&format!("Couldn't transcribe {}. Open them to see why.", plural(many.len(), "voice note")), "error"),
    };
    page.render(&app, &headers)
}

async fn model_banner(State(app): State<App>, headers: HeaderMap) -> Reply {
    Page::new("_models.html").render(&app, &headers)
}

async fn model_onboard(State(app): State<App>, headers: HeaderMap) -> Reply {
    Page::new("_onboard_model.html").render(&app, &headers)
}

async fn model_download(State(app): State<App>, headers: HeaderMap, UrlPath(name): UrlPath<String>) -> Reply {
    let model = models::find(&name).ok_or_else(|| Failure(StatusCode::NOT_FOUND, format!("Unknown model “{name}”.")))?;
    models::download_in_background(model.name);
    Page::new("_models.html").toast(&format!("Retrying the {} model download", model.label)).render(&app, &headers)
}

async fn export(State(app): State<App>, Query(q): Query<Timestamps>) -> Reply {
    let timestamps = flag(&q.timestamps);
    let entries: Vec<(String, String)> = {
        let state = app.store.lock();
        let mut used = HashSet::new();
        let mut entries = Vec::new();
        for session in state.listed(&q.q) {
            let folder = local(session.created, "%Y-%m-%d %H.%M");
            for job in state.notes(&session.id) {
                let Some(text) = transcript_text(job, timestamps) else { continue };
                let stem = format!("{folder}/{}", Path::new(&job.name).file_stem().and_then(|s| s.to_str()).unwrap_or("note"));
                let name = (1..)
                    .map(|n| if n == 1 { format!("{stem}.txt") } else { format!("{stem} ({n}).txt") })
                    .find(|name| !used.contains(name))
                    .expect("a free name");
                used.insert(name.clone());
                entries.push((name, text + "\n"));
            }
        }
        entries
    };
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, text) in entries {
        archive.start_file(name, options).map_err(server_error)?;
        archive.write_all(text.as_bytes()).map_err(server_error)?;
    }
    let bytes = archive.finish().map_err(server_error)?.into_inner();
    Ok(attachment(&Local::now().format("voice-notes-%Y-%m-%d.zip").to_string(), "application/zip", bytes))
}

async fn note(State(app): State<App>, headers: HeaderMap, UrlPath(id): UrlPath<String>) -> Reply {
    let job = find_job(&app, &id)?;
    Page::new("_note.html").with("job", view::job(&job)).with("refresh_meta", true).render(&app, &headers)
}

async fn note_status(State(app): State<App>, headers: HeaderMap, UrlPath(id): UrlPath<String>) -> Reply {
    let job = find_job(&app, &id)?;
    Page::new("_status.html").with("job", view::job(&job)).render(&app, &headers)
}

async fn audio(State(app): State<App>, UrlPath(id): UrlPath<String>, request: Request) -> Reply {
    let job = find_job(&app, &id)?;
    let path = job.folder(app.store.root()).join(&job.audio);
    Ok(ServeFile::new(path).oneshot(request).await.map_err(server_error)?.into_response())
}

async fn note_download(State(app): State<App>, UrlPath(id): UrlPath<String>, Query(q): Query<Timestamps>) -> Reply {
    let job = find_job(&app, &id)?;
    let text = transcript_text(&job, flag(&q.timestamps))
        .ok_or_else(|| Failure(StatusCode::CONFLICT, "This voice note hasn't been transcribed yet.".into()))?;
    let stem = Path::new(&job.name).file_stem().and_then(|s| s.to_str()).unwrap_or("note");
    Ok(attachment(&format!("{stem}.txt"), "text/plain; charset=utf-8", text + "\n"))
}

#[derive(Deserialize)]
struct Redo {
    model: String,
    language: Option<String>,
}

async fn retry(State(app): State<App>, headers: HeaderMap, UrlPath(id): UrlPath<String>, Form(form): Form<Redo>) -> Reply {
    let job = find_job(&app, &id)?;
    let model = models::find(&form.model)
        .ok_or_else(|| Failure(StatusCode::BAD_REQUEST, format!("Unknown model “{}”.", form.model)))?;
    let language = form.language.unwrap_or(job.language.clone());
    if !is_language(&language) {
        return Err(Failure(StatusCode::BAD_REQUEST, format!("Unknown language “{language}”.")));
    }
    if job.active() {
        return Err(Failure(StatusCode::CONFLICT, "This voice note is already being transcribed.".into()));
    }
    let job = app.store.retry(&id, model.name, &language).map_err(server_error)?.ok_or_else(missing_note)?;
    Page::new("_note.html")
        .toast(&format!("Transcribing again with the {} model", model.label))
        .with("job", view::job(&job))
        .with("refresh_row", true)
        .render(&app, &headers)
}

async fn delete_note(State(app): State<App>, headers: HeaderMap, UrlPath(id): UrlPath<String>) -> Reply {
    let job = find_job(&app, &id)?;
    app.store.delete_job(&id);
    if app.store.lock().notes(&job.session).is_empty() {
        app.store.delete_session(&job.session);
        return Page::new("_empty.html")
            .push_url("/".into())
            .toast("Voice note removed · the session was empty, so it's gone too")
            .with("sessions", sessions_value(&app, ""))
            .with("refresh_side", true)
            .render(&app, &headers);
    }
    let current = find_session(&app, &job.session)?;
    Page::new("_session.html")
        .toast("Voice note removed")
        .with("current", session_value(&current))
        .with("sessions", sessions_value(&app, ""))
        .with("refresh_side", true)
        .render(&app, &headers)
}

async fn asset(UrlPath(path): UrlPath<String>) -> Response {
    match Ui::get(&format!("static/{path}")) {
        Some(file) => {
            let mime = file.metadata.mimetype().to_string();
            ([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "no-cache".into())], file.data.into_owned())
                .into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
