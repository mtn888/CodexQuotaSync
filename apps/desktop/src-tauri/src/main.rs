#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().any(|argument| argument == "--activity-hook") {
        if let Err(error) = codex_quota_sync_lib::run_activity_hook() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    codex_quota_sync_lib::run();
}
