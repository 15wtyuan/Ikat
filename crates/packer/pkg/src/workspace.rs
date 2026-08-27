//! 工作区配置（ikat.workspace.json）：AI 直接编辑的真相源。
//! 全路径相对工作区根 + 正斜杠。

use serde::{Deserialize, Serialize};
use std::path::Path;

/// 工作区根下 ikat.workspace.json 的文件名。
pub const WORKSPACE_FILE: &str = "ikat.workspace.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Workspace {
    pub version: u32,
    pub output_dir: String,
    /// 设计分辨率（设计 px）。缺省 = 集成层自兜底（Unity Driver Inspector 字段）。
    /// 分辨率适配的正主：设计稿多大是设计师事实，活在工作区（AI 可编辑文本空间），
    /// 不活在 Unity 场景手填。
    #[serde(default)]
    pub design: Option<crate::runtime::DesignDim>,
    /// 适配模式：letterbox | fit-width | fit-height（分辨率适配，见 main-design §11.5）。
    /// 缺省 = letterbox（contain，零行为变化）。
    #[serde(default)]
    pub match_mode: Option<String>,
    #[serde(default)]
    pub packages: Vec<PackageCfg>,
    #[serde(default)]
    pub atlases: Vec<AtlasCfg>,
    #[serde(default)]
    pub fonts: Vec<FontCfg>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackageCfg {
    pub name: String,
    pub dirs: Vec<String>,
    /// 空 = 自动态（打包时扫 dirs 顶层 .html）；非空 = 显式态（锁定这些文件）。
    #[serde(default)]
    pub html: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AtlasCfg {
    pub name: String,
    /// 每图独立成页不拼合（超大图用）。
    #[serde(default)]
    pub standalone: bool,
    pub dirs: Vec<String>,
    #[serde(default = "default_max_size")]
    pub max_size: u32,
    #[serde(default = "default_padding")]
    pub padding: u32,
}

fn default_max_size() -> u32 {
    2048
}
fn default_padding() -> u32 {
    4
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FontCfg {
    pub family: String,
    pub file: String,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub fallback: bool,
}

/// 读工作区根下的 ikat.workspace.json。
pub fn load_workspace(root: &Path) -> Result<Workspace, String> {
    let path = root.join(WORKSPACE_FILE);
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
}

/// 写回工作区根下的 ikat.workspace.json（pretty，AI 好读；带尾换行——手编文件
/// 的常规收尾，缺了则每次工具重写都产「删尾换行」的伪 diff）。
pub fn save_workspace(root: &Path, ws: &Workspace) -> Result<(), String> {
    let path = root.join(WORKSPACE_FILE);
    let mut text =
        serde_json::to_string_pretty(ws).map_err(|e| format!("serialize workspace: {e}"))?;
    text.push('\n');
    std::fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_workspace() {
        let json = r#"{
            "version": 1,
            "output_dir": "../dist",
            "packages": [{ "name": "showcase", "dirs": ["ui/showcase"], "html": [] }],
            "atlases": [{ "name": "ui", "default": true, "dirs": ["assets/icons"] }],
            "fonts": [{ "family": "NotoSansSC", "file": "fonts/NotoSansSC.ttc", "default": true, "fallback": true }]
        }"#;
        let ws: Workspace = serde_json::from_str(json).expect("parse");
        assert_eq!(ws.version, 1);
        assert_eq!(ws.packages[0].name, "showcase");
        assert!(ws.packages[0].html.is_empty(), "html 空 = 自动态");
        assert_eq!(ws.atlases[0].max_size, 2048, "max_size 缺省默认 2048");
        assert_eq!(ws.atlases[0].padding, 4, "padding 缺省默认 4");
        assert_eq!(ws.fonts[0].family, "NotoSansSC");
    }

    #[test]
    fn roundtrip_workspace() {
        let ws = Workspace {
            version: 1,
            output_dir: "../dist".into(),
            design: None,
            match_mode: None,
            packages: vec![PackageCfg {
                name: "p".into(),
                dirs: vec!["ui".into()],
                html: vec!["a.html".into()],
            }],
            atlases: vec![AtlasCfg {
                name: "ui".into(),
                standalone: false,
                dirs: vec!["assets".into()],
                max_size: 1024,
                padding: 2,
            }],
            fonts: vec![],
        };
        let text = serde_json::to_string(&ws).unwrap();
        let back: Workspace = serde_json::from_str(&text).unwrap();
        assert_eq!(ws, back, "round-trip 保持不变");
    }
}
