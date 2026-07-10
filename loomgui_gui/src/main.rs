//! LoomGUI packer GUI (Tauri shell). Commands call loomgui_pkg directly,
//! sharing the same build() as the CLI.

mod commands;
mod recent;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::recent_workspaces,
            commands::open_workspace,
            commands::create_workspace,
            commands::save_workspace_cmd,
            commands::scan_html,
            commands::run_build,
            commands::relativize,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
