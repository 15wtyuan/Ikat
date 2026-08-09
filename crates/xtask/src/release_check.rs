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
    // 以下三个变体由 Task 3 的文件完整性校验构造；本任务先声明，allow 避免 dead_code 误报。
    #[allow(dead_code)]
    DllNotFound,
    #[allow(dead_code)]
    DllStale,
    #[allow(dead_code)]
    AsmdefMissing(String),
    ChangelogMissingVersion(String),
    Io(String),
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingField(n) => write!(f, "package.json missing required field: {n}"),
            Self::InvalidSemver(v) => write!(f, "version is not valid SemVer: {v}"),
            Self::DllNotFound => write!(f, "loomgui_ffi_c.dll not found in package"),
            Self::DllStale => write!(
                f,
                "committed dll differs from target/release build (forgot to commit?)"
            ),
            Self::AsmdefMissing(n) => write!(f, "asmdef missing: {n}"),
            Self::ChangelogMissingVersion(v) => {
                write!(f, "CHANGELOG.md has no section for version {v}")
            }
            Self::Io(e) => write!(f, "io error: {e}"),
        }
    }
}
impl std::error::Error for CheckError {}

/// 解析 package.json 内容并校验必填字段 + version 合法性。
pub fn parse_and_validate_package(content: &str) -> Result<PackageMeta, CheckError> {
    let v: serde_json::Value =
        serde_json::from_str(content).map_err(|e| CheckError::Io(e.to_string()))?;
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

/// release-check 入口：校验 package.json + CHANGELOG + dll + asmdef。
/// 任意一项失败返回 Err，调用方据此退出非 0。
pub fn run_release_check() -> Result<(), Box<dyn std::error::Error>> {
    let pkg = paths::repo_root().join("unity/package/package.json");
    let meta = parse_and_validate_package(&std::fs::read_to_string(&pkg)?)?;

    let cl = paths::repo_root().join("unity/package/CHANGELOG.md");
    let cl_content = std::fs::read_to_string(&cl)?;
    if !changelog_has_version(&cl_content, &meta.version) {
        return Err(CheckError::ChangelogMissingVersion(meta.version).into());
    }

    // 文件完整性校验（dll / asmdef）见 Task 3 接入。
    let _ = Path::new("");

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
}
