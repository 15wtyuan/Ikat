//! LoomGUI packer GUI (Tauri shell). Commands call loomgui_pkg directly,
//! sharing the same build() as the CLI.

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![]) // Task 18 adds commands
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
