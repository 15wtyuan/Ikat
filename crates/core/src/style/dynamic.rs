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

/// prop 名 → 可继承属性 bit（非可继承返 None）。core 侧硬编码（fence schema 的
/// inherited 标志是 build-time 校验用的另一份；本 spike core 用此局部表，Spec-3 再统一）。
fn inherited_bit(prop: &str) -> Option<u16> {
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
    // 属性选择器匹配：当前仅 data-controller 存储在 Node 上（字面值匹配）。
    // data-page 是状态查询，已在 compound_matches_with_state 顶层短路验证，
    // 此处跳过避免误判为"未知属性"。
    // 其他属性（节点不携带）→ 不匹配。
    for a in &c.attrs {
        if a.name == "data-controller" {
            let got = node.data_controller.as_deref();
            match a.op {
                AttrOp::Exists => {
                    if got.is_none() {
                        return false;
                    }
                }
                AttrOp::Eq => {
                    if got != a.value.as_deref() {
                        return false;
                    }
                }
            }
        } else if a.name == "data-page" {
            // data-page 状态查询已在 compound_matches_with_state 验证，此处跳过。
            continue;
        } else {
            // 其他属性节点不携带字面值 → 此 compound 不匹配。
            return false;
        }
    }
    true
}

/// 从 node（含自身）向上找最近的带 data_controller 祖先，
/// 返回 (挂载节点 NodeId, Controller.selected_index)。
/// 声明了 data_controller 但 registry 无条目（加载未完成/孤儿）→ 该祖先不算，继续向上。
fn find_governing_controller(start: NodeId, scene: &Scene) -> Option<(NodeId, i32)> {
    let mut cur = Some(start);
    while let Some(nid) = cur {
        if let Some(n) = scene.get(nid) {
            if n.data_controller.is_some() {
                if let Some(ctrl) = scene.controllers.get(&nid) {
                    return Some((nid, ctrl.selected_index));
                }
                // data_controller 声明但 registry 无条目 → 跳过，继续向上
            }
            cur = n.parent;
        } else {
            break;
        }
    }
    None
}

/// 判定 compound 是否匹配 node + 状态门。
///
/// 状态门分两层：
/// 1. [data-page] 属性选择器是状态查询（非字面匹配）：使用预计算的 governing
///    （从最右目标节点查到的最近 data_controller 祖先的 selected_index），
///    与 CSS 值字面比较。多层 controller 嵌套时内层优先——governing 始终来自
///    原始目标节点，不在祖先链回溯中重查。
/// 2. 伪类状态门：hovered / active / disabled / focused。
///
/// 两层均通过后才调 compound_matches_node 做字面匹配（tag/classes/id_attr）。
fn compound_matches_with_state(
    c: &Compound,
    node_id: NodeId,
    scene: &Scene,
    governing: Option<(NodeId, i32)>,
) -> bool {
    // [data-page="N"] 是状态查询，需在伪类/字面匹配前短路判定。
    // 如果一个 compound 有多个 [data-page] attr，任一不匹配即整体不匹配。
    for a in &c.attrs {
        if a.name == "data-page" {
            // [data-page] 只认 = 运算符；Exists 等形式不匹配。
            if !matches!(a.op, AttrOp::Eq) {
                return false;
            }
            let want = match &a.value {
                Some(v) => v.as_str(),
                None => return false,
            };
            let matched = match governing {
                Some((_, selected)) => selected.to_string() == want,
                None => false,
            };
            if !matched {
                return false;
            }
        }
    }
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
    // data-controller 字面匹配在 compound_matches_node 中处理
    compound_matches_node(c, node)
}

/// 完整后代链匹配（从右往左，复用 selector.rs `matches` 算法，消费 Node/Scene）。
/// 最后一个 compound 必须命中目标 node 本身（含状态门）；前面按 combinator 沿
/// parent 链找（Child=直接父，Descendant=任一祖先，带回溯）。
///
/// governing 从 node_id 预计算一次（[data-page] 的最近 data_controller），
/// 整个链共用——不在祖先回溯时对中间节点重查，保证"最近 controller 治理"语义。
pub fn match_element_with_state(sel: &ParsedSelector, node_id: NodeId, scene: &Scene) -> bool {
    let comps = &sel.compound;
    if comps.is_empty() {
        return false;
    }
    // 从目标节点预计算 [data-page] 的 governing controller（整个链共用）
    let governing = find_governing_controller(node_id, scene);
    let last = &comps[comps.len() - 1];
    if !compound_matches_with_state(last, node_id, scene, governing) {
        return false;
    }
    if comps.len() == 1 {
        return true;
    }
    match_chain_with_state(comps, comps.len() - 1, node_id, scene, governing)
}

/// 递归匹配 comps[0..end_idx] 在 start_node 的祖先链上（同 selector.rs
/// `match_compound_chain`）。`start_node` 是已匹配 comps[end_idx] 的节点，
/// 为 comps[end_idx - 1] 找祖先（Child：直接父；Descendant：任一祖先+回溯）。
fn match_chain_with_state(
    comps: &[Compound],
    end_idx: usize,
    start_node: NodeId,
    scene: &Scene,
    governing: Option<(NodeId, i32)>,
) -> bool {
    if end_idx == 0 {
        return true;
    }
    let target_comp = &comps[end_idx - 1];
    let combinator = comps[end_idx].combinator;
    match combinator {
        Combinator::Child => match scene.get(start_node).and_then(|n| n.parent) {
            Some(parent) => {
                compound_matches_with_state(target_comp, parent, scene, governing)
                    && match_chain_with_state(comps, end_idx - 1, parent, scene, governing)
            }
            None => false,
        },
        Combinator::Descendant => {
            let mut cur = scene.get(start_node).and_then(|n| n.parent);
            while let Some(ancestor) = cur {
                if compound_matches_with_state(target_comp, ancestor, scene, governing)
                    && match_chain_with_state(comps, end_idx - 1, ancestor, scene, governing)
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
        assert!(matches!(
            s.get(bid).unwrap().style.taffy_style.size.width,
            Dimension::Length(200.0)
        ));
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
                taffy::style::LengthPercentage::Length(8.0),
                "hover → border-radius 水平 8px"
            );
            assert_eq!(
                c.v,
                taffy::style::LengthPercentage::Length(8.0),
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

    #[test]
    fn attr_selector_literal_matches_data_controller() {
        // node with data_controller="tab" matches [data-controller="tab"]
        let mut root = Node::default();
        root.classes = vec!["root".to_string()];
        root.data_controller = Some("tab".into());
        let mut s = Scene::from_nodes(vec![root], vec![]);
        let rid = s.roots[0];
        s.dynamic_rules
            .rules
            .push(rule(r#"[data-controller="tab"]"#, "color", "#0000ff"));
        rematch_pseudo_classes(&mut s);
        assert_eq!(s.get(rid).unwrap().style.color, [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn attr_selector_not_match_different_value() {
        let mut root = Node::default();
        root.data_controller = Some("tab".into());
        let mut s = Scene::from_nodes(vec![root], vec![]);
        let rid = s.roots[0];
        s.dynamic_rules
            .rules
            .push(rule(r#"[data-controller="modal"]"#, "color", "#ff0000"));
        rematch_pseudo_classes(&mut s);
        assert_eq!(
            s.get(rid).unwrap().style.color,
            [0.0, 0.0, 0.0, 1.0],
            "different value → no match → base color"
        );
    }

    // ── [data-page] state query tests ──

    #[test]
    fn data_page_matches_governing_controller_page() {
        // root 挂载 controller "tab"（selected_index=1），子节点 .panel 匹配 [data-page="1"]。
        let mut root = Node::default();
        root.data_controller = Some("tab".into());
        let mut child = Node::default();
        child.classes = vec!["panel".to_string()];
        let mut s = Scene::from_nodes(vec![root, child], vec![(0, 1)]);
        let root_id = s.roots[0];
        let child_id = s.get(root_id).unwrap().children[0];
        s.set_controller_selected(root_id, 1);
        s.dynamic_rules
            .rules
            .push(rule(r#"[data-page="1"] .panel"#, "color", "#0000ff"));
        rematch_pseudo_classes(&mut s);
        assert_eq!(s.get(child_id).unwrap().style.color, [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn data_page_no_match_different_page() {
        // root 挂载 controller "tab"（selected_index=0），[data-page="1"] 不匹配。
        let mut root = Node::default();
        root.data_controller = Some("tab".into());
        let mut child = Node::default();
        child.classes = vec!["panel".to_string()];
        let mut s = Scene::from_nodes(vec![root, child], vec![(0, 1)]);
        let root_id = s.roots[0];
        let child_id = s.get(root_id).unwrap().children[0];
        s.set_controller_selected(root_id, 0);
        s.dynamic_rules
            .rules
            .push(rule(r#"[data-page="1"] .panel"#, "color", "#0000ff"));
        rematch_pseudo_classes(&mut s);
        assert_eq!(
            s.get(child_id).unwrap().style.color,
            [0.0, 0.0, 0.0, 1.0],
            "selected_index=0 → [data-page=\"1\"] 不匹配 → base color"
        );
    }

    #[test]
    fn nested_controllers_nearest_governs() {
        // 外层 data-controller="tab"（page 0），内层 data-controller="sub"（page 1）。
        // 内层子节点匹配 [data-page="1"]（内层 nearest 胜），不匹配 [data-page="0"]。
        let mut outer = Node::default();
        outer.data_controller = Some("tab".into());
        let mut inner = Node::default();
        inner.data_controller = Some("sub".into());
        let mut child = Node::default();
        child.classes = vec!["panel".to_string()];
        // outer → inner → child
        let mut s = Scene::from_nodes(vec![outer, inner, child], vec![(0, 1), (1, 2)]);
        let outer_id = s.roots[0];
        let inner_id = s.get(outer_id).unwrap().children[0];
        let child_id = s.get(inner_id).unwrap().children[0];
        s.set_controller_selected(outer_id, 0);
        s.set_controller_selected(inner_id, 1);
        // [data-page="1"] .panel：inner nearest → selected_index=1 → 匹配
        s.dynamic_rules
            .rules
            .push(rule(r#"[data-page="1"] .panel"#, "color", "#0000ff"));
        rematch_pseudo_classes(&mut s);
        assert_eq!(
            s.get(child_id).unwrap().style.color,
            [0.0, 0.0, 1.0, 1.0],
            "inner controller page=1 → [data-page=\"1\"] 匹配"
        );
        // 换规则：[data-page="0"] .panel：inner nearest → selected_index=1 ≠ 0 → 不匹配
        s.dynamic_rules.rules.clear();
        s.dynamic_rules
            .rules
            .push(rule(r#"[data-page="0"] .panel"#, "color", "#ff0000"));
        rematch_pseudo_classes(&mut s);
        assert_eq!(
            s.get(child_id).unwrap().style.color,
            [0.0, 0.0, 0.0, 1.0],
            "inner controller page=1 → [data-page=\"0\"] 不匹配（nearest wins）"
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
}
