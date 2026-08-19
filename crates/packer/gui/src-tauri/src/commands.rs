//! Tauri commands: workspace 读写、recent 列表、HTML 扫描、构建。
//!
//! build / init 语义走 `loom` CLI 子进程（与 agent 会话同一二进制、同一版本——
//! GUI 不再自带打包语义）；workspace 表单读写仍走库（人类检查 AI 配置的驾驶舱）。
//! 找不到 loom.exe（dev 模式）时降级进程内调用并注明。

use crate::recent;
use crate::recent::StateDir;
use crate::UnityRoot;
use loomgui_pkg::build::{build, BuildReport};
use loomgui_pkg::config;
use loomgui_pkg::diag::BuildFailure;
use loomgui_pkg::init::{init, CliSource, InitOptions};
use loomgui_pkg::workspace::{load_workspace, save_workspace as write_workspace, Workspace};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 打开/新建工作区的返回：workspace + 解析出的 ui 路径。用户给 GUI 的可能是会话
/// 根（`.loom/config.json` 所在），workspace.json 实际在 `ui_root` 指向处——前端
/// 后续的 save / scan / build 一律用 `ui_path`。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedWorkspace {
    pub ws: Workspace,
    pub ui_path: String,
}

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
pub fn open_workspace(
    path: String,
    state: tauri::State<StateDir>,
) -> Result<OpenedWorkspace, String> {
    let located = config::locate(Path::new(&path)).map_err(|e| e.message)?;
    let ws = load_workspace(&located.ui)?;
    recent::push_recent(state.0.as_deref(), &path);
    Ok(OpenedWorkspace {
        ws,
        ui_path: located.ui.to_string_lossy().into_owned(),
    })
}

/// 新建工作区 = 完整 init：会话根上 skills + CLI + config（`--ui` 分离 ui 目录）+
/// workspace.json 骨架；`--unity-root` 来自 Unity 菜单拉起时的工程根。agent 选择由
/// 前端弹窗传入；目标已有 workspace.json 时拒绝（防误覆盖，走「打开工作区」）。
#[tauri::command]
pub fn create_workspace(
    path: String,
    ui_dir: String,
    agents: Vec<String>,
    state: tauri::State<StateDir>,
    unity_root: tauri::State<UnityRoot>,
) -> Result<OpenedWorkspace, String> {
    let root = Path::new(&path);
    // 首选子进程（与 agent 会话同一 loom.exe；自拷贝语义天然正确）。
    if let Some(loom) = locate_loom(None) {
        let mut cmd = loom_command(&loom);
        cmd.arg("init")
            .arg(root)
            .arg("--ui")
            .arg(&ui_dir)
            .arg("--output")
            .arg("Assets/Bundles");
        for a in &agents {
            cmd.arg("--agent").arg(a);
        }
        if let Some(u) = &unity_root.0 {
            cmd.arg("--unity-root").arg(u);
        }
        let out = run_capture(cmd).map_err(|e| format!("spawn loom init: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "loom init 失败（exit {}）：{}",
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    } else {
        // dev fallback：进程内 init；CLI 源用 GUI 同目录的 loom（若有），否则跳过拷贝。
        let cli_source = std::env::current_exe()
            .ok()
            .and_then(|g| g.parent().map(|d| d.join(loom_file_name())))
            .filter(|p| p.is_file())
            .map(CliSource::Explicit)
            .unwrap_or(CliSource::Skip);
        let opts = InitOptions {
            agents,
            ui_dir: Some(PathBuf::from(&ui_dir)),
            unity_root: unity_root.0.clone(),
            output_dir: "Assets/Bundles".to_string(),
            force: false,
            cli_source,
        };
        init(root, opts).map_err(|f| format!("init failed: {}", f.message))?;
    }
    recent::push_recent(state.0.as_deref(), &path);
    let located = config::locate(root).map_err(|e| e.message)?;
    let ws = load_workspace(&located.ui)?;
    Ok(OpenedWorkspace {
        ws,
        ui_path: located.ui.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub fn save_workspace(path: String, ws: Workspace) -> Result<(), String> {
    write_workspace(Path::new(&path), &ws)
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

/// 打包走 `loom build` 子进程（stdout JSON → 还原 BuildReport）；失败时 message +
/// 诊断文本化给前端。找不到 loom.exe（dev 模式 target 目录分离）→ 进程内降级。
/// 入参 path 接受会话根或 ui 目录（config 发现统一解析）。
#[tauri::command]
pub fn run_build(path: String) -> Result<BuildReport, String> {
    let located = config::locate(Path::new(&path)).map_err(|e| e.message)?;
    let Some(loom) = locate_loom(Some(&located.root)) else {
        eprintln!("dev fallback: loom.exe not found next to GUI / in .loom/ — in-process build");
        return build(&located.ui).map_err(|f| failure_to_text(&f));
    };
    let mut cmd = loom_command(&loom);
    cmd.arg("build")
        .arg(&located.ui)
        .arg("--format")
        .arg("json");
    let out = run_capture(cmd).map_err(|e| format!("spawn loom build: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| format!("parse loom build output: {e}\nstdout: {stdout}"))?;
    if json["success"].as_bool() == Some(true) {
        serde_json::from_value(json["report"].clone())
            .map_err(|e| format!("decode BuildReport: {e}"))
    } else {
        let msg = json["message"]
            .as_str()
            .unwrap_or("unknown loom build failure");
        let mut text = msg.to_string();
        if let Some(diags) = json["diagnostics"].as_array() {
            for d in diags {
                let sev = d["severity"].as_str().unwrap_or("error");
                let code = d["code"].as_str().unwrap_or("?");
                let file = d["file"].as_str().unwrap_or("?");
                let line = d["line"].as_u64().unwrap_or(0);
                let col = d["column"].as_u64().unwrap_or(0);
                let m = d["message"].as_str().unwrap_or("");
                text.push_str(&format!("\n{sev}[{code}]: {m} ({file}:{line}:{col})"));
            }
        }
        Err(text)
    }
}

/// 把 BuildFailure 文本化（dev fallback 的失败路径与子进程路径同形态）。
fn failure_to_text(f: &BuildFailure) -> String {
    let mut text = f.message.clone();
    for d in &f.diagnostics {
        text.push('\n');
        text.push_str(&d.render());
    }
    text
}

fn loom_file_name() -> String {
    if cfg!(windows) {
        "loom.exe".to_string()
    } else {
        "loom".to_string()
    }
}

/// 定位 loom CLI：(1) GUI 同目录（release = Editor/Tools 双 exe，版本配套）
/// (2) `<workspace>/.loom/`。找不到 = None（dev 模式 target 目录分离，调用方降级）。
fn locate_loom(workspace: Option<&Path>) -> Option<PathBuf> {
    let name = loom_file_name();
    if let Ok(exe) = std::env::current_exe() {
        let sibling = exe.parent()?.join(&name);
        if sibling.is_file() {
            return Some(sibling);
        }
    }
    if let Some(ws) = workspace {
        let bundled = ws.join(".loom").join(&name);
        if bundled.is_file() {
            return Some(bundled);
        }
    }
    None
}

/// 构造 loom 子进程命令（Windows 下 CREATE_NO_WINDOW 防 GUI 黑窗闪烁）。
fn loom_command(exe: &Path) -> Command {
    let mut cmd = Command::new(exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// 捕获 stdout/stderr 跑完子进程。
fn run_capture(mut cmd: Command) -> std::io::Result<std::process::Output> {
    cmd.output()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_root(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("loomgui_gui_test_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scaffold_writes_skills_only_no_instruction_doc() {
        let root = temp_root("layout");
        loomgui_pkg::scaffold::write_agent_scaffold(
            &root,
            &["claude".to_string(), "agents".to_string()],
        )
        .unwrap();
        for skills in [".claude/skills", ".agents/skills"] {
            assert!(
                root.join(skills).join("loomgui-editor/SKILL.md").is_file(),
                "{skills} 的 editor skill 须落位"
            );
            assert!(
                root.join(skills)
                    .join("loomgui-editor/references/patterns.md")
                    .is_file(),
                "{skills} 的 editor references 须落位"
            );
            assert!(
                root.join(skills).join("loomgui-runtime/SKILL.md").is_file(),
                "{skills} 的 runtime skill 须落位"
            );
            assert!(root.join(skills).join("loom/SKILL.md").is_file());
        }
        // 不生成指令文档（AGENTS.md / CLAUDE.md 由用户自持）。
        assert!(!root.join("AGENTS.md").exists());
        assert!(!root.join("CLAUDE.md").exists());

        assert!(loomgui_pkg::scaffold::write_agent_scaffold(&root, &[]).is_err());
        assert!(
            loomgui_pkg::scaffold::write_agent_scaffold(&root, &["cursor".to_string()]).is_err()
        );
        fs::remove_dir_all(&root).unwrap();
    }
}
