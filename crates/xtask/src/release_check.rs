//! release-check 子命令：发布前包完整性自检。

use crate::paths;
use std::path::Path;

/// 解析后的包元数据（只取发布相关字段）。
#[derive(Debug, PartialEq, Eq)]
pub struct PackageMeta {
    pub version: String,
}

/// release-check 检出的问题类别。
#[derive(Debug)]
pub enum CheckError {
    MissingField(String),
    InvalidSemver(String),
    DllNotFound,
    AsmdefMissing(String),
    ChangelogMissingVersion(String),
    /// package.json 解析失败（非合法 JSON）。
    PackageJsonInvalid(String),
    /// 文件读取失败，携带路径上下文（避免裸 `io: No such file...` 没有指明是哪个文件）。
    ReadFailed {
        path: String,
        source: String,
    },
    /// ikat_pkg crate 版本与 unity 包不同轨（ikat CLI 的 `cli == unity` 契约）。
    PkgCrateVersionMismatch {
        crate_version: String,
        package_version: String,
    },
    /// 产物（dll/exe）上次入库提交之后 crates/ 有影响产物字节的源变更——
    /// 入库产物可疑陈旧，拒绝发版（重出后重跑）。
    ArtifactsStale {
        files: Vec<String>,
    },
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingField(n) => write!(f, "package.json missing required field: {n}"),
            Self::InvalidSemver(v) => write!(f, "version is not valid SemVer: {v}"),
            Self::DllNotFound => write!(f, "ikat_ffi_c.dll not found in package"),
            Self::AsmdefMissing(n) => write!(f, "asmdef missing: {n}"),
            Self::ChangelogMissingVersion(v) => {
                write!(f, "CHANGELOG.md has no section for version {v}")
            }
            Self::PackageJsonInvalid(e) => write!(f, "package.json is not valid JSON: {e}"),
            Self::ReadFailed { path, source } => {
                write!(f, "failed to read {path}: {source}")
            }
            Self::PkgCrateVersionMismatch {
                crate_version,
                package_version,
            } => write!(
                f,
                "ikat_pkg crate version {crate_version} != unity package version \
                 {package_version}; bump crates/packer/pkg/Cargo.toml to align (ikat CLI \
                 reports both from the crate version — a mismatch ships a lying `ikat version`)"
            ),
            Self::ArtifactsStale { files } => write!(
                f,
                "Rust sources changed after the last dll/exe artifact commit — committed \
                 artifacts may be stale; re-out and commit first \
                 (`cargo run -p xtask -- reout`): [{}]",
                files.join(", ")
            ),
        }
    }
}
impl std::error::Error for CheckError {}

/// dll 校验结果。
#[derive(Debug, PartialEq, Eq)]
pub enum DllStatus {
    Ok,
    NotFound,
}

/// 解析 package.json 内容并校验必填字段 + version 合法性。
pub fn parse_and_validate_package(content: &str) -> Result<PackageMeta, CheckError> {
    let v: serde_json::Value =
        serde_json::from_str(content).map_err(|e| CheckError::PackageJsonInvalid(e.to_string()))?;
    for field in ["name", "version", "unity", "displayName"] {
        let missing = v.get(field).map(|x| x.is_null()).unwrap_or(true);
        if missing {
            return Err(CheckError::MissingField(field.to_string()));
        }
    }
    let version = v["version"]
        .as_str()
        .ok_or_else(|| CheckError::MissingField("version".to_string()))?;
    semver::Version::parse(version).map_err(|_| CheckError::InvalidSemver(version.into()))?;
    Ok(PackageMeta {
        version: version.to_string(),
    })
}

/// CHANGELOG 是否含 `## [<version>]` 段落（Keep a Changelog 格式）。
pub fn changelog_has_version(content: &str, version: &str) -> bool {
    let needle = format!("## [{version}]");
    content
        .lines()
        .any(|line| line.trim_start().starts_with(&needle))
}

/// 读取文件，失败时把路径带入错误上下文。
fn read_file(path: &Path) -> Result<String, CheckError> {
    std::fs::read_to_string(path).map_err(|e| CheckError::ReadFailed {
        path: path.to_string_lossy().into_owned(),
        source: e.to_string(),
    })
}

/// 校验入库 dll 是否存在。
pub fn dll_status(committed: &Path) -> DllStatus {
    if committed.exists() {
        DllStatus::Ok
    } else {
        DllStatus::NotFound
    }
}

/// 校验三个 asmdef 齐全。任一缺失返回 `AsmdefMissing`。
pub fn check_asmdef_present(pkg_dir: &Path) -> Result<(), CheckError> {
    let expected = [
        "Ikat.Runtime.asmdef",
        "Editor/Ikat.Editor.asmdef",
        "Plugins/Ikat/Ikat.Bindings.asmdef",
    ];
    for rel in expected {
        let p = pkg_dir.join(rel);
        if !p.exists() {
            return Err(CheckError::AsmdefMissing(rel.to_string()));
        }
    }
    Ok(())
}

/// 从 crate Cargo.toml 内容抓 `[package]` 段的 `version = "x.y.z"`。
/// 只在 [package] 声明之后、下一个 section 之前查找（避免误抓 [[bin]] 等段内同名字段）。
pub fn parse_crate_version(cargo_toml: &str) -> Option<String> {
    let mut in_package = false;
    for line in cargo_toml.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_package = t == "[package]";
            continue;
        }
        if in_package {
            if let Some(rest) = t.strip_prefix("version") {
                let rest = rest.trim_start();
                if let Some(v) = rest.strip_prefix('=') {
                    let v = v.trim().trim_matches('"');
                    if !v.is_empty() {
                        return Some(v.to_string());
                    }
                }
            }
        }
    }
    None
}

/// staleness 判定的单文件过滤：`crates/` 下会影响 dll/exe/gui 产物字节的源变更。
/// 排除 xtask（编排工具自身不进产物）。GUI（packer/gui）**在内**——它直链
/// ikat_pkg/ikat_fence 编进 exe（dev fallback 进程内路径），与 dll/ikat.exe
/// 无条件同批重出。
fn affects_artifacts(path: &str) -> bool {
    path.starts_with("crates/") && !path.starts_with("crates/xtask/")
}

/// 两组「自产物锚点提交以来的变更清单」→ 影响产物的违例清单（去重保序）。
/// 纯函数供单测；git 侧采集见 [`collect_staleness`]。
pub fn staleness_violations(
    changed_since_dll: &[String],
    changed_since_exe: &[String],
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in changed_since_dll.iter().chain(changed_since_exe.iter()) {
        if affects_artifacts(p) && !out.contains(p) {
            out.push(p.clone());
        }
    }
    out
}

/// git 拓扑采集：dll 与 exe 各自上次入库提交为锚点，diff 到 HEAD 的 crates/ 变更。
/// 锚点为空（产物从未入库）按全量违例处理——存在性门只盯 dll，exe 缺席在此兜底。
fn collect_staleness() -> Result<Vec<String>, CheckError> {
    let anchor = |path: &str| -> Result<String, CheckError> {
        crate::git::git(&["log", "-1", "--format=%H", "--", path]).map_err(|e| {
            CheckError::ReadFailed {
                path: path.into(),
                source: e,
            }
        })
    };
    let dll_anchor = anchor("unity/package/Plugins/Ikat/ikat_ffi_c.dll")?;
    let exe_anchor = anchor("unity/package/Editor/Tools/ikat.exe")?;
    let changed_since = |a: &str| -> Vec<String> {
        if a.is_empty() {
            return vec!["<artifact never committed>".to_string()];
        }
        crate::git::git(&[
            "diff",
            "--name-only",
            &format!("{a}..HEAD"),
            "--",
            "crates/",
        ])
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default()
    };
    Ok(staleness_violations(
        &changed_since(&dll_anchor),
        &changed_since(&exe_anchor),
    ))
}

/// release-check 入口：校验 package.json + CHANGELOG + dll + asmdef + ikat crate 版本同轨。
/// 任意一项失败返回 Err，调用方据此退出非 0。
pub fn run_release_check() -> Result<(), Box<dyn std::error::Error>> {
    let pkg = paths::repo_root().join("unity/package/package.json");
    let meta = parse_and_validate_package(&read_file(&pkg)?)?;

    let cl = paths::repo_root().join("unity/package/CHANGELOG.md");
    let cl_content = read_file(&cl)?;
    if !changelog_has_version(&cl_content, &meta.version) {
        return Err(CheckError::ChangelogMissingVersion(meta.version).into());
    }

    // dll：入库必须存在。
    let pkg_dir = paths::repo_root().join("unity/package");
    let committed_dll = pkg_dir.join("Plugins/Ikat/ikat_ffi_c.dll");
    match dll_status(&committed_dll) {
        DllStatus::NotFound => return Err(CheckError::DllNotFound.into()),
        DllStatus::Ok => {}
    }

    // staleness 硬门：dll/exe 上次入库提交之后 crates/ 有影响产物字节的源变更 → 拒绝。
    // 堵此前自认的「只查存在、不查新旧」洞（stale dll 提交史：本地缓存掩盖、干净环境才
    // 暴露）。字节哈希对拍不可行——构建路径非确定性（同源码不同目录产物字节不同），
    // 只能用 git 提交拓扑代理；过近似可接受（多重出一次便宜，漏报才是灾难）。
    let stale = collect_staleness()?;
    if !stale.is_empty() {
        return Err(CheckError::ArtifactsStale { files: stale }.into());
    }

    check_asmdef_present(&pkg_dir)?;

    // ikat CLI 版本同轨：crate 版本 == unity 包版本。链条 tag → package.json →
    // pkg crate → `ikat version`，缺环即消费者装错版本号。
    let cargo_toml = read_file(&paths::repo_root().join("crates/packer/pkg/Cargo.toml"))?;
    let crate_version = parse_crate_version(&cargo_toml).ok_or_else(|| CheckError::ReadFailed {
        path: "crates/packer/pkg/Cargo.toml".into(),
        source: "no `version` field found in [package]".into(),
    })?;
    if crate_version != meta.version {
        return Err(CheckError::PkgCrateVersionMismatch {
            crate_version,
            package_version: meta.version,
        }
        .into());
    }

    println!("release-check: OK (version {})", meta.version);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_package() {
        let s = r#"{"name":"x","version":"0.0.1","unity":"6000.0","displayName":"X"}"#;
        assert_eq!(
            parse_and_validate_package(s).unwrap(),
            PackageMeta {
                version: "0.0.1".into()
            }
        );
    }

    #[test]
    fn crate_version_takes_package_section_only() {
        // version 出现在 [[bin]] 之前（[package] 内）才算；[dependencies] 段内的
        // version 行不得干扰。
        let toml = r#"
[package]
name = "ikat_pkg"
version = "0.0.5"
edition = "2021"

[[bin]]
name = "ikat"
path = "src/main.rs"

[dependencies]
serde = { version = "1", features = ["derive"] }
"#;
        assert_eq!(parse_crate_version(toml).as_deref(), Some("0.0.5"));
        // 无 [package] 段 → None。
        assert_eq!(
            parse_crate_version("[dependencies]\nversion = \"9\"\n"),
            None
        );
    }

    #[test]
    fn missing_required_field() {
        let s = r#"{"name":"x","version":"0.0.1"}"#; // 缺 unity、displayName
        assert!(matches!(
            parse_and_validate_package(s),
            Err(CheckError::MissingField(_))
        ));
    }

    #[test]
    fn invalid_semver() {
        let s = r#"{"name":"x","version":"not-a-version","unity":"6000.0","displayName":"X"}"#;
        assert!(matches!(
            parse_and_validate_package(s),
            Err(CheckError::InvalidSemver(_))
        ));
    }

    #[test]
    fn changelog_has_matching_section() {
        let s = "## [Unreleased]\n\n## [0.0.1] - 2026-08-09\n- x\n";
        assert!(changelog_has_version(s, "0.0.1"));
    }

    #[test]
    fn changelog_missing_section() {
        let s = "## [Unreleased]\n\n## [0.0.2] - 2026-08-09\n";
        assert!(!changelog_has_version(s, "0.0.1"));
    }

    fn tmp_bytes(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let id = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("xtask-rc-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    /// 创建一个唯名的临时 "pkg dir"，用于 asmdef 校验测试。
    fn tmp_pkg_dir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static M: AtomicU64 = AtomicU64::new(0);
        let id = M.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("xtask-rc-asmdef-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 在 dir 下写相对路径文件（自动创建父目录）。
    fn write_rel(dir: &std::path::Path, rel: &str, bytes: &[u8]) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, bytes).unwrap();
    }

    #[test]
    fn asmdef_present_ok() {
        let dir = tmp_pkg_dir();
        write_rel(&dir, "Ikat.Runtime.asmdef", b"{}");
        write_rel(&dir, "Editor/Ikat.Editor.asmdef", b"{}");
        write_rel(&dir, "Plugins/Ikat/Ikat.Bindings.asmdef", b"{}");
        assert!(check_asmdef_present(&dir).is_ok());
    }

    #[test]
    fn asmdef_present_missing() {
        let dir = tmp_pkg_dir();
        write_rel(&dir, "Ikat.Runtime.asmdef", b"{}");
        write_rel(&dir, "Editor/Ikat.Editor.asmdef", b"{}");
        // 故意不写 Plugins/Ikat/Ikat.Bindings.asmdef
        assert!(matches!(
            check_asmdef_present(&dir),
            Err(CheckError::AsmdefMissing(n))
                if n == "Plugins/Ikat/Ikat.Bindings.asmdef"
        ));
    }

    #[test]
    fn dll_not_found() {
        let missing = std::env::temp_dir().join("xtask-rc-nope-a");
        assert_eq!(dll_status(&missing), DllStatus::NotFound);
    }

    #[test]
    fn dll_present() {
        let present = tmp_bytes("a.dll", b"AAA");
        assert_eq!(dll_status(&present), DllStatus::Ok);
    }

    /// staleness 过滤：xtask/gui 排除、非 crates/ 排除、跨两组去重保序。
    #[test]
    fn staleness_filters_and_dedups() {
        let dll_side = [
            "crates/core/src/lib.rs".to_string(),
            "crates/xtask/src/main.rs".to_string(), // 编排工具，不进产物
            "docs/design/fence.md".to_string(),     // 非 crates/
        ];
        let exe_side = [
            "crates/core/src/lib.rs".to_string(), // 与 dll 侧重复 → 去重
            "crates/packer/pkg/src/bridge.rs".to_string(),
            "crates/packer/gui/src-tauri/main.rs".to_string(), // GUI 直链 pkg/fence，同批重出
        ];
        assert_eq!(
            staleness_violations(&dll_side, &exe_side),
            vec![
                "crates/core/src/lib.rs".to_string(),
                "crates/packer/pkg/src/bridge.rs".to_string(),
                "crates/packer/gui/src-tauri/main.rs".to_string(),
            ]
        );
        // 全部被排除 → 无违例（xtask 是唯一豁免路径）。
        assert!(staleness_violations(
            &["crates/xtask/src/git.rs".to_string()],
            &["crates/xtask/src/gui.rs".to_string()]
        )
        .is_empty());
    }
}
