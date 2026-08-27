//! Ikat packer GUI (Tauri shell). Build/init semantics go through the
//! `ikat` CLI subprocess; workspace form read/write stays in-process (the
//! human's cockpit for inspecting what the AI configured).

// Release 下隐藏 Windows 控制台黑窗（GUI 应用，不需要终端）。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod recent;

/// Unity 菜单拉起时传入的 Unity 工程根（`--unity-root <path>`）；直接打开 GUI 时
/// 为 None——新建工作区不写反向配置（纯本地输出形态）。
pub struct UnityRoot(pub Option<std::path::PathBuf>);

fn unity_root_from_args(args: &[String]) -> Option<std::path::PathBuf> {
    let mut i = 0;
    while i + 1 < args.len() {
        if args[i] == "--unity-root" {
            return Some(std::path::PathBuf::from(&args[i + 1]));
        }
        i += 1;
    }
    None
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let state_dir = recent::state_dir_from_args(&args);
    let unity_root = unity_root_from_args(&args);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(recent::StateDir(state_dir))
        .manage(UnityRoot(unity_root))
        .invoke_handler(tauri::generate_handler![
            commands::recent_workspaces,
            commands::remove_recent,
            commands::open_workspace,
            commands::create_workspace,
            commands::save_workspace,
            commands::scan_html,
            commands::scan_pngs,
            commands::run_build,
            commands::relativize,
            commands::workspace_update_state,
            commands::update_workspace,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
