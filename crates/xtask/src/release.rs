//! `xtask release <ver>` 发版编排：把 AGENTS 发版段的手工流程收敛为一条命令。
//!
//! 设计约束逐条来自踩坑史（注释标注出处）：
//! - 产物必须从 tag 提交快照构建（旧坑 135：bump 与重打分裂两提交、worktree 可能用
//!   旧 crate；共享工作树里并行会话的在途代码会渗进产物字节）。
//! - 临时 worktree 钉本地 HEAD 派生（旧坑 154：baseRef 取 fresh 会丢本地未 push 提交）；
//!   构建回来后复核 HEAD 未被并行会话推进再提交产物（旧坑 147 精神）。
//! - 产物验证看字节与实跑输出，不信「文件变了」（旧坑 135/100：stale 产物本地缓存掩盖）。
//! - tag 存在性精确匹配（现行 pitfalls §3：`git tag` 字典序陷阱，`v0.0.10` < `v0.0.5`）。
//! - push 分步（先 main 后 tag）+ ls-remote 三方核对（AGENTS 移 tag 坑 ②：多 refspec
//!   单推在 main 被拒时 tag 可能已单独上远端）。
//! - 提交一律 pathspec 限定（`git commit -- <paths>`）：并行用户会话共享工作树时只动
//!   本命令管理的文件。
//!
//! 断点续跑：bump 提交后中断（构建失败/HEAD 被推进），重跑同版本号自动进入 resume
//! 语义（跳过 bump，从干净树构建继续）。

use crate::git::{git, git_success};
use crate::paths;
use crate::release_check;
use std::path::Path;
use std::process::Command;

/// 本命令独占管理的路径（bump 4 + 产物 4）。前置检查要求它们全部干净（工作树 + 暂存区），
/// 防并行会话的在途改动被 pathspec 提交捎带。
const OWNED_PATHS: [&str; 8] = [
    "unity/package/package.json",
    "crates/packer/pkg/Cargo.toml",
    "Cargo.lock",
    "unity/package/CHANGELOG.md",
    "unity/package/Plugins/Ikat/ikat_ffi_c.dll",
    "unity/package/Editor/Tools/ikat.exe",
    "unity/showcase-unity/Assets/Bundles/ui/showcase.pkg.bin",
    "unity/showcase-unity/Assets/Bundles/ikat.runtime.json",
];

pub fn run_release(ver: &str, dry_run: bool) -> Result<(), Box<dyn std::error::Error>> {
    // 版本号形态先行（后续所有路径/引用都拿它拼）。
    semver::Version::parse(ver).map_err(|e| format!("invalid version `{ver}`: {e}"))?;
    let tag = format!("v{ver}");
    let root = paths::repo_root();
    println!("== release {ver} (dry_run={dry_run}) ==");

    // ---- 前置检查（全部只读，dry-run 与真跑共用）----
    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"])?;
    if branch != "main" {
        return Err(format!("must run on `main`, currently on `{branch}`").into());
    }
    let head0 = git(&["rev-parse", "HEAD"])?;

    // tag 存在性：本地 + 远端精确匹配（字典序陷阱 → 绝不用 `git tag | tail` 判断）。
    if !git(&["tag", "--list", &tag])?.is_empty() {
        return Err(format!("tag {tag} already exists locally").into());
    }
    let remote_tag = git(&["ls-remote", "--tags", "origin", &format!("refs/tags/{tag}")])?;
    if !remote_tag.is_empty() {
        return Err(format!("tag {tag} already exists on origin").into());
    }

    // 远端同步：origin/main 必须是本地 HEAD 祖先（落后/分叉都拒绝——push 必被拒）。
    git(&["fetch", "origin"])?;
    if !git_success(&["merge-base", "--is-ancestor", "origin/main", "HEAD"]) {
        return Err("origin/main is not an ancestor of local main — pull/rebase first".into());
    }

    // Unity 锁预检：dll 被编辑器持有时拷贝必败，提前报（而不是流程走到一半断）。
    let dll_path = root.join("unity/package/Plugins/Ikat/ikat_ffi_c.dll");
    if unity_dll_locked(&dll_path) {
        return Err(
            "ikat_ffi_c.dll is locked (Unity Editor open with the project?) — \
                    close Unity and re-run"
                .into(),
        );
    }

    // 独占路径干净（工作树 + 暂存区）：并行会话在途改动不许被 pathspec 提交捎带。
    // dry-run 只警告（演练不该被在途内容挡住），真跑才硬门。
    let dirty: Vec<&str> = OWNED_PATHS
        .iter()
        .filter(|p| {
            !git_success(&["diff", "--quiet", "HEAD", "--", p])
                || !git_success(&["diff", "--cached", "--quiet", "--", p])
        })
        .copied()
        .collect();
    if !dirty.is_empty() {
        let msg = format!(
            "release-managed paths have uncommitted changes (parallel session?): [{}]",
            dirty.join(", ")
        );
        if dry_run {
            eprintln!("[warn] {msg}");
        } else {
            return Err(msg.into());
        }
    }

    // CHANGELOG 状态：fresh 需要非空 Unreleased；resume 需要目标版本段已折好。
    let changelog_path = root.join("unity/package/CHANGELOG.md");
    let changelog = fs_read(&changelog_path)?;
    let pkg_json_path = root.join("unity/package/package.json");
    let current_ver = release_check::parse_and_validate_package(&fs_read(&pkg_json_path)?)?.version;
    let resuming = current_ver == ver;
    if resuming {
        if !release_check::changelog_has_version(&changelog, ver) {
            return Err(format!(
                "package.json already at {ver} but CHANGELOG lacks `## [{ver}]` — \
                 bump commit half-applied; fix changelog manually or reset"
            )
            .into());
        }
        println!("[resume] version bump already committed; continuing from artifacts");
    } else if !unreleased_has_entries(&changelog) {
        return Err(
            "CHANGELOG `[Unreleased]` has no entries — nothing to release (fold entries first)"
                .into(),
        );
    }

    if dry_run {
        println!("[dry-run] all preconditions green. plan:");
        println!("  1. bump package.json + pkg Cargo.toml + Cargo.lock -> {ver}");
        println!("  2. fold CHANGELOG `[Unreleased]` into `## [{ver}] - <today>`");
        println!("  3. commit bump (pathspec-limited)");
        println!("  4. temp worktree at bump commit -> build exe/dll + showcase bundle");
        println!("     (CARGO_TARGET_DIR reuses main target cache)");
        println!("  5. verify artifacts (ikat version output + pkg.bin header) -> copy back");
        println!("  6. commit artifacts -> release-check -> tag {tag}");
        println!("  7. push origin main; push origin {tag}; ls-remote cross-check");
        return Ok(());
    }

    // ---- bump（fresh 才走）----
    let bump_sha;
    if resuming {
        bump_sha = head0.clone();
    } else {
        let date = git(&["log", "-1", "--date=short", "--format=%ad"])?;
        let new_json = replace_version_json(&fs_read(&pkg_json_path)?, ver)?;
        fs_write(&pkg_json_path, &new_json)?;
        let cargo_toml_path = root.join("crates/packer/pkg/Cargo.toml");
        let new_toml = replace_version_toml(&fs_read(&cargo_toml_path)?, ver)?;
        fs_write(&cargo_toml_path, &new_toml)?;
        let new_cl = fold_changelog(&fs_read(&changelog_path)?, ver, &date)?;
        fs_write(&changelog_path, &new_cl)?;
        // lock 刷新只跑 metadata（不编译）；--offline 优先（现行 pitfalls §1：在线 cargo
        // 半刷新索引可把 lock 写坏——提交态 lock 全缓存，离线必成）。
        run_cargo(&root, &["metadata", "--format-version", "1"], true)?;
        commit_paths(
            &format!(
                "chore(release): v{ver} — bump unity package + ikat_pkg crate versions, fold changelog"
            ),
            &[
                "unity/package/package.json",
                "crates/packer/pkg/Cargo.toml",
                "Cargo.lock",
                "unity/package/CHANGELOG.md",
            ],
        )?;
        bump_sha = git(&["rev-parse", "HEAD"])?;
        println!("[bump] committed {bump_sha} ({date})");
    }

    // ---- 干净树构建（tag 提交快照 = 唯一产物来源）----
    let wt = root.parent().unwrap().join(".ikat-release-wt");
    if wt.exists() {
        let _ = git(&["worktree", "remove", "--force", wt.to_str().unwrap()]);
    }
    git(&[
        "worktree",
        "add",
        "--detach",
        wt.to_str().unwrap(),
        &bump_sha,
    ])?;
    let build_result = build_artifacts_in(&wt, &root, ver);
    let _ = git(&["worktree", "remove", "--force", wt.to_str().unwrap()]);
    build_result?;

    // HEAD 守卫：构建耗时窗口内并行会话可能推进 main——产物提交必须仍锚在 bump 提交上
    //（否则 tag 打进的提交包含未经本流程验证的他人改动）。
    let head_now = git(&["rev-parse", "HEAD"])?;
    if head_now != bump_sha {
        return Err(format!(
            "HEAD moved during build ({bump_sha} -> {head_now}, parallel session?). \
             Artifacts are copied but uncommitted; bump is committed — re-run to resume"
        )
        .into());
    }

    // ---- 产物提交（只提交实际有差异的路径）----
    let artifact_paths = [
        "unity/package/Plugins/Ikat/ikat_ffi_c.dll",
        "unity/package/Editor/Tools/ikat.exe",
        "unity/showcase-unity/Assets/Bundles/ui/showcase.pkg.bin",
        "unity/showcase-unity/Assets/Bundles/ikat.runtime.json",
    ];
    let changed: Vec<&str> = artifact_paths
        .iter()
        .filter(|p| !git_success(&["diff", "--quiet", "HEAD", "--", p]))
        .copied()
        .collect();
    if changed.is_empty() {
        println!("[artifacts] byte-identical to committed — no artifact commit needed");
    } else {
        commit_paths(
            &format!(
                "chore(release): v{ver} artifacts — exe/dll/bundle re-out from clean tag-commit worktree"
            ),
            &changed,
        )?;
        println!("[artifacts] committed [{}]", changed.join(", "));
    }

    // ---- 门 + tag + push 舞步 ----
    release_check::run_release_check()?;
    println!("[gate] release-check OK");

    git(&["tag", &tag])?;
    println!("[tag] {tag} -> {}", git(&["rev-parse", "HEAD"])?);

    git(&["push", "origin", "main"])?;
    git(&["push", "origin", &tag])?;
    // 三方核对：远端 main、远端 tag、本地 HEAD 必须同 sha（AGENTS 移 tag 坑 ② 的验收步）。
    let head_final = git(&["rev-parse", "HEAD"])?;
    let ls = git(&[
        "ls-remote",
        "origin",
        "refs/heads/main",
        &format!("refs/tags/{tag}"),
    ])?;
    let refs: Vec<(&str, &str)> = ls
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((it.next()?, it.next()?))
        })
        .collect();
    for (want_ref, name) in [
        ("refs/heads/main", "main"),
        (&format!("refs/tags/{tag}"), "tag"),
    ] {
        match refs.iter().find(|(_, r)| *r == want_ref) {
            Some((sha, _)) if *sha == head_final => {}
            other => {
                return Err(format!(
                    "remote ref {name} mismatch after push: {other:?} vs local {head_final} \
                     — do NOT re-tag blindly; see AGENTS 发版 section (移 tag 坑)",
                )
                .into());
            }
        }
    }
    println!("[push] remote main == remote {tag} == local {head_final}");
    println!("== done: watch the Release workflow for tag {tag} ==");
    Ok(())
}

/// 在干净 worktree（锚定 bump 提交）构建全部产物并拷回主工作树。
fn build_artifacts_in(wt: &Path, root: &Path, ver: &str) -> Result<(), Box<dyn std::error::Error>> {
    // 离线优先，失败回退在线（lock 已随 bump 提交，被写坏可 checkout 恢复）。
    run_cargo(
        wt,
        &["build", "-p", "ikat_pkg", "-p", "ikat_ffi_c", "--release"],
        true,
    )
    .or_else(|e| {
        eprintln!("[build] offline failed ({e}); retrying online");
        run_cargo(
            wt,
            &["build", "-p", "ikat_pkg", "-p", "ikat_ffi_c", "--release"],
            false,
        )
    })?;
    let target_dir = root.join("target/release");
    let ikat_exe = target_dir.join("ikat.exe");

    // 产物验证 ①：exe 内嵌版本实跑（旧坑 100：本地缓存会骗过进程，「文件变了」不算数）。
    let ver_out = Command::new(&ikat_exe)
        .arg("version")
        .output()
        .map_err(|e| format!("run ikat version: {e}"))?;
    let ver_txt = String::from_utf8_lossy(&ver_out.stdout).to_string();
    if !ver_txt.contains(&format!("unity {ver}")) {
        return Err(format!(
            "built ikat.exe reports wrong version: {ver_txt:?} (expect `unity {ver}`)"
        )
        .into());
    }
    println!("[build] ikat version: {}", ver_txt.trim());

    // bundle 重打（parse-time 逻辑可能变化，无条件重打 + 字节对比幂等——旧坑 66 精神）。
    let bundle_status = Command::new(&ikat_exe)
        .args(["build", wt.join("showcase").to_str().unwrap()])
        .current_dir(wt)
        .output()
        .map_err(|e| format!("run ikat build showcase: {e}"))?;
    if !bundle_status.status.success() {
        return Err(format!(
            "ikat build showcase failed: {}",
            String::from_utf8_lossy(&bundle_status.stderr)
        )
        .into());
    }

    // 产物验证 ②：bundle 头 8 字节 = magic "LPKG" + u32 LE 格式版本，须等于源码常量
    //（旧坑 135：worktree 用旧 crate 重打漏 → 入库版本号字段旧值，hexdump 才现形）。
    let wt_bundle = wt.join("unity/showcase-unity/Assets/Bundles/ui/showcase.pkg.bin");
    let want =
        parse_pkg_format_version(&fs_read(&wt.join("crates/core/src/asset/mod.rs"))?)?.to_string();
    let header = std::fs::read(&wt_bundle)?;
    match pkg_header_version(&header) {
        Some(v) if v.to_string() == want => println!("[build] bundle header v{v} == source const"),
        other => {
            return Err(format!(
                "showcase.pkg.bin header {:?} != PKG_FORMAT_VERSION {want}",
                other.map(|v| v.to_string())
            )
            .into());
        }
    }

    // 拷回主工作树（bundle/runtime.json 字节相同则跳过，免伪脏提交）。
    let pairs = [
        (
            target_dir.join("ikat_ffi_c.dll"),
            root.join("unity/package/Plugins/Ikat/ikat_ffi_c.dll"),
        ),
        (
            ikat_exe.clone(),
            root.join("unity/package/Editor/Tools/ikat.exe"),
        ),
        (
            wt_bundle.clone(),
            root.join("unity/showcase-unity/Assets/Bundles/ui/showcase.pkg.bin"),
        ),
        (
            wt.join("unity/showcase-unity/Assets/Bundles/ikat.runtime.json"),
            root.join("unity/showcase-unity/Assets/Bundles/ikat.runtime.json"),
        ),
    ];
    for (src, dst) in &pairs {
        let identical = file_bytes_eq(src, dst).unwrap_or(false);
        if identical {
            println!("[copy] {} (identical, skipped)", dst.display());
        } else {
            std::fs::copy(src, dst)?;
            println!("[copy] {} <- {}", dst.display(), src.display());
        }
    }
    Ok(())
}

// ---- 纯函数（单测覆盖）----

/// CHANGELOG 折段：`## [Unreleased]` 之后插入 `## [<ver>] - <date>`（保留新空 Unreleased）。
pub fn fold_changelog(content: &str, ver: &str, date: &str) -> Result<String, String> {
    let mut out = String::with_capacity(content.len() + 64);
    let mut folded = false;
    for line in content.split_inclusive('\n') {
        out.push_str(line);
        if line.trim() == "## [Unreleased]" && !folded {
            out.push_str(&format!("\n## [{ver}] - {date}\n"));
            folded = true;
        }
    }
    if !folded {
        return Err("CHANGELOG has no `## [Unreleased]` header".into());
    }
    Ok(out)
}

/// `[Unreleased]` 段是否有实际条目（到下一个 `## [` 段头之间有非空白行）。
pub fn unreleased_has_entries(content: &str) -> bool {
    let mut in_unreleased = false;
    for line in content.lines() {
        let t = line.trim();
        if t == "## [Unreleased]" {
            in_unreleased = true;
            continue;
        }
        if in_unreleased {
            if t.starts_with("## [") {
                return false;
            }
            if !t.is_empty() {
                return true;
            }
        }
    }
    false
}

/// package.json 顶层 `"version": "x.y.z"` 单点替换。恰好一处命中才算成（GNU sed 静默
/// 失效教训：替换后必须可断言命中率，不信任「跑完没报错」）。
pub fn replace_version_json(content: &str, new: &str) -> Result<String, String> {
    let needle_old: Vec<&str> = content
        .lines()
        .filter(|l| l.trim_start().starts_with("\"version\":"))
        .collect();
    if needle_old.len() != 1 {
        return Err(format!(
            "package.json expected exactly 1 `\"version\":` line, found {}",
            needle_old.len()
        ));
    }
    let line = needle_old[0];
    let replaced = line.replace(line.split('"').nth(3).unwrap_or_default(), new);
    if replaced == *line {
        return Err("package.json version line replacement was a no-op".into());
    }
    Ok(content.replacen(line, &replaced, 1))
}

/// pkg Cargo.toml `[package]` 段 version 单点替换（段定位复用 release_check 解析口径）。
pub fn replace_version_toml(content: &str, new: &str) -> Result<String, String> {
    let old = release_check::parse_crate_version(content)
        .ok_or("crates/packer/pkg/Cargo.toml has no [package] version")?;
    let target = format!("version = \"{old}\"");
    let count = content.lines().filter(|l| l.trim() == target).count();
    if count != 1 {
        return Err(format!(
            "Cargo.toml `version = \"{old}\"` expected exactly once, found {count}"
        ));
    }
    Ok(content.replacen(&target, &format!("version = \"{new}\""), 1))
}

/// 从 asset/mod.rs 源文本抓 `pub const PKG_FORMAT_VERSION: u32 = <n>;`。
pub fn parse_pkg_format_version(src: &str) -> Result<u32, String> {
    for line in src.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("pub const PKG_FORMAT_VERSION: u32 =") {
            let digits: String = rest
                .trim()
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(v) = digits.parse() {
                return Ok(v);
            }
        }
    }
    Err("PKG_FORMAT_VERSION const not found in asset/mod.rs".into())
}

/// pkg.bin 头：magic `LPKG` + u32 LE 格式版本。非 magic / 过短 → None。
pub fn pkg_header_version(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 8 || &bytes[0..4] != b"LPKG" {
        return None;
    }
    Some(u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]))
}

// ---- 副作用辅助 ----

/// Unity 是否锁着入库 dll：对已存在文件尝试 write-open（Windows 对已加载 dll 开写句柄
/// 失败——正是拷贝会遇到的失败，提前探测）。
pub fn unity_dll_locked(path: &Path) -> bool {
    path.exists() && std::fs::OpenOptions::new().write(true).open(path).is_err()
}

/// cargo 命令；offline=true 先离线（本地缓存全时可避开在线半刷新索引写坏 lock 的坑），
/// 调用方自行决定回退。CARGO_TARGET_DIR 显式指主仓 target（跨 worktree 复用编译缓存；
/// cargo 自带文件锁，与并行会话的构建串行化等待，不冲突只排队）。
pub(crate) fn run_cargo(cwd: &Path, args: &[&str], offline: bool) -> Result<(), String> {
    let mut cmd = Command::new("cargo");
    cmd.args(args)
        .current_dir(cwd)
        .env_remove("CARGO_TERM_COLOR");
    if offline {
        cmd.arg("--offline");
    }
    let main_target = paths::repo_root().join("target");
    cmd.env("CARGO_TARGET_DIR", &main_target);
    let out = cmd
        .output()
        .map_err(|e| format!("cargo {}: {e}", args.join(" ")))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

/// pathspec 限定提交：`git add` 显式路径 + `git commit -- paths`，并行会话的暂存/在途
/// 改动不被捎带（`git commit -- paths` 按工作树内容直提这些路径）。
fn commit_paths(msg: &str, paths: &[&str]) -> Result<(), String> {
    let mut add_args: Vec<&str> = vec!["add", "--"];
    add_args.extend_from_slice(paths);
    git(&add_args)?;
    let mut commit_args: Vec<&str> = vec!["commit", "-m", msg, "--"];
    commit_args.extend_from_slice(paths);
    git(&commit_args)?;
    Ok(())
}

fn file_bytes_eq(a: &Path, b: &Path) -> Result<bool, String> {
    let (ma, mb) = (std::fs::metadata(a), std::fs::metadata(b));
    match (ma, mb) {
        (Ok(ma), Ok(mb)) if ma.len() == mb.len() => {
            let (xa, xb) = (std::fs::read(a), std::fs::read(b));
            Ok(matches!((xa, xb), (Ok(xa), Ok(xb)) if xa == xb))
        }
        _ => Ok(false),
    }
}

fn fs_read(p: &Path) -> Result<String, String> {
    std::fs::read_to_string(p).map_err(|e| format!("read {}: {e}", p.display()))
}

fn fs_write(p: &Path, content: &str) -> Result<(), String> {
    std::fs::write(p, content).map_err(|e| format!("write {}: {e}", p.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_inserts_section_after_unreleased() {
        let cl = "# Changelog\n\n## [Unreleased]\n\n### Added\n- x\n";
        let out = fold_changelog(cl, "0.0.15", "2026-08-28").unwrap();
        assert!(out.contains("## [Unreleased]\n\n## [0.0.15] - 2026-08-28\n\n### Added"));
        // 幂等性由调用侧（resume 判定）保证；重复 fold 会再插一段——此处钉住单次行为。
        assert_eq!(out.matches("## [0.0.15]").count(), 1);
    }

    #[test]
    fn fold_requires_unreleased_header() {
        assert!(fold_changelog("## [0.0.14] - x\n", "0.0.15", "d").is_err());
    }

    #[test]
    fn unreleased_entries_detection() {
        assert!(unreleased_has_entries(
            "## [Unreleased]\n\n### Added\n- x\n\n## [0.0.13]\n"
        ));
        assert!(!unreleased_has_entries(
            "## [Unreleased]\n\n## [0.0.13]\n- x\n"
        ));
        assert!(!unreleased_has_entries("## [0.0.13]\n- x\n"));
    }

    #[test]
    fn json_version_single_site_replace() {
        let j = "{\n  \"name\": \"com.ikat.unity\",\n  \"version\": \"0.0.13\",\n  \"unity\": \"6000.0\"\n}";
        let out = replace_version_json(j, "0.0.14").unwrap();
        assert!(out.contains("\"version\": \"0.0.14\""));
        assert!(!out.contains("0.0.13"));
        // 多处 version 行拒绝（避免误替依赖版本类字段）。
        let bad = format!("{j}\n  \"version\": \"1.0\"");
        assert!(replace_version_json(&bad, "0.0.14").is_err());
    }

    #[test]
    fn toml_version_single_site_replace() {
        let t =
            "[package]\nname = \"ikat_pkg\"\nversion = \"0.0.13\"\n\n[[bin]]\nname = \"ikat\"\n";
        let out = replace_version_toml(t, "0.0.14").unwrap();
        assert!(out.starts_with("[package]\nname = \"ikat_pkg\"\nversion = \"0.0.14\""));
        // 段外同名值不参与计数（parse_crate_version 只认 [package] 段——此处验替换唯一性）。
        assert_eq!(out.matches("version =").count(), 1);
    }

    #[test]
    fn pkg_format_version_scan() {
        let src = "// comment\npub const PKG_FORMAT_VERSION: u32 = 47; // v47 note\npub const OTHER: u32 = 1;\n";
        assert_eq!(parse_pkg_format_version(src).unwrap(), 47);
        assert!(parse_pkg_format_version("no const here").is_err());
    }

    #[test]
    fn pkg_header_roundtrip() {
        let mut b = vec![b'L', b'P', b'K', b'G'];
        b.extend_from_slice(&47u32.to_le_bytes());
        assert_eq!(pkg_header_version(&b), Some(47));
        assert_eq!(pkg_header_version(&b[..6]), None);
        b[0] = b'X';
        assert_eq!(pkg_header_version(&b), None);
    }
}
