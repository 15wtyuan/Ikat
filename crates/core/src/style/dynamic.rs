//! 运行时伪类匹配的动态规则层。
//!
//! 本模块实现 \match_element_with_state\：选择器匹配 + 伪类状态标 +
//! ematch_pseudo_classes\，对所有节点做重算（写 Node.style + 标 layout dirty）。
//!
//! 类型模型（\ParsedSelector\/\Compound\/\Combinator\/\Specificity\）+ \Declaration\（CSS 声明）+
//! \compound_matches_node\（运行时 compound 匹配）+ 动态规则匹配全部无条件编译——
//! bincode 反序列化的 \.pkg.bin\ 就是这些结构，runtime 不再 parse 选择器，直接用反序列化结构。
//! 字符串 → 这些结构的解析器在 fence crate（\loomgui_fence\）——由 spike（css_rules.rs）落地。

use serde::{Deserialize, Serialize};

// 运行时选择器类型模型（无条件编译，支持 bincode 反序列化 + rematch）。

/// CSS 声明（prop + value），序列化进 .pkg.bin DynamicRuleSection。
/// \PartialEq\ 用于 instantiate 伪类去重（同选择器 + 同声明视为重复，跳过）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Declaration {
    pub prop: String,
    pub value: String,
}

/// 选择器组合子：标签/类/id/后代/子代 + 伪类状态门（hover/active/disabled）。
/// `PartialEq` 供 instantiate 规则去重（结构相等 = 同选择器，含 raw/compound/specificity）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedSelector {
    pub raw: String,
    pub compound: Vec<Compound>, // 复合选择器链（后代/子代分隔）
    pub specificity: Specificity,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Compound {
    pub tag: Option<String>,
    pub classes: Vec<String>,
    pub id: Option<String>,
    pub combinator: Combinator, // 本 compound 与前一个的关系
    pub pseudo_hover: bool,
    pub pseudo_active: bool,
    pub pseudo_disabled: bool,
    pub pseudo_focus: bool,
    /// 属性选择器（`[attr]` / `[attr="val"]`）。出现属性选择器即把规则划入动态规则表
    /// （运行时按节点 attrs 匹配，静态 cascade 无法预判），由 compound_matches_node 匹配。
    pub attrs: Vec<AttrSelector>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Combinator {
    Descendant,
    Child,
}

/// 属性选择器运算符（围栏子集：仅存在性 + 相等；不做 ~=, ^=, $=, *=）。
/// Copy：无字段枚举，运行时 attr 匹配按值分派（Eq/Exists），Copy 使其在元组 match 等
/// 场景零成本取用，无需克隆。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttrOp {
    /// `[attr]` — 属性存在即匹配。
    Exists,
    /// `[attr="val"]` — 属性值字面相等。
    Eq,
}

/// 单条属性选择器（如 `[data-page="1"]`）。name 用小写归一（HTML 属性名大小写不敏感）。
/// value 仅 Eq 有意义；Exists 时为 None。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttrSelector {
    pub name: String,
    pub op: AttrOp,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Specificity(pub u32, pub u32, pub u32); // (id 数, class 数, tag 数)

// ── 动态规则表（DynamicRule 持有 ParsedSelector + Declarations，均 bincode 可序列化）──

use crate::scene::node::{NodeFlags, NodeId, Scene};
use crate::style::mapping::apply_decl;
use crate::style::resolved::{ResolvedStyle, TransitionSpec};
use std::collections::HashMap;

/// cascade 期 transient：节点显式声明了哪些可继承属性（bitmask）。
/// 不进 ResolvedStyle（避免改 bincode/pkg 格式），不进 Scene 持久字段。
use crate::style::resolved::InheritedSet;

const INH_FONT_SIZE: u16 = 1 << 0;
const INH_COLOR: u16 = 1 << 1;
const INH_FONT_FAMILY: u16 = 1 << 2;
const INH_FONT_WEIGHT: u16 = 1 << 3;
const INH_TEXT_ALIGN: u16 = 1 << 4;
const INH_LINE_HEIGHT: u16 = 1 << 5;
const INH_LETTER_SPACING: u16 = 1 << 6;
const INH_WHITE_SPACE_NOWRAP: u16 = 1 << 7;

/// prop 名 → 可继承属性 bit（非可继承返 None）。单一真相源：bit 的定义（本表 INH_*）与
/// 消费（rematch set bit + propagate copy_if_unset）都在 core。fence css_resolve 调本函数
/// 把 inline 可继承声明 bake 进 ResolvedStyle.inherited_set，避免运行时被父值覆盖。
pub fn inherited_bit(prop: &str) -> Option<u16> {
    match prop.trim() {
        "font-size" => Some(INH_FONT_SIZE),
        "color" => Some(INH_COLOR),
        "font-family" => Some(INH_FONT_FAMILY),
        "font-weight" => Some(INH_FONT_WEIGHT),
        "text-align" => Some(INH_TEXT_ALIGN),
        "line-height" => Some(INH_LINE_HEIGHT),
        "letter-spacing" => Some(INH_LETTER_SPACING),
        "white-space" => Some(INH_WHITE_SPACE_NOWRAP),
        _ => None,
    }
}

// ── Spec-4a：inline override（便签层）set-ness 位图 ────────────────────────────
//
// InlineSet 与 InheritedSet 同构（newtype 包位图），但语义相反：
//   - InheritedSet: 打包期 bake 进 base_style.inherited_set，序列化进 pkg.bin
//   - InlineSet:    运行时 transient，C# Style.X=v 写入，不进 pkg.bin
// 继承属性 bit 复用 INH_*（同一位空间，不重新编号）；非继承属性用 INLINE_*。
//
// **位编号说明（为何从 bit 8 起而不是 bit 9）：** INH_* 实际占用 bits 0-7（8 个继承属性，
// 不是 9 个）。task spec 草稿的 `INLINE_WIDTH = 1 << 9` 与"前 9 bit"措辞是 off-by-one——
// 按 INH_* 实际位数，bit 8 是下一个可用位。从 bit 8 起，bits 8-31 共 24 位恰好容纳
// apply_decl 处理的全部 24 个非继承属性（width/height/min-*/max-*/padding/margin/
// border-width/gap/flex-*/display/overflow-x/y/position/left/top/right/bottom/
// background-color/opacity）。无任何属性被遗漏（task spec 硬约束："不要漏"）。
// 若未来需要扩展（如 visibility/z-index 进 inline），位图升级到 u64 即可，无需改 API。

/// inline override 的 set-ness 位图。复用 INH_* 给继承属性（bits 0-7），
/// 其后是 INLINE_* 非继承属性 bit。rematch 用它应用便签层；继承子集 OR 进 set_map
/// 让 propagate 自动传播父的 inline 继承值给未自设的子。纯运行时 transient，不进 pkg.bin。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InlineSet(pub u32);

/// 所有继承属性 bit 的 OR——rematch 用它把 inline 的继承部分（bits 0-7）并进 set_map，
/// 使 propagate_inherited 把父的 inline 继承值（如 inline color）传给未自设的子。
/// INH_* 是 u16，这里 OR 成 u32 与 InlineSet 同位宽。
pub const INH_ALL_MASK: u32 = INH_FONT_SIZE as u32
    | INH_COLOR as u32
    | INH_FONT_FAMILY as u32
    | INH_FONT_WEIGHT as u32
    | INH_TEXT_ALIGN as u32
    | INH_LINE_HEIGHT as u32
    | INH_LETTER_SPACING as u32
    | INH_WHITE_SPACE_NOWRAP as u32;

// 非继承属性 bit（编号接在 INH_* 之后，从 bit 8 起）。对照 apply_decl 能处理的属性清单，
// 逐个分配 1 bit。INH_* 位（继承属性）复用 inherited_bit，不重复定义。
pub const INLINE_WIDTH: u32 = 1 << 8;
pub const INLINE_HEIGHT: u32 = 1 << 9;
pub const INLINE_MIN_WIDTH: u32 = 1 << 10;
pub const INLINE_MIN_HEIGHT: u32 = 1 << 11;
pub const INLINE_MAX_WIDTH: u32 = 1 << 12;
pub const INLINE_MAX_HEIGHT: u32 = 1 << 13;
pub const INLINE_PADDING: u32 = 1 << 14;
pub const INLINE_MARGIN: u32 = 1 << 15;
pub const INLINE_BORDER_WIDTH: u32 = 1 << 16;
pub const INLINE_GAP: u32 = 1 << 17;
pub const INLINE_FLEX_DIRECTION: u32 = 1 << 18;
pub const INLINE_FLEX_WRAP: u32 = 1 << 19;
pub const INLINE_JUSTIFY_CONTENT: u32 = 1 << 20;
pub const INLINE_ALIGN_ITEMS: u32 = 1 << 21;
pub const INLINE_DISPLAY: u32 = 1 << 22;
pub const INLINE_OVERFLOW_X: u32 = 1 << 23;
pub const INLINE_OVERFLOW_Y: u32 = 1 << 24;
pub const INLINE_POSITION: u32 = 1 << 25;
pub const INLINE_LEFT: u32 = 1 << 26;
pub const INLINE_TOP: u32 = 1 << 27;
pub const INLINE_RIGHT: u32 = 1 << 28;
pub const INLINE_BOTTOM: u32 = 1 << 29;
pub const INLINE_BACKGROUND_COLOR: u32 = 1 << 30;
pub const INLINE_OPACITY: u32 = 1 << 31;

/// prop 名 → InlineSet bit。继承属性复用 `inherited_bit`（bits 0-7），非继承属性走
/// INLINE_*（bits 8-31）。返回 None = 该属性不可 inline（apply_decl 也不处理）。
///
/// **覆盖范围：** apply_decl 处理的所有非继承属性都有 bit（对照
/// `crates/core/src/style/mapping.rs::apply_decl`）。inset 四边（top/right/bottom/left）
/// 各占独立 bit（虽由 position 派生，但 C# Style API 暴露为 4 个独立 Length setter）。
/// 少数装饰性/列表型属性（transition / text-shadow / -webkit-text-stroke / font-effect /
/// box-shadow / background-image / background-size / border-color / border-radius /
/// transform / order / pointer-events / background-clip）不在 inline 范围：
/// 它们要么是列表（Vec）不便简单 set/unset，要么已有独立路径（transform 走 NodeAnim），
/// 要么设计期声明为主（bg-image 等）。这些若后续需要 inline，再扩位图。
pub fn inline_bit(prop: &str) -> Option<u32> {
    if let Some(b) = inherited_bit(prop) {
        return Some(b as u32);
    }
    match prop.trim() {
        "width" => Some(INLINE_WIDTH),
        "height" => Some(INLINE_HEIGHT),
        "min-width" => Some(INLINE_MIN_WIDTH),
        "min-height" => Some(INLINE_MIN_HEIGHT),
        "max-width" => Some(INLINE_MAX_WIDTH),
        "max-height" => Some(INLINE_MAX_HEIGHT),
        "padding" => Some(INLINE_PADDING),
        "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => Some(INLINE_PADDING),
        "margin" => Some(INLINE_MARGIN),
        "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => Some(INLINE_MARGIN),
        "border-width" => Some(INLINE_BORDER_WIDTH),
        "border-top" | "border-right" | "border-bottom" | "border-left" => {
            Some(INLINE_BORDER_WIDTH)
        }
        "gap" => Some(INLINE_GAP),
        "flex-direction" => Some(INLINE_FLEX_DIRECTION),
        "flex-wrap" => Some(INLINE_FLEX_WRAP),
        "justify-content" => Some(INLINE_JUSTIFY_CONTENT),
        "align-items" => Some(INLINE_ALIGN_ITEMS),
        "display" => Some(INLINE_DISPLAY),
        "overflow-x" => Some(INLINE_OVERFLOW_X),
        "overflow-y" => Some(INLINE_OVERFLOW_Y),
        "position" => Some(INLINE_POSITION),
        "left" => Some(INLINE_LEFT),
        "top" => Some(INLINE_TOP),
        "right" => Some(INLINE_RIGHT),
        "bottom" => Some(INLINE_BOTTOM),
        "background-color" => Some(INLINE_BACKGROUND_COLOR),
        "opacity" => Some(INLINE_OPACITY),
        _ => None,
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DynamicRuleTable {
    pub rules: Vec<DynamicRule>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DynamicRule {
    pub selector: ParsedSelector,
    pub declarations: Vec<Declaration>,
}

/// 带作用域的动态规则（scene 运行时态，不进 pkg）。main-design §5.4 Shadow DOM 风格：
/// 模板实例化时，规则绑定到实例根 NodeId；rematch 只在 scope 内匹配 + 后代选择器不穿透边界。
/// `scope_root == NodeId::INVALID` = 全局规则（UIContext.StyleSheet 逃生舱），跨作用域命中。
/// pkg 里的 DynamicRuleTable（无 scope）在 instantiate 时包装成 ScopedRule。
#[derive(Debug, Clone)]
pub struct ScopedRule {
    pub rule: DynamicRule,
    pub scope_root: crate::scene::node::NodeId,
}

/// scene 级规则容器（运行时）。pkg 用 DynamicRuleTable（无 scope，可序列化）。
#[derive(Debug, Clone, Default)]
pub struct ScopedRuleTable {
    pub entries: Vec<ScopedRule>,
}

use crate::scene::node::{ControlState, NodeKind};

/// 运行时版 compound 匹配（消费 Node 而非 ElementData，运行时无 ElementTree）。
/// 匹配 tag/classes/id（不含伪类状态——状态由 match_element_with_state 门控）。
/// id 属性存 Node.id_attr（`id="..."`）；Node.id 是 NodeId 索引身份，二者不同。
///
/// **常驻：**runtime rematch 用，不依赖 parse feature。
pub fn compound_matches_node(c: &Compound, node_id: NodeId, scene: &Scene) -> bool {
    let node = scene.get(node_id).expect("live node");
    if let Some(t) = &c.tag {
        // NodeKind → HTML 标签名：标准元素用其 tag，控件 kind 回溯到作者写的 tag
        // （input/progress），使 `input[type="range"]`、`progress` 等选择器在运行时 rematch
        // 仍能命中。与 fence schema/tag.rs resolve_semantic（tag→SemanticKind→NodeKind）互逆：
        // fence 列出的、且在运行时成为 Node 的每个 tag 都有对应 arm，使任何通过围栏的 tag
        // 选择器在 rematch 仍命中。
        //
        // 例外：
        //  - CustomElement：作者写的自定义元素 tag 含连字符（如 `<my-widget>`），NodeKind 只记
        //    CustomElement 一个判别值、丢弃原始 tag 名，无法逆推。因此带连字符的自定义 tag
        //    选择器在 rematch 不会命中（围栏放行，运行时退回 div）。要支持需在 NodeKind 侧保留
        //    原始 tag 字面值，属于已知限制，非本映射 bug。
        let kind_tag = match node.kind {
            NodeKind::Container => "div",
            NodeKind::Button => "button",
            NodeKind::Image => "img",
            NodeKind::TextNode | NodeKind::TextElement => "span",
            NodeKind::TextArea => "textarea",
            NodeKind::Dropdown => "select",
            NodeKind::OptionItem => "option",
            NodeKind::ListItem => "li",
            NodeKind::ListView => "ul",
            NodeKind::Slot => "slot",
            // <template> 已进场景树（强制 display:none），保留逆映射使 `template` tag 选择器可命中。
            NodeKind::Template => "template",
            // role 驱动控件（TabList/Tab）无专属 tag，逆映射取最接近的宿主 tag：
            //  - TabList → div（容器宿主，与 ListView 同）；
            //  - Tab → button（settings.html 用 <button role=tab>，点击语义同 Button）。
            // 作者几乎不靠 tag 选择器选 tab，role/class 选择器不受影响。
            NodeKind::TabList => "div",
            NodeKind::Tab => "button",
            // input 变体：type 在 parse 期固化为独立 kind，tag 统一为 "input"
            NodeKind::TextField
            | NodeKind::NumberField
            | NodeKind::Slider
            | NodeKind::Toggle
            | NodeKind::RadioButton => "input",
            NodeKind::ProgressBar => "progress",
            // CustomElement：原始带连字符 tag 已在 NodeKind 中丢失，无法逆推（见上方注释）。
            NodeKind::CustomElement => "div",
        };
        if kind_tag != t.as_str() {
            return false;
        }
    }
    if let Some(id) = &c.id {
        if node.id_attr.as_deref() != Some(id.as_str()) {
            return false;
        }
    }
    for cls in &c.classes {
        if !node.classes.iter().any(|nc| nc == cls) {
            return false;
        }
    }
    // 属性选择器：attr_matches_node 按 name 分派——[type=] 查 NodeKind、[aria-*] 合成
    // ControlState 实时值、[role=]/[data-slot=] 查 RoleTable。Node 不携带任意 HTML 属性
    // 字面值，故非这几类的 attr 不命中。
    for a in &c.attrs {
        if !attr_matches_node(scene, node_id, a) {
            return false;
        }
    }
    true
}

/// 运行时属性选择器匹配。Node 不存任意 HTML 属性字面值，按 name 分派到三类来源：
/// - `[type="x"]`：role 化重构后控件由 `role` 驱动，但 `[type=...]` 选择器作为便利层保留——selector 值 → NodeKind 精确对应（`type_matches_nodekind`），如 `[type="checkbox"]` 命中 Toggle。
/// - `[aria-*]`：aria 实时值从 ControlState 合成（`synth_aria_value`），随控件状态变。
/// - `[role="x"]` / `[data-slot="x"]`：从 RoleTable 查打包期提取的静态值。
///
/// 其他 attr name 不命中（Node 无对应字面值）。Exists 与 Eq 均支持：Exists 判该 attr 对本
/// 节点是否有语义（如 `[aria-checked]` 对 Toggle 有、对普通 div 无），Eq 判合成/查表值 == 字面值。
fn attr_matches_node(scene: &Scene, id: NodeId, a: &AttrSelector) -> bool {
    let name = a.name.as_str();
    // aria-* 实时合成（值随 ControlState 变）
    if let Some(rest) = name.strip_prefix("aria-") {
        return match a.op {
            AttrOp::Eq => synth_aria_value(scene, id, rest).as_deref() == a.value.as_deref(),
            AttrOp::Exists => synth_aria_value(scene, id, rest).is_some(),
        };
    }
    match (name, a.op) {
        ("role", AttrOp::Eq) => scene.roles.role_of(id) == a.value.as_deref(),
        ("role", AttrOp::Exists) => scene.roles.role_of(id).is_some(),
        ("data-slot", AttrOp::Eq) => a
            .value
            .as_deref()
            .is_some_and(|v| scene.roles.slot_of(id, v).is_some()),
        ("data-slot", AttrOp::Exists) => scene.roles.get(id).is_some_and(|i| !i.slots.is_empty()),
        ("type", AttrOp::Eq) => match &a.value {
            Some(val) => type_matches_nodekind(scene, id, val),
            None => false,
        },
        // [type] 存在形式不支持：type 不再是结构性属性（input 已下线，控件走 role），
        // 且 Node 不存 type 字面值，存在性无匹配意义。
        ("type", AttrOp::Exists) => false,
        _ => false,
    }
}

/// `[type="x"]` selector 值 → NodeKind 精确匹配（runtime 选择器便利层）。
///
/// role 化重构后控件由 `role` 驱动（`<div role="switch">` → Toggle），但作者仍可写
/// `[type="checkbox"]` 这类属性选择器——本函数把选择器值映射到对应 NodeKind（`checkbox` →
/// Toggle、`range` → Slider、`text` → TextField ...）与节点实际 kind 比对。映射表是 selector
/// 便利层，不再与 fence `resolve_semantic`（已无 input 分支）耦合。
fn type_matches_nodekind(scene: &Scene, id: NodeId, val: &str) -> bool {
    let Some(node) = scene.get(id) else {
        return false;
    };
    let expected = match val {
        "text" => NodeKind::TextField,
        "number" => NodeKind::NumberField,
        "range" => NodeKind::Slider,
        "checkbox" => NodeKind::Toggle,
        "radio" => NodeKind::RadioButton,
        _ => return false,
    };
    node.kind == expected
}

/// 从 ControlState 合成 aria 属性的实时值（运行时随控件状态变）。Node 不存 HTML 属性字面值，
/// aria-* 的当前值必须由控件状态派生。返回 None = 该 aria 对本节点无语义（选择器不命中）。
///
/// - `aria-checked`：Toggle / Radio 的 `checked`（"true"/"false"）。
/// - `aria-expanded`：Dropdown 的 `open`。
/// - `aria-valuenow`：Progress / Slider 的 `value`（f32）或 NumberField 的数值文本。
/// - `aria-multiline`：**静态**，按 NodeKind（TextArea="true"），不查 ControlState——TextArea
///   与 TextField 共用 EditState，多行属性由标签（textarea vs input）决定而非运行时状态。
fn synth_aria_value(scene: &Scene, id: NodeId, aria: &str) -> Option<String> {
    // aria-multiline 静态：按 NodeKind 判（TextArea vs TextField），不依赖 ControlState。
    if aria == "multiline" {
        return match scene.get(id).map(|n| n.kind) {
            Some(NodeKind::TextArea) => Some("true".to_string()),
            _ => None,
        };
    }
    let cs = scene.controls.get(id)?;
    Some(match (aria, cs) {
        ("checked", ControlState::Toggle { checked } | ControlState::Radio { checked, .. }) => {
            checked.to_string()
        }
        ("expanded", ControlState::Dropdown { open, .. }) => open.to_string(),
        ("valuenow", ControlState::Progress { value, .. } | ControlState::Slider { value, .. }) => {
            // f32 Display 用最短往返表示（50.0→"50"、33.5→"33.5"）。CSS 作者按量化后的值写选择器。
            value.to_string()
        }
        ("valuenow", ControlState::NumberField { edit, .. }) => edit.value.clone(),
        _ => return None,
    })
}

/// 判定 compound 是否匹配 node + 状态门。
///
/// 状态门：伪类（hovered / active / disabled / focused）。
/// 通过后调 compound_matches_node 做字面匹配（tag/classes/id_attr）。
fn compound_matches_with_state(c: &Compound, node_id: NodeId, scene: &Scene) -> bool {
    // 伪类状态门
    let node = scene.get(node_id).expect("live node");
    if c.pseudo_hover && !node.interaction.flags.contains(NodeFlags::HOVERED) {
        return false;
    }
    if c.pseudo_active && !node.interaction.flags.contains(NodeFlags::ACTIVE) {
        return false;
    }
    if c.pseudo_disabled && !node.interaction.flags.contains(NodeFlags::DISABLED) {
        return false;
    }
    if c.pseudo_focus && !node.interaction.flags.contains(NodeFlags::FOCUSED) {
        return false;
    }
    compound_matches_node(c, node_id, scene)
}

/// 完整后代链匹配（从右往左，复用 selector.rs `matches` 算法，消费 Node/Scene）。
/// 最后一个 compound 必须命中目标 node 本身（含状态门）；前面按 combinator 沿
/// parent 链找（Child=直接父，Descendant=任一祖先，带回溯）。
/// 匹配选择器 `sel` 到节点 `node_id`。`scope_bound` = 规则所属作用域根（NodeId::INVALID=全局，无边界）。
/// 后代/子代选择器沿祖先链匹配时，不穿透 scope_bound（其父在作用域外）——main-design §5.4。
pub fn match_element_with_state(
    sel: &ParsedSelector,
    node_id: NodeId,
    scene: &Scene,
    scope_bound: NodeId,
) -> bool {
    let comps = &sel.compound;
    if comps.is_empty() {
        return false;
    }
    let last = &comps[comps.len() - 1];
    if !compound_matches_with_state(last, node_id, scene) {
        return false;
    }
    if comps.len() == 1 {
        return true;
    }
    match_chain_with_state(comps, comps.len() - 1, node_id, scene, scope_bound)
}

/// 递归匹配 comps[0..end_idx] 在 start_node 的祖先链上（同 selector.rs
/// `match_compound_chain`）。`start_node` 是已匹配 comps[end_idx] 的节点，
/// 为 comps[end_idx - 1] 找祖先（Child：直接父；Descendant：任一祖先+回溯）。
/// `scope_bound` 约束祖先链不穿透作用域根（其父在作用域外）。
fn match_chain_with_state(
    comps: &[Compound],
    end_idx: usize,
    start_node: NodeId,
    scene: &Scene,
    scope_bound: NodeId,
) -> bool {
    if end_idx == 0 {
        return true;
    }
    let target_comp = &comps[end_idx - 1];
    let combinator = comps[end_idx].combinator;
    match combinator {
        Combinator::Child => match parent_in_scope(scene, start_node, scope_bound) {
            Some(parent) => {
                compound_matches_with_state(target_comp, parent, scene)
                    && match_chain_with_state(comps, end_idx - 1, parent, scene, scope_bound)
            }
            None => false,
        },
        Combinator::Descendant => {
            let mut cur = parent_in_scope(scene, start_node, scope_bound);
            while let Some(ancestor) = cur {
                if compound_matches_with_state(target_comp, ancestor, scene)
                    && match_chain_with_state(comps, end_idx - 1, ancestor, scene, scope_bound)
                {
                    return true;
                }
                // 此祖先匹配但更左链匹配不上 → 继续往上找
                cur = parent_in_scope(scene, ancestor, scope_bound);
            }
            false
        }
    }
}

/// 取 `node` 的父，但不超过 `scope_bound`：若 node == scope_bound，其父在作用域外 → None。
/// `scope_bound == NodeId::INVALID` = 全局规则，无边界 → 直接返父（可能跨作用域）。
/// 作用域根节点的父在作用域外，后代/子代选择器不应据它匹配（main-design §5.4 不穿透边界）。
fn parent_in_scope(scene: &Scene, node: NodeId, scope_bound: NodeId) -> Option<NodeId> {
    if scope_bound != NodeId::INVALID && node == scope_bound {
        return None;
    }
    scene.get(node).and_then(|n| n.parent)
}

/// 计算每节点的所属作用域根（沿父链最近的 SCOPE_ROOT，含自身）。无作用域根祖先 → INVALID。
/// 每帧 rematch 调一次，O(节点 × 深度)；scope 校验快路径用此表 O(1) 查。
/// 根节点通常由 create_root/instantiate 打 SCOPE_ROOT，故多数节点能命中某作用域根。
fn compute_scope_map(scene: &Scene, node_ids: &[NodeId]) -> HashMap<NodeId, NodeId> {
    let mut map = HashMap::with_capacity(node_ids.len());
    for &id in node_ids {
        let mut cur = Some(id);
        let mut found = NodeId::INVALID;
        while let Some(nid) = cur {
            if let Some(n) = scene.get(nid) {
                if n.interaction.flags.contains(NodeFlags::SCOPE_ROOT) {
                    found = nid;
                    break;
                }
                cur = n.parent;
            } else {
                break;
            }
        }
        map.insert(id, found);
    }
    map
}

/// 全量节点重匹配（仅动态规则子集）。每节点从 base_style 重起，
/// 收集命中的动态规则（match_element_with_state），按 specificity 升序排
/// （高 specificity 后 apply 胜出）→ apply_decl 叠加 → 写 Node.style。
/// solve 每帧全量，无需 dirty 驱动。
///
/// **transition 请求发射**：对已 cascade 过（cascaded_once=true）且声明了 transition 的节点，
/// 比较旧/新级联值的可动画通道（BgColor/TextColor/Opacity）；变化则推入
/// `scene.pending_transitions`，供 Stage tick drain 后 kill 旧 tween + 提交新 tween。
/// 首次 cascade（cascaded_once=false）即时生效不产请求，并置 cascaded_once=true。
pub fn rematch_pseudo_classes(scene: &mut Scene) {
    // 预提取 specificity 元组 + owned rule 副本 + scope_root（避免借 scene.dynamic_rules 跨 get_mut）。
    let rules_with_spec: Vec<(u32, u32, u32, DynamicRule, NodeId)> = scene
        .dynamic_rules
        .entries
        .iter()
        .map(|sr| {
            (
                sr.rule.selector.specificity.0,
                sr.rule.selector.specificity.1,
                sr.rule.selector.specificity.2,
                sr.rule.clone(),
                sr.scope_root,
            )
        })
        .collect();
    // 收集所有 NodeId（slotmap 分配，不能手造 NodeId(i)）。
    let node_ids: Vec<NodeId> = scene.nodes.values().map(|n| n.id).collect();
    // 每节点的所属作用域根（沿父链最近的 SCOPE_ROOT，含自身）。全局规则（scope_root=INVALID）
    // 跳过此过滤；scoped 规则只匹配 node_scope == rule.scope_root 的节点（main-design §5.4）。
    let scope_map = compute_scope_map(scene, &node_ids);
    // set-ness：每节点显式声明了哪些可继承属性。cascade 期收集，继承 pass 消费。
    let mut set_map: HashMap<NodeId, InheritedSet> = HashMap::new();
    for node_id in node_ids {
        // 捕获旧级联值 + cascaded_once + transition 声明（写新 style 前留快照）。
        // transition 读自 base_style（打包期烘焙的静态声明，rematch 不改 base_style）。
        let (old_style, cascaded_once, transition_decl) = {
            let n = scene.get(node_id).expect("live node");
            (
                n.style.clone(),
                n.interaction.flags.contains(NodeFlags::CASCALED),
                n.base_style.transition.clone(),
            )
        };
        // 从 base_style 重起
        let mut new_style = scene.get(node_id).expect("live node").base_style.clone();
        // 收集命中规则（按作用域过滤：全局规则 scope_root=INVALID 总匹配；scoped 规则只匹配本作用域节点）
        let node_scope = scope_map.get(&node_id).copied().unwrap_or(NodeId::INVALID);
        let mut matched: Vec<(u32, u32, u32, DynamicRule)> = Vec::new();
        for r in &rules_with_spec {
            let scope_root = r.4;
            if scope_root != NodeId::INVALID && scope_root != node_scope {
                continue; // scoped 规则不匹配他作用域节点
            }
            if match_element_with_state(&r.3.selector, node_id, scene, scope_root) {
                matched.push((r.0, r.1, r.2, r.3.clone()));
            }
        }
        // specificity 升序（高 specificity 后 apply 胜出）；同级保持原序（stable sort）
        matched.sort_by_key(|r| (r.0, r.1, r.2));
        // apply 声明并收集 set-ness：apply_decl 返回 true = 该 prop 成功写入
        // → 若是可继承属性，记对应 bit，供继承 pass 判"子是否显式声明"。
        // Seed from base_style.inherited_set (package-time baked declarations),
        // then OR dynamic cascade bits on top.
        // new_style is a fresh clone of base_style (not yet modified by apply_decl below),
        // so its inherited_set == base_style.inherited_set at this point.
        let mut inh: InheritedSet = new_style.inherited_set;
        for (_, _, _, r) in &matched {
            for decl in &r.declarations {
                // CSS inline > class：base_style.inline_declared 标记的属性由打包期 inline
                // style 声明，class 规则不覆盖（如 dialog-overlay 的 inline display:none 不被
                // .dialog-overlay{display:flex} 覆盖）。inline_bit 返 None 的属性（transition 等）
                // 无 inline 来源，照常应用。
                if let Some(bit) = inline_bit(&decl.prop) {
                    if new_style.inline_declared & bit != 0 {
                        continue;
                    }
                }
                if apply_decl(&mut new_style, &decl.prop, &decl.value) {
                    if let Some(bit) = inherited_bit(&decl.prop) {
                        inh.0 |= bit;
                    }
                }
            }
        }
        // inline_override 应用（最高优先级，动态规则之后）。
        // 按 inline_set 把 inline_override 字段拷进 new_style；继承子集 OR 进 set_map，
        // 使 propagate 把含 inline 的父值传给未自设的子、且本节点自身不被父覆盖。
        // inline_set 默认空 → 对没设 inline 的节点 no-op（Spec-3 probe 不回归）。
        {
            let n_ref = scene.get(node_id).expect("live node");
            let inline_set = n_ref.inline_set;
            if inline_set.0 != 0 {
                // 直传 &n_ref.inline_override（不可变借，new_style 是 local 不冲突；
                // block 结束 n_ref 借释放，后续 set_map.insert/get_mut 不受影响）。
                // 省 ResolvedStyle clone（含 Vec<TransitionSpec>/text_effects，每帧每 inline 节点）。
                apply_inline_override(&mut new_style, &n_ref.inline_override, inline_set);
                // 只把继承子集（bits 0-7）并进 set_map；非继承 bit 不影响 propagate。
                // InheritedSet.0 是 u16，INH_* 全在 u16 范围内，安全截位。
                inh.0 |= (inline_set.0 & INH_ALL_MASK) as u16;
            }
        }
        set_map.insert(node_id, inh);
        // transition 检测：仅 cascaded_once 后（首次 cascade 即时生效不动画），
        // 且声明了非零 duration 的 transition 时，比较可动画通道变化推请求。
        for ts in &transition_decl {
            if cascaded_once && ts.duration > 0.0 {
                emit_transition_requests(scene, node_id, *ts, &old_style, &new_style);
            }
        }
        // 写 style + 标 cascaded_once
        let node = scene.get_mut(node_id).expect("live node");
        node.style = new_style;
        node.interaction.flags.insert(NodeFlags::CASCALED);
    }
    // 通用可继承属性传播：每节点从 base_style 独立 cascade（不读父），故继承须 rematch 后
    // 按 tree order 补一次：子未显式声明（set_map 无该 bit）→ 取父 effective 值。
    propagate_inherited(scene, &set_map);
}

/// 按 set 位图把 `inline_override` 字段拷进 style（最高优先级覆盖）。覆盖全部 8 个继承
/// 字段（INH_*，bits 0-7）+ 24 个非继承字段（INLINE_*，bits 8-31）。INLINE_DISPLAY
/// 一对应两字段（`taffy_style.display` + `display_mode`，与 apply_decl 行为对齐），
/// 其余 INLINE_* 一对一映射到 ResolvedStyle/taffy_style 字段。
///
/// 该函数不改 `style.inherited_set`——inline 的继承子集由调用方 OR 进 set_map。
fn apply_inline_override(style: &mut ResolvedStyle, inline: &ResolvedStyle, set: InlineSet) {
    let s = set.0;
    // 单字段拷贝：`$($f:ident).+` 支持顶层（color）+ taffy 嵌套（taffy_style.size.width）路径。
    // `$bit as u32` 同时容纳 INH_*（u16）与 INLINE_*（u32）。
    macro_rules! cpy {
        ($($f:ident).+, $bit:expr) => {
            if s & (($bit) as u32) != 0 {
                style.$($f).+ = inline.$($f).+.clone();
            }
        };
    }
    // 继承属性（bits 0-7）
    cpy!(font_size, INH_FONT_SIZE);
    cpy!(color, INH_COLOR);
    cpy!(font_family, INH_FONT_FAMILY);
    cpy!(font_weight, INH_FONT_WEIGHT);
    cpy!(text_align, INH_TEXT_ALIGN);
    cpy!(line_height, INH_LINE_HEIGHT);
    cpy!(letter_spacing, INH_LETTER_SPACING);
    cpy!(white_space_nowrap, INH_WHITE_SPACE_NOWRAP);
    // 非继承属性（bits 8-31）——taffy_style 子字段
    cpy!(taffy_style.size.width, INLINE_WIDTH);
    cpy!(taffy_style.size.height, INLINE_HEIGHT);
    cpy!(taffy_style.min_size.width, INLINE_MIN_WIDTH);
    cpy!(taffy_style.min_size.height, INLINE_MIN_HEIGHT);
    cpy!(taffy_style.max_size.width, INLINE_MAX_WIDTH);
    cpy!(taffy_style.max_size.height, INLINE_MAX_HEIGHT);
    cpy!(taffy_style.padding, INLINE_PADDING);
    cpy!(taffy_style.margin, INLINE_MARGIN);
    cpy!(taffy_style.border, INLINE_BORDER_WIDTH);
    cpy!(taffy_style.gap, INLINE_GAP);
    cpy!(taffy_style.flex_direction, INLINE_FLEX_DIRECTION);
    cpy!(taffy_style.flex_wrap, INLINE_FLEX_WRAP);
    cpy!(taffy_style.justify_content, INLINE_JUSTIFY_CONTENT);
    cpy!(taffy_style.align_items, INLINE_ALIGN_ITEMS);
    cpy!(overflow_x, INLINE_OVERFLOW_X);
    cpy!(overflow_y, INLINE_OVERFLOW_Y);
    cpy!(taffy_style.position, INLINE_POSITION);
    cpy!(taffy_style.inset.left, INLINE_LEFT);
    cpy!(taffy_style.inset.top, INLINE_TOP);
    cpy!(taffy_style.inset.right, INLINE_RIGHT);
    cpy!(taffy_style.inset.bottom, INLINE_BOTTOM);
    // 非继承属性——视觉/渲染字段
    cpy!(background_color, INLINE_BACKGROUND_COLOR);
    cpy!(opacity, INLINE_OPACITY);
    // INLINE_DISPLAY：apply_decl 同时设 taffy_style.display + display_mode，需双字段覆盖。
    if s & INLINE_DISPLAY != 0 {
        style.taffy_style.display = inline.taffy_style.display;
        style.display_mode = inline.display_mode;
    }
}

/// 通用可继承属性传播（tree-order DFS）。子未显式声明的可继承字段 → 取父 effective 值。
/// `effective` = 节点当前 style 值（anim override 本轮仅 color 用过，font 等无 anim）。
fn propagate_inherited(scene: &mut Scene, set_map: &HashMap<NodeId, InheritedSet>) {
    let roots = scene.roots.clone();
    for root in roots {
        propagate_inherited_rec(scene, root, None, set_map);
    }
}

fn propagate_inherited_rec(
    scene: &mut Scene,
    id: NodeId,
    parent_style: Option<ResolvedStyle>,
    set_map: &HashMap<NodeId, InheritedSet>,
) {
    let (my_style, children) = {
        let n = scene.get(id).expect("live node");
        (n.style.clone(), n.children.clone())
    };
    // 父 effective = 父传下来的 style 快照（已含父自己的继承结果，因 tree order）
    if let Some(parent_eff) = parent_style {
        let inh = set_map.get(&id).copied().unwrap_or_default();
        let mut new_style = my_style.clone();
        macro_rules! copy_if_unset {
            ($field:ident, $bit:expr) => {
                if (inh.0 & $bit) == 0 {
                    new_style.$field = parent_eff.$field;
                }
            };
        }
        copy_if_unset!(font_size, INH_FONT_SIZE);
        // ponytail: anim text-color override to children dropped (old propagate_color_inheritance had it); restore in Spec-3 when text anim + inheritance interact.
        copy_if_unset!(color, INH_COLOR);
        copy_if_unset!(font_family, INH_FONT_FAMILY);
        copy_if_unset!(font_weight, INH_FONT_WEIGHT);
        copy_if_unset!(text_align, INH_TEXT_ALIGN);
        copy_if_unset!(line_height, INH_LINE_HEIGHT);
        copy_if_unset!(letter_spacing, INH_LETTER_SPACING);
        copy_if_unset!(white_space_nowrap, INH_WHITE_SPACE_NOWRAP);
        // ponytail: per-clone，节点多时换就地改 + 父快照
        let eff_for_children = new_style.clone();
        scene.get_mut(id).expect("live node").style = new_style;
        // 向下传我更新后的 style 作为子 effective
        for c in children {
            propagate_inherited_rec(scene, c, Some(eff_for_children.clone()), set_map);
        }
    } else {
        // 根节点：无父继承，effective = 自己 style，直接向下传
        for c in children {
            propagate_inherited_rec(scene, c, Some(my_style.clone()), set_map);
        }
    }
}

/// 比较旧/新级联值的可动画通道；变化的（且 transition 声明覆盖该通道）推入 pending_transitions。
/// `ts.prop`=None 表 all（任一通道变化都触发）；Some(p) 仅触发该通道。
/// start 取进行中 tween 的 override（mid-flight 连续，无 snap），无则取旧级联值。
fn emit_transition_requests(
    scene: &mut Scene,
    node: NodeId,
    ts: TransitionSpec,
    old: &ResolvedStyle,
    new: &ResolvedStyle,
) {
    use crate::tween::{TransitionRequest, TweenProp};
    let wants = |p: TweenProp| ts.prop.is_none() || matches!(ts.prop, Some(q) if q == p);
    let anim = scene.anim.get(node); // mid-flight override 作 start（无则 old cascade 值）
                                     // background-color: Option<[f32;4]>（None 视作透明 [0,0,0,0]）
    if wants(TweenProp::BgColor) {
        let a = old.background_color.unwrap_or([0.0; 4]);
        let b = new.background_color.unwrap_or([0.0; 4]);
        if a != b {
            let start = anim.and_then(|x| x.bg_color).unwrap_or(a);
            scene.pending_transitions.push(TransitionRequest {
                node,
                prop: TweenProp::BgColor,
                start,
                end: b,
                ease: ts.ease,
                delay: ts.delay,
                duration: ts.duration,
            });
        }
    }
    // text color: [f32;4]（color 字段，非 Option）
    if wants(TweenProp::TextColor) {
        let a = old.color;
        let b = new.color;
        if a != b {
            let start = anim.and_then(|x| x.text_color).unwrap_or(a);
            scene.pending_transitions.push(TransitionRequest {
                node,
                prop: TweenProp::TextColor,
                start,
                end: b,
                ease: ts.ease,
                delay: ts.delay,
                duration: ts.duration,
            });
        }
    }
    // opacity: f32（标量，pack 进 [f32;4] 首分量）
    if wants(TweenProp::Opacity) && (old.opacity - new.opacity).abs() > 1e-6 {
        let start = anim.and_then(|x| x.opacity).unwrap_or(old.opacity);
        scene.pending_transitions.push(TransitionRequest {
            node,
            prop: TweenProp::Opacity,
            start: [start, 0.0, 0.0, 0.0],
            end: [new.opacity, 0.0, 0.0, 0.0],
            ease: ts.ease,
            delay: ts.delay,
            duration: ts.duration,
        });
    }
    // translate/scale/rotation 需分解 transform 矩阵——围栏 transition 不支持 transform，此处不做。
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::node::{
        ControlState, EditState, Node, NodeFlags, NodeId, NodeKind, Rect, RoleInfo, Scene,
    };
    use crate::style::resolved::TransitionSpec;
    use crate::tween::{Ease, TweenProp};

    /// 把规则作为全局规则（scope_root=INVALID）推进 scene——跨作用域命中。
    /// 现有单场景测试无实例化，规则走全局路径保持原有行为（node_scope=INVALID，全局规则匹配）。
    fn push_global(s: &mut Scene, r: DynamicRule) {
        s.dynamic_rules.entries.push(ScopedRule {
            rule: r,
            scope_root: NodeId::INVALID,
        });
    }

    /// 把规则作为 scoped 规则（scope_root = 指定实例根）推进 scene——只命中该作用域内节点。
    /// 用于 CSS 作用域隔离测试。
    fn push_scoped(s: &mut Scene, scope_root: NodeId, r: DynamicRule) {
        s.dynamic_rules.entries.push(ScopedRule {
            rule: r,
            scope_root,
        });
    }

    /// 构造 root + button(.btn) scene，button 在 (0,0,100,100)。
    fn btn_scene() -> Scene {
        let mut root = Node::default();
        root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        };
        let mut btn = Node::default();
        btn.kind = NodeKind::Button;
        btn.classes = vec!["btn".to_string()];
        btn.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        Scene::from_nodes(vec![root, btn], vec![(0, 1)])
    }

    /// btn 的 NodeId（root 的唯一子）。
    fn btn_id(s: &Scene) -> NodeId {
        s.get(s.roots[0]).unwrap().children[0]
    }

    /// Construct a ParsedSelector manually (no parse dependency).
    /// Supports the subset used in these tests: `.class`, `.class:hover`,
    /// `.class:active`, `.class:disabled`, `[attr]`, `[attr="val"]`, and
    /// tag selectors.
    /// Construct a ParsedSelector manually (no parse dependency).
    /// Supports: .class, #id, :hover/:active/:disabled/:focus,
    /// [attr], [attr="val"], tag, and descendant combinator (space).
    fn hand_selector(sel: &str) -> ParsedSelector {
        let raw = sel.to_string();
        let mut compounds = Vec::new();
        for part in sel.split_whitespace() {
            let mut c = Compound {
                tag: None,
                classes: Vec::new(),
                id: None,
                combinator: Combinator::Descendant,
                pseudo_hover: false,
                pseudo_active: false,
                pseudo_disabled: false,
                pseudo_focus: false,
                attrs: Vec::new(),
            };
            let mut rest = part;
            while !rest.is_empty() {
                if rest.starts_with('.') {
                    let r = &rest[1..];
                    let end = r
                        .find(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != '_')
                        .unwrap_or(r.len());
                    c.classes.push(r[..end].to_string());
                    rest = &r[end..];
                } else if rest.starts_with('#') {
                    let r = &rest[1..];
                    let end = r
                        .find(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != '_')
                        .unwrap_or(r.len());
                    c.id = Some(r[..end].to_string());
                    rest = &r[end..];
                } else if let Some(r) = rest.strip_prefix(":hover") {
                    c.pseudo_hover = true;
                    rest = r;
                } else if let Some(r) = rest.strip_prefix(":active") {
                    c.pseudo_active = true;
                    rest = r;
                } else if let Some(r) = rest.strip_prefix(":disabled") {
                    c.pseudo_disabled = true;
                    rest = r;
                } else if let Some(r) = rest.strip_prefix(":focus") {
                    c.pseudo_focus = true;
                    rest = r;
                } else if rest.starts_with('[') {
                    let close = rest.find(']').unwrap();
                    let inner = &rest[1..close];
                    if let Some(eq) = inner.find('=') {
                        c.attrs.push(AttrSelector {
                            name: inner[..eq].to_string(),
                            op: AttrOp::Eq,
                            value: Some(inner[eq + 1..].trim_matches('"').to_string()),
                        });
                    } else {
                        c.attrs.push(AttrSelector {
                            name: inner.to_string(),
                            op: AttrOp::Exists,
                            value: None,
                        });
                    }
                    rest = &rest[close + 1..];
                } else {
                    let end = rest.find(['.', '#', ':', '[']).unwrap_or(rest.len());
                    if end > 0 {
                        c.tag = Some(rest[..end].to_string());
                    }
                    rest = &rest[end..];
                }
            }
            compounds.push(c);
        }
        ParsedSelector {
            raw,
            compound: compounds,
            specificity: Specificity(0, 0, 0),
        }
    }

    fn rule(sel: &str, prop: &str, val: &str) -> DynamicRule {
        DynamicRule {
            selector: hand_selector(sel),
            declarations: vec![Declaration {
                prop: prop.to_string(),
                value: val.to_string(),
            }],
        }
    }

    #[test]
    fn hover_pseudo_changes_background_color() {
        let mut s = btn_scene();
        let bid = btn_id(&s);
        push_global(&mut s, rule(".btn:hover", "background-color", "#0000ff"));
        s.get_mut(bid)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::HOVERED); // 模拟命中 diff 后状态
        rematch_pseudo_classes(&mut s);
        // background_color 是视觉字段，不触发 layout dirty
        assert_eq!(
            s.get(bid).unwrap().style.background_color,
            Some([0.0, 0.0, 1.0, 1.0]),
            "hover → 蓝"
        );
    }

    #[test]
    fn child_text_inherits_parent_runtime_color() {
        // root → parent(.par) → Text child。模拟打包期：parent base color=灰（.par 声明），
        // child base color=灰（打包期继承 parent base）。.par:hover 命中改 parent color 深。
        // CSS color 继承：child Text 无自己 color 声明 → runtime 该继承 parent runtime 深。
        let mut root = Node::default();
        root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        };
        let mut parent = Node::default();
        parent.classes = vec!["par".to_string()];
        parent.base_style.color = [0.6, 0.63, 0.71, 1.0]; // 灰（.par 声明）
        parent.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        let mut child = Node::default();
        child.kind = NodeKind::TextNode;
        child.base_style.color = [0.6, 0.63, 0.71, 1.0]; // 灰（打包期继承 parent base）
        child.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 50.0,
            h: 20.0,
        };
        let mut s = Scene::from_nodes(vec![root, parent, child], vec![(0, 1), (1, 2)]);
        push_global(&mut s, rule(".par:hover", "color", "#1a1d2e"));
        let pid = s.get(s.roots[0]).unwrap().children[0];
        s.get_mut(pid)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::HOVERED);
        rematch_pseudo_classes(&mut s);
        let cid = s.get(pid).unwrap().children[0];
        let c = s.get(cid).unwrap().style.color;
        // #1a1d2e = (26,29,46)/255
        assert!(
            (c[0] - 26.0 / 255.0).abs() < 1e-3
                && (c[1] - 29.0 / 255.0).abs() < 1e-3
                && (c[2] - 46.0 / 255.0).abs() < 1e-3,
            "child Text 该继承 parent hover color (#1a1d2e)，实际 {:?}",
            c
        );
    }

    #[test]
    fn child_inherits_parent_font_size() {
        // root(.par font-size:24) > Text child。child 无 font-size 规则 → 该继承 24。
        // 证明通用继承（非 color-only）。
        let mut root = Node::default();
        root.classes = vec!["par".to_string()];
        root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        };
        let mut child = Node::default();
        child.kind = NodeKind::TextNode;
        child.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 50.0,
            h: 20.0,
        };
        let mut s = Scene::from_nodes(vec![root, child], vec![(0, 1)]);
        push_global(&mut s, rule(".par", "font-size", "24px"));
        rematch_pseudo_classes(&mut s);
        let cid = s.get(s.roots[0]).unwrap().children[0];
        assert_eq!(
            s.get(cid).unwrap().style.font_size,
            24.0,
            "child Text 该继承 parent .par 的 font-size:24"
        );
    }

    #[test]
    fn child_explicit_font_size_not_overridden_by_inheritance() {
        // child 自己声明 font-size:12 → 不被父的 24 覆盖（set-ness 阻断继承）。
        let mut root = Node::default();
        root.classes = vec!["par".to_string()];
        let mut child = Node::default();
        child.classes = vec!["c".to_string()];
        child.kind = NodeKind::TextNode;
        let mut s = Scene::from_nodes(vec![root, child], vec![(0, 1)]);
        push_global(&mut s, rule(".par", "font-size", "24px"));
        push_global(&mut s, rule(".c", "font-size", "12px"));
        rematch_pseudo_classes(&mut s);
        let cid = s.get(s.roots[0]).unwrap().children[0];
        assert_eq!(
            s.get(cid).unwrap().style.font_size,
            12.0,
            "child 显式声明 12 不被继承覆盖"
        );
    }

    #[test]
    fn inheritance_cascades_two_levels() {
        // root(.a font-size:20) > mid > leaf(Text)。mid/leaf 都不声明 → leaf 继承 20（跨两级）。
        let mut root = Node::default();
        root.classes = vec!["a".to_string()];
        let mid = Node::default();
        let mut leaf = Node::default();
        leaf.kind = NodeKind::TextNode;
        let mut s = Scene::from_nodes(vec![root, mid, leaf], vec![(0, 1), (1, 2)]);
        push_global(&mut s, rule(".a", "font-size", "20px"));
        rematch_pseudo_classes(&mut s);
        let mid_id = s.get(s.roots[0]).unwrap().children[0];
        let leaf_id = s.get(mid_id).unwrap().children[0];
        assert_eq!(
            s.get(leaf_id).unwrap().style.font_size,
            20.0,
            "leaf 跨级继承 20"
        );
    }

    #[test]
    fn active_pseudo_on_down() {
        let mut s = btn_scene();
        let bid = btn_id(&s);
        push_global(&mut s, rule(".btn:active", "background-color", "#ff0000"));
        s.get_mut(bid)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::ACTIVE);
        rematch_pseudo_classes(&mut s);
        assert_eq!(
            s.get(bid).unwrap().style.background_color,
            Some([1.0, 0.0, 0.0, 1.0]),
            "active → 红"
        );
    }

    #[test]
    fn disabled_pseudo_via_disabled_flag() {
        let mut s = btn_scene();
        let bid = btn_id(&s);
        push_global(&mut s, rule(".btn:disabled", "opacity", "0.5"));
        s.get_mut(bid)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::DISABLED);
        rematch_pseudo_classes(&mut s);
        assert_eq!(
            s.get(bid).unwrap().style.opacity,
            0.5,
            "disabled → opacity 0.5"
        );
    }

    #[test]
    fn rematch_layout_dirty_when_size_changes() {
        let mut s = btn_scene();
        let bid = btn_id(&s);
        push_global(&mut s, rule(".btn:hover", "width", "200px"));
        s.get_mut(bid)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::HOVERED);
        rematch_pseudo_classes(&mut s);
        // 验 style.taffy_style.size.width 被改
        use taffy::style::Dimension;
        assert_eq!(
            s.get(bid).unwrap().style.taffy_style.size.width,
            Dimension::length(200.0)
        );
    }

    #[test]
    fn rematch_no_dirty_when_only_visual_changes() {
        let mut s = btn_scene();
        let bid = btn_id(&s);
        push_global(&mut s, rule(".btn:hover", "color", "#ff0000"));
        s.get_mut(bid)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::HOVERED);
        rematch_pseudo_classes(&mut s);
        assert_eq!(s.get(bid).unwrap().style.color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn descendant_pseudo_rule_matched() {
        // .parent:hover .child —— hover parent → child style 变（跨节点伪类联动）
        let mut root = Node::default();
        root.classes = vec!["parent".to_string()];
        root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        };
        let mut child = Node::default();
        child.classes = vec!["child".to_string()];
        child.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 50.0,
            h: 50.0,
        };
        let mut s = Scene::from_nodes(vec![root, child], vec![(0, 1)]);
        let root_id = s.roots[0];
        let child_id = s.get(root_id).unwrap().children[0];
        push_global(&mut s, rule(".parent:hover .child", "color", "#0000ff"));
        s.get_mut(root_id)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::HOVERED); // parent hovered
        rematch_pseudo_classes(&mut s);
        assert_eq!(
            s.get(child_id).unwrap().style.color,
            [0.0, 0.0, 1.0, 1.0],
            "parent:hover → child 变蓝"
        );
    }

    #[test]
    fn no_pseudo_rule_not_in_dynamic_rules() {
        // 纯静态规则不进 dynamic_rules（打包器分流）——rematch 不区分有无伪类，
        // 只看状态门。若纯静态规则混进 dynamic，hovered=true 时仍匹配（无伪类规则恒匹配）。
        // 打包器保证无伪类规则不进 dynamic_rules。此测断言 color 变红。
        let mut s = btn_scene();
        let bid = btn_id(&s);
        push_global(&mut s, rule(".btn", "color", "#ff0000"));
        s.get_mut(bid)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::HOVERED);
        rematch_pseudo_classes(&mut s);
        assert_eq!(s.get(bid).unwrap().style.color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn rematch_resets_to_base_when_no_rule_matches() {
        // hover 后变蓝 → hover 取消 → rematch 应回 base_style
        let mut s = btn_scene();
        let bid = btn_id(&s);
        s.get_mut(bid).unwrap().base_style.background_color = None; // base 无 bg
        push_global(&mut s, rule(".btn:hover", "background-color", "#0000ff"));
        s.get_mut(bid)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::HOVERED);
        rematch_pseudo_classes(&mut s);
        assert_eq!(
            s.get(bid).unwrap().style.background_color,
            Some([0.0, 0.0, 1.0, 1.0])
        );
        s.get_mut(bid)
            .unwrap()
            .interaction
            .flags
            .remove(NodeFlags::HOVERED); // 取消 hover
        rematch_pseudo_classes(&mut s);
        assert_eq!(
            s.get(bid).unwrap().style.background_color,
            None,
            "取消 hover → 回 base"
        );
    }

    #[test]
    fn focus_pseudo_matches_focused_node() {
        let mut s = btn_scene();
        let bid = btn_id(&s);
        push_global(&mut s, rule(".btn:focus", "background-color", "#0000ff"));
        s.get_mut(bid)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::FOCUSED);
        rematch_pseudo_classes(&mut s);
        assert_eq!(
            s.get(bid).unwrap().style.background_color,
            Some([0.0, 0.0, 1.0, 1.0]),
            "focused → :focus 匹配 → 蓝"
        );
    }

    #[test]
    fn focus_pseudo_no_match_unfocused() {
        let mut s = btn_scene();
        let bid = btn_id(&s);
        s.get_mut(bid).unwrap().base_style.background_color = None;
        push_global(&mut s, rule(".btn:focus", "background-color", "#0000ff"));
        s.get_mut(bid)
            .unwrap()
            .interaction
            .flags
            .remove(NodeFlags::FOCUSED);
        rematch_pseudo_classes(&mut s);
        assert_eq!(
            s.get(bid).unwrap().style.background_color,
            None,
            "unfocused → :focus 不匹配 → 回 base"
        );
    }

    #[test]
    fn focus_pseudo_in_descendant_chain() {
        // .parent:focus .child —— parent 聚焦 → child style 变
        let mut root = Node::default();
        root.classes = vec!["parent".to_string()];
        root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        };
        let mut child = Node::default();
        child.classes = vec!["child".to_string()];
        child.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 50.0,
            h: 50.0,
        };
        let mut s = Scene::from_nodes(vec![root, child], vec![(0, 1)]);
        let root_id = s.roots[0];
        let child_id = s.get(root_id).unwrap().children[0];
        push_global(&mut s, rule(".parent:focus .child", "color", "#0000ff"));
        s.get_mut(root_id)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::FOCUSED);
        rematch_pseudo_classes(&mut s);
        assert_eq!(
            s.get(child_id).unwrap().style.color,
            [0.0, 0.0, 1.0, 1.0],
            "parent:focus → child 变蓝"
        );
    }

    #[test]
    fn background_image_change_is_visual_not_layout_dirty() {
        // background-image 是视觉字段（非 taffy_style/order）→ rematch 不 layout dirty
        let mut s = btn_scene();
        let bid = btn_id(&s);
        push_global(
            &mut s,
            rule(".btn:hover", "background-image", "url(icons/home.png)"),
        );
        s.get_mut(bid)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::HOVERED);
        rematch_pseudo_classes(&mut s);
        assert_eq!(
            s.get(bid).unwrap().style.background_image.as_deref(),
            Some("icons/home.png"),
            "hover → background-image 生效"
        );
    }

    #[test]
    fn background_size_change_is_visual_not_layout_dirty() {
        // background-size 是视觉字段 → rematch 不 layout dirty
        let mut s = btn_scene();
        let bid = btn_id(&s);
        push_global(&mut s, rule(".btn:hover", "background-size", "cover"));
        s.get_mut(bid)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::HOVERED);
        rematch_pseudo_classes(&mut s);
        assert_eq!(
            s.get(bid).unwrap().style.background_size,
            crate::style::resolved::BackgroundSize::Cover,
            "hover → background-size:cover 生效"
        );
    }

    #[test]
    fn border_radius_change_is_visual_not_layout_dirty() {
        // border-radius 是视觉字段（非 taffy_style/order）→ rematch 不 layout dirty
        let mut s = btn_scene();
        let bid = btn_id(&s);
        push_global(&mut s, rule(".btn:hover", "border-radius", "8px"));
        s.get_mut(bid)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::HOVERED);
        rematch_pseudo_classes(&mut s);
        // hover → border-radius:8px 生效（四角 h=v=Length(8)）
        let bc = &s.get(bid).unwrap().style.border_radius.corners;
        for c in bc {
            assert_eq!(
                c.h,
                taffy::style::LengthPercentage::length(8.0),
                "hover → border-radius 水平 8px"
            );
            assert_eq!(
                c.v,
                taffy::style::LengthPercentage::length(8.0),
                "hover → border-radius 垂直 8px"
            );
        }
    }

    #[test]
    fn attr_selector_bincode_roundtrip() {
        let s = hand_selector(r#"[data-page="1"]"#);
        assert_eq!(s.compound.len(), 1);
        assert_eq!(s.compound[0].attrs.len(), 1, "[data-page=\"1\"] → one attr");
        let a = &s.compound[0].attrs[0];
        assert_eq!(a.name, "data-page");
        assert!(matches!(a.op, AttrOp::Eq));
        assert_eq!(a.value.as_deref(), Some("1"));
        // bincode roundtrip (pkg.bin dynamic blob is bincode)
        let bytes = bincode::serialize(&s).unwrap();
        let back: ParsedSelector = bincode::deserialize(&bytes).unwrap();
        assert_eq!(back.compound[0].attrs.len(), 1);
        assert_eq!(back.compound[0].attrs[0].name, "data-page");
    }

    /// 极简 Node 构造（仅设 kind）。attr selector 测试不需要 layout_rect/scene。
    fn test_node(kind: NodeKind) -> Node {
        let mut n = Node::default();
        n.kind = kind;
        n
    }

    /// 单节点 scene（root = 该节点）。compound_matches_node 现需 scene+id（attr 匹配读
    /// ControlState/RoleTable），故 attr/tag 测试经此包装。
    fn single_node_scene(node: Node) -> (Scene, NodeId) {
        let s = Scene::from_nodes(vec![node], vec![]);
        let id = s.roots[0];
        (s, id)
    }

    /// 单节点 scene + 注入 ControlState（aria 合成测试用）。
    fn control_scene(kind: NodeKind, state: ControlState) -> (Scene, NodeId) {
        let mut s = Scene::from_nodes(vec![test_node(kind)], vec![]);
        let id = s.roots[0];
        s.controls.ensure(id, state);
        (s, id)
    }

    /// 单节点 scene + 注入 RoleInfo（role/data-slot 匹配测试用）。slots 复刻 instantiate
    /// 路径：key=slot 名，值空串占位（`data-slot="thumb"` → slots["thumb"]=""）。
    fn role_scene(role: Option<&str>, slots: &[&str]) -> (Scene, NodeId) {
        let mut s = Scene::from_nodes(vec![test_node(NodeKind::Container)], vec![]);
        let id = s.roots[0];
        let info = RoleInfo {
            role: role.map(String::from),
            slots: slots
                .iter()
                .map(|&k| (k.to_string(), String::new()))
                .collect(),
        };
        s.roles.insert(id, info);
        (s, id)
    }

    #[test]
    fn attr_selector_type_matches_nodekind_precisely() {
        // [type="text"] 只匹配 TextField，不匹配 NumberField
        let sel = hand_selector(r#"[type="text"]"#);
        let (s_text, text) = single_node_scene(test_node(NodeKind::TextField));
        let (s_num, num) = single_node_scene(test_node(NodeKind::NumberField));
        assert!(compound_matches_node(&sel.compound[0], text, &s_text));
        assert!(!compound_matches_node(&sel.compound[0], num, &s_num));
        // [type="number"] 只匹配 NumberField
        let sel_num = hand_selector(r#"[type="number"]"#);
        assert!(compound_matches_node(&sel_num.compound[0], num, &s_num));
        assert!(!compound_matches_node(&sel_num.compound[0], text, &s_text));
    }

    #[test]
    fn tag_selector_matches_nodekind_roundtrip() {
        // 标签选择器 ↔ NodeKind 互逆回归：fence resolve_semantic 放行的、且在运行时成为 Node
        // 的每个 tag 都须在 rematch 命中。这里覆盖曾误退回 "div" 的 ListView/Slot，
        // 以及控件回溯 tag（progress）与基础 tag 作为健全性检查。
        //
        // 注意 `<template>` 不在此列：它在打包期被消费进 ComponentTemplate，不生成运行时 Node，
        // 故无对应 NodeKind，不参与 rematch。
        // fence tag → NodeKind 真相源：crates/fence/src/schema/tag.rs::resolve_semantic
        let cases: &[(&str, NodeKind)] = &[
            ("ul", NodeKind::ListView),
            ("slot", NodeKind::Slot),
            ("progress", NodeKind::ProgressBar),
            ("div", NodeKind::Container),
        ];
        for (tag, kind) in cases {
            let sel = hand_selector(tag);
            let (s, id) = single_node_scene(test_node(*kind));
            assert!(
                compound_matches_node(&sel.compound[0], id, &s),
                "tag 选择器 `{tag}` 应命中 NodeKind::{kind:?}（rematch 路径）"
            );
        }
        // 负向：`ul` 不该命中 Container（防误退回 div 后误匹配）
        let ul_sel = hand_selector("ul");
        let (s_div, div) = single_node_scene(test_node(NodeKind::Container));
        assert!(
            !compound_matches_node(&ul_sel.compound[0], div, &s_div),
            "`ul` 不应命中 Container"
        );
    }

    #[test]
    fn attr_selector_non_type_attr_does_not_match() {
        // 非已知 attr（type/aria-*/role/data-slot）：不匹配（Node 不携带任意 HTML 属性字面值）
        let sel = hand_selector("[disabled]");
        let (s, id) = single_node_scene(test_node(NodeKind::TextField));
        assert!(!compound_matches_node(&sel.compound[0], id, &s));
    }

    #[test]
    fn attr_selector_type_exists_form_does_not_match() {
        // [type] 存在形式（无 = val）：不支持（type 是结构性属性，存在性无匹配意义）
        let sel = hand_selector("[type]");
        let (s, id) = single_node_scene(test_node(NodeKind::TextField));
        assert!(!compound_matches_node(&sel.compound[0], id, &s));
    }

    // ── [aria-*] 合成：值随 ControlState 实时变 ──

    #[test]
    fn attr_matches_aria_checked_from_toggle() {
        // Toggle{checked:true} → [aria-checked="true"] 命中；翻成 false → [aria-checked="false"] 命中
        let (mut s, id) = control_scene(NodeKind::Toggle, ControlState::Toggle { checked: true });
        let sel_true = hand_selector(r#"[aria-checked="true"]"#);
        let sel_false = hand_selector(r#"[aria-checked="false"]"#);
        assert!(
            compound_matches_node(&sel_true.compound[0], id, &s),
            "checked:true → [aria-checked=\"true\"] 命中"
        );
        assert!(
            !compound_matches_node(&sel_false.compound[0], id, &s),
            "checked:true → [aria-checked=\"false\"] 不命中"
        );
        // flip → 实时合成值变
        s.controls
            .ensure(id, ControlState::Toggle { checked: false });
        assert!(
            !compound_matches_node(&sel_true.compound[0], id, &s),
            "checked:false → [aria-checked=\"true\"] 不命中"
        );
        assert!(
            compound_matches_node(&sel_false.compound[0], id, &s),
            "checked:false → [aria-checked=\"false\"] 命中"
        );
    }

    #[test]
    fn attr_matches_aria_checked_from_radio() {
        // Radio{checked:true} → [aria-checked="true"] 命中（与 Toggle 共用 aria-checked）
        let (s, id) = control_scene(
            NodeKind::RadioButton,
            ControlState::Radio {
                checked: true,
                name: "grp".into(),
            },
        );
        let sel = hand_selector(r#"[aria-checked="true"]"#);
        assert!(
            compound_matches_node(&sel.compound[0], id, &s),
            "Radio checked → [aria-checked=\"true\"] 命中"
        );
    }

    #[test]
    fn attr_matches_aria_checked_no_state_no_match() {
        // 无 ControlState 的普通 div → aria-checked 不合成 → Eq / Exists 均不命中
        let (s, id) = single_node_scene(test_node(NodeKind::Container));
        let sel_eq = hand_selector(r#"[aria-checked="true"]"#);
        assert!(!compound_matches_node(&sel_eq.compound[0], id, &s));
        let sel_exists = hand_selector("[aria-checked]");
        assert!(
            !compound_matches_node(&sel_exists.compound[0], id, &s),
            "[aria-checked] 存在形式：普通 div 无 aria-checked 语义"
        );
    }

    #[test]
    fn attr_matches_aria_expanded_from_dropdown() {
        // Dropdown{open:true} → [aria-expanded="true"] 命中
        let (s, id) = control_scene(
            NodeKind::Dropdown,
            ControlState::Dropdown {
                selected_index: 0,
                open: true,
                value_lock: false,
                open_selected_index: None,
            },
        );
        let sel = hand_selector(r#"[aria-expanded="true"]"#);
        assert!(
            compound_matches_node(&sel.compound[0], id, &s),
            "Dropdown open → [aria-expanded=\"true\"] 命中"
        );
        let sel_false = hand_selector(r#"[aria-expanded="false"]"#);
        assert!(!compound_matches_node(&sel_false.compound[0], id, &s));
    }

    #[test]
    fn attr_matches_aria_valuenow_from_progress() {
        // Progress{value:50.0} → [aria-valuenow="50"] 命中（f32 Display：50.0→"50"）
        let (s, id) = control_scene(
            NodeKind::ProgressBar,
            ControlState::Progress {
                value: 50.0,
                max: 100.0,
                indeterminate: false,
            },
        );
        let sel = hand_selector(r#"[aria-valuenow="50"]"#);
        assert!(
            compound_matches_node(&sel.compound[0], id, &s),
            "Progress 50.0 → [aria-valuenow=\"50\"] 命中"
        );
    }

    #[test]
    fn attr_matches_aria_valuenow_from_slider() {
        // Slider{value:33.5} → [aria-valuenow="33.5"] 命中
        let (s, id) = control_scene(
            NodeKind::Slider,
            ControlState::Slider {
                value: 33.5,
                min: 0.0,
                max: 100.0,
                step: 0.5,
                dragging: false,
            },
        );
        let sel = hand_selector(r#"[aria-valuenow="33.5"]"#);
        assert!(
            compound_matches_node(&sel.compound[0], id, &s),
            "Slider 33.5 → [aria-valuenow=\"33.5\"] 命中"
        );
    }

    #[test]
    fn attr_matches_aria_valuenow_from_numberfield() {
        // NumberField{edit.value="42"} → [aria-valuenow="42"] 命中（数值文本直传）
        let (s, id) = control_scene(
            NodeKind::NumberField,
            ControlState::NumberField {
                edit: EditState::from_init("42".into(), String::new(), 0, false),
                min: 0.0,
                max: 100.0,
                step: 1.0,
            },
        );
        let sel = hand_selector(r#"[aria-valuenow="42"]"#);
        assert!(
            compound_matches_node(&sel.compound[0], id, &s),
            "NumberField value=42 → [aria-valuenow=\"42\"] 命中"
        );
    }

    #[test]
    fn attr_matches_aria_multiline_from_textarea() {
        // aria-multiline 静态：TextArea → "true"；TextField → 不合成（单行无 multiline 语义）
        let (s_ta, ta) = control_scene(
            NodeKind::TextArea,
            ControlState::TextArea(EditState::from_init(String::new(), String::new(), 0, false)),
        );
        let sel = hand_selector(r#"[aria-multiline="true"]"#);
        assert!(
            compound_matches_node(&sel.compound[0], ta, &s_ta),
            "TextArea → [aria-multiline=\"true\"] 命中"
        );
        let (s_tf, tf) = control_scene(
            NodeKind::TextField,
            ControlState::TextField(EditState::from_init(String::new(), String::new(), 0, false)),
        );
        assert!(
            !compound_matches_node(&sel.compound[0], tf, &s_tf),
            "TextField → [aria-multiline=\"true\"] 不命中"
        );
    }

    // ── [role=] / [data-slot=]：从 RoleTable 查打包期提取的静态值 ──

    #[test]
    fn attr_matches_role_eq_and_exists() {
        let (s, id) = role_scene(Some("switch"), &[]);
        let sel_eq = hand_selector(r#"[role="switch"]"#);
        assert!(
            compound_matches_node(&sel_eq.compound[0], id, &s),
            "[role=\"switch\"] 命中 role=switch"
        );
        let sel_other = hand_selector(r#"[role="slider"]"#);
        assert!(
            !compound_matches_node(&sel_other.compound[0], id, &s),
            "[role=\"slider\"] 不命中 switch"
        );
        let sel_exists = hand_selector("[role]");
        assert!(
            compound_matches_node(&sel_exists.compound[0], id, &s),
            "[role] 存在形式命中"
        );
        let (s_div, div) = role_scene(None, &[]);
        assert!(
            !compound_matches_node(&sel_exists.compound[0], div, &s_div),
            "无 role 的 div → [role] 不命中"
        );
    }

    #[test]
    fn attr_matches_data_slot_eq_and_exists() {
        let (s, id) = role_scene(None, &["thumb"]);
        let sel_eq = hand_selector(r#"[data-slot="thumb"]"#);
        assert!(
            compound_matches_node(&sel_eq.compound[0], id, &s),
            "[data-slot=\"thumb\"] 命中"
        );
        let sel_other = hand_selector(r#"[data-slot="fill"]"#);
        assert!(
            !compound_matches_node(&sel_other.compound[0], id, &s),
            "[data-slot=\"fill\"] 不命中 thumb 节点"
        );
        let sel_exists = hand_selector("[data-slot]");
        assert!(
            compound_matches_node(&sel_exists.compound[0], id, &s),
            "[data-slot] 存在形式命中"
        );
    }

    #[test]
    fn aria_checked_rematch_drives_style() {
        // 端到端：Toggle + [aria-checked="true"]{color:red}。checked:true → 染红；翻 false → 回落。
        let mut root = Node::default();
        root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        let mut s = Scene::from_nodes(vec![root], vec![]);
        let id = s.roots[0];
        s.controls
            .ensure(id, ControlState::Toggle { checked: true });
        push_global(&mut s, rule(r#"[aria-checked="true"]"#, "color", "#ff0000"));
        rematch_pseudo_classes(&mut s);
        assert_eq!(
            s.get(id).unwrap().style.color,
            [1.0, 0.0, 0.0, 1.0],
            "checked:true → [aria-checked=\"true\"] 染红"
        );
        s.controls
            .ensure(id, ControlState::Toggle { checked: false });
        rematch_pseudo_classes(&mut s);
        assert_ne!(
            s.get(id).unwrap().style.color,
            [1.0, 0.0, 0.0, 1.0],
            "checked:false → 不再染红"
        );
    }

    // ── transition 请求发射测 ──

    #[test]
    fn rematch_emits_transition_request_on_change() {
        let mut s = btn_scene();
        let bid = btn_id(&s);
        s.get_mut(bid).unwrap().base_style.transition = vec![TransitionSpec {
            prop: Some(TweenProp::BgColor),
            duration: 0.3,
            ease: Ease::Linear,
            delay: 0.0,
        }];
        s.get_mut(bid)
            .unwrap()
            .interaction
            .flags
            .insert(crate::scene::node::NodeFlags::CASCALED); // 已 warmup
        push_global(&mut s, rule(".btn:hover", "background-color", "#0000ff"));
        s.get_mut(bid)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::HOVERED);
        s.pending_transitions.clear();
        rematch_pseudo_classes(&mut s);
        assert_eq!(
            s.pending_transitions.len(),
            1,
            "bg changed + transition declared → 1 request"
        );
        let r = &s.pending_transitions[0];
        assert!(matches!(r.prop, TweenProp::BgColor));
    }

    #[test]
    fn first_cascade_no_transition() {
        // cascaded_once=false → 首次 cascade 即时生效，不产 transition 请求
        let mut s = btn_scene();
        let bid = btn_id(&s);
        s.get_mut(bid).unwrap().base_style.transition = vec![TransitionSpec {
            prop: Some(TweenProp::BgColor),
            duration: 0.3,
            ease: Ease::Linear,
            delay: 0.0,
        }];
        // cascaded_once 保持 false
        push_global(&mut s, rule(".btn:hover", "background-color", "#0000ff"));
        s.get_mut(bid)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::HOVERED);
        rematch_pseudo_classes(&mut s);
        assert_eq!(
            s.pending_transitions.len(),
            0,
            "first cascade instant (no transition)"
        );
        assert!(
            s.get(bid)
                .unwrap()
                .interaction
                .flags
                .contains(crate::scene::node::NodeFlags::CASCALED),
            "首次 cascade 后 cascaded_once 置 true"
        );
    }

    // ── Spec-4a A2：inline_override（便签层）应用步 ──

    /// 建 root → child（child 为 TextNode，无自身 color 声明）。
    fn build_parent_child() -> (Scene, NodeId, NodeId) {
        let mut root = Node::default();
        root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        };
        let mut child = Node::default();
        child.kind = NodeKind::TextNode;
        child.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 50.0,
            h: 20.0,
        };
        let s = Scene::from_nodes(vec![root, child], vec![(0, 1)]);
        let root_id = s.roots[0];
        let child_id = s.get(root_id).unwrap().children[0];
        (s, root_id, child_id)
    }

    /// 建 root-only scene（无子），用于 probe。
    fn build_simple_tree() -> (Scene, NodeId) {
        let mut root = Node::default();
        root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        };
        let s = Scene::from_nodes(vec![root], vec![]);
        let root_id = s.roots[0];
        (s, root_id)
    }

    #[test]
    fn inline_override_color_inherits_to_child() {
        // 父 inline 设 color:red；child 无自身 color 声明 → 该继承父 inline 值。
        // 验证 set_map 接收 inline 的继承 bit（经 propagate_inherited 传父子）。
        let (mut scene, root, child) = build_parent_child();
        crate::scene::dynamic::set_inline_override(&mut scene, root, "color:#ff0000").unwrap();
        rematch_pseudo_classes(&mut scene);
        let child_color = scene.get(child).unwrap().style.color;
        assert_eq!(
            child_color,
            [1.0, 0.0, 0.0, 1.0],
            "child 无 color 声明 → 继承父 inline red"
        );
        // 父自己也该是 red（inline 应用到自身）
        assert_eq!(scene.get(root).unwrap().style.color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn inline_override_unset_falls_back() {
        // set 后 unset → bit 清 → rematch 不应用 → 回 base_style 值（非 red）。
        let (mut scene, root, _child) = build_parent_child();
        crate::scene::dynamic::set_inline_override(&mut scene, root, "color:#ff0000").unwrap();
        crate::scene::dynamic::unset_inline_override(&mut scene, root, "color").unwrap();
        rematch_pseudo_classes(&mut scene);
        assert_ne!(
            scene.get(root).unwrap().style.color,
            [1.0, 0.0, 0.0, 1.0],
            "unset 后 color 回落（不再 red）"
        );
        // inline_set 的 color bit 应被清
        let set = scene.get(root).unwrap().inline_set.0;
        assert_eq!(set & INH_COLOR as u32, 0, "INH_COLOR bit 清零");
    }

    #[test]
    fn spec3_probe_no_regress_when_no_inline() {
        // 没设 inline 的节点（inline_set == 0）：rematch 不 panic、行为同 Spec-3。
        let (mut scene, root) = build_simple_tree();
        assert_eq!(scene.get(root).unwrap().inline_set.0, 0);
        rematch_pseudo_classes(&mut scene);
        // base color = 默认黑（ResolvedStyle::default）
        assert_eq!(scene.get(root).unwrap().style.color, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn inline_override_beats_dynamic_rule() {
        // inline 优先级 > dynamic rule。同节点 hover 规则设 blue，inline 设 red → red 胜。
        let (mut scene, root, _child) = build_parent_child();
        scene.get_mut(root).unwrap().classes = vec!["r".to_string()];
        push_global(&mut scene, rule(".r", "color", "#0000ff"));
        crate::scene::dynamic::set_inline_override(&mut scene, root, "color:#ff0000").unwrap();
        rematch_pseudo_classes(&mut scene);
        assert_eq!(
            scene.get(root).unwrap().style.color,
            [1.0, 0.0, 0.0, 1.0],
            "inline red 胜过 dynamic blue"
        );
    }

    #[test]
    fn inline_override_background_color_non_inherited() {
        // 非继承属性 background-color：inline 设 → 仅本节点生效，不传子。
        let (mut scene, root, child) = build_parent_child();
        crate::scene::dynamic::set_inline_override(&mut scene, root, "background-color:#00ff00")
            .unwrap();
        rematch_pseudo_classes(&mut scene);
        assert_eq!(
            scene.get(root).unwrap().style.background_color,
            Some([0.0, 1.0, 0.0, 1.0]),
            "root inline bg green"
        );
        // child 无 inline，background_color 不继承 → 仍 None
        assert_eq!(
            scene.get(child).unwrap().style.background_color,
            None,
            "background_color 非继承 → 不传子"
        );
    }

    #[test]
    fn inline_override_width_non_inherited() {
        // 非继承 taffy 字段 width：inline 设 → 应用到 style.taffy_style.size.width。
        let (mut scene, root, _child) = build_parent_child();
        crate::scene::dynamic::set_inline_override(&mut scene, root, "width:123px").unwrap();
        rematch_pseudo_classes(&mut scene);
        use taffy::style::Dimension;
        assert_eq!(
            scene.get(root).unwrap().style.taffy_style.size.width,
            Dimension::length(123.0)
        );
    }

    #[test]
    fn inline_override_display_copies_both_taffy_and_mode() {
        // inline display:none 应同时设 taffy_style.display=None + display_mode=None。
        // 验证 INLINE_DISPLAY bit 的双字段覆盖。
        let (mut scene, root, _child) = build_parent_child();
        crate::scene::dynamic::set_inline_override(&mut scene, root, "display:none").unwrap();
        rematch_pseudo_classes(&mut scene);
        assert_eq!(
            scene.get(root).unwrap().style.taffy_style.display,
            taffy::Display::None,
            "inline display:none → taffy Display::None"
        );
        assert_eq!(
            scene.get(root).unwrap().style.display_mode,
            crate::style::resolved::DisplayMode::None,
            "inline display:none → display_mode=None"
        );
    }

    #[test]
    fn package_inline_display_beats_class_rule() {
        // 打包期 inline style display:none（base_style + inline_declared 标记）不被 class 规则
        // .r{display:flex} 覆盖（CSS inline > class）。回归 showcase/shop dialog-overlay：class
        // display:flex + inline display:none 被错误覆盖成 flex，全屏 overlay 吞掉 back-home 点击。
        let (mut scene, root, _child) = build_parent_child();
        scene.get_mut(root).unwrap().classes = vec!["r".to_string()];
        {
            // 模拟打包期 inline display:none：base_style 双字段 + inline_declared 标记
            let n = scene.get_mut(root).unwrap();
            n.base_style.taffy_style.display = taffy::Display::None;
            n.base_style.display_mode = crate::style::resolved::DisplayMode::None;
            n.base_style.inline_declared |= INLINE_DISPLAY;
        }
        push_global(&mut scene, rule(".r", "display", "flex"));
        rematch_pseudo_classes(&mut scene);
        assert_eq!(
            scene.get(root).unwrap().style.taffy_style.display,
            taffy::Display::None,
            "inline display:none 胜过 class display:flex"
        );
        assert_eq!(
            scene.get(root).unwrap().style.display_mode,
            crate::style::resolved::DisplayMode::None,
            "display_mode 同步 None"
        );
    }

    #[test]
    fn child_explicit_color_not_overridden_by_parent_inline() {
        // 父 inline color red；child 自身 dynamic rule color blue（child inh bit 设）
        // → propagate 不覆盖 child，child 仍 blue。验证 set_map 阻断继承。
        let (mut scene, root, child) = build_parent_child();
        scene.get_mut(child).unwrap().classes = vec!["c".to_string()];
        push_global(&mut scene, rule(".c", "color", "#0000ff"));
        crate::scene::dynamic::set_inline_override(&mut scene, root, "color:#ff0000").unwrap();
        rematch_pseudo_classes(&mut scene);
        assert_eq!(
            scene.get(child).unwrap().style.color,
            [0.0, 0.0, 1.0, 1.0],
            "child 显式声明 blue 不被父 inline red 覆盖"
        );
    }

    #[test]
    fn inline_override_two_level_inheritance() {
        // root inline color red → mid → leaf（均无自身 color）→ leaf 应继承 red。
        let mut root = Node::default();
        root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        };
        let mid = Node::default();
        let mut leaf = Node::default();
        leaf.kind = NodeKind::TextNode;
        let mut scene = Scene::from_nodes(vec![root, mid, leaf], vec![(0, 1), (1, 2)]);
        let root_id = scene.roots[0];
        let mid_id = scene.get(root_id).unwrap().children[0];
        let leaf_id = scene.get(mid_id).unwrap().children[0];
        crate::scene::dynamic::set_inline_override(&mut scene, root_id, "color:#ff0000").unwrap();
        rematch_pseudo_classes(&mut scene);
        assert_eq!(
            scene.get(leaf_id).unwrap().style.color,
            [1.0, 0.0, 0.0, 1.0],
            "leaf 跨级继承父 inline red"
        );
    }

    // ── Spec-4a review I1：unsupported prop 不写 ghost state ──

    #[test]
    fn set_inline_override_ignores_unsupported_prop_no_ghost() {
        // transform 不在 inline_bit 表：完全不写 inline_override（无 ghost state）。
        // padding-top 现在在表（复用 INLINE_PADDING bit）→ 正常置 bit + 生效。
        // 修复前（padding-top 无 bit）：apply_decl 写 taffy_style.padding.top 字段但不置 bit
        // → 下帧 rematch apply_inline_override 不拷该字段 → 静默丢失（ghost state）。
        let (mut scene, root) = build_simple_tree();
        crate::scene::dynamic::set_inline_override(
            &mut scene,
            root,
            "transform: scale(2); padding-top: 10px",
        )
        .unwrap();
        let n = scene.get(root).unwrap();
        // padding-top 有 bit（INLINE_PADDING），transform 无 bit
        assert_ne!(n.inline_set.0, 0, "padding-top 置 bit");
        assert_eq!(
            n.inline_set.0 & INLINE_PADDING,
            INLINE_PADDING,
            "padding-top 复用 INLINE_PADDING bit"
        );
        // padding-top 写进 inline_override（有 bit → 有效）
        use taffy::style::LengthPercentage;
        assert_eq!(
            n.inline_override.taffy_style.padding.top,
            LengthPercentage::length(10.0),
            "padding-top 写入 inline_override"
        );
        // transform 无 bit → 不写 inline_override（transform 字段保持默认，无 ghost）
        // rematch 不 panic
        rematch_pseudo_classes(&mut scene);
        let n = scene.get(root).unwrap();
        assert_eq!(
            n.style.taffy_style.padding.top,
            LengthPercentage::length(10.0),
            "rematch 后 padding-top 生效（10px）"
        );
        // 对照：set "width:100px"（支持）→ bit 置 + 生效
        crate::scene::dynamic::set_inline_override(&mut scene, root, "width:100px").unwrap();
        let bit_after = scene.get(root).unwrap().inline_set.0;
        assert_ne!(bit_after, 0, "supported prop（width）置 bit");
        assert_eq!(
            bit_after & INLINE_WIDTH,
            INLINE_WIDTH,
            "INLINE_WIDTH bit 被置"
        );
    }

    #[test]
    fn set_inline_override_no_ghost_when_shorthand_then_longhand() {
        // ghost-state 复现测：set "border:1px solid red"（border 不在 inline_bit 表）→
        // 修复前：apply_decl 写 taffy_style.border + border_color=red 字段但不置 bit。
        // 再 set "border-width:2px"（在表）→ 置 INLINE_BORDER_WIDTH bit。
        // rematch 时 apply_inline_override 拷 inline.taffy_style.border=2px，但
        // border_color 无 bit → 不拷 → style.border_color 用 base 值（None）。但
        // inline_override.border_color 留 red ghost 值污染字段。
        //
        // 修复后：border 整步被跳过（不写字段），只有 border-width 写入；
        // inline_override.border_color 保持默认 None，无 ghost。
        let (mut scene, root) = build_simple_tree();
        crate::scene::dynamic::set_inline_override(&mut scene, root, "border:1px solid red")
            .unwrap();
        // border 不在 inline_bit → 整步跳过，border_color 字段也不写
        assert_eq!(
            scene.get(root).unwrap().inline_set.0,
            0,
            "border shorthand 不置 bit"
        );
        assert_eq!(
            scene.get(root).unwrap().inline_override.border_color,
            None,
            "border shorthand 不写 border_color 字段（无 ghost）"
        );
        // 再 set border-width（在表）
        crate::scene::dynamic::set_inline_override(&mut scene, root, "border-width:2px").unwrap();
        let n = scene.get(root).unwrap();
        assert_ne!(
            n.inline_set.0 & INLINE_BORDER_WIDTH,
            0,
            "border-width 置 bit"
        );
        // border_color 字段仍未被污染（默认 None，未因 border shorthand 的 red 残留）
        assert_eq!(
            n.inline_override.border_color, None,
            "border_color 无 ghost red 残留"
        );
        // rematch：border 2px 生效（bit 置），border_color 仍 None（base 值）
        rematch_pseudo_classes(&mut scene);
        use taffy::style::LengthPercentage;
        let s = &scene.get(root).unwrap().style;
        assert_eq!(
            s.taffy_style.border.top,
            LengthPercentage::length(2.0),
            "border-width 2px inline 生效"
        );
        assert_eq!(
            s.border_color, None,
            "border_color 用 base 值（无 ghost red）"
        );
    }

    /// CSS 作用域隔离回归测试（main-design §5.4）：页面根(SCOPE_ROOT) → child → 组件实例根(SCOPE_ROOT)
    /// → leaf(.leaf)。页面根作用域的 .leaf 规则不应命中实例内部节点——leaf 的 node_scope =
    /// 实例根（沿父链最近的 SCOPE_ROOT）≠ 页面根，scoped 规则被过滤。
    ///
    /// 此测试锁定 SCOPE_ROOT/LOOKUP_SCOPE 拆分后 CSS 作用域隔离语义不变（SCOPE_ROOT 仍是
    /// 作用域隔离的唯一依据）。注意：实例根在此测同样打双 flag（复现生产 instantiate 路径），
    /// 但即便只打 SCOPE_ROOT，leaf 的 node_scope 仍是实例根，隔离仍成立。
    #[test]
    fn page_scoped_rule_does_not_match_component_instance_node() {
        let mut page_root = Node::default();
        page_root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 400.0,
            h: 400.0,
        };
        let mut child = Node::default();
        child.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        };
        let mut instance_root = Node::default();
        instance_root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        let mut leaf = Node::default();
        leaf.classes = vec!["leaf".to_string()];
        leaf.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 50.0,
            h: 50.0,
        };
        // 树：page_root → child → instance_root → leaf
        let mut s = Scene::from_nodes(
            vec![page_root, child, instance_root, leaf],
            vec![(0, 1), (1, 2), (2, 3)],
        );
        let page_root_id = s.roots[0];
        let child_id = s.get(page_root_id).unwrap().children[0];
        let instance_root_id = s.get(child_id).unwrap().children[0];
        let leaf_id = s.get(instance_root_id).unwrap().children[0];
        // 页面根 + 实例根都打 SCOPE_ROOT（复现 create_root / instantiate 生产路径）；
        // 页面根同时打 LOOKUP_SCOPE（lookup 边界，此测不验证 lookup，只验证 CSS 隔离）。
        s.get_mut(page_root_id)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::SCOPE_ROOT | NodeFlags::LOOKUP_SCOPE);
        s.get_mut(instance_root_id)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::SCOPE_ROOT | NodeFlags::LOOKUP_SCOPE);
        // 页面根作用域的 .leaf 规则：叶子 base color 为默认（非红）。若作用域隔离失效，
        // 该规则会穿透命中 leaf 染红。
        let base_color = s.get(leaf_id).unwrap().base_style.color;
        push_scoped(&mut s, page_root_id, rule(".leaf", "color", "#ff0000"));
        rematch_pseudo_classes(&mut s);
        assert_eq!(
            s.get(leaf_id).unwrap().style.color,
            base_color,
            "页面根作用域的 .leaf 规则不应命中实例内节点（作用域隔离：leaf scope = 实例根 ≠ 页面根）"
        );
    }
}
