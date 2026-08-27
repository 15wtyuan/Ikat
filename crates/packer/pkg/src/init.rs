//! `ikat init`：初始化工作区——会话根上落 agent skills + CLI 自拷贝 + 接线 config
//! （`.ikat/config.json`），ui 目录上落 workspace.json 骨架。
//!
//! 产出即自足：游戏仓库的 agent 会话零安装（`.ikat/ikat(.exe)` 就地可用），打开
//! 即知道怎么干（skills + config 指针）。分离形态（`--ui`）下会话根 ≠ ui 目录，
//! 单目录形态（无 `--ui`）下根即 ui 工作区——config 的 `ui_root = "."`。

use crate::diag::BuildFailure;
use crate::workspace::{save_workspace, Workspace};
use std::path::{Path, PathBuf};

/// `.ikat/` 里 CLI 二进制的来源。
pub enum CliSource {
    /// 拷 `current_exe`（CLI 自身跑 init 的正常形态）。
    CurrentExe,
    /// 从指定路径拷（GUI dev fallback：GUI 同目录的 ikat.exe）。
    Explicit(PathBuf),
    /// 不拷（找不到 ikat 二进制——工作区靠 PATH / Release 下载位兜底）。
    Skip,
}

pub struct InitOptions {
    /// agent 种类（"claude" / "agents"，可多个）。空列表默认 ["agents"]。
    pub agents: Vec<String>,
    /// ui 工作区位置（相对根目录的路径或绝对路径）。None = 单目录形态（根即 ui）。
    pub ui_dir: Option<PathBuf>,
    /// Unity 工程根：写入 `.ikat/config.json` 的 `unity_root`（内部相对化；None = 不写，
    /// 本地输出）。
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
            ui_dir: None,
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
    /// 会话根（`.ikat/` 与 skills 所在）。
    pub root: PathBuf,
    /// ui 工作区（`ikat.workspace.json` 所在）。
    pub ui: PathBuf,
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
    // 词法绝对化（相对 cwd 的调用路径）：指针相对化与 InitOutcome 都需要稳定的绝对
    // 基。用 std::path::absolute 而非 canonicalize——后者在 Windows 产 `\\?\` verbatim
    // 前缀，会让指针的盘符比较失配（相对化静默失效）。
    let root: PathBuf = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    let ui: PathBuf = match &opts.ui_dir {
        // 绝对 ui_dir 覆盖 join；相对 ui_dir 基于根解析。
        Some(u) => std::path::absolute(root.join(u)).unwrap_or_else(|_| root.join(u)),
        None => root.clone(),
    };
    std::fs::create_dir_all(&ui).map_err(|e| format!("create {}: {e}", ui.display()))?;
    let ws_path = ui.join(crate::workspace::WORKSPACE_FILE);
    if ws_path.exists() && !opts.force {
        return Err(BuildFailure::config(format!(
            "{} already exists; pass --force to overwrite",
            ws_path.display()
        )));
    }

    // workspace.json 骨架（ui 目录——构建宇宙的路径基准不变）。
    save_workspace(
        &ui,
        &Workspace {
            version: 1,
            output_dir: opts.output_dir.clone(),
            design: None,
            match_mode: None,
            packages: Vec::new(),
            atlases: Vec::new(),
            fonts: Vec::new(),
        },
    )?;

    // agent skills（会话根——覆盖式，模板升级后重跑 init / scaffold 即刷新）。
    let agents = if opts.agents.is_empty() {
        vec!["agents".to_string()]
    } else {
        opts.agents.clone()
    };
    crate::scaffold::write_agent_scaffold(&root, &agents)?;

    // CLI 自拷贝 + 接线 config + 版本戳（同住根上 .ikat/：整个目录入库，团队 clone 即得）。
    let cli_copied = copy_cli_into(&root, &opts.cli_source);
    let _ = std::fs::write(
        root.join(".ikat").join(crate::scaffold::VERSION_STAMP),
        crate::scaffold::IKAT_VERSION,
    );
    crate::config::write(&root, &ui, opts.unity_root.as_deref()).map_err(BuildFailure::config)?;
    let unity_root_written = opts.unity_root.is_some();

    Ok(InitOutcome {
        root,
        ui,
        agents,
        unity_root_written,
        cli_copied,
    })
}

/// 拷 CLI 二进制到 `<root>/.ikat/`。失败（如 exe 被锁）不阻断 init——工作区已可用
///（PATH 里的 ikat / Release 下载位是兜底），CLI 缺席由调用方提示。
fn copy_cli_into(root: &Path, source: &CliSource) -> bool {
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
    let ikat_dir = root.join(".ikat");
    if std::fs::create_dir_all(&ikat_dir).is_err() {
        return false;
    }
    let dst = ikat_dir.join(src.file_name().unwrap_or_default());
    if std::fs::copy(&src, &dst).is_ok() {
        return true;
    }
    let _ = std::fs::remove_file(&dst);
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 分离形态全流程：根上 skills/.ikat/config，ui 上 workspace.json；指针相对化。
    #[test]
    fn init_split_layout() {
        let tmp = std::env::temp_dir().join(format!("ikat_init_split_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let unity = tmp.join("unity");
        std::fs::create_dir_all(&unity).unwrap();

        let out = init(
            &tmp,
            InitOptions {
                agents: vec!["agents".to_string()],
                ui_dir: Some(PathBuf::from("ui")),
                unity_root: Some(unity.clone()),
                output_dir: "Assets/Bundles".to_string(),
                force: false,
                cli_source: CliSource::Skip, // 单测不拷二进制
            },
        )
        .unwrap();
        assert_eq!(out.ui, tmp.join("ui"));
        assert_eq!(out.root, tmp);
        assert!(out.unity_root_written);
        // ui 目录：workspace.json。
        assert!(tmp.join("ui/ikat.workspace.json").exists());
        // 会话根：skills + config（无指令文档——AGENTS.md 不再生成）。
        assert!(tmp.join(".agents/skills/ikat-editor/SKILL.md").exists());
        assert!(!tmp.join("ui/AGENTS.md").exists());
        assert!(!tmp.join("AGENTS.md").exists());
        let cfg: crate::config::IkatConfig = serde_json::from_str(
            &std::fs::read_to_string(tmp.join(crate::config::CONFIG_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(cfg.ui_root, "ui");
        assert_eq!(cfg.unity_root.as_deref(), Some("unity"));

        // 定位：从根、从 ui 目录都解析到同一工作区。
        let loc = crate::config::locate(&tmp).unwrap();
        assert_eq!(loc.ui, tmp.join("ui"));

        // 重复 init 拒绝（workspace.json 已在 ui 上）；--force 覆盖。
        let err = init(
            &tmp,
            InitOptions {
                ui_dir: Some(PathBuf::from("ui")),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(err.exit_code, 2);
        init(
            &tmp,
            InitOptions {
                ui_dir: Some(PathBuf::from("ui")),
                force: true,
                ..Default::default()
            },
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 单目录形态（无 --ui）：根即 ui，ui_root = "."，config 就近命中。
    #[test]
    fn init_standalone_layout() {
        let tmp = std::env::temp_dir().join(format!("ikat_init_solo_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let out = init(&tmp, InitOptions::default()).unwrap();
        assert_eq!(out.root, tmp);
        assert_eq!(out.ui, tmp);
        assert!(tmp.join("ikat.workspace.json").exists());
        let cfg: crate::config::IkatConfig = serde_json::from_str(
            &std::fs::read_to_string(tmp.join(crate::config::CONFIG_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(cfg.ui_root, ".");
        assert_eq!(cfg.unity_root, None);
        let loc = crate::config::locate(&tmp).unwrap();
        assert_eq!(loc.ui, tmp);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
