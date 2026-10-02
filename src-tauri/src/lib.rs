pub mod cli;
pub mod engine;
pub mod web;

use std::fs::{self, OpenOptions};
use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::thread;

use simplelog::{ColorChoice, CombinedLogger, ConfigBuilder, LevelFilter, TermLogger, TerminalMode, WriteLogger};
use tauri::webview::DownloadEvent;
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_notification::NotificationExt;

use engine::store::{Batch, Store};

pub fn setup_logging() {
    let dir = engine::paths::log_dir();
    let file = dir.join("vscribe.log");
    let _ = fs::create_dir_all(&dir);
    if fs::metadata(&file).is_ok_and(|m| m.len() > 1_000_000) {
        let _ = fs::rename(&file, dir.join("vscribe.log.1"));
    }
    let config = ConfigBuilder::new()
        .add_filter_allow_str("vscribe")
        .build();
    let mut loggers: Vec<Box<dyn simplelog::SharedLogger>> =
        vec![TermLogger::new(LevelFilter::Warn, config.clone(), TerminalMode::Stderr, ColorChoice::Auto)];
    if let Ok(out) = OpenOptions::new().create(true).append(true).open(&file) {
        loggers.push(WriteLogger::new(LevelFilter::Info, config, out));
    }
    let _ = CombinedLogger::init(loggers);
}

pub fn open_store() -> Result<Store, String> {
    Store::open().map_err(|e| format!("Couldn't open the data folder: {e}"))
}

pub fn bind(port: u16) -> Result<TcpListener, String> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port)).map_err(|e| format!("Couldn't open port {port}: {e}"))
}

fn bind_remembered_port() -> Result<TcpListener, String> {
    let file = engine::paths::data_dir().join("port");
    let saved = fs::read_to_string(&file).ok().and_then(|p| p.trim().parse::<u16>().ok());
    let listener = match saved.and_then(|port| bind(port).ok()) {
        Some(listener) => listener,
        None => bind(0)?,
    };
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    if saved != Some(port) {
        let _ = fs::write(&file, port.to_string());
    }
    Ok(listener)
}

pub fn start_server(store: Store, listener: TcpListener) -> Result<String, String> {
    let url = format!("http://{}", listener.local_addr().map_err(|e| e.to_string())?);
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("a tokio runtime");
        runtime.block_on(async {
            let listener = tokio::net::TcpListener::from_std(listener).expect("a tokio listener");
            if let Err(error) = axum::serve(listener, web::router(store)).await {
                log::error!("The server stopped: {error}");
            }
        });
    });
    log::info!("Serving at {url}");
    Ok(url)
}

pub fn startup_problem() -> Option<String> {
    let missing = engine::missing_elements();
    (!missing.is_empty()).then(|| {
        format!("Audio decoding needs GStreamer packages that aren't installed. Run: sudo apt install {}.", missing.join(" "))
    })
}

fn is_app_url(url: &Url) -> bool {
    matches!(url.scheme(), "tauri" | "about" | "blob" | "data")
        || matches!(url.host_str(), Some("127.0.0.1" | "tauri.localhost"))
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("download");
    let ext = path.extension().and_then(|s| s.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
    (1..).map(|n| dir.join(format!("{stem} ({n}){ext}"))).find(|p| !p.exists()).expect("a free file name")
}

#[cfg(target_os = "linux")]
fn allow_microphone(window: &WebviewWindow) {
    use webkit2gtk::{PermissionRequestExt, SettingsExt, WebViewExt};
    let _ = window.with_webview(|webview| {
        let view = webview.inner();
        if let Some(settings) = WebViewExt::settings(&view) {
            settings.set_enable_media_stream(true);
            settings.set_enable_mediasource(true);
        }
        view.connect_permission_request(|_, request| {
            request.allow();
            true
        });
    });
}

fn notify_finished(app: &AppHandle, batch: Batch) {
    let focused = app.get_webview_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false);
    if focused || batch.total == 0 {
        return;
    }
    let notes = |n: usize| format!("{n} voice note{}", if n == 1 { "" } else { "s" });
    let (title, body) = match batch.failed {
        0 => ("Transcription finished", format!("{} ready to read.", notes(batch.total))),
        failed if failed == batch.total => ("Transcription failed", format!("Couldn't transcribe {}. Open the app to see why.", notes(failed))),
        failed => ("Transcription finished", format!("{} ready, {failed} couldn't be transcribed.", notes(batch.total - failed))),
    };
    if let Err(error) = app.notification().builder().title(title).body(body).show() {
        log::warn!("Couldn't show a notification: {error}");
    }
}

fn js(value: &str) -> String {
    serde_json::to_string(value).expect("a string serializes")
}

pub fn run() {
    setup_logging();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let started = startup_problem().map_or_else(open_store, Err).and_then(|store| {
                let handle = app.handle().clone();
                store.on_batch_finished(move |batch| notify_finished(&handle, batch));
                start_server(store, bind_remembered_port()?)
            });
            let (url, error) = match started.and_then(|u| u.parse::<Url>().map_err(|e| e.to_string())) {
                Ok(url) => (WebviewUrl::External(url), None),
                Err(message) => {
                    log::error!("{message}");
                    (WebviewUrl::App("index.html".into()), Some(message))
                }
            };
            let downloads = dirs::download_dir().unwrap_or_else(std::env::temp_dir);
            let mut builder = WebviewWindowBuilder::new(app, "main", url)
                .title("vScribe")
                .inner_size(1280.0, 820.0)
                .min_inner_size(420.0, 560.0)
                .decorations(false)
                .disable_drag_drop_handler()
                .on_navigation(|url| {
                    if is_app_url(url) {
                        return true;
                    }
                    let _ = tauri_plugin_opener::open_url(url.as_str(), None::<&str>);
                    false
                })
                .on_download(move |webview, event| {
                    match event {
                        DownloadEvent::Requested { url, destination } => {
                            let name = destination
                                .file_name()
                                .and_then(|n| n.to_str())
                                .map(str::to_owned)
                                .or_else(|| url.path_segments().and_then(|mut s| s.next_back()).map(str::to_owned))
                                .unwrap_or_else(|| "download".into());
                            *destination = unique_path(&downloads, &name);
                        }
                        DownloadEvent::Finished { path: Some(path), success: true, .. } => {
                            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            let _ = webview.eval(format!(
                                "window.downloaded?.({}, {})",
                                js(&path.to_string_lossy()),
                                js(&name)
                            ));
                        }
                        DownloadEvent::Finished { .. } => {
                            let _ = webview.eval("window.toast?.(\"The download didn't finish. Try again.\")");
                        }
                        _ => {}
                    }
                    true
                });
            if let Some(message) = error {
                builder = builder.initialization_script(format!("window.startupError = {};", js(&message)));
            }
            let window = builder.build()?;
            #[cfg(target_os = "linux")]
            allow_microphone(&window);
            #[cfg(not(target_os = "linux"))]
            let _ = window;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to start vScribe");
}
