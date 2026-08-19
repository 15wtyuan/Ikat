//! `loom init`：初始化 UI 工作区——workspace.json 骨架 + agent 脚手架 + CLI 自拷贝
//! 到 `.loom/` + 反向配置（`.loom/unity.json`，基座链见 unity.rs）。
//!
//! 产出即自足：游戏仓库的 agent 会话零安装（`.loom/loom(.exe)` 就地可用），
//! 打开即知道怎么干（AGENTS.md + skills）。

use crate::diag::BuildFailure;
use crate::workspace::{save_workspace, Workspace};
use std::path::{Path, PathBuf};

/// `.loom/` 里 CLI 二进制的来源。
pub enum CliSource {
    /// 拷 `current_exe`（CLI 自身跑 init 的正常形态）。
    CurrentExe,
    /// 从指定路径拷（GUI dev fallback：GUI 同目录的 loom.exe）。
    Explicit(PathBuf),
    /// 不拷（找不到 loom 二进制——工作区靠 PATH / Release 下载位兜底）。
    Skip,
}

pub struct InitOptions {
    /// agent 种类（"claude" / "agents"，可多个）。空列表默认 ["agents"]。
    pub agents: Vec<String>,
    /// Unity 工程根：写入 `.loom/unity.json`（内部相对化；None = 不写，纯本地输出）。
    pub unity_root: Option<PathBuf>,
    /// workspace.json 的 output_dir 初始值。
    pub output_dir: String,
    /// workspace.json 已存在时覆盖（默认拒绝）。
    pub force: bool,
    /// CLI 自拷贝来源（GUI 库调用时注入；默认 current_exe）。
    pub cli_source: CliSource,
}

impl Default for InitOptions {
    fn default() -> Self {
        Self {
            agents: vec!["agents".to_string()],
            unity_root: None,
            output_dir: "dist".to_string(),
            force: false,
            cli_source: CliSource::CurrentExe,
        }
    }
}

/// init 的产出摘要（CLI 打印后续步骤提示用）。
#[derive(Debug)]
pub struct InitOutcome {
    pub root: PathBuf,
    pub agents: Vec<String>,
    pub unity_root_written: bool,
    pub cli_copied: bool,
}

pub fn init(dir: &Path, opts: InitOptions) -> Result<InitOutcome, BuildFailure> {
    if dir.exists() && !dir.is_dir() {
        return Err(BuildFailure::config(format!(
            "{} exists and is not a directory",
            dir.display()
        )));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    // 词法绝对化（相对 cwd 的调用路径）：unity_root 相对化与 InitOutcome.root 都需要
    // 稳定的绝对基。用 std::path::absolute 而非 canonicalize——后者在 Windows 产
    // `\\?\` verbatim 前缀，会让 unity_root 的盘符比较失配（相对化静默失效）。
    let dir: std::path::PathBuf = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    let ws_path = dir.join(crate::workspace::WORKSPACE_FILE);
    if ws_path.exists() && !opts.force {
        return Err(BuildFailure::config(format!(
            "{} already exists; pass --force to overwrite",
            ws_path.display()
        )));
    }

    // workspace.json 骨架。
    save_workspace(
        &dir,
        &Workspace {
            version: 1,
            output_dir: opts.output_dir.clone(),
            packages: Vec::new(),
            atlases: Vec::new(),
            fonts: Vec::new(),
        },
    )?;

    // agent 脚手架（覆盖式——模板升级后重跑 init 即刷新）。
    let agents = if opts.agents.is_empty() {
        vec!["agents".to_string()]
    } else {
        opts.agents.clone()
    };
    crate::scaffold::write_agent_scaffold(&dir, &agents)?;

    // CLI 自拷贝：`.loom/` 与反向配置同住（游戏仓库 gitignore 二进制、保留 unity.json）。
    let cli_copied = copy_cli_into(&dir, &opts.cli_source);

    // 反向配置（基座）：unity_root 相对化落盘。
    let unity_root_written = match &opts.unity_root {
        Some(u) => {
            crate::unity::write(&dir, u)?;
            true
        }
        None => false,
    };

    Ok(InitOutcome {
        root: dir,
        agents,
        unity_root_written,
        cli_copied,
    })
}

/// 拷 CLI 二进制到 `<dir>/.loom/`。失败（如 exe 被锁）不阻断 init——工作区已可用
///（PATH 里的 loom / Release 下载位是兜底），CLI 缺席由调用方提示。
fn copy_cli_into(dir: &Path, source: &CliSource) -> bool {
    let src: PathBuf = match source {
        CliSource::Skip => return false,
        CliSource::CurrentExe => match std::env::current_exe() {
            Ok(p) => p,
            Err(_) => return false,
        },
        CliSource::Explicit(p) => {
            if !p.is_file() {
                return false;
            }
            p.clone()
        }
    };
    let loom_dir = dir.join(".loom");
    if std::fs::create_dir_all(&loom_dir).is_err() {
        return false;
    }
    let dst = loom_dir.join(src.file_name().unwrap_or_default());
    if std::fs::copy(&src, &dst).is_ok() {
        return true;
    }
    let _ = std::fs::remove_file(&dst);
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// init 全流程：骨架 + 脚手架 + unity.json；重复 init 拒绝、--force 覆盖。
    #[test]
    fn init_creates_workspace_and_refuses_without_force() {
        let tmp = std::env::temp_dir().join(format!("loom_init_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);

        let unity = tmp.parent().unwrap().join("fake-unity");
        std::fs::create_dir_all(&unity).unwrap();

        let ws_dir = tmp.join("ui");
        let out = init(
            &ws_dir,
            InitOptions {
                agents: vec!["agents".to_string()],
                unity_root: Some(unity.clone()),
                output_dir: "Assets/Bundles".to_string(),
                force: false,
                cli_source: CliSource::Skip, // 单测不拷二进制
            },
        )
        .unwrap();
        assert!(out.unity_root_written);
        assert!(ws_dir.join("loom.workspace.json").exists());
        assert!(ws_dir.join("AGENTS.md").exists());
        assert!(ws_dir
            .join(".agents/skills/loomgui-editor/SKILL.md")
            .exists());
        assert!(ws_dir.join(".loom/unity.json").exists());
        // unity_root 相对化落盘（ws = tmp/ui，unity = temp/fake-unity → 上溯两级）。
        let cfg: crate::unity::UnityConfig = serde_json::from_str(
            &std::fs::read_to_string(ws_dir.join(".loom/unity.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(cfg.unity_root, "../../fake-unity");

        // 再次 init 拒绝；--force 覆盖。
        let err = init(&ws_dir, InitOptions::default()).unwrap_err();
        assert_eq!(err.exit_code, 2);
        init(
            &ws_dir,
            InitOptions {
                force: true,
                ..Default::default()
            },
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
