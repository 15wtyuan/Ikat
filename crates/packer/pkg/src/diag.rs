//! 打包器结构化诊断：围栏/打包错误与警告的统一载体。
//!
//! fence 库的诊断（`fence::Diagnostic`）在单文件内 collect-all；本模块把它与打包器
//! 自产错误（组件注册、资源覆盖、重名等）拉平成同一结构 `PackDiagnostic`，供
//! CLI 人类渲染 / `--format json` 机读 / GUI 呈现三端共用。失败的诊断列表携带
//! collect-all 全量（含 warning）——AI 一轮修完 error 的同时能看到 warning。

use serde::{Deserialize, Serialize};

/// 打包器合成诊断码（非 fence `DiagnosticCode`；与围栏码同场输出，靠字符串区分）。
pub mod code {
    /// 包内页面组件重名 / 注册表组件重名。
    pub const DUPLICATE_COMPONENT_NAME: &str = "DuplicateComponentName";
    /// 组件文件名缺连字符（custom element 命名规则）。
    pub const COMPONENT_NAME_REQUIRES_HYPHEN: &str = "ComponentNameRequiresHyphen";
    /// 组件模板非单一根元素。
    pub const COMPONENT_MULTIPLE_ROOTS: &str = "ComponentMultipleRoots";
    /// 组件动画与宿主同名碰撞（warning：宿主优先）。
    pub const COMPONENT_KEYFRAMES_COLLISION: &str = "ComponentKeyframesNameCollision";
    /// 字体文件缺失。
    pub const FONT_FILE_MISSING: &str = "FontFileMissing";
    /// HTML 引用的图不在任何 atlas（覆盖缺失）。
    pub const SPRITE_MISSING_FROM_ATLAS: &str = "SpriteMissingFromAtlas";
    /// 同一张图进了多个 atlas（覆盖冲突）。
    pub const SPRITE_ATLAS_CONFLICT: &str = "SpriteAtlasConflict";
    /// 单图 + padding 超过图集单页上限。
    pub const ATLAS_IMAGE_OVERFLOW: &str = "AtlasImageOverflow";
    /// 打包器通用错误：结构性错误暂无专属码时落此（bridge/子树校验等 String 错误）。
    pub const PACK_ERROR: &str = "PackError";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

impl From<loomgui_fence::diagnostic::Severity> for Severity {
    fn from(s: loomgui_fence::diagnostic::Severity) -> Self {
        match s {
            loomgui_fence::diagnostic::Severity::Error => Severity::Error,
            loomgui_fence::diagnostic::Severity::Warning => Severity::Warning,
        }
    }
}

/// 一条打包诊断：围栏诊断或打包器合成错误的统一形态。
///
/// 序列化字段是机读契约（`--format json` 的 diagnostics[] 元素），版本内只增不改。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackDiagnostic {
    pub severity: Severity,
    /// 短码：围栏码 = `DiagnosticCode` 的 Debug 名（字符串稳定）；合成码见 [`code`]。
    pub code: String,
    /// 产生诊断的组件名（页面名或 components/&lt;tag&gt;.html 的 tag）。
    pub component: String,
    /// 源文件相对 workspace_root（正斜杠）。
    pub file: String,
    /// 1-based 行号。
    pub line: u32,
    /// 1-based 列号。
    pub column: u32,
    /// 人类可读主信息（含修复引导）。
    pub message: String,
    /// help note 文案（围栏 diagnostic 带 `NoteKind::Help` 时）。
    pub help: Option<String>,
}

impl PackDiagnostic {
    /// fence 诊断 → 打包诊断。
    ///
    /// file 用 `html_rel`（真实磁盘路径）覆盖而非 `diagnostic.location.file`——后者被
    /// `parse_template(src, name)` 设成组件名，作者拿组件名定位不到磁盘文件；line/column
    /// 相对 src 算出，与 html_rel 指向同一份文件，覆盖后仍准确。
    pub fn from_fence(
        d: &loomgui_fence::diagnostic::Diagnostic,
        component: &str,
        html_rel: &str,
    ) -> Self {
        Self {
            severity: d.severity.into(),
            code: format!("{:?}", d.code),
            component: component.to_string(),
            file: html_rel.to_string(),
            line: d.location.line,
            column: d.location.column,
            message: d.message.clone(),
            help: d
                .notes
                .iter()
                .find(|n| n.kind == loomgui_fence::diagnostic::NoteKind::Help)
                .map(|n| n.text.clone()),
        }
    }

    /// 打包器合成错误（非围栏来源）：无结构化位置，行列占位 1:1。
    pub fn synthetic_error(
        code: &str,
        component: &str,
        file: &str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: Severity::Error,
            code: code.to_string(),
            component: component.to_string(),
            file: file.to_string(),
            line: 1,
            column: 1,
            message: message.into(),
            help: None,
        }
    }

    /// 打包器合成 warning（非围栏来源）。
    pub fn synthetic_warning(
        code: &str,
        component: &str,
        file: &str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: Severity::Warning,
            code: code.to_string(),
            component: component.to_string(),
            file: file.to_string(),
            line: 1,
            column: 1,
            message: message.into(),
            help: None,
        }
    }

    /// 渲染 CLI 文本（stderr）。error 用 rustc 风格 header；warning 沿用历史多行
    /// 格式（message 含多句引导，逐行缩进保持视觉层级）——不破坏既有呈现习惯。
    pub fn render(&self) -> String {
        match self.severity {
            Severity::Error => {
                let mut out = format!(
                    "error[{}]: {} ({}:{}:{})",
                    self.code, self.message, self.file, self.line, self.column
                );
                if let Some(help) = &self.help {
                    out.push_str("\n  help: ");
                    out.push_str(help);
                }
                out
            }
            Severity::Warning => {
                let mut out = format!(
                    "warning[{}] in component \"{}\" ({}:{}:{}):",
                    self.code, self.component, self.file, self.line, self.column
                );
                for line in self.message.lines() {
                    out.push_str("\n  ");
                    out.push_str(line);
                }
                if let Some(help) = &self.help {
                    out.push_str("\n  help: ");
                    out.push_str(help);
                }
                out
            }
        }
    }
}

/// 构建失败：携带退出码语义 + collect-all 诊断全量。
///
/// 退出码契约（第一天锁定）：1 = 数据性失败（内容错误，输出里有全部诊断）；
/// 2 = 工具性失败（用法/配置/io，修源文件解决不了）。
#[derive(Debug, Clone, Serialize)]
pub struct BuildFailure {
    pub exit_code: u8,
    pub message: String,
    /// collect-all 全量（error + warning——失败时 warning 一并给出）。
    pub diagnostics: Vec<PackDiagnostic>,
}

impl BuildFailure {
    /// 工具性失败（exit 2）。
    pub fn config(message: impl Into<String>) -> Self {
        Self {
            exit_code: 2,
            message: message.into(),
            diagnostics: Vec::new(),
        }
    }

    /// 数据性失败（exit 1）。
    pub fn validation(message: impl Into<String>, diagnostics: Vec<PackDiagnostic>) -> Self {
        Self {
            exit_code: 1,
            message: message.into(),
            diagnostics,
        }
    }
}

impl From<String> for BuildFailure {
    /// 子模块遗留的 `Result<_, String>` 错误（io/workspace 读取等）统一归工具性失败。
    fn from(s: String) -> Self {
        Self::config(s)
    }
}

impl std::fmt::Display for BuildFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_error_is_rustc_style() {
        let d = PackDiagnostic {
            severity: Severity::Error,
            code: "FenceUnknownTag".into(),
            component: "main".into(),
            file: "ui/battle/main.html".into(),
            line: 12,
            column: 3,
            message: "tag `p` is not in the fence".into(),
            help: Some("use div, or a role-driven control".into()),
        };
        let r = d.render();
        assert!(
            r.starts_with(
                "error[FenceUnknownTag]: tag `p` is not in the fence (ui/battle/main.html:12:3)"
            ),
            "got: {r}"
        );
        assert!(r.contains("\n  help: use div"), "got: {r}");
    }

    #[test]
    fn render_warning_keeps_multiline_format() {
        let d = PackDiagnostic {
            severity: Severity::Warning,
            code: "FenceBorderWithoutStyle".into(),
            component: "warn".into(),
            file: "warn.html".into(),
            line: 1,
            column: 1,
            message: "第一行\n第二行".into(),
            help: None,
        };
        let r = d.render();
        assert!(r.starts_with(
            "warning[FenceBorderWithoutStyle] in component \"warn\" (warn.html:1:1):"
        ));
        assert!(
            r.contains("\n  第一行\n  第二行"),
            "多行 message 逐行缩进: {r}"
        );
    }

    #[test]
    fn failure_exit_codes() {
        assert_eq!(BuildFailure::config("x").exit_code, 2);
        assert_eq!(BuildFailure::validation("x", vec![]).exit_code, 1);
        assert_eq!(BuildFailure::from("io".to_string()).exit_code, 2);
    }
}
