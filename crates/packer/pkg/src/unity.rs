//! `.loom/unity.json`：反向配置（基座）——UI 工作区指回 Unity 工程的指针。
//!
//! `unity_root` 支持相对（相对工作区根；ui/ 与 unity/ 同父目录的布局下此文件可入库，
//! 团队 clone 即用）或绝对（跨盘符等无法相对化时 fallback，机器绑定、团队各自重建）。
//! `output_dir` 语义随基座：有反向配置 → 相对 `unity_root` 解析（写 `Assets/Bundles`
//! 即直达 Unity 工程）；无 → 相对工作区根（老工作区行为不变）。

use crate::diag::BuildFailure;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

pub const UNITY_CONFIG_FILE: &str = ".loom/unity.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnityConfig {
    pub unity_root: String,
}

/// 解析输出基座：无反向配置 → `None`（本地模式，合法形态）；有 → 校验后返回目录。
///
/// 文件存在但 `unity_root` 指向的目录不存在 → 工具性失败（exit 2），不静默 fallback
/// 本地——用户显然期望产物进 Unity，静默改道会把问题藏到 Unity 里找不到产物时才发现。
pub fn resolve_output_base(workspace_root: &Path) -> Result<Option<PathBuf>, BuildFailure> {
    let path = workspace_root.join(UNITY_CONFIG_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let cfg: UnityConfig = serde_json::from_str(&text)
        .map_err(|e| BuildFailure::config(format!("parse {}: {e}", path.display())))?;
    let root = if Path::new(&cfg.unity_root).is_absolute() {
        PathBuf::from(&cfg.unity_root)
    } else {
        workspace_root.join(&cfg.unity_root)
    };
    if !root.is_dir() {
        return Err(BuildFailure::config(format!(
            "{}: unity_root `{}` 指向的目录不存在（Unity 工程挪位或本机无此工程）；\
             重开 GUI 打包器重建工作区，或删除该文件回退本地输出",
            path.display(),
            cfg.unity_root
        )));
    }
    Ok(Some(root))
}

/// 写反向配置。`unity_root` 优先相对化（相对工作区根）；跨盘符等无法相对化时写绝对
/// 路径（机器绑定）。覆盖写（重复 init / GUI 重建即刷新）。
pub fn write(workspace_root: &Path, unity_root: &Path) -> Result<(), String> {
    let stored = relativize(workspace_root, unity_root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| unity_root.to_string_lossy().into_owned());
    let cfg = UnityConfig { unity_root: stored };
    let dir = workspace_root.join(".loom");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = workspace_root.join(UNITY_CONFIG_FILE);
    let text = serde_json::to_string_pretty(&cfg).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))
}

/// 纯词法相对化（from 目录 → to 路径的相对表示）。基不同（一方绝对一方相对）或盘符
/// 不同（跨盘）→ None——相对化无意义，调用方存绝对原样。
fn relativize(from: &Path, to: &Path) -> Option<PathBuf> {
    if to.is_absolute() != from.is_absolute() {
        return None;
    }
    let mut fc = from.components().peekable();
    let mut tc = to.components().peekable();
    if let (Some(Component::Prefix(a)), Some(Component::Prefix(b))) = (fc.peek(), tc.peek()) {
        if a != b {
            return None;
        }
    }
    loop {
        match (fc.peek(), tc.peek()) {
            (Some(a), Some(b)) if a == b => {
                fc.next();
                tc.next();
            }
            _ => break,
        }
    }
    let mut out = PathBuf::new();
    for _ in 0..fc.count() {
        out.push("..");
    }
    for c in tc {
        out.push(c.as_os_str());
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relativize_same_drive() {
        assert_eq!(
            relativize(Path::new("F:/proj/ui"), Path::new("F:/proj/unity")).unwrap(),
            Path::new("../unity")
        );
        // 深层上溯：多级 ../
        assert_eq!(
            relativize(Path::new("F:/a/b/c"), Path::new("F:/a/d")).unwrap(),
            Path::new("../../d")
        );
        // to 在 from 之下
        assert_eq!(
            relativize(Path::new("F:/proj"), Path::new("F:/proj/unity/Assets")).unwrap(),
            Path::new("unity/Assets")
        );
    }

    #[test]
    fn relativize_cross_drive_is_none() {
        assert!(relativize(Path::new("C:/ui"), Path::new("D:/unity")).is_none());
    }

    /// 基座链三态：无文件 → None；相对有效 → Some(拼好的目录)；路径失效 → exit 2。
    #[test]
    fn output_base_three_states() {
        let tmp = std::env::temp_dir().join("loom_unity_base_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("ws")).unwrap();
        std::fs::create_dir_all(tmp.join("unity")).unwrap();
        let ws = tmp.join("ws");

        // 1. 无反向配置 → 本地模式。
        assert!(resolve_output_base(&ws).unwrap().is_none());

        // 2. 相对 unity_root（写入时相对化）→ 解析回绝对目录。
        write(&ws, &tmp.join("unity")).unwrap();
        let base = resolve_output_base(&ws).unwrap().expect("base resolved");
        assert_eq!(
            base.canonicalize().unwrap(),
            tmp.join("unity").canonicalize().unwrap(),
            "join(\"../unity\") 词法含 .. 但指向同一目录"
        );
        // 落盘的是相对形态（正斜杠）。
        let text = std::fs::read_to_string(ws.join(UNITY_CONFIG_FILE)).unwrap();
        let cfg: UnityConfig = serde_json::from_str(&text).unwrap();
        assert_eq!(cfg.unity_root, "../unity");

        // 3. 指向不存在的目录 → 工具性失败（exit 2），不静默 fallback。
        std::fs::write(ws.join(UNITY_CONFIG_FILE), r#"{ "unity_root": "../gone" }"#).unwrap();
        let err = resolve_output_base(&ws).unwrap_err();
        assert_eq!(err.exit_code, 2);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
