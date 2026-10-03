#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    if let (Some(_), Some(dir)) = (std::env::var_os("APPIMAGE"), std::env::var_os("OWD")) {
        let _ = std::env::set_current_dir(dir);
    }
    if std::env::args_os().len() > 1 {
        return vscribe_lib::cli::main();
    }
    vscribe_lib::run();
    std::process::ExitCode::SUCCESS
}
