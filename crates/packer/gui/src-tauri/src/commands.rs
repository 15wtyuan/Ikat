//! Tauri commands: workspace 读写、recent 列表、HTML 扫描、构建。

use crate::recent;
use crate::recent::StateDir;
use loomgui_pkg::build::{build, BuildReport};
use loomgui_pkg::workspace::{load_workspace, save_workspace as write_workspace, Workspace};
use std::fs;
use std::path::Path;

#[tauri::command]
pub fn recent_workspaces(state: tauri::State<StateDir>) -> Vec<String> {
    recent::load_recent(state.0.as_deref())
}

/// 从最近列表移除一条（只删记录，不删工作区目录）。
#[tauri::command]
pub fn remove_recent(path: String, state: tauri::State<StateDir>) {
    recent::remove_recent(state.0.as_deref(), &path);
}

#[tauri::command]
pub fn open_workspace(path: String, state: tauri::State<StateDir>) -> Result<Workspace, String> {
    let ws = load_workspace(Path::new(&path))?;
    recent::push_recent(state.0.as_deref(), &path);
    Ok(ws)
}

#[tauri::command]
pub fn create_workspace(path: String, state: tauri::State<StateDir>) -> Result<Workspace, String> {
    let ws = Workspace {
        version: 1,
        output_dir: String::new(),
        packages: vec![],
        atlases: vec![],
        fonts: vec![],
    };
    let root = Path::new(&path);
    fs::create_dir_all(root).map_err(|e| format!("create dir: {e}"))?;
    write_workspace(root, &ws).map_err(|e| format!("save workspace: {e}"))?;

    // Inject workspace CLAUDE.md and loomgui-editor skill from templates.
    let claude_md = include_str!("../templates/workspace-CLAUDE.md");
    fs::write(root.join("CLAUDE.md"), claude_md).map_err(|e| format!("write CLAUDE.md: {e}"))?;

    let skill_dir = root.join(".claude").join("skills").join("loomgui-editor");
    fs::create_dir_all(&skill_dir).map_err(|e| format!("create skill dir: {e}"))?;
    let skill_md = include_str!("../templates/skill/SKILL.md");
    fs::write(skill_dir.join("SKILL.md"), skill_md).map_err(|e| format!("write SKILL.md: {e}"))?;

    recent::push_recent(state.0.as_deref(), &path);
    Ok(ws)
}

#[tauri::command]
pub fn save_workspace(path: String, ws: Workspace) -> Result<(), String> {
    write_workspace(Path::new(&path), &ws)
}

/// 补齐 / 更新工作区脚手架（CLAUDE.md + loomgui-editor skill），
/// 从 templates 覆盖拷入。不碰 workspace.json 和源文件。
#[tauri::command]
pub fn init_workspace(path: String) -> Result<(), String> {
    let root = Path::new(&path);
    if !root.is_dir() {
        return Err(format!("workspace dir not found: {}", root.display()));
    }
    let claude_md = include_str!("../templates/workspace-CLAUDE.md");
    fs::write(root.join("CLAUDE.md"), claude_md).map_err(|e| format!("write CLAUDE.md: {e}"))?;
    let skill_dir = root.join(".claude").join("skills").join("loomgui-editor");
    fs::create_dir_all(&skill_dir).map_err(|e| format!("create skill dir: {e}"))?;
    let skill_md = include_str!("../templates/skill/SKILL.md");
    fs::write(skill_dir.join("SKILL.md"), skill_md).map_err(|e| format!("write SKILL.md: {e}"))?;
    Ok(())
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
pub fn scan_pngs(pkg_dir: String) -> Result<Vec<String>, String> {
    let dir = Path::new(&pkg_dir);
    let mut pngs = Vec::new();
    if !dir.is_dir() {
        return Ok(pngs);
    }
    for entry in fs::read_dir(dir).map_err(|e| format!("read dir: {e}"))? {
        let entry = entry.map_err(|e| format!("entry: {e}"))?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("png") {
            pngs.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    pngs.sort();
    Ok(pngs)
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
    // 工作区内：直接 strip
    if let Ok(rel) = abs_path.strip_prefix(root_path) {
        return Ok(rel.to_string_lossy().replace('\\', "/"));
    }
    // 工作区外（如输出目录在 ../）：算含 ../ 的相对路径
    let abs_c: Vec<_> = abs_path.components().collect();
    let root_c: Vec<_> = root_path.components().collect();
    let mut common = 0;
    while common < abs_c.len() && common < root_c.len() && abs_c[common] == root_c[common] {
        common += 1;
    }
    let mut out = std::path::PathBuf::new();
    for _ in common..root_c.len() {
        out.push("..");
    }
    for c in &abs_c[common..] {
        out.push(c);
    }
    Ok(out.to_string_lossy().replace('\\', "/"))
}
