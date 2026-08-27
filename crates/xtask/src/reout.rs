//! `xtask reout` 日常产物重出：改 Rust 后「重编 dll → 拷贝 → sync-bindings → 重出
//! ikat.exe → 重打 showcase bundle」一条命令（AGENTS 的 Rust→dll 闭环机械化）。
//!
//! bundle 无条件重打 + 字节对比幂等——parse-time 逻辑变更与否难静态判定，难判定就
//! 无条件做（旧坑 66：改 parse-time 只重编 dll 不够）。GUI exe（ikat_gui.exe）重出
//! 判据不变（Workspace struct / GUI 自身代码变动才重出，人工判断），不在本命令管辖。
//!
//! 不自动提交：日常开发批可能还要带源码一起提交，这里只重出 + 报告待提交清单，
//! 提交时机留给操作者。

use crate::git::git;
use crate::paths;
use crate::release::{run_cargo, unity_dll_locked};
use std::process::Command;

const ARTIFACT_PATHS: [&str; 4] = [
    "unity/package/Plugins/Ikat/ikat_ffi_c.dll",
    "unity/package/Editor/Tools/ikat.exe",
    "unity/showcase-unity/Assets/Bundles/ui/showcase.pkg.bin",
    "unity/showcase-unity/Assets/Bundles/ikat.runtime.json",
];

pub fn run_reout(dry_run: bool) -> Result<(), Box<dyn std::error::Error>> {
    let root = paths::repo_root();
    println!("== reout (dry_run={dry_run}) ==");

    let dll = root.join("unity/package/Plugins/Ikat/ikat_ffi_c.dll");
    if unity_dll_locked(&dll) {
        return Err(
            "ikat_ffi_c.dll is locked (Unity Editor open with the project?) — \
                    close Unity and re-run"
                .into(),
        );
    }
    if dry_run {
        println!(
            "[dry-run] dll not locked. plan: build release dll+exe -> copy -> \
                  sync-bindings -> rebuild showcase bundle -> report dirty artifact paths"
        );
        return Ok(());
    }

    run_cargo(
        &root,
        &["build", "-p", "ikat_ffi_c", "-p", "ikat_pkg", "--release"],
        true,
    )
    .or_else(|e| {
        eprintln!("[build] offline failed ({e}); retrying online");
        run_cargo(
            &root,
            &["build", "-p", "ikat_ffi_c", "-p", "ikat_pkg", "--release"],
            false,
        )
    })?;

    let target = root.join("target/release");
    std::fs::copy(target.join("ikat_ffi_c.dll"), &dll)?;
    std::fs::copy(
        target.join("ikat.exe"),
        root.join("unity/package/Editor/Tools/ikat.exe"),
    )?;
    println!("[copy] ikat_ffi_c.dll + ikat.exe -> unity/package");

    crate::bindings::sync_bindings()?;
    println!("[bindings] synced");

    let bundle = Command::new(target.join("ikat.exe"))
        .args(["build", "showcase"])
        .current_dir(&root)
        .output()
        .map_err(|e| format!("run ikat build showcase: {e}"))?;
    if !bundle.status.success() {
        return Err(format!(
            "ikat build showcase failed: {}",
            String::from_utf8_lossy(&bundle.stderr)
        )
        .into());
    }
    println!("[bundle] showcase rebuilt");

    // 待提交报告：产物路径的 git 状态（并行会话可能同时在途——只报告不自动提交）。
    let mut status_args: Vec<&str> = vec!["status", "--porcelain", "--"];
    status_args.extend_from_slice(&ARTIFACT_PATHS);
    let status = git(&status_args)?;
    if status.is_empty() {
        println!("== reout done: artifacts byte-identical, nothing to commit ==");
    } else {
        println!("== reout done. dirty artifact paths (commit with your batch): ==");
        for l in status.lines() {
            println!("  {l}");
        }
        println!("hint: `cargo run -p xtask -- release-check` now passes the staleness gate");
    }
    Ok(())
}
