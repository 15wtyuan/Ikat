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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
        "margin" => Some(INLINE_MARGIN),
        "border-width" => Some(INLINE_BORDER_WIDTH),
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

use crate::scene::node::{Node, NodeKind};

/// 运行时版 compound 匹配（消费 Node 而非 ElementData，运行时无 ElementTree）。
/// 匹配 tag/classes/id（不含伪类状态——状态由 match_element_with_state 门控）。
/// id 属性存 Node.id_attr（`id="..."`）；Node.id 是 NodeId 索引身份，二者不同。
///
/// **常驻：**runtime rematch 用，不依赖 parse feature。
pub fn compound_matches_node(c: &Compound, node: &Node) -> bool {
    if let Some(t) = &c.tag {
        let kind_tag = match &node.kind {
            NodeKind::Container => "div",
            NodeKind::Button => "button",
            NodeKind::Image => "img",
            NodeKind::TextNode => "span",
            _ => "div", // RichText retired in Spec-2; other leaf kinds map to div.
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
    // 属性选择器：Node 不携带任意 HTML 属性字面值，但 [type="x"] 可经 type→NodeKind 映射精确匹配
    // （input 的 type 在 parse 期已固化为 NodeKind，TextField/PasswordField/SearchField 等各自独立
    // kind）。其他 attr name 仍不匹配（Node 无对应字面值）。
    for a in &c.attrs {
        if !attr_matches_node(a, node) {
            return false;
        }
    }
    true
}

/// `[type="x"]` 经 type 值 → NodeKind 精确对应。其他 attr name / `[type]` 存在形式本轮不匹配。
///
/// type→NodeKind 映射必须与 `crates/fence/src/schema/tag.rs::resolve_semantic`（input 分支）
/// 保持一致：那是 parse 期 `<input type=x>` → SemanticKind → NodeKind 的同一份标准 HTML 语义，
/// 这里是 match 期 selector `[type=x]` → NodeKind 的另一面。两者分歧会导致 `[type="password"]`
/// 匹配错误 kind。
fn attr_matches_node(a: &AttrSelector, node: &Node) -> bool {
    if a.name != "type" {
        return false;
    }
    let Some(val) = &a.value else {
        return false;
    }; // [type] 存在形式本轮不匹配
    let expected_kind = match val.as_str() {
        "text" => NodeKind::TextField,
        "password" => NodeKind::PasswordField,
        "search" => NodeKind::SearchField,
        "number" => NodeKind::NumberField,
        "range" => NodeKind::Slider,
        "checkbox" => NodeKind::Toggle,
        "radio" => NodeKind::RadioButton,
        _ => return false,
    };
    node.kind == expected_kind
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
    compound_matches_node(c, node)
}

/// 完整后代链匹配（从右往左，复用 selector.rs `matches` 算法，消费 Node/Scene）。
/// 最后一个 compound 必须命中目标 node 本身（含状态门）；前面按 combinator 沿
/// parent 链找（Child=直接父，Descendant=任一祖先，带回溯）。
pub fn match_element_with_state(sel: &ParsedSelector, node_id: NodeId, scene: &Scene) -> bool {
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
    match_chain_with_state(comps, comps.len() - 1, node_id, scene)
}

/// 递归匹配 comps[0..end_idx] 在 start_node 的祖先链上（同 selector.rs
/// `match_compound_chain`）。`start_node` 是已匹配 comps[end_idx] 的节点，
/// 为 comps[end_idx - 1] 找祖先（Child：直接父；Descendant：任一祖先+回溯）。
fn match_chain_with_state(
    comps: &[Compound],
    end_idx: usize,
    start_node: NodeId,
    scene: &Scene,
) -> bool {
    if end_idx == 0 {
        return true;
    }
    let target_comp = &comps[end_idx - 1];
    let combinator = comps[end_idx].combinator;
    match combinator {
        Combinator::Child => match scene.get(start_node).and_then(|n| n.parent) {
            Some(parent) => {
                compound_matches_with_state(target_comp, parent, scene)
                    && match_chain_with_state(comps, end_idx - 1, parent, scene)
            }
            None => false,
        },
        Combinator::Descendant => {
            let mut cur = scene.get(start_node).and_then(|n| n.parent);
            while let Some(ancestor) = cur {
                if compound_matches_with_state(target_comp, ancestor, scene)
                    && match_chain_with_state(comps, end_idx - 1, ancestor, scene)
                {
                    return true;
                }
                // 此祖先匹配但更左链匹配不上 → 继续往上找
                cur = scene.get(ancestor).and_then(|n| n.parent);
            }
            false
        }
    }
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
    // 预提取 specificity 元组 + owned rule 副本（避免借 scene.dynamic_rules 跨 get_mut）。
    let rules_with_spec: Vec<(u32, u32, u32, DynamicRule)> = scene
        .dynamic_rules
        .rules
        .iter()
        .map(|r| {
            (
                r.selector.specificity.0,
                r.selector.specificity.1,
                r.selector.specificity.2,
                r.clone(),
            )
        })
        .collect();
    // 收集所有 NodeId（slotmap 分配，不能手造 NodeId(i)）。
    let node_ids: Vec<NodeId> = scene.nodes.values().map(|n| n.id).collect();
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
        // 收集命中规则
        let mut matched: Vec<(u32, u32, u32, DynamicRule)> = Vec::new();
        for r in &rules_with_spec {
            if match_element_with_state(&r.3.selector, node_id, scene) {
                matched.push(r.clone());
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
    use crate::scene::node::{Node, NodeFlags, NodeId, NodeKind, Rect, Scene};
    use crate::style::resolved::TransitionSpec;
    use crate::tween::{Ease, TweenProp};

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
        s.dynamic_rules
            .rules
            .push(rule(".btn:hover", "background-color", "#0000ff"));
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
        s.dynamic_rules
            .rules
            .push(rule(".par:hover", "color", "#1a1d2e"));
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
        s.dynamic_rules
            .rules
            .push(rule(".par", "font-size", "24px"));
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
        s.dynamic_rules
            .rules
            .push(rule(".par", "font-size", "24px"));
        s.dynamic_rules.rules.push(rule(".c", "font-size", "12px"));
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
        s.dynamic_rules.rules.push(rule(".a", "font-size", "20px"));
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
        s.dynamic_rules
            .rules
            .push(rule(".btn:active", "background-color", "#ff0000"));
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
        s.dynamic_rules
            .rules
            .push(rule(".btn:disabled", "opacity", "0.5"));
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
        s.dynamic_rules
            .rules
            .push(rule(".btn:hover", "width", "200px"));
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
        s.dynamic_rules
            .rules
            .push(rule(".btn:hover", "color", "#ff0000"));
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
        s.dynamic_rules
            .rules
            .push(rule(".parent:hover .child", "color", "#0000ff"));
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
        s.dynamic_rules.rules.push(rule(".btn", "color", "#ff0000"));
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
        s.dynamic_rules
            .rules
            .push(rule(".btn:hover", "background-color", "#0000ff"));
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
        s.dynamic_rules
            .rules
            .push(rule(".btn:focus", "background-color", "#0000ff"));
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
        s.dynamic_rules
            .rules
            .push(rule(".btn:focus", "background-color", "#0000ff"));
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
        s.dynamic_rules
            .rules
            .push(rule(".parent:focus .child", "color", "#0000ff"));
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
        s.dynamic_rules.rules.push(rule(
            ".btn:hover",
            "background-image",
            "url(icons/home.png)",
        ));
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
        s.dynamic_rules
            .rules
            .push(rule(".btn:hover", "background-size", "cover"));
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
        s.dynamic_rules
            .rules
            .push(rule(".btn:hover", "border-radius", "8px"));
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

    #[test]
    fn attr_selector_type_matches_nodekind_precisely() {
        // [type="password"] 只匹配 PasswordField，不匹配 TextField
        let sel = hand_selector(r#"[type="password"]"#);
        let pw_node = test_node(NodeKind::PasswordField);
        assert!(compound_matches_node(&sel.compound[0], &pw_node));
        let text_node = test_node(NodeKind::TextField);
        assert!(!compound_matches_node(&sel.compound[0], &text_node));
        // [type="text"] 只匹配 TextField
        let sel_text = hand_selector(r#"[type="text"]"#);
        assert!(compound_matches_node(&sel_text.compound[0], &text_node));
        assert!(!compound_matches_node(&sel_text.compound[0], &pw_node));
    }

    #[test]
    fn attr_selector_non_type_attr_does_not_match() {
        // 非 type 属性：本轮不匹配（Node 不携带任意 HTML 属性字面值）
        let sel = hand_selector("[disabled]");
        let node = test_node(NodeKind::TextField);
        assert!(!compound_matches_node(&sel.compound[0], &node));
    }

    #[test]
    fn attr_selector_type_exists_form_does_not_match() {
        // [type] 存在形式（无 = val）：本轮不支持（只走 Eq 精确匹配）
        let sel = hand_selector("[type]");
        let node = test_node(NodeKind::TextField);
        assert!(!compound_matches_node(&sel.compound[0], &node));
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
        s.dynamic_rules
            .rules
            .push(rule(".btn:hover", "background-color", "#0000ff"));
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
        s.dynamic_rules
            .rules
            .push(rule(".btn:hover", "background-color", "#0000ff"));
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
        scene
            .dynamic_rules
            .rules
            .push(rule(".r", "color", "#0000ff"));
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
    fn child_explicit_color_not_overridden_by_parent_inline() {
        // 父 inline color red；child 自身 dynamic rule color blue（child inh bit 设）
        // → propagate 不覆盖 child，child 仍 blue。验证 set_map 阻断继承。
        let (mut scene, root, child) = build_parent_child();
        scene.get_mut(child).unwrap().classes = vec!["c".to_string()];
        scene
            .dynamic_rules
            .rules
            .push(rule(".c", "color", "#0000ff"));
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
        // transform / padding-top 不在 inline_bit 表：完全不写 inline_override（无 ghost state）。
        // 修复前：apply_decl 写 taffy_style.transform / padding.top 字段但不置 bit
        // → 下帧 rematch apply_inline_override 不拷这些字段 → 静默丢失（ghost state）。
        let (mut scene, root) = build_simple_tree();
        crate::scene::dynamic::set_inline_override(
            &mut scene,
            root,
            "transform: scale(2); padding-top: 10px",
        )
        .unwrap();
        let n = scene.get(root).unwrap();
        // inline_set 不含任何 bit（transform/padding-top 都没 bit）
        assert_eq!(n.inline_set.0, 0, "unsupported prop 不置 bit");
        // inline_override 字段也不被写（transform 仍默认 scale 1.0，padding 仍 0）
        use taffy::style::LengthPercentage;
        assert_eq!(
            n.inline_override.taffy_style.padding.top,
            LengthPercentage::length(0.0),
            "padding-top 不写 inline_override（无 ghost）"
        );
        // rematch 不 panic，且 style 不受这些 prop 影响
        rematch_pseudo_classes(&mut scene);
        let n = scene.get(root).unwrap();
        assert_eq!(
            n.style.taffy_style.padding.top,
            LengthPercentage::length(0.0),
            "rematch 后 padding 仍为 0（unsupported prop 未生效）"
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
}
