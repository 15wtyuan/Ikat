//! ikat.runtime.json：后端自举清单（打包器产，替代 Unity IkatSettings SO）。
//! packages / atlases / fonts 是 workspace 级平行列表——atlas 与字体不隶属任何包，
//! UnloadPackage 只动模板注册表。

use serde::{Deserialize, Serialize};

pub const RUNTIME_FILE: &str = "ikat.runtime.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeManifest {
    pub version: u32,
    /// .pkg.bin 文件名（不含扩展）。
    pub packages: Vec<String>,
    /// 每个对应 <name>.atlas.json + png。
    pub atlases: Vec<String>,
    pub fonts: Vec<RuntimeFont>,
    /// 设计分辨率（分辨率适配正主，workspace.design 透传）。None = 集成层兜底。
    /// additive 可选字段——version 不跳（旧读者忽略未知键）。
    #[serde(default)]
    pub design: Option<DesignDim>,
    /// 适配模式 letterbox | fit-width | fit-height（workspace.match_mode 透传）。
    /// None = letterbox（集成层默认）。
    #[serde(default)]
    pub match_mode: Option<String>,
}

/// 设计分辨率（设计 px）。w/h 须为正有限值（check 校验）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct DesignDim {
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeFont {
    pub family: String,
    /// 产物 fonts/ 下文件名（源名 + ".bytes"）。
    pub file: String,
    pub default: bool,
    pub fallback: bool,
}
