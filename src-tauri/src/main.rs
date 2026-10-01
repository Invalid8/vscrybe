#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    if std::env::args_os().len() > 1 {
        return vscribe_lib::cli::main();
    }
    vscribe_lib::run();
    std::process::ExitCode::SUCCESS
}
