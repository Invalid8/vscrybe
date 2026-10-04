#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    if std::env::args_os().len() > 1 {
        if let (Some(_), Some(dir)) = (std::env::var_os("APPIMAGE"), std::env::var_os("OWD")) {
            let _ = std::env::set_current_dir(dir);
        }
        return vscrybe_lib::cli::main();
    }
    vscrybe_lib::run();
    std::process::ExitCode::SUCCESS
}
