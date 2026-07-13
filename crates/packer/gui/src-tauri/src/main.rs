//! LoomGUI packer GUI (Tauri shell). Commands call loomgui_pkg directly,
//! sharing the same build() as the CLI.

// Release 下隐藏 Windows 控制台黑窗（GUI 应用，不需要终端）。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod recent;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::recent_workspaces,
            commands::open_workspace,
            commands::create_workspace,
            commands::save_workspace,
            commands::init_workspace,
            commands::scan_html,
            commands::scan_pngs,
            commands::run_build,
            commands::relativize,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
