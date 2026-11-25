#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Dual-mode single-binary support: when spawned with `--elevated-worker`,
    // immediately enter the headless Named Pipe worker loop without touching WebView2/Tauri.
    if args.iter().any(|a| a == "--elevated-worker") {
        let mut pipe = String::new();
        let mut token = String::new();
        let mut parent_pid: u32 = 0;

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--pipe" if i + 1 < args.len() => {
                    pipe = args[i + 1].clone();
                    i += 2;
                }
                "--token" if i + 1 < args.len() => {
                    token = args[i + 1].clone();
                    i += 2;
                }
                "--parent-pid" if i + 1 < args.len() => {
                    parent_pid = args[i + 1].parse().unwrap_or(0);
                    i += 2;
                }
                _ => i += 1,
            }
        }

        if !pipe.is_empty() && !token.is_empty() {
            let _ = unbind_core::ipc::run_elevated_worker(&pipe, &token, parent_pid);
        }
        return;
    }

    // If the user explicitly launched the UI as Administrator, isolate WebView2 user data dir
    // to prevent Windows 11 Administrator Protection EBWebView permission lockouts.
    if unbind_core::safety::is_process_elevated() && std::env::var("WEBVIEW2_USER_DATA_FOLDER").is_err() {
        let storage = unbind_core::portable::AppStorage::detect();
        let wv2_dir: PathBuf = storage.data_dir.join("WebView2Admin");
        let _ = std::fs::create_dir_all(&wv2_dir);
        std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", wv2_dir);
    }

    unbind_lib::run();
}
