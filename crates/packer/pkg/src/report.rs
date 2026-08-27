//! 命令机读输出层：`--format json` 的 stdout 单文档契约。
//!
//! 约定（agent 开始使用后即冻结）：`format_version` 版本内只增不改字段语义；
//! 成功失败都输出完整文档，退出码另行表达成败；进度日志走 stderr，stdout 只有数据。

use crate::build::BuildReport;
use crate::diag::{BuildFailure, PackDiagnostic, Severity};
use serde::Serialize;

pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Summary {
    pub errors: usize,
    pub warnings: usize,
}

/// `ikat version [--format json]` 的输出。cli == unity（crate 版本与 Unity 包同轨，
/// 由 release-check 断言）；pkg_format 引 core 的 `PKG_FORMAT_VERSION` 常量。
#[derive(Debug, Clone, Serialize)]
pub struct VersionInfo {
    pub cli: &'static str,
    pub unity: &'static str,
    pub pkg_format: u32,
    pub format_version: u32,
}

impl VersionInfo {
    pub fn current() -> Self {
        Self {
            cli: env!("CARGO_PKG_VERSION"),
            unity: env!("CARGO_PKG_VERSION"),
            pkg_format: ikat_core::asset::PKG_FORMAT_VERSION,
            format_version: FORMAT_VERSION,
        }
    }

    pub fn render_human(&self) -> String {
        format!(
            "ikat {} (unity {}, pkg_format {})",
            self.cli, self.unity, self.pkg_format
        )
    }
}

/// 命令输出的顶层 JSON 文档（check / build；后续命令沿用同骨架）。
#[derive(Serialize)]
pub struct CommandOutput {
    pub command: &'static str,
    pub format_version: u32,
    pub success: bool,
    pub summary: Summary,
    /// 失败时的顶层摘要消息（人类向；诊断详情在 diagnostics[]）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// 全量诊断（成功时为 warning；失败时 error + warning 混合，collect-all）。
    pub diagnostics: Vec<PackDiagnostic>,
    /// 产物报告，仅 build 成功时出现。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<BuildReport>,
}

impl CommandOutput {
    /// check 成功（零写入，可能有 warning）。
    pub fn check_ok(warnings: Vec<PackDiagnostic>) -> Self {
        Self {
            command: "check",
            format_version: FORMAT_VERSION,
            success: true,
            summary: Summary {
                errors: 0,
                warnings: warnings.len(),
            },
            message: None,
            diagnostics: warnings,
            report: None,
        }
    }

    /// build 成功（report 原样携带；warnings 同时出现在顶层 diagnostics 供统一消费）。
    pub fn build_ok(report: BuildReport) -> Self {
        let warnings = report.warnings.clone();
        Self {
            command: "build",
            format_version: FORMAT_VERSION,
            success: true,
            summary: Summary {
                errors: 0,
                warnings: warnings.len(),
            },
            message: None,
            diagnostics: warnings,
            report: Some(report),
        }
    }

    /// 失败：`BuildFailure` 的诊断全量 + 顶层 message。
    pub fn failure(command: &'static str, f: &BuildFailure) -> Self {
        Self {
            command,
            format_version: FORMAT_VERSION,
            success: false,
            summary: Summary {
                errors: f
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == Severity::Error)
                    .count(),
                warnings: f
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == Severity::Warning)
                    .count(),
            },
            message: Some(f.message.clone()),
            diagnostics: f.diagnostics.clone(),
            report: None,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            // BuildReport/PackDiagnostic 全是 String/数值字段，序列化不可能失败；
            // 兜底保 stdout 永远是合法 JSON。
            format!(
                "{{\"command\":\"{}\",\"format_version\":{},\"success\":false,\
                 \"summary\":{{\"errors\":0,\"warnings\":0}},\"diagnostics\":[]}}",
                self.command, FORMAT_VERSION
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JSON 契约快照：关键字段（command/format_version/success/summary/diagnostics
    /// 元素字段名）在 format_version 1 内不得变动。
    #[test]
    fn failure_json_shape() {
        let f = BuildFailure::validation(
            "1 error(s) in package",
            vec![crate::diag::PackDiagnostic::synthetic_error(
                "FenceUnknownTag",
                "main",
                "ui/main.html",
                "tag `p` is not in the fence",
            )],
        );
        let out = CommandOutput::failure("check", &f);
        let v: serde_json::Value = serde_json::from_str(&out.to_json()).unwrap();
        assert_eq!(v["command"], "check");
        assert_eq!(v["format_version"], 1);
        assert_eq!(v["success"], false);
        assert_eq!(v["summary"]["errors"], 1);
        assert_eq!(v["summary"]["warnings"], 0);
        assert_eq!(v["message"], "1 error(s) in package");
        let d = &v["diagnostics"][0];
        assert_eq!(d["severity"], "error");
        assert_eq!(d["code"], "FenceUnknownTag");
        assert_eq!(d["component"], "main");
        assert_eq!(d["file"], "ui/main.html");
        assert_eq!(d["line"], 1);
        assert_eq!(d["column"], 1);
        assert_eq!(d["message"], "tag `p` is not in the fence");
        assert!(v.get("report").is_none(), "失败不带 report 字段");
    }

    #[test]
    fn check_ok_json_shape() {
        let out = CommandOutput::check_ok(vec![]);
        let v: serde_json::Value = serde_json::from_str(&out.to_json()).unwrap();
        assert_eq!(v["command"], "check");
        assert_eq!(v["success"], true);
        assert_eq!(v["summary"]["errors"], 0);
        assert_eq!(v["diagnostics"].as_array().unwrap().len(), 0);
        assert!(v.get("message").is_none());
        assert!(v.get("report").is_none());
    }
}
