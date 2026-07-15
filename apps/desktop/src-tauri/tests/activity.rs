#![allow(dead_code)]

// Keep the hook tracker independently compilable until it is wired into the
// Tauri entry point. The module's own unit tests run through this target too.
#[path = "../src/activity.rs"]
mod activity;
