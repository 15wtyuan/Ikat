//! Tauri commands: workspace 读写、recent 列表、HTML 扫描、构建。

use crate::recent;
use loomgui_pkg::build::{build, BuildReport};
use loomgui_pkg::workspace::{load_workspace, save_workspace, Workspace};
use std::fs;
use std::path::Path;

#[tauri::command]
pub fn recent_workspaces() -> Vec<String> {
    recent::load_recent()
}

#[tauri::command]
pub fn open_workspace(path: String) -> Result<Workspace, String> {
    let ws = load_workspace(Path::new(&path))?;
    recent::push_recent(&path);
    Ok(ws)
}

#[tauri::command]
pub fn create_workspace(path: String) -> Result<Workspace, String> {
    let ws = Workspace {
        version: 1,
        output_dir: "../dist".into(),
        packages: vec![],
        atlases: vec![],
        fonts: vec![],
    };
    let root = Path::new(&path);
    fs::create_dir_all(root).map_err(|e| format!("create dir: {e}"))?;
    save_workspace(root, &ws).map_err(|e| format!("save workspace: {e}"))?;

    // Inject workspace CLAUDE.md and loomgui-editor skill from templates.
    let claude_md = include_str!("../templates/workspace-CLAUDE.md");
    fs::write(root.join("CLAUDE.md"), claude_md).map_err(|e| format!("write CLAUDE.md: {e}"))?;

    let skill_dir = root.join(".claude").join("skills").join("loomgui-editor");
    fs::create_dir_all(&skill_dir).map_err(|e| format!("create skill dir: {e}"))?;
    let skill_md = include_str!("../templates/skill/SKILL.md");
    fs::write(skill_dir.join("SKILL.md"), skill_md).map_err(|e| format!("write SKILL.md: {e}"))?;

    recent::push_recent(&path);
    Ok(ws)
}

#[tauri::command(name = "save_workspace")]
pub fn save_workspace_cmd(path: String, ws: Workspace) -> Result<(), String> {
    save_workspace(Path::new(&path), &ws)
}

#[tauri::command]
pub fn scan_html(pkg_dir: String) -> Result<Vec<String>, String> {
    let dir = Path::new(&pkg_dir);
    let mut htmls: Vec<String> = Vec::new();
    let entries = fs::read_dir(dir).map_err(|e| format!("read dir: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("entry error: {e}"))?;
        let name = entry.file_name();
        let fname = name.to_string_lossy();
        let is_file = entry.file_type().map(|ft| ft.is_file()).unwrap_or(false);
        if is_file && fname.ends_with(".html") {
            htmls.push(fname.to_string());
        }
    }
    htmls.sort();
    Ok(htmls)
}

#[tauri::command]
pub fn run_build(path: String) -> Result<BuildReport, String> {
    build(Path::new(&path))
}

/// 将绝对路径转换为相对于 root 的路径（正斜杠）。
/// # Examples
///
/// ```ignore
/// let rel = relativize("/proj/ui", "/proj/ui/showcase/index.html");
/// assert_eq!(rel, "showcase/index.html");
/// ```
#[tauri::command]
pub fn relativize(root: String, abs: String) -> Result<String, String> {
    let root_path = Path::new(&root);
    let abs_path = Path::new(&abs);
    let rel = abs_path
        .strip_prefix(root_path)
        .map_err(|e| format!("strip prefix: {e}"))?;
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    Ok(rel_str)
}
