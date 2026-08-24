//! loom.runtime.json：后端自举清单（打包器产，替代 Unity LoomSettings SO）。
//! packages / atlases / fonts 是 workspace 级平行列表——atlas 与字体不隶属任何包，
//! UnloadPackage 只动模板注册表。

use serde::{Deserialize, Serialize};

pub const RUNTIME_FILE: &str = "loom.runtime.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeManifest {
    pub version: u32,
    /// .pkg.bin 文件名（不含扩展）。
    pub packages: Vec<String>,
    /// 每个对应 <name>.atlas.json + png。
    pub atlases: Vec<String>,
    pub fonts: Vec<RuntimeFont>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeFont {
    pub family: String,
    /// 产物 fonts/ 下文件名（源名 + ".bytes"）。
    pub file: String,
    pub default: bool,
    pub fallback: bool,
}
