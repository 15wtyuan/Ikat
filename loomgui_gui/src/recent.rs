//! 最近打开的工作区列表（~/.loomgui/recent.json）。跨平台用 dirs 定位 home。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Default)]
struct Recent {
    recent: Vec<String>,
}

fn recent_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".loomgui").join("recent.json"))
}

pub fn load_recent() -> Vec<String> {
    let Some(p) = recent_path() else {
        return vec![];
    };
    let Ok(text) = std::fs::read_to_string(&p) else {
        return vec![];
    };
    serde_json::from_str::<Recent>(&text)
        .map(|r| r.recent)
        .unwrap_or_default()
}

pub fn push_recent(path: &str) {
    let mut list = load_recent();
    list.retain(|p| p != path);
    list.insert(0, path.to_string());
    list.truncate(10);
    if let Some(p) = recent_path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(&Recent { recent: list }) {
            let _ = std::fs::write(&p, json);
        }
    }
}

/// 纯逻辑：把 path 提到列表首、去重、截断到 max。可单测不碰磁盘。
#[allow(dead_code)]
pub fn merge_recent(existing: &[String], path: &str, max: usize) -> Vec<String> {
    let mut list: Vec<String> = existing.iter().filter(|p| *p != path).cloned().collect();
    list.insert(0, path.to_string());
    list.truncate(max);
    list
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
}
