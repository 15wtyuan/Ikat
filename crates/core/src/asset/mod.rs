//! 包格式（.pkg.bin，当前版本见 `PKG_FORMAT_VERSION` 常量及其行尾 changelog）：
//! Rust-internal（packager 写、runtime 读，C# 不解析）。
//! 布局锁：同一 fixture 的打包字节哈希有 CI 门（packer `schema_lock.rs`）——
//! 任何改变字节的布局改动都会翻转哈希，bump 版本时须同步更新登记值。
//! v46：TemplateNode 加 href 列（`<a>` 链接目标，#74）+ ResolvedStyle 加 text_decoration 字段。布局变，旧 v45 pkg 加载报 TooOld。
//! v45：ResolvedStyle 加 white_space/overflow_wrap/word_break/text_wrap 四字段 + InheritedSet u16→u64（#73 换行控制全集，bincode 布局变）。旧 v44 pkg 加载报 TooOld。
//! v44：KeyframeStop 加 layout/box-shadow 通道（width/height 域+值、flex_grow、box_shadow 列表，#10 layout 动画）。手编 keyframes 布局变，旧 v43 pkg 加载报 TooOld。
//! v42：ResolvedStyle 加 line_height_px 字段（CSS line-height px 形双槽，#65 高度爆炸修复）。
//! v41：ResolvedStyle 加 viewport 字段（vw/vh/vmin/vmax 平行长度声明，分辨率适配重排语言，bincode 布局变）。
//! v40：ResolvedStyle 加 position_declared（absolute 包含块语义）。
//! v39：TweenProp 加 Transform 变体（transition: transform 复合 TRS 通道，bincode 判别值扩展）。
//! v35：TemplateNode 加 custom_tag 列 + component_scope 位（flags bit 0x04）+ PerComponentScopes
//!   段（组件展开域锚定规则表；Custom Element 打包期展开产物）。
//! v34：ResolvedStyle.background_gradient Option<Gradient2>→Option<Gradient>（radial + 多 stop + 任意角度，bincode 布局变）。
//! v33：TemplateNode flags 字节新增 rich_text_block 位（rich-text-block 容器根标记，bit 0x02）。
//! v32：ResolvedStyle.box_shadow Option<BoxShadow>→Vec<BoxShadow> + blur/inset 字段（box-shadow 全语义，bincode 布局变）。
//! v31：Compound 加 pseudo_nth_child 字段（:nth-child selector，bincode 布局变）。
//! v30：ComponentTemplate 加 keyframes 表（@keyframes runtime 地基）+ ResolvedStyle 加 animation。
//! v29：TemplateNode 加 aria_controls 列（TabList tab→panel 跨树关联的 panel id）。
//! v28：TemplateNode 加 role/data-slot 列（role-driven controls 地基）。
//! v27：<template> 子树进 pkg（NodeKind::Template 新增，旧 v26 pkg 加载报 TooOld）。
//! v26：ControlInit 加 Dropdown/NumberField 变体（bincode 布局变，旧 v25 pkg 加载报 TooOld）。
//! v25：ControlInit 加 TextField/TextArea 变体（bincode 布局变，旧 v24 pkg 加载报 TooOld）。
//! v24：TemplateNode 加 control_init 字段（bincode 布局变，旧 v23 pkg 加载报 TooOld）。
//!
//! 多组件格式：一个 pkg.bin = 多个具名组件（ComponentTable 切分）。
//! 布局：Header(20B) + StringTable + ComponentTable + NodeBlock + PerComponent(DynamicRules)
//!   + PerComponent(Keyframes)。
//!   - Header 不含 root_w/root_h（root_size 归 Stage）+ 不含 atlas 引用（图集归 Unity）。
//!   - StringTable：组件名 / text content / img path / classes / id_attr / keyframes 名 /
//!     hook 名共用一张表（intern 去重）。
//!   - ComponentTable：每组件 {name_idx, root_node_idx, node_count, dynamic_rules_blob_len}。
//!   - NodeBlock：所有组件节点平铺，parent_idx 用 -1 表组件根（全局位置索引）。
//!   - PerComponentDynamicRules：每组件 dynamic_rules 的 bincode blob（紧跟 ComponentTable 段）。
//!   - PerComponentKeyframes：每组件 keyframes 手动编码 blob（紧跟 DynamicRules 段，
//!     每 blob 前有 u32 长度；rule.name / stop.hook 走 StringTable intern）。
//! style 字段 = bincode(ResolvedStyle，已 bake，含 animation 声明)。img src 指向归一化 path
//! 字符串（非 atlas sprite）。
//!
//! 核心不知图集（运行时纹理/UV 归 Unity）。图尺寸由 Stage.set_image_sizes 在运行时灌入
//! （来自 atlas.json），不再进 pkg.bin。

use crate::scene::animation::{
    AnimatableProps, KeyframeStop, KeyframeStopSelector, KeyframesRule, TransformAnim,
};
use crate::scene::NodeKind;
use crate::style::dynamic::DynamicRuleTable;
use crate::style::resolved::ResolvedStyle;
use crate::tween::{ease_from_ffi, Ease};

pub const PKG_MAGIC: u32 = 0x474B504C; // 磁盘字节(LE) "LPKG"（不与 frame blob "LOOM" 撞）。两处魔数皆 LoomGUI 时代遗留：字节=格式兼容契约而非品牌，更名不改。
pub const PKG_FORMAT_VERSION: u32 = 46; // v46: TemplateNode 加 href（#74 `<a>`）+ ResolvedStyle 加 text_decoration。bincode 布局变，旧包拒绝。
pub(crate) const MIN_VERSION: u32 = 46;
pub(crate) const MAX_VERSION: u32 = 46;
const NULL_IDX: u16 = 0xFFFF;

/// 一个已加载的包（资源池条目）。`name` read 时填空串，由 `Stage::load_package(name, ..)` 覆盖。
#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub components: std::collections::HashMap<String, ComponentTemplate>,
}

/// 一个组件的模板（instantiate 的克隆源）。
#[derive(Debug, Clone)]
pub struct ComponentTemplate {
    pub name: String,
    pub nodes: Vec<TemplateNode>,
    pub dynamic_rules: DynamicRuleTable,
    /// @keyframes 规则表（打包期从组件 `<style>` 提取）。instantiate 时合并进
    /// Scene.keyframes 全局表（CSS 全局查找语义）。
    pub keyframes: Vec<KeyframesRule>,
    /// 组件展开域（Custom Element 打包期展开的实例）锚定规则：每实例一条
    /// (锚节点 idx, 组件模板自带动态规则)。instantiate 时按 scope_root=锚节点包装
    /// （组件内部选择器只在该展开域内匹配）。
    pub component_scopes: Vec<(usize, DynamicRuleTable)>,
}

/// 文本控件初始值（TextField/TextArea 共用，从 HTML value/placeholder 属性 bake）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EditInit {
    pub value: String,
    pub placeholder: String,
    pub max_length: usize, // 0 = 无限
    pub readonly: bool,
}

/// 控件初始值（从 HTML 属性 bake，按 NodeKind 分派）。打包期 bridge 提取 → 进
/// pkg.bin → core instantiate 时填 runtime side table。variant 与控件 NodeKind 一一对应；
/// 非 control 节点此字段为 None。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ControlInit {
    Progress {
        value: f32,
        max: f32,
        indeterminate: bool,
    },
    Toggle {
        checked: bool,
    },
    Radio {
        checked: bool,
        name: String,
    },
    Slider {
        value: f32,
        min: f32,
        max: f32,
        step: f32,
    },
    /// 单行文本输入（TextField）。EditInit 含 value/placeholder/max_length/readonly。
    TextField(EditInit),
    /// 多行文本输入（TextArea）。EditInit 含 value/placeholder/max_length/readonly。
    TextArea(EditInit),
    /// `<select>` 初始选中项索引（打包期扫 option[selected] 算出，无 selected 则 0）+
    /// 逐项 `value` 属性值（HTML 语义：`role=option` 的 `value` 内容属性；缺席项 = None，
    /// 运行时 SelectedValue 回落该项文本）。声明序与 selected_index 同口径。
    Dropdown {
        selected_index: u32,
        option_values: Vec<Option<String>>,
    },
    /// `<input type="number">`。edit 是 value/placeholder/maxlength/readonly；min/max/step 数值约束。
    NumberField {
        edit: EditInit,
        min: f32,
        max: f32,
        step: f32,
    },
    /// `role="tablist"` 初始激活 tab 序号（打包期扫 role=tab 子的 aria-selected="true"
    /// 算出，无则 0）。运行时映成 ControlState::TabList{selected_index}。
    TabList {
        selected_index: u32,
    },
}

/// 模板节点：序列化态（instantiate 时 build 成 live Node）。
/// 与 live Node 区别：无 NodeId（instantiate 时 slotmap 分配）、无 taffy_id（每帧 solve 重建）。
#[derive(Debug, Clone, Default)]
pub struct TemplateNode {
    pub kind: NodeKind,
    pub style: ResolvedStyle,      // base_style（已 bake）
    pub parent_idx: Option<usize>, // 模板内位置索引（None=组件根）
    pub classes: Vec<String>,
    pub id_attr: Option<String>,
    pub draggable: bool,
    pub tabindex: Option<i32>,
    pub content: Option<String>,
    pub src: Option<String>,
    /// `<a>` 链接目标（#74，opaque 标识符，无 URI 语义）。仅 Link 节点有值；
    /// fence 保证非空。instantiate 时灌进 `Scene.link_hrefs`。
    pub href: Option<String>,
    /// 控件初始值（按 kind 分派；None = 非控件节点）。打包期 bridge 从 HTML 属性提取。
    pub control_init: Option<ControlInit>,
    /// WAI-ARIA role（"combobox"/"slider"/...）。None = 普通容器/叶子。role 驱动语义分派。
    pub role: Option<String>,
    /// data-slot 值（"fill"/"thumb"）。控件视觉部件标识（HTML data-* 私有扩展机制）。
    pub data_slot: Option<String>,
    /// WAI-ARIA `aria-controls`（TabList tab→panel 跨树关联的 panel id）。None = 非关联节点。
    /// 运行时由 instantiate 拷进 RoleInfo.aria_controls，sync_control_visuals 据此 find_node_by_id 解析 panel。
    pub aria_controls: Option<String>,
    /// rich-text-block 容器根标记：`display:block` 容器且其直接子全是 inline 级
    /// （text/span/img）。打包期由 fence `rich_text_blocks`（ir_idx 集合）烘入，运行时
    /// compiler/solve/render 读此 flag 把 inline 子拍平成 RichRun 走 inline flow。
    /// Text 节点与 block 容器永远 false。
    pub rich_text_block: bool,
    /// CustomElement 的原始 hyphen 标签名（`<game-item-card>` → "game-item-card"）。
    /// 打包期展开保留字面量（tag 选择器 rematch 匹配 + dump 发射用）。非 CustomElement None。
    pub custom_tag: Option<String>,
    /// 组件展开域根标记（CustomElement host）：instantiate 时打
    /// SCOPE_ROOT | LOOKUP_SCOPE | HOST_IN_PARENT_SCOPE（对后代是 CSS/查找边界，
    /// 自身归外层页面作用域）。打包期由组件展开器烘入。
    pub component_scope: bool,
}

/// write_package_with_scopes 的展开域条目：锚节点（组件内局部 idx）+ 组件模板动态规则。
#[derive(Clone, Copy)]
pub struct ComponentScopeInput<'a> {
    pub component: &'a str,
    pub anchor_idx: usize,
    pub rules: &'a DynamicRuleTable,
}

/// write_package 的输入（打包器构造，已归一化：path 已相对、style 已 bake）。
/// 每组件 4 元组：(name, nodes, dynamic_rules, keyframes)。
pub struct PackageInput<'a> {
    pub components: Vec<(
        &'a str,
        &'a [TemplateNode],
        &'a DynamicRuleTable,
        &'a [KeyframesRule],
    )>,
}

#[derive(Debug)]
pub enum PkgError {
    BadMagic,
    TooOld(u32),
    TooNew(u32),
    Truncated(&'static str),
    OobString(u16),
    Bincode(bincode::Error),
    BadKind(u8),
    BadKeyframeSelector(u8),
    BadLenDomain(u8),
    BadEaseTag(u8),
    DupComponent(String),
}

impl std::fmt::Display for PkgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PkgError::BadMagic => write!(f, "bad magic (not a ikat package)"),
            PkgError::TooOld(v) => {
                write!(f, "package formatVersion {v} too old (min {MIN_VERSION})")
            }
            PkgError::TooNew(v) => {
                write!(f, "package formatVersion {v} too new (max {MAX_VERSION})")
            }
            PkgError::Truncated(ctx) => write!(f, "truncated package: {ctx}"),
            PkgError::OobString(i) => write!(f, "string index {i} out of range"),
            PkgError::Bincode(e) => write!(f, "style bincode: {e}"),
            PkgError::BadKind(k) => write!(f, "bad node kind tag {k}"),
            PkgError::BadKeyframeSelector(t) => {
                write!(f, "bad keyframe stop selector tag {t}")
            }
            PkgError::BadLenDomain(d) => {
                write!(f, "bad animatable length domain tag {d}")
            }
            PkgError::BadEaseTag(t) => write!(f, "bad ease tag {t}"),
            PkgError::DupComponent(n) => {
                write!(f, "duplicate component name in package: {n}")
            }
        }
    }
}

impl std::error::Error for PkgError {}

impl From<bincode::Error> for PkgError {
    fn from(e: bincode::Error) -> Self {
        PkgError::Bincode(e)
    }
}

/// 序列化 PackageInput → .pkg.bin bytes（多组件格式，无组件展开域）。
pub fn write_package(input: &PackageInput) -> Vec<u8> {
    write_package_with_scopes(input, &[])
}

/// 序列化 PackageInput → .pkg.bin bytes（多组件格式 + 组件展开域锚定规则）。
///
/// 布局：Header(20B) + StringTable + ComponentTable + NodeBlock + PerComponent(DynamicRules)
///   + PerComponent(Keyframes) + PerComponent(Scopes)。
/// 所有字符串（组件名 / text / img path / classes / id_attr）
/// 共用同一 StringTable（intern 去重）。`input` 须已归一化（path 相对、style bake）。
pub fn write_package_with_scopes(input: &PackageInput, scopes: &[ComponentScopeInput]) -> Vec<u8> {
    // 1. intern 全部字符串（组件名 + 每节点 text/src/classes/id_attr）。
    //    所有 intern 必须在写 header(string_count) 之前完成。
    let mut strings: Vec<String> = Vec::new();
    let mut idx_of: std::collections::HashMap<String, u16> = std::collections::HashMap::new();

    let component_count = input.components.len();
    // 每组件：(name_idx, root_node_idx, node_count, dynamic_blob, keyframes_blob)
    // 全局 NodeBlock 由各组件节点顺次拼接，root_node_idx = 该组件首节点在全局的位置。
    let mut comp_records: Vec<(u16, u32, u32, Vec<u8>, Vec<u8>)> =
        Vec::with_capacity(component_count);
    // 每节点（全局）：(parent_idx:i32, kind_tag, style_blob, text_idx, src_idx, class_idx[], id_idx, flags, tabindex, control_init_blob, role_idx, data_slot_idx, aria_controls_idx, custom_tag_idx, href_idx)
    let mut node_records: Vec<(
        i32,
        u8,
        Vec<u8>,
        u16,
        u16,
        Vec<u16>,
        u16,
        u8,
        i32,
        Vec<u8>,
        u16,
        u16,
        u16,
        u16,
        u16,
    )> = Vec::new();
    let mut global_node_offset: u32 = 0;
    for (name, nodes, dynamic_rules, keyframes) in &input.components {
        let name_idx = intern(name, &mut strings, &mut idx_of);
        let comp_base = global_node_offset;
        // spec 约定 nodes[0]=组件根（parent=None)。debug_assert：write 输入由打包器控制，
        // 违反即打包器 bug（非运行时 malformed 输入），故 debug_assert 足够（release 不付代价）。
        if !nodes.is_empty() {
            debug_assert!(
                nodes[0].parent_idx.is_none(),
                "component `{name}` nodes[0] must be root (parent_idx=None)"
            );
        }
        // intern 每节点字符串 + 收 (parent_idx 全局化, ...)。
        for tn in nodes.iter() {
            // parent_idx 是组件内局部位置；转全局（-1 = 组件根）
            let parent_global: i32 = match tn.parent_idx {
                None => -1,
                Some(p) => (comp_base as usize + p) as i32,
            };
            let (kind_tag, text_idx, src_idx) = {
                let text_idx = tn
                    .content
                    .as_ref()
                    .map(|c| intern(c, &mut strings, &mut idx_of))
                    .unwrap_or(NULL_IDX);
                let src_idx = tn
                    .src
                    .as_ref()
                    .map(|c| intern(c, &mut strings, &mut idx_of))
                    .unwrap_or(NULL_IDX);
                // kind_tag = NodeKind 判别值（repr(u8)），全 25 变体保真。
                let kind_tag = tn.kind as u8;
                match tn.kind {
                    NodeKind::Image => (kind_tag, NULL_IDX, src_idx),
                    NodeKind::TextNode => (kind_tag, text_idx, NULL_IDX),
                    _ => (kind_tag, NULL_IDX, NULL_IDX),
                }
            };
            let style_blob = bincode::serialize(&tn.style).expect("ResolvedStyle serializable");
            // control_init：Option<ControlInit> 整体 bincode（None 为 1B tag，Some 含 variant 载荷）。
            // 打包期 bake 自 HTML 属性；runtime instantiate 读取填 side table。
            let control_init_blob =
                bincode::serialize(&tn.control_init).expect("ControlInit serializable");
            let class_idx: Vec<u16> = tn
                .classes
                .iter()
                .map(|c| intern(c, &mut strings, &mut idx_of))
                .collect();
            let id_idx = tn
                .id_attr
                .as_ref()
                .map(|id| intern(id, &mut strings, &mut idx_of))
                .unwrap_or(NULL_IDX);
            let flags: u8 = (if tn.draggable { 0x01 } else { 0x00 })
                | (if tn.rich_text_block { 0x02 } else { 0x00 })
                | (if tn.component_scope { 0x04 } else { 0x00 });
            let tabindex = tn.tabindex.unwrap_or(i32::MIN);
            // role/data-slot：Option<String> → StringTable 索引（同 id_attr 模式，NULL_IDX 表 None）。
            let role_idx = tn
                .role
                .as_ref()
                .map(|r| intern(r, &mut strings, &mut idx_of))
                .unwrap_or(NULL_IDX);
            let data_slot_idx = tn
                .data_slot
                .as_ref()
                .map(|s| intern(s, &mut strings, &mut idx_of))
                .unwrap_or(NULL_IDX);
            // aria_controls：Option<String> → StringTable 索引（同 role/data_slot 模式，NULL_IDX 表 None）。
            let aria_controls_idx = tn
                .aria_controls
                .as_ref()
                .map(|s| intern(s, &mut strings, &mut idx_of))
                .unwrap_or(NULL_IDX);
            // custom_tag：CustomElement 原始 hyphen 标签（同 role/data_slot/aria_controls 模式）。
            let custom_tag_idx = tn
                .custom_tag
                .as_ref()
                .map(|s| intern(s, &mut strings, &mut idx_of))
                .unwrap_or(NULL_IDX);
            // href（#74）：`<a>` 链接目标（同 role/data_slot 模式，NULL_IDX 表 None）。
            let href_idx = tn
                .href
                .as_ref()
                .map(|s| intern(s, &mut strings, &mut idx_of))
                .unwrap_or(NULL_IDX);
            node_records.push((
                parent_global,
                kind_tag,
                style_blob,
                text_idx,
                src_idx,
                class_idx,
                id_idx,
                flags,
                tabindex,
                control_init_blob,
                role_idx,
                data_slot_idx,
                aria_controls_idx,
                custom_tag_idx,
                href_idx,
            ));
        }
        let node_count = nodes.len() as u32;
        let dynamic_blob =
            bincode::serialize(dynamic_rules).expect("DynamicRuleTable serializable");
        // keyframes blob：手动编码（rule.name / stop.hook 走 StringTable intern，同
        // role/data_slot 模式；其余字段定长 LE 直写）。interning 必须在 header 写前完成。
        let keyframes_blob = encode_keyframes(keyframes, &mut strings, &mut idx_of);
        comp_records.push((
            name_idx,
            comp_base,
            node_count,
            dynamic_blob,
            keyframes_blob,
        ));
        global_node_offset += node_count;
    }

    let mut out: Vec<u8> = Vec::new();
    // Header (20B): magic + version + flags + component_count + string_count
    out.extend_from_slice(&PKG_MAGIC.to_le_bytes());
    out.extend_from_slice(&PKG_FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // flags
    out.extend_from_slice(&(component_count as u32).to_le_bytes());
    out.extend_from_slice(&(strings.len() as u32).to_le_bytes());
    // StringTable
    for s in &strings {
        let bytes = s.as_bytes();
        out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(bytes);
    }
    // ComponentTable: 每组件 {name_idx(u16), root_node_idx(u32), node_count(u32), dynamic_rules_blob_len(u32)}
    for (name_idx, root_node_idx, node_count, dynamic_blob, _) in &comp_records {
        out.extend_from_slice(&name_idx.to_le_bytes());
        out.extend_from_slice(&root_node_idx.to_le_bytes());
        out.extend_from_slice(&node_count.to_le_bytes());
        out.extend_from_slice(&(dynamic_blob.len() as u32).to_le_bytes());
    }
    // NodeBlock: 每节点 {parent_idx(i32), kind_tag(u8), style_len(u32)+style_blob, text_idx(u16), src_idx(u16),
    //   class_count(u16)+class_idx[], id_idx(u16), flags(u8), tabindex(i32), control_init_len(u32)+control_init_blob,
    //   role_idx(u16), data_slot_idx(u16), aria_controls_idx(u16), custom_tag_idx(u16), href_idx(u16)}
    for (
        parent_idx,
        kind_tag,
        style_blob,
        text_idx,
        src_idx,
        class_idx,
        id_idx,
        flags,
        tabindex,
        control_init_blob,
        role_idx,
        data_slot_idx,
        aria_controls_idx,
        custom_tag_idx,
        href_idx,
    ) in &node_records
    {
        out.extend_from_slice(&parent_idx.to_le_bytes());
        out.push(*kind_tag);
        out.extend_from_slice(&(style_blob.len() as u32).to_le_bytes());
        out.extend_from_slice(style_blob);
        out.extend_from_slice(&text_idx.to_le_bytes());
        out.extend_from_slice(&src_idx.to_le_bytes());
        out.extend_from_slice(&(class_idx.len() as u16).to_le_bytes());
        for &cidx in class_idx {
            out.extend_from_slice(&cidx.to_le_bytes());
        }
        out.extend_from_slice(&id_idx.to_le_bytes());
        out.push(*flags);
        out.extend_from_slice(&tabindex.to_le_bytes());
        out.extend_from_slice(&(control_init_blob.len() as u32).to_le_bytes());
        out.extend_from_slice(control_init_blob);
        out.extend_from_slice(&role_idx.to_le_bytes());
        out.extend_from_slice(&data_slot_idx.to_le_bytes());
        out.extend_from_slice(&aria_controls_idx.to_le_bytes());
        out.extend_from_slice(&custom_tag_idx.to_le_bytes());
        out.extend_from_slice(&href_idx.to_le_bytes());
    }
    // PerComponentDynamicRules：每组件 dynamic_blob（同 ComponentTable 顺序）。read 按同序逐组件读。
    for (_, _, _, dynamic_blob, _) in &comp_records {
        out.extend_from_slice(dynamic_blob);
    }
    // PerComponentKeyframes：每组件 keyframes blob（u32 len + blob，同 ComponentTable 顺序）。
    // 紧随 DynamicRules 段（ComponentTable 记录不含此长度，保持记录尺寸 14B 不变）。
    for (_, _, _, _, keyframes_blob) in &comp_records {
        out.extend_from_slice(&(keyframes_blob.len() as u32).to_le_bytes());
        out.extend_from_slice(keyframes_blob);
    }
    // PerComponentScopes：每组件（同 ComponentTable 顺序）{scope_count(u32),
    //   每条 [anchor_idx(u32) + rules_blob_len(u32) + rules_blob]}。组件展开域锚定规则
    //   （Custom Element 打包期展开产物）；无展开域的组件写 count=0（统一布局，无省略歧义）。
    for (name, _, _, _) in &input.components {
        let comp_scopes: Vec<&ComponentScopeInput> =
            scopes.iter().filter(|sc| sc.component == *name).collect();
        out.extend_from_slice(&(comp_scopes.len() as u32).to_le_bytes());
        for sc in comp_scopes {
            out.extend_from_slice(&(sc.anchor_idx as u32).to_le_bytes());
            let blob = bincode::serialize(sc.rules).expect("DynamicRuleTable serializable");
            out.extend_from_slice(&(blob.len() as u32).to_le_bytes());
            out.extend_from_slice(&blob);
        }
    }
    out
}

/// 反序列化 .pkg.bin → Package（多组件格式，含版本协商）。
/// `Package.name` read 时填空串（read 不知包名），由 `Stage::load_package(name, ..)` 覆盖。
pub fn read_package(bytes: &[u8]) -> Result<Package, PkgError> {
    let mut r = Reader::new(bytes);
    // Header (20B)
    let magic = r.u32("magic")?;
    if magic != PKG_MAGIC {
        return Err(PkgError::BadMagic);
    }
    let version = r.u32("version")?;
    if version < MIN_VERSION {
        return Err(PkgError::TooOld(version));
    }
    if version > MAX_VERSION {
        return Err(PkgError::TooNew(version));
    }
    let _flags = r.u32("flags")?;
    let component_count = r.u32("component_count")? as usize;
    let string_count = r.u32("string_count")? as usize;
    // StringTable
    let mut strings: Vec<String> = Vec::with_capacity(string_count);
    for _ in 0..string_count {
        let len = r.u16("str_len")? as usize;
        let s = r.utf8(len, "str_bytes")?;
        strings.push(s);
    }
    // ComponentTable: 每组件 {name_idx(u16), root_node_idx(u32), node_count(u32), dynamic_rules_blob_len(u32)}
    let mut comp_table: Vec<(u16, u32, u32, u32)> = Vec::with_capacity(component_count);
    for _ in 0..component_count {
        let name_idx = r.u16("comp_name_idx")?;
        let root_node_idx = r.u32("comp_root_node_idx")?;
        let node_count = r.u32("comp_node_count")?;
        let dynamic_len = r.u32("comp_dynamic_len")?;
        comp_table.push((name_idx, root_node_idx, node_count, dynamic_len));
    }
    let total_nodes: u32 = comp_table.iter().map(|(_, _, n, _)| *n).sum();
    // NodeBlock → TemplateNode（平铺，parent_idx 存盘是全局位置；读后转回组件内局部）
    let mut all_nodes: Vec<TemplateNode> = Vec::with_capacity(total_nodes as usize);
    for _ in 0..total_nodes {
        let pidx = r.i32("parent_idx")?;
        let kind_tag = r.u8("kind")?;
        let style_len = r.u32("style_len")? as usize;
        let style: ResolvedStyle = bincode::deserialize(r.take(style_len, "style_blob")?)?;
        let text_idx = r.u16("text_idx")?;
        let src_idx = r.u16("src_idx")?;
        let class_count = r.u16("class_count")? as usize;
        let mut classes: Vec<String> = Vec::with_capacity(class_count);
        for _ in 0..class_count {
            let cidx = r.u16("class_idx")?;
            classes.push(string_at(&strings, cidx)?);
        }
        let id_idx = r.u16("id_idx")?;
        let id_attr = if id_idx == NULL_IDX {
            None
        } else {
            Some(string_at(&strings, id_idx)?)
        };
        let flags = r.u8("flags")?;
        let draggable = (flags & 0x01) != 0;
        let rich_text_block = (flags & 0x02) != 0;
        let component_scope = (flags & 0x04) != 0;
        let tab_raw = r.i32("tabindex")?;
        let tabindex = if tab_raw == i32::MIN {
            None
        } else {
            Some(tab_raw)
        };
        let control_init_len = r.u32("control_init_len")? as usize;
        let control_init: Option<ControlInit> =
            bincode::deserialize(r.take(control_init_len, "control_init_blob")?)?;
        // role/data-slot/aria_controls：StringTable 索引（同 id_attr，NULL_IDX 表 None）。
        let role_idx = r.u16("role_idx")?;
        let data_slot_idx = r.u16("data_slot_idx")?;
        let aria_controls_idx = r.u16("aria_controls_idx")?;
        let custom_tag_idx = r.u16("custom_tag_idx")?;
        // href（#74）：`<a>` 链接目标（同 role 模式，NULL_IDX 表 None）。
        let href_idx = r.u16("href_idx")?;
        let role = if role_idx == NULL_IDX {
            None
        } else {
            Some(string_at(&strings, role_idx)?)
        };
        let data_slot = if data_slot_idx == NULL_IDX {
            None
        } else {
            Some(string_at(&strings, data_slot_idx)?)
        };
        let aria_controls = if aria_controls_idx == NULL_IDX {
            None
        } else {
            Some(string_at(&strings, aria_controls_idx)?)
        };
        let custom_tag = if custom_tag_idx == NULL_IDX {
            None
        } else {
            Some(string_at(&strings, custom_tag_idx)?)
        };
        let href = if href_idx == NULL_IDX {
            None
        } else {
            Some(string_at(&strings, href_idx)?)
        };
        // 存盘 parent_idx 是 NodeBlock 全局位置（-1=组件根）；先存全局，待切分组件时减 base 转局部
        let parent_global = if pidx < 0 { None } else { Some(pidx as usize) };
        let (kind, content, src) = match NodeKind::from_u8(kind_tag) {
            Some(NodeKind::Image) => (
                NodeKind::Image,
                None,
                if src_idx == NULL_IDX {
                    None
                } else {
                    Some(string_at(&strings, src_idx)?)
                },
            ),
            Some(NodeKind::TextNode) => (
                NodeKind::TextNode,
                if text_idx == NULL_IDX {
                    None
                } else {
                    Some(string_at(&strings, text_idx)?)
                },
                None,
            ),
            Some(k) => (k, None, None),
            None => return Err(PkgError::BadKind(kind_tag)),
        };
        all_nodes.push(TemplateNode {
            kind,
            style,
            content,
            src,
            href,
            parent_idx: parent_global, // 临时存全局，下方切分时减 base
            classes,
            id_attr,
            draggable,
            tabindex,
            control_init,
            role,
            data_slot,
            aria_controls,
            rich_text_block,
            custom_tag,
            component_scope,
        });
    }
    // PerComponentDynamicRules: 每组件 dynamic_blob（按 ComponentTable 序）
    let mut components: std::collections::HashMap<String, ComponentTemplate> =
        std::collections::HashMap::with_capacity(component_count);
    for (name_idx, root_node_idx, node_count, dynamic_len) in &comp_table {
        let name = string_at(&strings, *name_idx)?;
        let start = *root_node_idx as usize;
        let end = start + *node_count as usize;
        // 防御 malformed ComponentTable：root_node_idx/node_count 越界 → Truncated（避免 slice panic）
        if start > all_nodes.len() || end > all_nodes.len() {
            return Err(PkgError::Truncated("comp_node_slice"));
        }
        let base = start;
        // 组件内 parent_idx：全局 - base（组件根 parent_idx=None 仍是 None）。
        // 防御 malformed：parent_global < base 表示父节点落到更早的组件 → Truncated（不允许跨组件父）
        let mut nodes = all_nodes[start..end].to_vec();
        for tn in nodes.iter_mut() {
            if let Some(p) = tn.parent_idx {
                if p < base {
                    return Err(PkgError::Truncated("cross_comp_parent"));
                }
                tn.parent_idx = Some(p - base);
            }
        }
        let dynamic_rules: DynamicRuleTable =
            bincode::deserialize(r.take(*dynamic_len as usize, "comp_dynamic_blob")?)?;
        // 防御 malformed：同名组件 → DupComponent（避免静默覆盖丢数据）
        if components.contains_key(&name) {
            return Err(PkgError::DupComponent(name));
        }
        components.insert(
            name.clone(),
            ComponentTemplate {
                name,
                nodes,
                dynamic_rules,
                keyframes: Vec::new(), // PerComponentKeyframes 段在下方第二遍读回后填
                component_scopes: Vec::new(), // PerComponentScopes 段在下方第三遍读回后填
            },
        );
    }
    // PerComponentKeyframes: 每组件 keyframes blob（u32 len + blob，同 ComponentTable 序）。
    // 流位置在所有 dynamic blob 之后（write 时紧随 DynamicRules 段），故组件循环后单独读。
    for (name_idx, _, _, _) in &comp_table {
        let name = string_at(&strings, *name_idx)?;
        let kf_len = r.u32("comp_keyframes_len")? as usize;
        let kf_bytes = r.take(kf_len, "comp_keyframes_blob")?;
        let keyframes = decode_keyframes(kf_bytes, &strings)?;
        // 组件已在上方循环插入（同名唯一性已由 DupComponent 保证）；if-let 防御不可达态。
        if let Some(ct) = components.get_mut(&name) {
            ct.keyframes = keyframes;
        }
    }
    // PerComponentScopes: 每组件（同 ComponentTable 序）{scope_count(u32),
    //   每条 [anchor_idx(u32) + rules_blob_len(u32) + rules_blob]}。
    // anchor_idx 是组件内局部 TemplateNode idx（instantiate 的 id_map 直接可用）。
    for (name_idx, _, node_count, _) in &comp_table {
        let name = string_at(&strings, *name_idx)?;
        let scope_count = r.u32("comp_scope_count")? as usize;
        let mut scopes = Vec::with_capacity(scope_count);
        for _ in 0..scope_count {
            let anchor_idx = r.u32("comp_scope_anchor")? as usize;
            let blob_len = r.u32("comp_scope_blob_len")? as usize;
            let rules: DynamicRuleTable =
                bincode::deserialize(r.take(blob_len, "comp_scope_blob")?)?;
            // 防御 malformed：anchor 越界 → Truncated（避免 instantiate 索引 panic）。
            if anchor_idx >= *node_count as usize {
                return Err(PkgError::Truncated("comp_scope_anchor_oob"));
            }
            scopes.push((anchor_idx, rules));
        }
        if let Some(ct) = components.get_mut(&name) {
            ct.component_scopes = scopes;
        }
    }
    Ok(Package {
        name: String::new(),
        components,
    })
}

fn string_at(strings: &[String], idx: u16) -> Result<String, PkgError> {
    if idx == NULL_IDX {
        return Ok(String::new());
    }
    strings
        .get(idx as usize)
        .cloned()
        .ok_or(PkgError::OobString(idx))
}

/// 把字符串 intern 进 stringTable（首次出现分配新索引，重复返回既有索引）。
fn intern(
    s: &str,
    strings: &mut Vec<String>,
    idx_of: &mut std::collections::HashMap<String, u16>,
) -> u16 {
    if let Some(&i) = idx_of.get(s) {
        return i;
    }
    // u16 索引 + NULL_IDX(0xFFFF) 哨兵：真实索引只能 0..65534。下一个串的索引
    // 若 = NULL_IDX 会读回空串、若回绕到 0 会撞首串——均静默 corrupt。打包期直接 panic。
    if strings.len() >= NULL_IDX as usize {
        panic!(
            "string table overflow: StringTable holds {} distinct strings (u16 index, \
             NULL_IDX=0xFFFF reserved); component/text/src/class/id/manifest/keyframes share this table",
            strings.len()
        );
    }
    let i = strings.len() as u16;
    strings.push(s.to_string());
    idx_of.insert(s.to_string(), i);
    i
}

/// 手动编码 Ease（v43）：tag(u8) [+载荷]。tag 与 ease_ffi kind 同值域（0..9 keyword /
/// 10 StepEnd / 11 StepStart / 12 CubicBezier+4×f32 / 13..18 elastic/bounce）——单一
/// 数值契约两处消费（pkg 手编 + FFI spec struct），防两套判别值漂移。
fn encode_ease(out: &mut Vec<u8>, e: Ease) {
    use crate::tween::ease_ffi as k;
    let tag: u8 = match e {
        Ease::Linear => k::LINEAR as u8,
        Ease::QuadIn => k::QUAD_IN as u8,
        Ease::QuadOut => k::QUAD_OUT as u8,
        Ease::QuadInOut => k::QUAD_IN_OUT as u8,
        Ease::CubicIn => k::CUBIC_IN as u8,
        Ease::CubicOut => k::CUBIC_OUT as u8,
        Ease::CubicInOut => k::CUBIC_IN_OUT as u8,
        Ease::BackIn => k::BACK_IN as u8,
        Ease::BackOut => k::BACK_OUT as u8,
        Ease::BackInOut => k::BACK_IN_OUT as u8,
        Ease::Step { start: false } => k::STEP_END as u8,
        Ease::Step { start: true } => k::STEP_START as u8,
        Ease::ElasticIn => k::ELASTIC_IN as u8,
        Ease::ElasticOut => k::ELASTIC_OUT as u8,
        Ease::ElasticInOut => k::ELASTIC_IN_OUT as u8,
        Ease::BounceIn => k::BOUNCE_IN as u8,
        Ease::BounceOut => k::BOUNCE_OUT as u8,
        Ease::BounceInOut => k::BOUNCE_IN_OUT as u8,
        Ease::CubicBezier { .. } => k::CUBIC_BEZIER as u8,
    };
    out.push(tag);
    // 参数槽定长 4×f32（单形解码免按 tag 分支跳读；非 bezier 恒零）。
    let params: [f32; 4] = match e {
        Ease::CubicBezier { x1, y1, x2, y2 } => [x1, y1, x2, y2],
        _ => [0.0; 4],
    };
    for v in params {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

/// 手动解码 Ease（encode_ease 的逆）。未知 tag → BadEaseTag。
fn decode_ease(r: &mut Reader<'_>) -> Result<Ease, PkgError> {
    let tag = r.u8("ease_tag")?;
    let params = [
        r.f32("ease_p0")?,
        r.f32("ease_p1")?,
        r.f32("ease_p2")?,
        r.f32("ease_p3")?,
    ];
    // 参数槽定长 4×f32（tag=12 是 bezier 控制点，其余恒零——编码/解码严格对称）。
    ease_from_ffi(u32::from(tag), params).ok_or(PkgError::BadEaseTag(tag))
}

/// 手动编码组件 keyframes 表（v30 pkg 格式）。
/// 布局：u16 rule_count + 逐 rule { u16 name_idx, u16 stop_count, 逐 stop }。
/// stop 布局：selector_tag(u8: 0=From/1=To/2=Percent) [+pct(u8)] + 4 个可动画字段
/// （每字段 flag(u8)+载荷；transform 内部 TRS 三分量各自 flag）+ hook_idx(u16)。
/// rule.name / stop.hook 走 StringTable intern（同 role/data_slot 模式，NULL_IDX 表 None）。
fn encode_keyframes(
    keyframes: &[KeyframesRule],
    strings: &mut Vec<String>,
    idx_of: &mut std::collections::HashMap<String, u16>,
) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&(keyframes.len() as u16).to_le_bytes());
    for rule in keyframes {
        let name_idx = intern(&rule.name, strings, idx_of);
        out.extend_from_slice(&name_idx.to_le_bytes());
        out.extend_from_slice(&(rule.stops.len() as u16).to_le_bytes());
        for stop in &rule.stops {
            // KeyframeStopSelector 带数据变体，与 #[repr(u8)] 不兼容 → 手动 match 判别值。
            match stop.selector {
                KeyframeStopSelector::From => out.push(0),
                KeyframeStopSelector::To => out.push(1),
                KeyframeStopSelector::Percent(pct) => {
                    out.push(2);
                    out.push(pct);
                }
            }
            // 每个可动画字段：flag(u8) + 载荷（flag=0 即 None，无载荷）。
            match stop.props.opacity {
                Some(v) => {
                    out.push(1);
                    out.extend_from_slice(&v.to_le_bytes());
                }
                None => out.push(0),
            }
            match stop.props.transform {
                Some(t) => {
                    out.push(1);
                    match t.translate {
                        Some([x, y]) => {
                            // v43：LenPct = (px, pct) 混合长度（#77 百分比形）
                            out.push(1);
                            out.extend_from_slice(&x.px.to_le_bytes());
                            out.extend_from_slice(&x.pct.to_le_bytes());
                            out.extend_from_slice(&y.px.to_le_bytes());
                            out.extend_from_slice(&y.pct.to_le_bytes());
                        }
                        None => out.push(0),
                    }
                    match t.scale {
                        Some([x, y]) => {
                            out.push(1);
                            out.extend_from_slice(&x.to_le_bytes());
                            out.extend_from_slice(&y.to_le_bytes());
                        }
                        None => out.push(0),
                    }
                    match t.rotate {
                        Some(r) => {
                            out.push(1);
                            out.extend_from_slice(&r.to_le_bytes());
                        }
                        None => out.push(0),
                    }
                }
                None => out.push(0),
            }
            match stop.props.bg_color {
                Some(c) => {
                    out.push(1);
                    for v in c {
                        out.extend_from_slice(&v.to_le_bytes());
                    }
                }
                None => out.push(0),
            }
            match stop.props.text_color {
                Some(c) => {
                    out.push(1);
                    for v in c {
                        out.extend_from_slice(&v.to_le_bytes());
                    }
                }
                None => out.push(0),
            }
            // v44：layout/box-shadow 通道（#10）。width/height = 域(u8)+值(f32)；
            // flex_grow = 标量；box_shadow = 层数(u8) + 逐层 [ox,oy,spread,blur,color4,inset]。
            for len in [stop.props.width, stop.props.height] {
                match len {
                    Some(l) => {
                        out.push(1);
                        out.push(l.domain as u8);
                        out.extend_from_slice(&l.value.to_le_bytes());
                    }
                    None => out.push(0),
                }
            }
            match stop.props.flex_grow {
                Some(v) => {
                    out.push(1);
                    out.extend_from_slice(&v.to_le_bytes());
                }
                None => out.push(0),
            }
            match &stop.props.box_shadow {
                Some(list) => {
                    out.push(1);
                    out.push(list.len() as u8);
                    for s in list {
                        out.extend_from_slice(&s.ox.to_le_bytes());
                        out.extend_from_slice(&s.oy.to_le_bytes());
                        out.extend_from_slice(&s.spread.to_le_bytes());
                        out.extend_from_slice(&s.blur.to_le_bytes());
                        for v in s.color {
                            out.extend_from_slice(&v.to_le_bytes());
                        }
                        out.push(u8::from(s.inset));
                    }
                }
                None => out.push(0),
            }
            // v43：per-stop timing（None = 单字节 0；Some = 1 + encode_ease 载荷）
            match stop.timing {
                None => out.push(0),
                Some(e) => {
                    out.push(1);
                    encode_ease(&mut out, e);
                }
            }
            let hook_idx = stop
                .hook
                .as_ref()
                .map(|h| intern(h, strings, idx_of))
                .unwrap_or(NULL_IDX);
            out.extend_from_slice(&hook_idx.to_le_bytes());
        }
    }
    out
}

/// 手动解码组件 keyframes blob（encode_keyframes 的逆）。name/hook 索引经 StringTable 解析。
fn decode_keyframes(bytes: &[u8], strings: &[String]) -> Result<Vec<KeyframesRule>, PkgError> {
    let mut r = Reader::new(bytes);
    let rule_count = r.u16("kf_rule_count")? as usize;
    let mut rules: Vec<KeyframesRule> = Vec::with_capacity(rule_count);
    for _ in 0..rule_count {
        let name = string_at(strings, r.u16("kf_name_idx")?)?;
        let stop_count = r.u16("kf_stop_count")? as usize;
        let mut stops: Vec<KeyframeStop> = Vec::with_capacity(stop_count);
        for _ in 0..stop_count {
            let selector = match r.u8("kf_selector_tag")? {
                0 => KeyframeStopSelector::From,
                1 => KeyframeStopSelector::To,
                2 => KeyframeStopSelector::Percent(r.u8("kf_selector_pct")?),
                t => return Err(PkgError::BadKeyframeSelector(t)),
            };
            // 每个可动画字段：flag(u8) + 载荷（flag=0 即 None）。
            let opacity = if r.u8("kf_opacity_flag")? != 0 {
                Some(r.f32("kf_opacity")?)
            } else {
                None
            };
            let transform = if r.u8("kf_transform_flag")? != 0 {
                let translate = if r.u8("kf_translate_flag")? != 0 {
                    // v43：LenPct = (px, pct) × 2（x 后 y，各自 px 前 pct 后）
                    Some([
                        crate::transform::LenPct {
                            px: r.f32("kf_translate_x_px")?,
                            pct: r.f32("kf_translate_x_pct")?,
                        },
                        crate::transform::LenPct {
                            px: r.f32("kf_translate_y_px")?,
                            pct: r.f32("kf_translate_y_pct")?,
                        },
                    ])
                } else {
                    None
                };
                let scale = if r.u8("kf_scale_flag")? != 0 {
                    Some([r.f32("kf_scale_x")?, r.f32("kf_scale_y")?])
                } else {
                    None
                };
                let rotate = if r.u8("kf_rotate_flag")? != 0 {
                    Some(r.f32("kf_rotate")?)
                } else {
                    None
                };
                Some(TransformAnim {
                    translate,
                    scale,
                    rotate,
                })
            } else {
                None
            };
            let bg_color = if r.u8("kf_bg_color_flag")? != 0 {
                Some([
                    r.f32("kf_bg_color_0")?,
                    r.f32("kf_bg_color_1")?,
                    r.f32("kf_bg_color_2")?,
                    r.f32("kf_bg_color_3")?,
                ])
            } else {
                None
            };
            let text_color = if r.u8("kf_text_color_flag")? != 0 {
                Some([
                    r.f32("kf_text_color_0")?,
                    r.f32("kf_text_color_1")?,
                    r.f32("kf_text_color_2")?,
                    r.f32("kf_text_color_3")?,
                ])
            } else {
                None
            };
            // v44：layout/box-shadow 通道（encode 侧同序）。
            fn anim_len(r: &mut Reader) -> Result<Option<crate::scene::AnimLen>, PkgError> {
                if r.u8("kf_len_flag")? == 0 {
                    return Ok(None);
                }
                let domain = match r.u8("kf_len_domain")? {
                    0 => crate::scene::LenDomain::Px,
                    1 => crate::scene::LenDomain::Pct,
                    2 => crate::scene::LenDomain::Vw,
                    3 => crate::scene::LenDomain::Vh,
                    4 => crate::scene::LenDomain::Vmin,
                    5 => crate::scene::LenDomain::Vmax,
                    d => return Err(PkgError::BadLenDomain(d)),
                };
                Ok(Some(crate::scene::AnimLen {
                    domain,
                    value: r.f32("kf_len_value")?,
                }))
            }
            let width = anim_len(&mut r)?;
            let height = anim_len(&mut r)?;
            let flex_grow = if r.u8("kf_flex_grow_flag")? != 0 {
                Some(r.f32("kf_flex_grow")?)
            } else {
                None
            };
            let box_shadow = if r.u8("kf_box_shadow_flag")? != 0 {
                let count = r.u8("kf_box_shadow_count")? as usize;
                let mut list = Vec::with_capacity(count);
                for _ in 0..count {
                    list.push(crate::style::resolved::BoxShadow {
                        ox: r.f32("kf_shadow_ox")?,
                        oy: r.f32("kf_shadow_oy")?,
                        spread: r.f32("kf_shadow_spread")?,
                        blur: r.f32("kf_shadow_blur")?,
                        color: [
                            r.f32("kf_shadow_r")?,
                            r.f32("kf_shadow_g")?,
                            r.f32("kf_shadow_b")?,
                            r.f32("kf_shadow_a")?,
                        ],
                        inset: r.u8("kf_shadow_inset")? != 0,
                    });
                }
                Some(list)
            } else {
                None
            };
            let timing = if r.u8("kf_timing_flag")? != 0 {
                Some(decode_ease(&mut r)?)
            } else {
                None
            };
            let hook_idx = r.u16("kf_hook_idx")?;
            let hook = if hook_idx == NULL_IDX {
                None
            } else {
                Some(string_at(strings, hook_idx)?)
            };
            stops.push(KeyframeStop {
                selector,
                props: AnimatableProps {
                    opacity,
                    transform,
                    bg_color,
                    text_color,
                    width,
                    height,
                    flex_grow,
                    box_shadow,
                },
                timing,
                hook,
            });
        }
        rules.push(KeyframesRule { name, stops });
    }
    Ok(rules)
}

/// 极简游标 reader：定长小端读取 + 截断保护。
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }
    fn need(&mut self, n: usize, ctx: &'static str) -> Result<&'a [u8], PkgError> {
        if self.pos + n > self.buf.len() {
            return Err(PkgError::Truncated(ctx));
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self, ctx: &'static str) -> Result<u8, PkgError> {
        Ok(self.need(1, ctx)?[0])
    }
    fn u16(&mut self, ctx: &'static str) -> Result<u16, PkgError> {
        Ok(u16::from_le_bytes(self.need(2, ctx)?.try_into().unwrap()))
    }
    fn u32(&mut self, ctx: &'static str) -> Result<u32, PkgError> {
        Ok(u32::from_le_bytes(self.need(4, ctx)?.try_into().unwrap()))
    }
    fn i32(&mut self, ctx: &'static str) -> Result<i32, PkgError> {
        Ok(i32::from_le_bytes(self.need(4, ctx)?.try_into().unwrap()))
    }
    fn f32(&mut self, ctx: &'static str) -> Result<f32, PkgError> {
        Ok(f32::from_le_bytes(self.need(4, ctx)?.try_into().unwrap()))
    }
    fn take(&mut self, n: usize, ctx: &'static str) -> Result<&'a [u8], PkgError> {
        self.need(n, ctx)
    }
    fn utf8(&mut self, n: usize, ctx: &'static str) -> Result<String, PkgError> {
        let s = self.need(n, ctx)?;
        std::str::from_utf8(s)
            .map(String::from)
            .map_err(|_| PkgError::Truncated(ctx))
    }
}

#[cfg(test)]
mod tests;
