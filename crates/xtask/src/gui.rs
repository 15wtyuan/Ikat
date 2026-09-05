//! GUI exe（yio_gui.exe，Tauri 2）构建：与 dll/yio.exe **无条件一同重出**——
//! GUI 直链 yio_pkg/yio_fence（dev fallback 进程内路径），任何 pkg/fence 改动都编进
//! exe 字节；旧「仅 Workspace struct / GUI 自身代码变动才重出」的收窄判据漏掉这条
//! 内嵌路径，且判据本身就是心智负担。无判据 = 无遗漏。
//!
//! 前置：tauri CLI 在 PATH（`npm install -g @tauri-apps/cli`，prebuilt）。必须走
//! tauri CLI 而非裸 cargo build——前端资产 embed 由 CLI 驱动，裸编出 localhost 白屏
//! exe。`--no-bundle` 跳 NSIS/MSI（仓库只要裸 exe）；产物落 workspace 根
//! target/release/yio_gui.exe（GUI 是 workspace 成员，target 走 CARGO_TARGET_DIR）。

use std::path::{Path, PathBuf};
use std::process::Command;

pub const GUI_EXE_NAME: &str = "yio_gui.exe";

/// 在指定 repo 根（主工作树或发版干净 worktree）构建 GUI exe。
/// `target_dir` 传主仓 target（缓存复用，与 run_cargo 同一口径）。
pub fn build_gui(repo_root: &Path, target_dir: &Path) -> Result<PathBuf, String> {
    let src_tauri = repo_root.join("crates/packer/gui/src-tauri");
    if !src_tauri.join("tauri.conf.json").exists() {
        return Err(format!(
            "tauri.conf.json not found at {} — GUI crate moved?",
            src_tauri.display()
        ));
    }
    // npm 全局装的是 tauri.cmd shim：Windows CreateProcess 对裸名只补 .exe，cmd/bat
    // shim 必须显式点名（std 会自动包 cmd /C）。先试裸名（cargo install 的 tauri.exe），
    // spawn 失败再回落 .cmd（npm prebuilt）。
    let out = Command::new("tauri")
        .args(["build", "--no-bundle"])
        .current_dir(&src_tauri)
        .env("CARGO_TARGET_DIR", target_dir)
        .env_remove("CARGO_TERM_COLOR")
        .output()
        .or_else(|_| {
            Command::new(if cfg!(windows) { "tauri.cmd" } else { "tauri" })
                .args(["build", "--no-bundle"])
                .current_dir(&src_tauri)
                .env("CARGO_TARGET_DIR", target_dir)
                .env_remove("CARGO_TERM_COLOR")
                .output()
        })
        .map_err(|e| {
            format!(
                "tauri CLI not runnable: {e} — install with \
                 `npm install -g @tauri-apps/cli` (prebuilt)"
            )
        })?;
    if !out.status.success() {
        return Err(format!(
            "tauri build --no-bundle failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let exe = target_dir.join("release").join(GUI_EXE_NAME);
    if !exe.exists() {
        return Err(format!(
            "tauri build reported success but {} missing",
            exe.display()
        ));
    }
    Ok(exe)
}
