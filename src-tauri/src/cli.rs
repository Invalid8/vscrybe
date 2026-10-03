use std::fs;
use std::io::Read;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use crate::engine::models;
use crate::engine::transcribe::{DEFAULT_LANGUAGE, Engine, LANGUAGES, is_language};
use crate::web::AUDIO_EXTENSIONS;

const READY: &str = "VSCRIBE_READY";

#[derive(Parser)]
#[command(name = "vscribe", about = "Convert audio and voice notes to text, locally.", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Transcribe files or folders, writing a .txt next to each
    Transcribe {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// small (Fast), large-v3-turbo (Accurate), or a model you added (see `vscribe models`)
        #[arg(short, long, default_value = models::DEFAULT)]
        model: String,
        /// Language spoken in the recordings, as a code such as en, fr or yo
        #[arg(short, long, default_value = DEFAULT_LANGUAGE)]
        language: String,
        /// Prefix each line with [m:ss]
        #[arg(short, long)]
        timestamps: bool,
        /// Redo files that already have a .txt
        #[arg(short, long)]
        force: bool,
        /// Print transcripts instead of writing files
        #[arg(long)]
        stdout: bool,
    },
    /// List, add or remove Whisper models
    Models {
        #[command(subcommand)]
        action: Option<ModelAction>,
    },
    /// Run the web UI on 127.0.0.1 and print its URL when ready
    Serve {
        /// Port to listen on (default: any free port)
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// Stop when the parent process closes stdin
        #[arg(long)]
        exit_with_stdin: bool,
    },
}

#[derive(Subcommand)]
enum ModelAction {
    /// List every model and whether it is ready to use
    List,
    /// Add a CTranslate2 Whisper model from Hugging Face (owner/name) or a folder
    Add {
        /// A Hugging Face model id such as Sunbird/faster-whisper-51-african-languages, or a folder
        source: String,
        /// The name to show for it
        #[arg(long)]
        name: String,
    },
    /// Remove a model you added, deleting its downloaded files
    Remove { model: String },
}

fn manage_models(action: Option<ModelAction>) -> ExitCode {
    let result = match action.unwrap_or(ModelAction::List) {
        ModelAction::List => {
            for model in models::all() {
                let kind = if model.builtin { "built in" } else { "added" };
                let ready = if model.is_ready() { "ready" } else { "not downloaded" };
                println!("{:<28} {:<24} {kind}, {ready}", model.name, model.label);
            }
            Ok(())
        }
        ModelAction::Add { source, name } => {
            let folder = PathBuf::from(&source);
            let adding = if folder.is_dir() { models::Adding::Folder(folder) } else { models::Adding::Repo(source) };
            models::add(adding, &name).map(|model| {
                println!("Added {} as {}. Use it with: vscribe transcribe -m {} …", model.label, model.name, model.name)
            })
        }
        ModelAction::Remove { model } => models::remove(&model).map(|()| println!("Removed {model}.")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn collect_audio(paths: &[PathBuf]) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_lowercase().as_str())) {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            walk(path, &mut files);
        } else if path.is_file() {
            files.push(path.clone());
        }
    }
    files
}

fn transcribe(paths: &[PathBuf], model: &str, language: &str, timestamps: bool, force: bool, stdout: bool) -> ExitCode {
    if models::find(model).is_none() {
        eprintln!("Unknown model “{model}”. Use one of: {}.", models::all().iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join(", "));
        return ExitCode::FAILURE;
    }
    if !is_language(language) {
        eprintln!("Unknown language “{language}”. Use one of: {}.", LANGUAGES.iter().map(|l| l.0).collect::<Vec<_>>().join(", "));
        return ExitCode::FAILURE;
    }
    let files = collect_audio(paths);
    if files.is_empty() {
        eprintln!("No audio files found.");
        return ExitCode::FAILURE;
    }
    if !models::is_ready(model) {
        eprintln!("Downloading the {} model (one time only)...", models::label(model));
    }
    let engine = match Engine::load(model) {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut failed = 0;
    for path in &files {
        let out = path.with_extension("txt");
        if out.exists() && !force && !stdout {
            eprintln!("skip  {}  (already has {})", path.display(), out.file_name().unwrap_or_default().to_string_lossy());
            continue;
        }
        eprintln!("...   {}", path.display());
        let result = engine.transcribe(path, language, &mut |_| ControlFlow::Continue(())).map_err(|e| e.to_string()).and_then(|t| {
            let text = if timestamps { t.timestamped() } else { t.text() };
            if stdout {
                println!("{text}");
                Ok(())
            } else {
                fs::write(&out, text + "\n").map_err(|e| e.to_string()).map(|()| eprintln!("done  {}", out.display()))
            }
        });
        if let Err(error) = result {
            failed += 1;
            eprintln!("FAIL  {}: {error}", path.display());
        }
    }
    if failed > 0 {
        eprintln!("{failed} of {} files failed.", files.len());
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn serve(port: u16, exit_with_stdin: bool) -> ExitCode {
    crate::setup_logging();
    let started = crate::open_store().and_then(|store| Ok((store.clone(), crate::start_server(store, crate::bind(port)?)?)));
    let (store, url) = match started {
        Ok(started) => started,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    println!("{READY} {url}");
    if exit_with_stdin {
        let mut sink = [0; 1024];
        let mut stdin = std::io::stdin();
        while stdin.read(&mut sink).is_ok_and(|n| n > 0) {}
        store.shutdown();
        return ExitCode::SUCCESS;
    }
    loop {
        std::thread::park();
    }
}

pub fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Transcribe { paths, model, language, timestamps, force, stdout } => {
            transcribe(&paths, &model, &language, timestamps, force, stdout)
        }
        Command::Models { action } => manage_models(action),
        Command::Serve { port, exit_with_stdin } => serve(port, exit_with_stdin),
    }
}
