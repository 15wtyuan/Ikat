//! 最近打开的工作区列表。存储跟随拉起方传入的 `--state-dir`（引擎集成侧
//! 按各自工程惯例决定目录，如 Unity 传 <Project>/UserSettings/Ikat），
//! 双击 exe 等无参启动不落盘。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MAX_RECENT: usize = 10;

/// GUI 进程的持久化目录（来自 `--state-dir` 启动参数）。None = 本次运行不持久化。
pub struct StateDir(pub Option<PathBuf>);

#[derive(Serialize, Deserialize, Default)]
struct Recent {
    recent: Vec<String>,
}

/// 从命令行参数解析 `--state-dir <dir>`。纯函数可单测。
pub fn state_dir_from_args(args: &[String]) -> Option<PathBuf> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--state-dir" {
            return it.next().map(PathBuf::from);
        }
    }
    None
}

fn recent_path(state_dir: &Path) -> PathBuf {
    state_dir.join("recent.json")
}

pub fn load_recent(state_dir: Option<&Path>) -> Vec<String> {
    let Some(dir) = state_dir else {
        return vec![];
    };
    let Ok(text) = std::fs::read_to_string(recent_path(dir)) else {
        return vec![];
    };
    serde_json::from_str::<Recent>(&text)
        .map(|r| r.recent)
        .unwrap_or_default()
}

pub fn push_recent(state_dir: Option<&Path>, path: &str) {
    let Some(dir) = state_dir else {
        return;
    };
    let list = merge_recent(&load_recent(state_dir), path, MAX_RECENT);
    let p = recent_path(dir);
    let _ = std::fs::create_dir_all(dir);
    if let Ok(json) = serde_json::to_string_pretty(&Recent { recent: list }) {
        let _ = std::fs::write(&p, json);
    }
}

/// 从列表移除一条（只删记录，不碰工作区目录）。幂等。
pub fn remove_recent(state_dir: Option<&Path>, path: &str) {
    let Some(dir) = state_dir else {
        return;
    };
    let list = remove_from_recent(&load_recent(state_dir), path);
    let p = recent_path(dir);
    let _ = std::fs::create_dir_all(dir);
    if let Ok(json) = serde_json::to_string_pretty(&Recent { recent: list }) {
        let _ = std::fs::write(&p, json);
    }
}

/// 纯逻辑：把 path 提到列表首、去重、截断到 max。可单测不碰磁盘。
pub fn merge_recent(existing: &[String], path: &str, max: usize) -> Vec<String> {
    let mut list: Vec<String> = existing.iter().filter(|p| *p != path).cloned().collect();
    list.insert(0, path.to_string());
    list.truncate(max);
    list
}

/// 纯逻辑：剔除 path。可单测不碰磁盘。
pub fn remove_from_recent(existing: &[String], path: &str) -> Vec<String> {
    existing.iter().filter(|p| *p != path).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_dedup_and_cap() {
        let start = vec!["a".to_string(), "b".to_string()];
        let r = merge_recent(&start, "b", 10);
        assert_eq!(r, vec!["b", "a"], "b 提到首 + 去重");
        let capped = merge_recent(&(0..15).map(|i| i.to_string()).collect::<Vec<_>>(), "x", 10);
        assert_eq!(capped.len(), 10);
        assert_eq!(capped[0], "x");
    }

    #[test]
    fn remove_filters_and_is_idempotent() {
        let start = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let r = remove_from_recent(&start, "b");
        assert_eq!(r, vec!["a", "c"]);
        let again = remove_from_recent(&r, "b");
        assert_eq!(again, r, "再删一次不变");
    }

    #[test]
    fn parse_state_dir_arg() {
        let f = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            state_dir_from_args(&f(&["exe", "--state-dir", "C:/proj/UserSettings/Ikat"])),
            Some(PathBuf::from("C:/proj/UserSettings/Ikat"))
        );
        assert_eq!(state_dir_from_args(&f(&["exe"])), None);
        assert_eq!(
            state_dir_from_args(&f(&["exe", "--state-dir"])),
            None,
            "缺值不算"
        );
    }

    #[test]
    fn disk_roundtrip_push_load_remove() {
        let dir = std::env::temp_dir().join(format!("ikat_recent_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let dir = dir.as_path();

        push_recent(Some(dir), "E:/ws/a");
        push_recent(Some(dir), "E:/ws/b");
        assert_eq!(load_recent(Some(dir)), vec!["E:/ws/b", "E:/ws/a"]);

        remove_recent(Some(dir), "E:/ws/b");
        assert_eq!(load_recent(Some(dir)), vec!["E:/ws/a"]);

        // 无 state-dir：纯内存语义，读为空、push 不落盘（盘上内容不变）
        assert!(load_recent(None).is_empty());
        push_recent(None, "E:/ws/x");
        assert_eq!(load_recent(Some(dir)), vec!["E:/ws/a"]);

        let _ = std::fs::remove_dir_all(dir);
    }
}
