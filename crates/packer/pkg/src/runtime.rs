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
    /// additive 可选字段——version 不跳（旧读者忽略未知键）。None 必须省键而非写
    /// `null`：Unity 侧手写 reader（IkatManifests.cs）对显式 null 直接抛解析错，
    /// manifest 整体作废。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design: Option<DesignDim>,
    /// 适配模式 letterbox | fit-width | fit-height（workspace.match_mode 透传）。
    /// None = letterbox（集成层默认）。省键纪律同 design（null 同样炸 Unity reader）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(design: Option<DesignDim>, match_mode: Option<String>) -> RuntimeManifest {
        RuntimeManifest {
            version: 1,
            packages: vec!["main".into()],
            atlases: vec![],
            fonts: vec![],
            design,
            match_mode,
        }
    }

    /// None 字段必须省键而非写 null（Unity reader 见 null 即抛），且省键产物
    /// 本 crate 读回仍为 None；设值时键必须在（透传契约）。
    #[test]
    fn optional_fields_omit_keys_when_none() {
        let none_json = serde_json::to_string(&manifest(None, None)).unwrap();
        assert!(
            !none_json.contains("design"),
            "design key must be absent: {none_json}"
        );
        assert!(
            !none_json.contains("match_mode"),
            "match_mode key must be absent: {none_json}"
        );
        assert!(
            !none_json.contains("null"),
            "no null literals allowed: {none_json}"
        );

        let back: RuntimeManifest = serde_json::from_str(&none_json).unwrap();
        assert_eq!(back.design, None);
        assert_eq!(back.match_mode, None);

        let some_json = serde_json::to_string(&manifest(
            Some(DesignDim {
                w: 1920.0,
                h: 1080.0,
            }),
            Some("letterbox".into()),
        ))
        .unwrap();
        assert!(
            some_json.contains("\"design\":{\"w\":1920.0,\"h\":1080.0}"),
            "design must round-trip: {some_json}"
        );
        assert!(
            some_json.contains("\"match_mode\":\"letterbox\""),
            "match_mode must round-trip: {some_json}"
        );
    }
}
