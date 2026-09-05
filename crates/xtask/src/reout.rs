//! `xtask reout` 日常产物重出：改 Rust 后「重编 dll → 拷贝 → sync-bindings → 重出
//! yio.exe + yio_gui.exe → 重打 showcase bundle → 重打 HeadlessTests fixture 包」
//! 一条命令（AGENTS 的 Rust→dll 闭环机械化）。GUI exe 与其余产物**无条件同批重出**
//! ——它直链 yio_pkg/yio_fence（dev fallback 进程内路径），判据省不掉遗漏风险，
//! 直接无判据。
//!
//! bundle / fixtures 无条件重打 + 字节对比幂等——parse-time 逻辑与 pkg 格式变更
//! 与否难静态判定，难判定就无条件做（旧坑 66：改 parse-time 只重编 dll 不够；
//! v48 bump 漏打 fixture 的实证：0.0.16 起 HeadlessTests 装载全炸、CI 红到下批
//! 才定位）。
//!
//! 不自动提交：日常开发批可能还要带源码一起提交，这里只重出 + 报告待提交清单，
//! 提交时机留给操作者。

use crate::git::git;
use crate::paths;
use crate::release::{run_cargo, unity_dll_locked};
use std::path::{Path, PathBuf};
use std::process::Command;

const ARTIFACT_PATHS: [&str; 6] = [
    "unity/package/Plugins/Yio/yio_ffi_c.dll",
    "unity/package/Editor/Tools/yio.exe",
    "unity/package/Editor/Tools/yio_gui.exe",
    "unity/showcase-unity/Assets/Bundles/ui/showcase.pkg.bin",
    "unity/showcase-unity/Assets/Bundles/yio.runtime.json",
    // fixture pkg（pathspec 覆盖目录：格式 bump 后全部 .pkg.bin 换字节）
    "tests/dotnet/Yio.HeadlessTests/fixtures",
];

/// HeadlessTests fixture 根：每个 `<name>.workspace/` 的构建产物拷成
/// `<name>.pkg.bin`（静态 `fonts/` 与构建现场 `*-ws-out/` 不在此列）。
const FIXTURES_DIR: &str = "tests/dotnet/Yio.HeadlessTests/fixtures";

pub fn run_reout(dry_run: bool) -> Result<(), Box<dyn std::error::Error>> {
    let root = paths::repo_root();
    println!("== reout (dry_run={dry_run}) ==");

    let dll = root.join("unity/package/Plugins/Yio/yio_ffi_c.dll");
    if unity_dll_locked(&dll) {
        return Err(
            "yio_ffi_c.dll is locked (Unity Editor open with the project?) — \
                    close Unity and re-run"
                .into(),
        );
    }
    if dry_run {
        println!(
            "[dry-run] dll not locked. plan: build release dll+exe+gui -> copy -> \
                  sync-bindings -> rebuild showcase bundle -> rebuild HeadlessTests \
                  fixtures -> report dirty artifact paths"
        );
        return Ok(());
    }

    run_cargo(
        &root,
        &["build", "-p", "yio_ffi_c", "-p", "yio_pkg", "--release"],
        true,
    )
    .or_else(|e| {
        eprintln!("[build] offline failed ({e}); retrying online");
        run_cargo(
            &root,
            &["build", "-p", "yio_ffi_c", "-p", "yio_pkg", "--release"],
            false,
        )
    })?;

    let target = root.join("target/release");
    std::fs::copy(target.join("yio_ffi_c.dll"), &dll)?;
    std::fs::copy(
        target.join("yio.exe"),
        root.join("unity/package/Editor/Tools/yio.exe"),
    )?;
    println!("[copy] yio_ffi_c.dll + yio.exe -> unity/package");

    let gui_exe = crate::gui::build_gui(&root, &root.join("target"))?;
    std::fs::copy(
        &gui_exe,
        root.join("unity/package/Editor/Tools/yio_gui.exe"),
    )?;
    println!("[copy] yio_gui.exe -> unity/package");

    crate::bindings::sync_bindings()?;
    println!("[bindings] synced");

    let bundle = Command::new(target.join("yio.exe"))
        .args(["build", "showcase"])
        .current_dir(&root)
        .output()
        .map_err(|e| format!("run yio build showcase: {e}"))?;
    if !bundle.status.success() {
        return Err(format!(
            "yio build showcase failed: {}",
            String::from_utf8_lossy(&bundle.stderr)
        )
        .into());
    }
    println!("[bundle] showcase rebuilt");

    rebuild_headless_fixtures(&root, &target.join("yio.exe"))?;
    println!("[fixtures] HeadlessTests fixture pkg rebuilt");

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

/// 重打 HeadlessTests 的全部 fixture 包：每个 `<name>.workspace/` 跑 `yio build`，
/// 把 `ui/*.pkg.bin` 拷回 `fixtures/<name>.pkg.bin`，构建现场（`*-ws-out/`）用后即删
/// ——那是手动构建时代的残留形态（历史实证：fixtures/ 里躺过一个没人清的
/// dropdown-ws-out）。json 的 `output_dir` 是权威（不硬编码 `-ws-out` 约定）。
fn rebuild_headless_fixtures(root: &Path, exe: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let dir = root.join(FIXTURES_DIR);
    let mut workspaces: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("read {FIXTURES_DIR}: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.extension().is_some_and(|x| x == "workspace"))
        .collect();
    workspaces.sort(); // 确定性顺序（报告/失败定位稳定）

    if workspaces.is_empty() {
        return Err(format!("no *.workspace found under {FIXTURES_DIR}").into());
    }

    for ws in &workspaces {
        let out = Command::new(exe)
            .arg("build")
            .arg(ws)
            .current_dir(root)
            .output()
            .map_err(|e| format!("run yio build {}: {e}", ws.display()))?;
        if !out.status.success() {
            return Err(format!(
                "yio build {} failed: {}",
                ws.display(),
                String::from_utf8_lossy(&out.stderr)
            )
            .into());
        }
        // output_dir 相对 workspace 目录解析（fixture json 形如 "../<name>-ws-out"）。
        let cfg: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(ws.join("yio.workspace.json"))
                .map_err(|e| format!("read {}: {e}", ws.join("yio.workspace.json").display()))?,
        )
        .map_err(|e| format!("parse {}: {e}", ws.display()))?;
        let output_dir = cfg["output_dir"]
            .as_str()
            .ok_or(format!("{}: output_dir missing", ws.display()))?;
        let ws_out = ws.join(output_dir);

        let ui = ws_out.join("ui");
        let mut pkgs: Vec<PathBuf> = std::fs::read_dir(&ui)
            .map_err(|e| format!("read {}: {e}（build 未产 ui/*.pkg.bin？）", ui.display()))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "bin"))
            .collect();
        pkgs.sort();
        if pkgs.is_empty() {
            return Err(format!("{}: build produced no .pkg.bin", ws.display()).into());
        }
        for pkg in pkgs {
            let dest = dir.join(pkg.file_name().unwrap());
            std::fs::copy(&pkg, &dest)?;
        }
        std::fs::remove_dir_all(&ws_out)?;
    }
    Ok(())
}
