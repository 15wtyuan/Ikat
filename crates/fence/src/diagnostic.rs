#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCode {
    FenceUnknownTag,
    FenceUnknownAttr,
    FenceUnknownCssProp,
    FenceBadCssValue,
    FenceBadAttrValue,
    DuplicateId,
    UnclosedTag,
    InvalidContentModel,
    UnregisteredCustomElement,
    InvalidAriaRelation,
    TokenizerError,
    /// inline 元素直接放在 block 容器里（非 flex）。
    /// LoomGUI 没有 flex 之外的 inline flow：inline 标签在 block 上下文里被当 block-level
    /// （撑满父宽 + 竖排），和浏览器的 inline 行为（按内容收缩 + 横排流）不一致 → 渲染不可预测。
    /// 强制作者把 inline 元素放进 flex 容器，让布局意图显式。
    /// 详见 fence.md「inline 元素布局上下文」。
    FenceInlineElementInBlockContext,
    /// block 容器（display:block，非 flex）的直接子既有 inline 级（text/span/img）又有 block 级
    /// （div/控件/template）。
    /// rich-text-block（Stage 6.4）要求直接子**全**是 inline 级才会触发 inline flow；
    /// 混入 block 子会让 inline flow 不可定义（一部分要横排流、一部分要撑满竖排，同一 formatting
    /// context 里无解）。属「fail-loud 不静默降级」原则——作者须显式选边：要么 inline 子全裹进
    /// 一个子 div（让外层变全 block），要么把容器改 display:flex（让所有子变 flex item）。
    /// 详见 fence.md「rich-text-block 分类（阶段 6.4）」。
    FenceMixedInlineBlock,
    /// border-width 已声明但 border-style 缺省（CSS initial=none）。
    /// 浏览器按 CSS 规范不画边框，而 LoomGUI 历史实现会画 → 预览 ≠ 运行时。
    /// 详见 fence.md「围栏内一致性 warning」。
    FenceBorderWithoutStyle,
    /// background-image 已声明但 background-size 缺省。
    /// CSS 默认 `auto`（原始尺寸），LoomGUI 默认 `stretch`（拉伸填满）→ 预览 ≠ 运行时。
    /// 详见 fence.md「围栏内一致性 warning」。
    FenceBgImageWithoutSize,
    /// LoomGUI 控件（role 驱动：`role="progressbar"`/`role="slider"`/...）无任何 CSS 规则命中。
    /// 控件不带 UA 默认样式（core 保持纯净，不开「框架自带样式源」先例），
    /// 未命中 = 运行时渲染空白。强制作者为控件及其内部 slot 子节点提供 CSS。
    /// 详见 fence.md「控件 CSS 命中校验」。
    FenceControlWithoutCss,
    /// 控件结构 CSS 契约缺失：运行时行为依赖的结构声明（如 combobox 的
    /// `position:relative` 锚点 + listbox `position:absolute` 脱流）未声明。
    /// 与「无命中」互补——命中只证明在样式，不证明结构齐全；缺失症状在
    /// PlayMode 才可见（弹层撑开容器/定位飞出）。表驱动（control_css_check
    /// `STRUCTURE_CSS_CONTRACTS`），详见 fence.md「控件 CSS 命中校验」。
    FenceControlStructureCss,
    /// role 驱动控件缺少 spec §2.2 规定的必需子角色/slot（如 `combobox` 缺
    /// `role=listbox` 子、`slider` 缺 `data-slot=thumb` 子）。旧模式下框架运行时
    /// 注入 `.loom-*` 子节点故结构必然完整；新模式由作者自写结构，可能漏写——
    /// 打包期严格拦截，不依赖运行时 reparent 兜底。详见 fence.md「控件结构契约」。
    FenceMissingControlChild,
    /// `role` 属性值不在 role 注册表内（通常是拼错，如 `role="silder"`）。
    /// role 在 LoomGUI 是控件类型系统本身：未知值若静默回退成基础标签类型，
    /// 元素会跳过全部控件校验（必需子结构、CSS 命中、结构 CSS 契约），构建
    /// 绿灯但运行时得到空白容器——属「不静默降级」原则要拦的典型类别。
    /// 注册表见 schema::tag::ROLE_TO_SEMANTIC + textbox/tabpanel 例外。
    FenceUnknownRole,
    /// `<link rel="stylesheet" href>` 的外部 CSS 读取失败（文件缺失 / io 错误）。
    /// href 相对所在 HTML 文件解析；加载失败即 error——静默丢样式是最难排查的
    /// 降级形态。
    FenceStylesheetNotFound,
    /// `display: inline` 声明（warning）。围栏没有 inline flow——inline 运行时映射为
    /// flex 容器，与浏览器 inline（收缩宽 + 横排流）语义不同；显式声明多半是先验误用。
    FenceDisplayInline,
    /// `transition` 声明了运行时不支持的属性（warning）。transition 引擎只覆盖
    /// background-color / color / opacity 三通道；布局属性（width/transform 等）
    /// 声明了也不过渡，浏览器先验会翻车。
    FenceTransitionUnsupportedProp,
    /// 行内文本元素（rich-text-block 归类的 span 等）上声明 width/height 族（warning）。
    /// 该类元素被折进父级 inline flow，无独立盒子——尺寸声明恒无效（与浏览器对
    /// inline 元素的行为一致，但 AI 先验常以为会生效）。可定尺寸路径：flex item
    /// div / img / 显式 display:flex。
    FenceInlineSizing,
    /// 页面侧 CSS 规则只可能命中 slot 投射内容（warning）。投影内容归组件样式宇宙
    /// （样式墙：页面规则不穿 host 边界），该规则运行时恒为死代码——给投影内容
    /// 定样式写在组件文件 `<style>` 里。
    FencePageRuleProjectedOnly,
    /// slider 滑块头（`data-slot="thumb"`）上声明定位属性（warning）。thumb 位移由
    /// 控件按 value 全权驱动（水平位移 + 垂直居中），运行时逐帧归零其 inset/margin
    /// ——作者的 `top`/`left`/`margin` 定位不生效且叠加会双偏移。尺寸与外观照常。
    FenceSliderThumbPositioned,
}

#[derive(Debug, Clone)]
pub struct SourceLocation {
    pub file: String,
    pub offset: usize,
    pub line: u32,
    pub column: u32,
    pub source_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    Help,
    Note,
    Related,
}

#[derive(Debug, Clone)]
pub struct DiagnosticNote {
    pub kind: NoteKind,
    pub text: String,
    pub location: Option<SourceLocation>,
}

/// A structured diagnostic produced by the fence pipeline.
///
/// The pipeline collects ALL diagnostics in a single pass and reports them
/// once, rather than failing on the first error -- this is critical for
/// AI-assisted authoring where fixing all errors in one round minimises
/// dialogue turns.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: DiagnosticCode,
    pub message: String,
    pub location: SourceLocation,
    pub notes: Vec<DiagnosticNote>,
}

impl Diagnostic {
    pub fn error(
        code: DiagnosticCode,
        message: impl Into<String>,
        location: SourceLocation,
    ) -> Self {
        Self {
            severity: Severity::Error,
            code,
            message: message.into(),
            location,
            notes: Vec::new(),
        }
    }

    /// 构造一条 warning（severity=Warning）。围栏内一致性诊断用——
    /// 这类问题是「合法但预览 ≠ 运行时」的不一致，不阻断打包，只提醒作者补全声明。
    pub fn warning(
        code: DiagnosticCode,
        message: impl Into<String>,
        location: SourceLocation,
    ) -> Self {
        Self {
            severity: Severity::Warning,
            code,
            message: message.into(),
            location,
            notes: Vec::new(),
        }
    }

    pub fn with_help(mut self, text: impl Into<String>) -> Self {
        self.notes.push(DiagnosticNote {
            kind: NoteKind::Help,
            text: text.into(),
            location: None,
        });
        self
    }
}

/// Pre-computed line-offset table for O(log n) offset-to-line/column lookup.
///
/// Built once per source file. `locate(offset)` performs a binary search over
/// `line_starts` to find the 1-based (line, column) pair.
#[derive(Debug, Clone)]
pub struct LineMap {
    line_starts: Vec<usize>,
    source: String,
}

impl LineMap {
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self {
            line_starts,
            source: source.to_string(),
        }
    }

    /// Convert a byte offset to a 1-based (line, column) pair.
    pub fn locate(&self, offset: usize) -> (u32, u32) {
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let col = offset.saturating_sub(self.line_starts[line_idx]);
        ((line_idx + 1) as u32, (col + 1) as u32)
    }

    /// Build a full `SourceLocation` for a byte offset, including the source
    /// text of the offending line (trimmed of trailing newlines).
    pub fn source_location(&self, offset: usize, file: String) -> SourceLocation {
        let (line, column) = self.locate(offset);
        let source_text = self.source_line(line);
        SourceLocation {
            file,
            offset,
            line,
            column,
            source_text,
        }
    }

    fn source_line(&self, line: u32) -> String {
        let idx = (line as usize).saturating_sub(1);
        let start = *self.line_starts.get(idx).unwrap_or(&0);
        let end = self
            .line_starts
            .get(idx + 1)
            .copied()
            .unwrap_or(self.source.len());
        self.source[start..end]
            .trim_end_matches('\n')
            .trim_end_matches('\r')
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_map_single_line() {
        let map = LineMap::new("hello world");
        assert_eq!(map.locate(0), (1, 1));
        assert_eq!(map.locate(5), (1, 6));
    }

    #[test]
    fn line_map_multi_line() {
        let map = LineMap::new("ab\ncd\nef");
        assert_eq!(map.locate(0), (1, 1));
        assert_eq!(map.locate(3), (2, 1));
        assert_eq!(map.locate(6), (3, 1));
    }

    #[test]
    fn source_location_has_line_text() {
        let map = LineMap::new("ab\ncd");
        let loc = map.source_location(3, "test.html".into());
        assert_eq!(loc.line, 2);
        assert_eq!(loc.column, 1);
        assert_eq!(loc.source_text, "cd");
    }
}
