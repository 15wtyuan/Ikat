//! 自绘图集：读 PNG → shelf 打包 → atlas.png + atlas.json。
//! 图集独立于包；sprite_key = 图相对工作区根路径（全局唯一）。
//! 见 spec §5。本模块的子模块 pack 做实际打包，本文件只定义清单格式。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// atlas.json：一个图集的产物清单。后端读它建 sprite_key→UV 表。
/// BTreeMap 保证 key 有序输出（AI diff 稳定）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AtlasManifest {
    /// 页 png 文件名（相对产物 atlas 目录，如 ["ui.png", "ui.1.png"]）。
    pub pages: Vec<String>,
    /// sprite_key → 条目。
    pub sprites: BTreeMap<String, SpriteEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpriteEntry {
    /// 该 sprite 所在页在 pages 中的索引。
    pub page: u32,
    /// 归一化 UV [u0, v0, u1, v1]，直接喂后端线性映射。
    pub uv: [f32; 4],
    /// 原图像素尺寸 [w, h]（measure + 九宫格基准）。
    pub orig: [u32; 2],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_roundtrip() {
        let mut sprites = BTreeMap::new();
        sprites.insert(
            "assets/icons/home.png".to_string(),
            SpriteEntry {
                page: 0,
                uv: [0.012, 0.048, 0.137, 0.170],
                orig: [64, 64],
            },
        );
        let m = AtlasManifest {
            pages: vec!["ui.png".into()],
            sprites,
        };
        let text = serde_json::to_string(&m).unwrap();
        let back: AtlasManifest = serde_json::from_str(&text).unwrap();
        assert_eq!(m, back);
    }
}
