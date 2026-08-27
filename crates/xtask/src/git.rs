//! git 子进程薄封装（xtask 内统一出口：错误带完整命令行，失败可归因）。

use std::process::Command;

/// 跑 git 命令，返 stdout（trim）。非 0 退出码 → Err（携带 stderr 摘要）。
pub fn git(args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// git 命令退出码探测（不取输出；用于 `merge-base --is-ancestor` 一类布尔问询）。
pub fn git_success(args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
