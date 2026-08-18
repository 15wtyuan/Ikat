//! Scene 层：持久 Node 树（场景图）。
//!
//! Node 树 + 数据结构定义。Scene::build 是建树入口（runtime + 包加载共用）。
//! layout 层后续往 `taffy_id`/`layout_rect` 写几何；render 层消费
//! `clip_rect`/`dirty_*`。本模块只管建树 + 初始脏标志。

use crate::style::dynamic::InlineSet;
use crate::style::resolved::{OverflowMode, ResolvedStyle};
use serde::{Deserialize, Serialize};
use slotmap::{DefaultKey, Key, KeyData, SlotMap};

bitflags::bitflags! {
    /// Pseudo-class source flags + cascade gate, packed into a single byte for cache locality.
    /// Only process + rematch passes touch these; solve/world/build skip entirely.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
    pub struct NodeFlags: u8 {
        const HOVERED  = 1 << 0;
        const ACTIVE   = 1 << 1;
        const FOCUSED  = 1 << 2;
        const DISABLED = 1 << 3;
        const CASCALED = 1 << 4;
        /// **仅** CSS scoped 规则隔离（Shadow DOM 风格，main-design §5.4）：模板实例化根 /
        /// 文档根打此位。rematch 的 scope 校验 + 后代选择器边界停止都读此位。与 `Get<T>`
        /// 查找边界无关（改读 `LOOKUP_SCOPE`）。
        const SCOPE_ROOT = 1 << 5;
        /// `Get<T>` 查找边界（与 CSS 作用域隔离解耦）：模板实例化根 / 文档根 / ListView slot 根打此位。
        /// 当前 `find_by_id_attr` 仍全局首匹配；scoped find（在此边界内停止向下穿透嵌套作用域）
        /// 是未来扩展，slot 根预打此位以备将来。
        /// 与 SCOPE_ROOT 独立：slot 根只打此位（CSS 规则仍按页面根 scope 匹配，页面 CSS 对 item 生效）。
        const LOOKUP_SCOPE = 1 << 6;
        /// 组件展开域 host 自身归属外层 CSS 作用域（Shadow DOM host 语义：host 元素在 light
        /// 域、shadow 树在 host 域）。host 同时打 SCOPE_ROOT（对后代是边界）+ 本位（对自己
        /// 不是边界）——compute_scope_map 对起始节点自身命中 SCOPE_ROOT+本位时跳过自己继续
        /// 向上，后代不受影响。只打在打包期展开的 CustomElement host 上。
        const HOST_IN_PARENT_SCOPE = 1 << 7;
    }
}

/// Interaction state grouped for cache locality — only process + rematch passes read/write.
/// solve/world_transforms/build passes never touch these fields.
#[derive(Debug, Clone, Default)]
pub struct NodeInteraction {
    pub flags: NodeFlags,
    pub touchable: bool,
    pub draggable: bool,
    pub tabindex: Option<i32>,
}

/// 不透明节点句柄。对外 u32（FFI/C# 透明），内部 = 高 20 bit index + 低 12 bit generation。
/// sentinel 0xFFFF_FFFF = INVALID。index 用于并行数组（anim/scroll/world_transforms）索引，
/// gen 由 slotmap 校验悬空。详见动态树 spec §3。
///
/// **与 slotmap 的衔接**（spec §3.2 实现期校准结果）：
/// slotmap 1.1.1 的 `new_key_type!` 生成的 Key 内部是 `KeyData { idx: u32, version: NonZeroU32 }`
/// （两字段均私有，仅 `as_ffi()/from_ffi()` 公开），其完整编码是 64 bit，**无法无损装入 u32**。
/// 而 FFI/C#/FrameBlob/`.pkg.bin` 全程硬约定 `node_id: u32` + sentinel `0xFFFF_FFFF`（spec §3.3、§7）。
/// 故不采用 `new_key_type!` 重定义 NodeId，而是保留 `NodeId(pub u32)`（应用层句柄），scene.nodes 用
/// `SlotMap<DefaultKey, Node>`，由 `Scene::key_for(NodeId)` 经 `KeyData::from_ffi` 桥接到 DefaultKey。
///
/// 位宽 20/12：index 20 bit（~100 万节点上限）+ generation 12 bit（4096 代，slotmap version ≤ 4095
/// 时无损；超过时 `key_for` 重构的 KeyData version 截断 → slotmap.get 安全返 None，符合 spec "4096 代足够"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

impl NodeId {
    /// 无效句柄 sentinel（同 FFI None/0 约定）。
    pub const INVALID: NodeId = NodeId(0xFFFF_FFFF);
    pub fn is_valid(self) -> bool {
        self.0 != 0xFFFF_FFFF
    }
    /// slotmap 槽位号（高 20 bit）。并行数组按此索引。
    pub fn index(self) -> usize {
        (self.0 >> 12) as usize
    }
    /// generation（低 12 bit）。slotmap 内部校验用。
    pub fn gen(self) -> u16 {
        (self.0 & 0xFFF) as u16
    }

    /// 从 slotmap DefaultKey 构造 NodeId（insert 后回填 Node.id / roots 用）。
    /// 编码：index = key.idx（slotmap 槽位号，1..=capacity），gen = key.version 低 12 bit。
    ///
    /// **12-bit gen 截断是硬约束**：单槽复用超过 ~4096 次后版本回卷，产生活着的
    /// 「幽灵死节点」（节点 id 字段是回卷值，与槽位真实版本不符，get(id) 永久 miss）。
    /// 超限时显式 panic（非 debug_assert——release 也要炸，静默数据腐坏比崩溃更糟）。
    /// 高频改写文本须走 TextNode.Text 就地 set_text（C# TextContent 快路径），
    /// 勿每帧清子重建烧 generation；根治需 NodeId 拓宽 u64 ABI（roadmap）。
    pub fn from_key(k: DefaultKey) -> NodeId {
        let ffi = k.data().as_ffi();
        let idx = (ffi & 0xFFFF_FFFF) as u32;
        let version = (ffi >> 32) as u32;
        if version > 0xFFF {
            panic!(
                "NodeId generation overflow: slot {idx} reused past 12-bit capacity \
                 (version {version}) — id aliasing would corrupt the scene. \
                 Reduce per-frame node churn (use TextNode.Text instead of rebuild), \
                 or widen NodeId ABI (roadmap)."
            );
        }
        NodeId((idx << 12) | (version & 0xFFF))
    }

    /// 重构 slotmap DefaultKey（Scene::get/get_mut 经此桥接）。
    /// slotmap KeyData::from_ffi 强制 version 奇数（与 slotmap 内部一致）。
    pub fn to_key(self) -> DefaultKey {
        let idx = (self.0 >> 12) as u64;
        let version = (self.0 & 0xFFF) as u64;
        DefaultKey::from(KeyData::from_ffi((version << 32) | idx))
    }
}

/// 默认 `Container`（无数据变体），render 层测试构造 Node 用 `Default::default()`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum NodeKind {
    #[default]
    Container,
    TextNode,
    TextElement,
    Button,
    Image,
    TextField,
    NumberField,
    Slider,
    Toggle,
    RadioButton,
    TextArea,
    Dropdown,
    OptionItem,
    ProgressBar,
    ListView,
    ListItem,
    Slot,
    CustomElement,
    /// `<template>` — ListView item blueprint. Forced display:none: never laid out,
    /// rendered, hit-tested or cascaded. Lives in the pkg so the runtime can clone
    /// its subtree to produce list slots.
    Template,
    /// WAI-ARIA `role="tablist"` — tab 容器。ControlState::TabList{selected_index}。
    /// 子节点是 role=tab；panel 跨树靠 aria-controls 关联（RoleInfo.aria_controls）。
    TabList,
    /// WAI-ARIA `role="tab"` — 单个 tab。无 ControlState，aria-selected 从父 TabList.selected_index 派生。
    /// 容器型（持 label 子节点），镜像 Button。
    Tab,
}

impl NodeKind {
    /// u8 判别值 → NodeKind（pkg.bin kind_tag read 用）。越界返 None。
    /// 变体只追加到 enum 末尾，保持既有判别值稳定（pkg 版本门拒旧包，故判别值重排不破跨版本）。
    pub fn from_u8(b: u8) -> Option<NodeKind> {
        match b {
            0 => Some(NodeKind::Container),
            1 => Some(NodeKind::TextNode),
            2 => Some(NodeKind::TextElement),
            3 => Some(NodeKind::Button),
            4 => Some(NodeKind::Image),
            5 => Some(NodeKind::TextField),
            6 => Some(NodeKind::NumberField),
            7 => Some(NodeKind::Slider),
            8 => Some(NodeKind::Toggle),
            9 => Some(NodeKind::RadioButton),
            10 => Some(NodeKind::TextArea),
            11 => Some(NodeKind::Dropdown),
            12 => Some(NodeKind::OptionItem),
            13 => Some(NodeKind::ProgressBar),
            14 => Some(NodeKind::ListView),
            15 => Some(NodeKind::ListItem),
            16 => Some(NodeKind::Slot),
            17 => Some(NodeKind::CustomElement),
            18 => Some(NodeKind::Template),
            19 => Some(NodeKind::TabList),
            20 => Some(NodeKind::Tab),
            _ => None,
        }
    }

    /// Container content model: user-arrangeable children (div/button/span/ul/li/...).
    /// Single source of truth for container vs leaf classification — adding a new
    /// container variant only requires changing this method.
    pub fn is_container(self) -> bool {
        matches!(
            self,
            Self::Container
                | Self::TextElement
                | Self::Button
                | Self::ListView
                | Self::ListItem
                | Self::Slot
                | Self::CustomElement
                | Self::TabList
                | Self::Tab
        )
    }

    /// Leaf: private internal structure, no user-arrangeable children.
    pub fn is_leaf(self) -> bool {
        !self.is_container()
    }

    /// Semantic alias for is_container — "has children in content model".
    pub fn has_children(self) -> bool {
        self.is_container()
    }
}

/// 编译期穷尽 guard：给 NodeKind 加变体却忘更新 `NodeKind::from_u8` 的 match，会让新变体
/// 静默映射到 None（→ `PkgError::BadKind` 反序列化失败）。此未被调用的 fn 内的穷尽 match
/// 强制加变体时同步触碰 from_u8，把静默运行时失败转成编译错。
const _: () = {
    fn _assert_from_u8_exhaustive(k: NodeKind) {
        match k {
            NodeKind::Container
            | NodeKind::TextNode
            | NodeKind::TextElement
            | NodeKind::Button
            | NodeKind::Image
            | NodeKind::TextField
            | NodeKind::NumberField
            | NodeKind::Slider
            | NodeKind::Toggle
            | NodeKind::RadioButton
            | NodeKind::TextArea
            | NodeKind::Dropdown
            | NodeKind::OptionItem
            | NodeKind::ProgressBar
            | NodeKind::ListView
            | NodeKind::ListItem
            | NodeKind::Slot
            | NodeKind::CustomElement
            | NodeKind::Template
            | NodeKind::TabList
            | NodeKind::Tab => {}
        }
    }
};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub kind: NodeKind,
    pub style: ResolvedStyle,
    /// taffy 节点 id（layout 层建立映射后填）。
    pub taffy_id: Option<taffy::NodeId>,
    /// taffy solve 后写（父坐标系）。
    pub layout_rect: Rect,
    /// overflow:hidden 时为本节点 border 框（Some 占位，值由 layout/render 填）。
    pub clip_rect: Option<Rect>,
    /// 仅 Container/Button 有（Text/Image 为叶子）。
    pub children: Vec<NodeId>,
    pub dirty_mesh: bool,
    pub dirty_text: bool,
    /// 打包期烘焙的 style（不变，rematch 基线）。style 是运行时 rematch 覆写值。
    pub base_style: ResolvedStyle,
    /// 运行时 class 列表（建树时从 ElementData.classes 填；供动态规则 class 选择器匹配）。
    pub classes: Vec<String>,
    /// 运行时 id（建树时从 ElementData.id 填；供动态规则 id 选择器匹配）。
    pub id_attr: Option<String>,
    /// CustomElement 的原始 hyphen 标签（`<game-item-card>` → "game-item-card"）。
    /// tag 选择器 rematch 匹配 + dump 发射用；非 CustomElement 恒 None。
    pub custom_tag: Option<String>,
    pub interaction: NodeInteraction,
    pub reuse_key: u32,
    /// 运行时 inline override（便签层）。C# Style.X=v 经 set_inline_override 写入；
    /// rematch 在动态规则后应用（最高优先级）。默认空 = 无 inline override。
    /// 纯运行时 transient，不进 pkg.bin（设计期无 inline override 概念）。
    pub inline_override: ResolvedStyle,
    /// inline_override 里哪些字段被设了（继承属性复用 INH_* bit，非继承用 INLINE_*）。
    /// 默认 0 = 无任何 inline override。rematch 据此把 inline_override 字段拷进 style。
    pub inline_set: InlineSet,
    /// 用户态 Transform（public-api Transform API 的 core 端存储）。解耦的 TRS 三元组，
    /// `compute_world_transforms` 在世界矩阵累计时并入，**不触发 layout solve**（与 CSS
    /// transform 同层：渲染/命中层）。default = identity。供高频拖拽（slider thumb）等
    /// 运行时定位用。纯运行时 transient，不进 pkg.bin。
    pub user_transform: crate::transform::NodeTransform,
    /// rich-text-block 容器根标记（同 TemplateNode.rich_text_block）：instantiate 时从
    /// 模板烘入。solve/render 读此 flag 走 inline flow（拍平 inline 子成 RichRun）。
    /// 非 rich-text-block 容器根 / TextNode / 叶子节点永远 false。solve/render 读写 →
    /// 必须是 Node 字段，不能是 NodeFlag bit（NodeFlag 只被 process/rematch 触碰）。
    pub rich_text_block: bool,
}

impl Default for Node {
    /// render 层 batch 测试构造占位 Node 用。
    /// id/parent/children 取空值，kind=Container（NodeKind::default），
    /// style 取 ResolvedStyle::default，layout_rect/clip_rect 取空。
    fn default() -> Self {
        Node {
            id: NodeId(0),
            parent: None,
            kind: NodeKind::default(),
            style: ResolvedStyle::default(),
            taffy_id: None,
            layout_rect: Rect::default(),
            clip_rect: None,
            children: Vec::new(),
            dirty_mesh: true,
            dirty_text: false,
            base_style: ResolvedStyle::default(),
            classes: Vec::new(),
            id_attr: None,
            custom_tag: None,
            interaction: NodeInteraction {
                flags: NodeFlags::empty(),
                touchable: true,
                draggable: false,
                tabindex: None,
            },
            reuse_key: 0,
            inline_override: ResolvedStyle::default(),
            inline_set: InlineSet(0),
            user_transform: crate::transform::NodeTransform::default(),
            rich_text_block: false,
        }
    }
}

/// 单节点动画 override（replace-override：Some 覆盖 ResolvedStyle 对应字段，None 退回 CSS）。
/// 全 None = 无动画。由 TweenManager.update 写，由 compute_world_transforms / build_render_nodes 读。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct NodeAnim {
    pub opacity: Option<f32>,
    pub transform: Option<crate::transform::Affine2>, // 覆盖 style.transform.matrix
    pub bg_color: Option<[f32; 4]>,
    pub text_color: Option<[f32; 4]>,
}

impl NodeAnim {
    pub fn is_empty(&self) -> bool {
        self.opacity.is_none()
            && self.transform.is_none()
            && self.bg_color.is_none()
            && self.text_color.is_none()
    }
}

/// 每节点动画 override 表（HashMap<NodeId, NodeAnim>）。运行时态，不进 pkg（同 world_transforms）。
///
/// **为何用 HashMap 而非 SecondaryMap**：slotmap 1.1 的 `Key` 是
/// `unsafe` trait（依赖 `KeyData` 内部不变量，slotmap 强烈建议用 `new_key_type!` 而非手 impl）；
/// 且 `KeyData` 内部是 `idx:u32 + version:NonZeroU32`（64 bit），与 NodeId 的 32 bit 应用句柄
/// 布局不匹配——手 `unsafe impl Key` 要把 20/12 编码强行映射到 32/32，语义错位比 HashMap 危险。
/// 故 `SecondaryMap<NodeId, _>` 不可行。DefaultKey 桥接使 anim/scroll 的访问句柄是 NodeId，
/// 若用 `SecondaryMap<DefaultKey, _>` 则每次访问要 `NodeId::to_key()` 转换，且 SecondaryMap 不
/// 自动跟踪主 SlotMap 的删除（删节点须手动 remove，否则残留）。改用 `HashMap<NodeId, NodeAnim>`：
/// NodeId 已 derive Hash+Eq，零 trait 限制、零转换、悬空安全（删节点联动 remove，否则
/// 残留条目但 get 用 live NodeId 查不到）。查询 O(1) HashMap，u32 hash 快，节点数千量级可接受。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnimTable(pub std::collections::HashMap<NodeId, NodeAnim>);

impl AnimTable {
    pub fn get(&self, node: NodeId) -> Option<&NodeAnim> {
        self.0.get(&node).filter(|a| !a.is_empty())
    }

    /// 确保该节点有 anim 槽并返回可变引用（update 调）。
    pub fn ensure(&mut self, node: NodeId) -> &mut NodeAnim {
        self.0.entry(node).or_default()
    }

    /// 清该节点所有通道（回 CSS）= remove。
    pub fn clear_node(&mut self, node: NodeId) {
        self.0.remove(&node);
    }

    /// 清该节点某 prop 对应通道（Translate/Scale/Rotation 都映射到 transform 通道）。
    pub fn clear_prop(&mut self, node: NodeId, prop: crate::tween::TweenProp) {
        let a = match self.0.get_mut(&node) {
            Some(a) => a,
            None => return,
        };
        use crate::tween::TweenProp;
        match prop {
            TweenProp::Opacity => a.opacity = None,
            TweenProp::Translate | TweenProp::Scale | TweenProp::Rotation => a.transform = None,
            TweenProp::BgColor => a.bg_color = None,
            TweenProp::TextColor => a.text_color = None,
        }
    }
}

/// 输入法组合态（composing text）。`text` 是预提交字符串，`pos` 是该串在 `EditState.value`
/// 中的插入字节偏移。预提交期间光标在 composition 内移动，IME 提交后并入 value。
#[derive(Debug, Clone, PartialEq)]
pub struct Composition {
    pub text: String,
    pub pos: usize,
}

/// 文本输入控件运行时状态（TextField / TextArea 共享）。cursor/ anchor 以字节偏移
/// 计量，二者构成的区间（闭区间：`selection_range()`）可退化为零宽光标。
///
/// 所有字节偏移必须落在合法 UTF-8 边界上——`from_init` 中 cursor/anchor 初始化为
/// `value.len()`（末尾后第 0 字节），后续所有写入/选区操作由调用方保证边界正确。
#[derive(Debug, Clone, PartialEq)]
pub struct EditState {
    pub value: String,
    /// 占位文本：value 为空时渲染此文本代替。来自 HTML `placeholder` 属性，
    /// 打包期 bake 进 EditInit，instantiate 传入运行时 EditState。
    pub placeholder: String,
    pub cursor: usize, // [0, value.len()]
    pub anchor: usize, // 选区锚；选区 = [min(anchor,cursor), max]
    pub composition: Option<Composition>,
    pub max_length: usize, // 0 = 无限（按 UTF-8 字符数）
    pub readonly: bool,
    pub cursor_visible: bool,
    pub cursor_timer: f32,
    pub ideal_cursor_x: f32, // 上下行 sticky x（TextArea 用）
}

impl EditState {
    /// 从打包期 `EditInit` 构造运行时 `EditState`。`cursor`/`anchor` 初始设在
    /// value 末尾（光标在文字最后），composition 初始 None，视觉标记皆默认值。
    pub fn from_init(
        value: String,
        placeholder: String,
        max_length: usize,
        readonly: bool,
    ) -> Self {
        let cursor = value.len();
        Self {
            value,
            placeholder,
            cursor,
            anchor: cursor,
            composition: None,
            max_length,
            readonly,
            cursor_visible: true,
            cursor_timer: 0.0,
            ideal_cursor_x: 0.0,
        }
    }

    /// 返回 (start, end) 闭区间字节偏移，start ≤ end。退化为零宽时 start == end。
    pub fn selection_range(&self) -> (usize, usize) {
        if self.anchor <= self.cursor {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        }
    }
}

/// 控件运行时状态（按 NodeKind 分派）。与 `ControlInit`（打包期 pkg.bin 载荷）
/// 一一对应，区别仅在 Slider 多一个 `dragging: bool`——拖拽中间态只在运行时存在，
/// 不进 pkg（松手即丢）。instantiate 时由 `ControlInit` 映射填入 `ControlTable`。
#[derive(Debug, Clone, PartialEq)]
pub enum ControlState {
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
        /// 拖拽中间态（按下→松手期间 true）。运行时独有，不进 pkg，故不在 `ControlInit`。
        dragging: bool,
    },
    /// 单行文本输入（TextField）。EditState 含 value/cursor/anchor 等运行时文字编辑态。
    TextField(EditState),
    /// 多行文本输入（TextArea）。EditState 含 value/cursor/anchor 等运行时文字编辑态。
    TextArea(EditState),
    /// `<select>` 下拉。selected_index=当前选中项（键盘 Up/Down 直接移动它作高亮，不另存高亮态）；
    /// open=popup 是否展开；value_lock 防反馈环；open_selected_index=展开时刻的 selected_index
    /// 快照（Esc 回滚用，仅 open 期间 Some，收起时清 None）。open/open_selected_index 是运行时态，
    /// 不进 pkg（ControlInit::Dropdown 载 selected_index + option_values 静态配置）。
    /// option_values=逐项 `value` 属性（instantiate 拷入，运行时只读；缺席项 None → 回落文本）。
    Dropdown {
        selected_index: usize,
        open: bool,
        value_lock: bool,
        open_selected_index: Option<usize>,
        option_values: Vec<Option<String>>,
    },
    /// `<input type="number">`。edit 复用 EditState（value 是数字的文本形式）；
    /// min/max/step 是数值约束，读写门做 clamp + 量化。
    NumberField {
        edit: EditState,
        min: f32,
        max: f32,
        step: f32,
    },
    /// WAI-ARIA `role="tablist"`。selected_index=当前激活 tab 序号（aria-selected 不存储于
    /// 各 Tab 子节点，由 synth_aria_value 从父 selected_index 派生，见 T5）。无 value_lock
    /// （aria-selected 是只读合成，无回写环——区别于 Dropdown）。panel 显隐由 T6 据
    /// RoleInfo.aria_controls 解析 panel id 后切换，不在此枚举存 panel_ids。
    TabList {
        selected_index: usize,
    },
}

/// 每节点控件状态表（`HashMap<NodeId, ControlState>`）。结构与访问约定同 `AnimTable`/
/// `ScrollTable`（见 AnimTable doc：用 HashMap 而非 SecondaryMap 的理由）。instantiate 时填、
/// `remove_node` 时联动清，防悬空 NodeId 残留。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ControlTable(pub std::collections::HashMap<NodeId, ControlState>);

impl ControlTable {
    pub fn get(&self, id: NodeId) -> Option<&ControlState> {
        self.0.get(&id)
    }
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut ControlState> {
        self.0.get_mut(&id)
    }
    /// 写入（或覆盖）该节点的控件状态。instantiate 时由 `ControlInit` 映射调用。
    pub fn ensure(&mut self, id: NodeId, state: ControlState) {
        self.0.insert(id, state);
    }
    /// 遍历所有控件槽（借出 NodeId + &ControlState）。select_radio 同名组互斥查全树用。
    pub fn iter(&self) -> impl Iterator<Item = (NodeId, &ControlState)> {
        self.0.iter().map(|(&k, v)| (k, v))
    }
    /// 删该节点控件槽（`remove_node` 联动调，防悬空 NodeId 残留）。
    pub fn remove(&mut self, id: NodeId) {
        self.0.remove(&id);
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// 节点的 role/data-slot 信息（打包期从 HTML 提取，运行时只读查表）。
/// role 驱动语义分派 + 控件结构定位（find_child_by_role / find_child_by_slot）。
/// 稀疏：只有带 role/data-slot 的节点进表。运行时态，不进 pkg（pkg 的
/// TemplateNode 携带 role/data_slot 字符串，instantiate 时填进此表）。
///
/// 注：aria-* 属性**大多不进 RoleInfo**——决策定为运行时从 ControlState 合成
/// （避免打包期初始值与运行时实时值双源）。aria-multiline 等派发提示在
/// fence 阶段用完即弃，不进 pkg、不进此表。**例外**：`aria-controls`（TabList
/// tab→panel 跨树关联字符串）不是从控件实时状态可派生的量，故作纯数据随模板
/// 迁移：TemplateNode.aria_controls → RoleInfo.aria_controls（instantiate 拷贝）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoleInfo {
    /// WAI-ARIA role（如 "combobox"/"slider"/"textbox"）。None = 普通 div，无控件语义。
    pub role: Option<String>,
    /// data-slot 值（如 "fill"/"thumb"）。ARIA 不覆盖控件内部视觉构造，用 HTML 标准的
    /// data-* 私有扩展机制表达「这是控件的哪个部件」。
    pub slots: std::collections::HashMap<String, String>,
    /// WAI-ARIA `aria-controls`（TabList tab→panel 跨树关联的 panel id 字符串）。
    /// None = 非关联节点。instantiate 从 TemplateNode.aria_controls 拷入；sync_control_visuals
    /// （T6）据此 find_node_by_id 解析 panel 切换显隐。
    pub aria_controls: Option<String>,
}

impl RoleInfo {
    /// 是否有任何 role/slot/aria-controls 信息（无则不入表，保持 RoleTable 稀疏）。
    pub fn is_empty(&self) -> bool {
        self.role.is_none() && self.slots.is_empty() && self.aria_controls.is_none()
    }
}

/// 每节点 role/data-slot 信息表（`HashMap<NodeId, RoleInfo>`）。结构与访问约定同
/// `ControlTable`/`AnimTable`（见 AnimTable doc：用 HashMap 而非 SecondaryMap 的理由）。
/// instantiate 时从 TemplateNode 填、`remove_node` 时联动清，防悬空 NodeId 残留。
#[derive(Debug, Clone, Default)]
pub struct RoleTable(std::collections::HashMap<NodeId, RoleInfo>);

impl RoleTable {
    pub fn get(&self, id: NodeId) -> Option<&RoleInfo> {
        self.0.get(&id)
    }
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut RoleInfo> {
        self.0.get_mut(&id)
    }
    /// 写入该节点的 role/slot 信息。空 info（无 role 无 slot）不入表，保持稀疏。
    pub fn insert(&mut self, id: NodeId, info: RoleInfo) {
        if !info.is_empty() {
            self.0.insert(id, info);
        }
    }
    /// 删该节点 role/slot 槽（`remove_node` 联动调，防悬空 NodeId 残留）。
    pub fn remove(&mut self, id: NodeId) {
        self.0.remove(&id);
    }
    /// 取节点 role 字符串（find_child_by_role 等查询用）。
    pub fn role_of(&self, id: NodeId) -> Option<&str> {
        self.0.get(&id).and_then(|i| i.role.as_deref())
    }
    /// 取节点某 data-slot 值（find_child_by_slot 等查询用）。
    pub fn slot_of(&self, id: NodeId, slot: &str) -> Option<&str> {
        self.0
            .get(&id)
            .and_then(|i| i.slots.get(slot).map(|s| s.as_str()))
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

#[derive(Debug, Clone, Default)]
pub struct Scene {
    pub roots: Vec<NodeId>,
    /// 释放审计（诊断用）：free_node_slot 记 (id, 单调序号)，环形 32 笔。
    /// get_live panic 时按死 id 查释放距离——区分「快照前已死」（id/槽位腐坏）
    /// 与「循环中途死」（存在绕过快照时点的释放路径）。
    pub free_log: std::collections::VecDeque<(NodeId, u64)>,
    /// 释放单调序号（free_log 配套）。
    pub free_seq: u64,
    /// 节点存储。Vec<Node> → SlotMap<DefaultKey, Node>（动态树 spec §4.1）。
    /// 应用层用 NodeId(u32) 句柄（FFI/C# 透明），经 `Scene::key_for`/`NodeId::to_key` 桥接到 DefaultKey。
    ///
    /// **节点生命周期与下方全部 per-node side table 的联动收口在
    /// [`Scene::alloc_node_slot`] / [`Scene::free_node_slot`]**——建/删节点必须经它们，
    /// 新增 per-node 状态表时在两处各加联动，勿在建/删点各自维护。
    pub nodes: SlotMap<DefaultKey, Node>,
    /// 运行时伪类重匹配规则表（带作用域，Shadow DOM 风格）。默认空；instantiate 时填
    /// （模板规则绑定实例根 scope_root），inline 路径空。不进 pkg（pkg 用无 scope 的 DynamicRuleTable）。
    pub dynamic_rules: crate::style::dynamic::ScopedRuleTable,
    /// 当前焦点节点（单一全局，照 fgui Stage.focus）。None=无焦点。
    pub focused_node: Option<NodeId>,
    /// 每节点累计世界矩阵（compute_world_transforms 填）。index = NodeId.index()。运行时态，不进 pkg。
    pub world_transforms: Vec<crate::transform::Affine2>,
    /// 每节点 sort_key 快照（assign_sort_keys 填，merge 前的 DFS 序号）。index = NodeId.index()。
    /// NativeHost FFI 查询用——merge_meshes 后空 div entry 消失，但 sort_keys 快照保留。
    /// 运行时态，不进 pkg。
    pub node_sort_keys: Vec<u32>,
    /// 每节点动画 override（TweenManager.update 填）。index = NodeId.index()。运行时态，不进 pkg。
    pub anim: AnimTable,
    /// 每节点滚动状态（refresh_content_sizes / scroll 物理填）。index = NodeId.index()。运行时态，不进 pkg。
    pub scroll: crate::scroll::ScrollTable,
    /// 每节点 ListView 虚拟化状态（enter_data_driven 填，update_visible 更新）。运行时态，不进 pkg。
    /// side table 模式（同 scroll/anim），不塞进 Node。
    pub lists: crate::list::ListTable,
    /// 每节点控件状态（instantiate 从 `ControlInit` 填、交互改）。运行时态，不进 pkg。
    pub controls: ControlTable,
    /// 每节点控件初值缓存（instantiate 从 pkg `ControlInit` 填）。运行时态，不进 pkg。
    /// 供 `clone_node_recursive` 重建克隆控件的 ControlState——否则克隆控件无 ControlState，
    /// `set_control_value` 静默失败 + `sync_control_visuals` 早退（虚拟列表每槽控件全相同的根因）。
    /// 稀疏：仅控件节点入表。
    pub control_inits: std::collections::HashMap<NodeId, crate::asset::ControlInit>,
    /// 每节点 role/data-slot（instantiate 从 TemplateNode 填）。运行时态，不进 pkg。
    /// 稀疏：仅带 role/data-slot 的节点入表。供后续 role 驱动的语义分派 + 控件部件
    /// 定位（find_child_by_role / find_child_by_slot）查表。
    pub roles: RoleTable,
    /// 每节点 text 测量结果（layout solve 填，render 复用——消除双测量不一致）。
    /// index = NodeId.index()，仅 Text 节点 Some。运行时态，不进 pkg。
    ///
    /// 根因：layout 闭包用 taffy 选定 max_width 测（短文本 intrinsic≈available → taffy 传 None
    /// 不换行；长文本 → Some(available) 换行），render 若用 rect.w（stretch 后的 available 整数宽）
    /// 重测，短文本因 intrinsic 亚像素超 available 误判换行。故 render 复用 layout 结果，不重测。
    pub text_layouts: Vec<Option<crate::text::layout::TextLayout>>,
    /// 跨帧 measure_text memo（每节点两槽 intrinsic/constrained，带 fingerprint）。
    /// solve 闭包命中 fingerprint → 复用 TextLayout 跳过 shaping（解坑 186 低帧）。详见
    /// text::layout::TextMeasureCache。render 不读此（读 text_layouts render 槽）。
    pub text_measure_cache: Vec<Option<crate::text::layout::TextMeasureCache>>,
    /// TextNode content (only TextNode nodes have entries).
    pub text_contents: std::collections::HashMap<NodeId, String>,
    /// Image src paths (only Image nodes have entries).
    pub image_srcs: std::collections::HashMap<NodeId, String>,
    /// 本帧 transition 请求（rematch 检测 data-page 通道变化时推入；Stage tick drain 后
    /// kill 旧 tween + 提交新 tween，见 Phase E）。运行时态，不进 pkg。
    pub pending_transitions: Vec<crate::tween::TransitionRequest>,
    /// 全局 @keyframes 查找表（CSS `@keyframes` 全局语义，spec §3.5）。instantiate 时
    /// 组件 keyframes 合并进来（同名后实例化覆盖）；KeyframePlayer 按 `AnimationSpec.name`
    /// 查此表。运行时态，不进 pkg（pkg 按组件存于 ComponentTemplate.keyframes）。
    pub keyframes: std::collections::HashMap<String, crate::scene::animation::KeyframesRule>,
    /// 活跃 @keyframes player（slotmap 稳定 Key = 未来 C# Animation 句柄）。
    /// 运行时态，不进 pkg。M2.5 池化时再优化（spec §4.3）。
    pub players:
        SlotMap<crate::scene::animation::PlayerKey, crate::scene::animation::KeyframePlayer>,
    /// 事件字符串表（动画事件 name/hook_name payload，spec §7.5）。持久 intern：索引跨
    /// tick 稳定，装 EventRecord 的 24-bit 槽（click_count+pad），C# demux（T11）按索引
    /// 读回字符串。运行时态，不进 pkg。
    pub event_strs: crate::event::EventStrTable,
}

impl Scene {
    /// 建节点后的 per-node side table 联动——**所有建节点路径的单一入口**：
    /// text 两表（`text_layouts` / `text_measure_cache`，index = NodeId.index()）对齐
    /// 容量并清本槽；world 两表（`world_transforms` / `node_sort_keys`）**不 resize**、
    /// 仅清已覆盖槽位（保持「未计算 = 越界 = 本帧不命中」语义，见 hit bounds guard）；
    /// 按 kind seed 稀疏内容表（空串占位，调用方可随后覆写实值）。
    ///
    /// 新增 per-node 状态表时在此与 [`Scene::free_node_slot`] 各加联动——平行表一致性
    /// 不靠建点各自维护（漏一处 = 静默错位读错数据）。
    pub(crate) fn alloc_node_slot(&mut self, id: NodeId, kind: NodeKind) {
        let need = self.nodes.capacity() + 1;
        if self.text_layouts.len() < need {
            self.text_layouts.resize(need, None);
        }
        if self.text_measure_cache.len() < need {
            self.text_measure_cache.resize(need, None);
        }
        // 清本槽残留：slotmap 槽位复用时 Vec 索引表不走 remove，上一任节点的值会留到
        // 下帧 compute/assign 整体覆盖——这个窗口内新节点会读到死节点数据（命中矩阵/
        // sort_key/文本缓存）。alloc 清一次、free 清一次，任何路径进来都是初值。
        let idx = id.index();
        if idx < self.world_transforms.len() {
            self.world_transforms[idx] = crate::transform::IDENTITY;
        }
        if idx < self.node_sort_keys.len() {
            self.node_sort_keys[idx] = 0;
        }
        self.text_layouts[idx] = None;
        self.text_measure_cache[idx] = None;
        match kind {
            NodeKind::TextNode => {
                self.text_contents.insert(id, String::new());
            }
            NodeKind::Image => {
                self.image_srcs.insert(id, String::new());
            }
            _ => {}
        }
    }

    /// 删节点的 per-node side table 联动——**所有删节点路径的单一入口**：
    /// Vec 索引表清初值（不 truncate——len 对齐容量，保持索引不变量）+ 稀疏表
    /// （anim/scroll/controls/control_inits/roles/lists/text_contents/image_srcs）remove。
    /// 树手术（children/roots/focused_node/dynamic_rules）与 tween kill 归调用方——
    /// 那些不是 per-node 表。与 [`Scene::alloc_node_slot`] 成对维护。
    pub(crate) fn free_node_slot(&mut self, id: NodeId) {
        self.free_seq += 1;
        self.free_log.push_back((id, self.free_seq));
        if self.free_log.len() > 32 {
            self.free_log.pop_front();
        }
        let idx = id.index();
        if idx < self.world_transforms.len() {
            self.world_transforms[idx] = crate::transform::IDENTITY;
        }
        if idx < self.node_sort_keys.len() {
            self.node_sort_keys[idx] = 0;
        }
        if idx < self.text_layouts.len() {
            self.text_layouts[idx] = None;
        }
        if idx < self.text_measure_cache.len() {
            self.text_measure_cache[idx] = None;
        }
        self.anim.clear_node(id);
        self.scroll.remove(id);
        self.controls.remove(id);
        self.control_inits.remove(&id);
        self.roles.remove(id);
        self.lists.remove(id);
        self.text_contents.remove(&id);
        self.image_srcs.remove(&id);
    }

    /// 从扁平 entries（DFS 先序）建 Node 树。`parent_idx` 指向 entries 下标，`None` = 根。
    /// clip_rect slot / dirty 标志按 style.overflow_x/y（非 Visible 即 clip）/ kind 派生。
    /// 包加载路径（read_package）也走此入口。
    ///
    /// **NodeId 由 slotmap 分配**：entries 第 i 个 → slotmap insert → NodeId（idx=i+1，version=1，
    /// 无删除时）。parent/children 用 entries 下标 → 经临时 ids 表映射到 NodeId。
    pub fn build(
        entries: &[(
            Option<usize>,
            NodeKind,
            ResolvedStyle,
            Vec<String>,
            Option<String>,
            bool,
            Option<i32>,
            Option<String>, // data_controller (dead tuple slot: live Node field + TemplateNode.data_controller both removed; slot remains until Scene::build signature is refactored to drop it)
            Option<String>,
            Option<String>,
        )],
    ) -> Scene {
        let mut scene = Scene::default();
        let mut ids: Vec<NodeId> = Vec::with_capacity(entries.len());
        for (
            _,
            kind,
            style,
            classes,
            id_attr,
            draggable,
            tabindex,
            _data_controller,
            content,
            src,
        ) in entries.iter()
        {
            let node = Node {
                id: NodeId::INVALID, // 临时，insert 后回填
                parent: None,        // 下一轮填
                kind: *kind,
                style: style.clone(),
                base_style: style.clone(),
                taffy_id: None,
                layout_rect: Rect::default(),
                clip_rect: if style.overflow_x != OverflowMode::Visible
                    || style.overflow_y != OverflowMode::Visible
                {
                    Some(Rect::default())
                } else {
                    None
                },
                children: Vec::new(),
                dirty_mesh: true,
                dirty_text: matches!(kind, NodeKind::TextNode),
                classes: classes.clone(),
                id_attr: id_attr.clone(),
                custom_tag: None,
                interaction: NodeInteraction {
                    flags: NodeFlags::empty(),
                    touchable: style.touchable,
                    draggable: *draggable,
                    tabindex: *tabindex,
                },
                reuse_key: 0,
                inline_override: ResolvedStyle::default(),
                inline_set: InlineSet(0),
                user_transform: crate::transform::NodeTransform::default(),
                rich_text_block: false,
            };
            let key = scene.nodes.insert(node);
            let id = NodeId::from_key(key);
            scene.nodes.get_mut(key).unwrap().id = id; // 回填
            scene.alloc_node_slot(id, *kind);
            ids.push(id);
            if let Some(c) = content {
                scene.text_contents.insert(id, c.clone());
            }
            if let Some(src) = src {
                scene.image_srcs.insert(id, src.clone());
            }
        }
        // 接 parent/children/roots（用 ids 映射 entries 下标 → NodeId）
        for (i, (parent_idx, _, _, _, _, _, _, _, _, _)) in entries.iter().enumerate() {
            match parent_idx {
                Some(p) => {
                    let child_id = ids[i];
                    let parent_id = ids[*p];
                    let ck = child_id.to_key();
                    let pk = parent_id.to_key();
                    scene.nodes.get_mut(ck).unwrap().parent = Some(parent_id);
                    scene.nodes.get_mut(pk).unwrap().children.push(child_id);
                }
                None => scene.roots.push(ids[i]),
            }
        }
        scene
    }

    /// test helper：从节点列表 + (parent_idx, child_idx) 边建 Scene。替代 70+ 字面量。
    /// roots = 无 parent 的节点（按插入序）。
    pub fn from_nodes(nodes: Vec<Node>, edges: Vec<(usize, usize)>) -> Scene {
        let mut scene = Scene::default();
        let mut ids: Vec<NodeId> = Vec::with_capacity(nodes.len());
        for n in nodes {
            let kind = n.kind;
            let key = scene.nodes.insert(n);
            let id = NodeId::from_key(key);
            scene.nodes.get_mut(key).unwrap().id = id;
            scene.alloc_node_slot(id, kind);
            ids.push(id);
        }
        for (p, c) in edges {
            let pid = ids[p];
            let cid = ids[c];
            let pk = pid.to_key();
            let ck = cid.to_key();
            scene.nodes.get_mut(ck).unwrap().parent = Some(pid);
            scene.nodes.get_mut(pk).unwrap().children.push(cid);
        }
        // roots = 无 parent 的（按 ids 插入序）
        for &id in &ids {
            if scene.nodes.get(id.to_key()).unwrap().parent.is_none() {
                scene.roots.push(id);
            }
        }
        scene
    }

    /// NodeId → slotmap DefaultKey 桥接（内部用）。
    pub fn key_for(&self, id: NodeId) -> DefaultKey {
        id.to_key()
    }

    /// 按 NodeId 取节点（slotmap get，自带 gen 校验，悬空返 None）。
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.to_key())
    }
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(id.to_key())
    }

    /// 带站点标签的 live 查取：死 id 时 panic 消息含 id/index + 副作用表残留
    /// （controls/lists/scroll 仍在 = 存在绕过 free_node_slot 漏斗的释放路径）。
    /// release dll 内联行号不可靠，站点标签是定位「快照后死亡」panic 的唯一可靠锚点。
    /// 标签约定：`模块/函数名[: 语义后缀]`（如 `layout/build`、`dynamic/rematch:write`）——
    /// **勿用行号**，行号随编辑漂移会把取证日志指向错误位置。
    pub fn get_live(&self, id: NodeId, site: &str) -> &Node {
        match self.get(id) {
            Some(n) => n,
            None => {
                let frees: Vec<String> = self
                    .free_log
                    .iter()
                    .rev()
                    .enumerate()
                    .filter(|(_, (fid, _))| *fid == id)
                    .map(|(age, (_, seq))| format!("seq={seq} age={age}frees-ago"))
                    .collect();
                let tail: Vec<String> = self
                    .free_log
                    .iter()
                    .rev()
                    .take(4)
                    .map(|(fid, seq)| format!("{:?}@{seq}", fid))
                    .collect();
                panic!(
                    "live node [{}] id={:?} idx={} side-table residue controls={} lists={} scroll={} | frees of this id: {} | free_log tail: {} | free_seq={}",
                    site,
                    id,
                    id.index(),
                    self.controls.get(id).is_some(),
                    self.lists.0.contains_key(&id),
                    self.scroll.get(id).is_some(),
                    if frees.is_empty() { "none (>32 frees ago or bypassed the free_node_slot funnel)".to_string() } else { frees.join("; ") },
                    tail.join(" "),
                    self.free_seq,
                );
            }
        }
    }
    pub fn get_live_mut(&mut self, id: NodeId, site: &str) -> &mut Node {
        if self.get(id).is_none() {
            panic!(
                "live node [{}] id={:?} idx={} side-table residue controls={} lists={} scroll={}",
                site,
                id,
                id.index(),
                self.controls.get(id).is_some(),
                self.lists.0.contains_key(&id),
                self.scroll.get(id).is_some(),
            );
        }
        self.get_mut(id).expect("live node (recheck)")
    }

    /// 按 CSS id 属性查节点（首个匹配）。无匹配 / 空 id → None。
    /// 供 FFI find_node_by_id：业务用 id 注册 listener / 设 disabled，替代硬编码 build 序 id
    /// （auto Text 子会偏移 build 序，硬编码不可靠）。
    pub fn find_by_id_attr(&self, id: &str) -> Option<NodeId> {
        self.nodes
            .iter()
            .find(|(_, n)| n.id_attr.as_deref() == Some(id))
            .map(|(_, n)| n.id)
    }

    /// 在 node 所属查找作用域内解析 id：沿父链找最近 LOOKUP_SCOPE 根（含自身），
    /// 在其子树内查（不穿透嵌套作用域）。跨树 id 关联（aria-controls）的多实例安全
    /// 解析用——同组件展开多实例时，各实例的 tab 只命中本实例的 panel，不串到首个
    /// 全局匹配。无 LOOKUP_SCOPE 祖先（防御：detached 节点）退化全局首匹配。
    pub fn find_node_by_id_in_own_scope(&self, node: NodeId, id: &str) -> Option<NodeId> {
        let mut cur = Some(node);
        while let Some(nid) = cur {
            let n = self.get(nid)?;
            if n.interaction.flags.contains(NodeFlags::LOOKUP_SCOPE) {
                return self.find_node_by_id_in_subtree(nid, id);
            }
            cur = n.parent;
        }
        self.find_by_id_attr(id)
    }

    /// 在 root 子树内 DFS 查找 id 属性匹配的首个节点（self-exclusive：从 root
    /// 的直接子开始，root 自身的 id_attr 不被命中）。与 DOM querySelectorAll / Query<T>
    /// 一致——在元素上调 query 只查后代不含自身。纯结构遍历，不检查 display:none。
    /// 供 FFI 子树作用域 id 查找，替代"全局首匹配 + 父链后过滤"。
    ///
    /// **L3 查找边界**：遇 `LOOKUP_SCOPE` 子节点（组件展开域 host / ListView slot 根）
    /// 检查其自身 id 后**不再下钻**——嵌套作用域内部 id 只归该作用域自己的 Get 查找
    /// （main-design §4.3「不穿透嵌套组件边界」）。作用域根自身仍可被外层命中（同
    /// Shadow DOM：host 元素在 light tree，shadow 内部不在）。
    pub fn find_node_by_id_in_subtree(&self, root: NodeId, id: &str) -> Option<NodeId> {
        let node = self.get(root)?;
        let mut stack: Vec<NodeId> = node.children.iter().rev().copied().collect();
        while let Some(nid) = stack.pop() {
            let n = self.get(nid)?;
            if n.id_attr.as_deref() == Some(id) {
                return Some(nid);
            }
            // L3 边界剪枝：查找边界子节点的内部 id 不可见，跳过其子树。
            if !n.interaction.flags.contains(NodeFlags::LOOKUP_SCOPE) {
                stack.extend(n.children.iter().rev());
            }
        }
        None
    }

    /// 每个休眠 list slot 子树里「active 时会成为渲染节点」的超集，供 build_blob 发
    /// keepalive——后端镜像池据 node_id / reuse_key 保留整子树 GO，不止 slot 根。
    ///
    /// 不发的话：slot park 时 display:none 剪掉整子树，子节点（文本 mesh 等）GO 被
    /// 当 stale 销毁，reactivate 时重建，每帧滚动 churn（item 闪没 + 掉帧）。
    ///
    /// 条目集是超集（多发无害）：slot 根 + 所有非自身 display:none、非纯空白文本的后代。
    /// 被 merge 吃掉 / 无 mesh 的节点后端 lookup miss 即 no-op；漏发才会 churn。slot 根
    /// 自身的 display:none 是 park override（active 时渲染），照发；其余后代若自身
    /// display:none（嵌套隐藏）则整子树跳过。
    pub fn parked_keepalive_nodes(&self) -> Vec<(NodeId, u32)> {
        let mut out: Vec<(NodeId, u32)> = Vec::new();
        for ls in self.lists.0.values() {
            for slot in ls.slots.iter().filter(|s| s.parked) {
                let mut stack = vec![slot.node];
                while let Some(nid) = stack.pop() {
                    let Some(n) = self.get(nid) else {
                        continue;
                    };
                    let nested_hidden = nid != slot.node
                        && matches!(n.style.taffy_style.display, taffy::style::Display::None);
                    if !nested_hidden && !is_whitespace_only_text(self, nid) {
                        out.push((nid, n.reuse_key));
                    }
                    if !nested_hidden {
                        stack.extend(n.children.iter().rev());
                    }
                }
            }
        }
        out
    }
}

/// 纯空白 TextNode 判定（HTML 元素源码里 tag 之间的换行+缩进）。
///
/// HTML 标准行为：block/flex 容器子节点间的纯空白应折叠，不成 box/item。
/// inline 间有意空格（如 `"A B"`）保留——那种 text 含非空白字符，不被此过滤误伤。
/// 用于 layout（不进 taffy 树）+ render（不画），避免空白 text 撑开 flex 父容器
/// 主轴或挤压兄弟 flex item。
pub fn is_whitespace_only_text(scene: &Scene, id: NodeId) -> bool {
    let node = match scene.nodes.get(id.to_key()) {
        Some(n) => n,
        None => return false,
    };
    if !matches!(node.kind, NodeKind::TextNode) {
        return false;
    }
    match scene.text_contents.get(&id) {
        // 空串（""）不算空白 text——空串本身就是 0 尺寸，不会撑开。
        // 只过滤含至少一个字符且全是空白的（"\n    "）。
        Some(c) if !c.is_empty() => c.chars().all(char::is_whitespace),
        _ => false,
    }
}

/// 兄弟绘制序：children 稳定按 `z_index` 升序排（等 z 保持 DOM 序——z 全 0 时
/// 逐位等于原 children 顺序）。子树整体移动：父的 z 决定整棵子树所在层，子树
/// 内部再按自身 z 排（DFS 先访问 = 先绘制 = 底层）。
///
/// render 主 DFS（batch.rs）与 open popup 末尾追加循环共用，保证两路一致；
/// hit 侧走 hit.rs `effective_draw_order`（逆序遍历，z 为主键）。
pub fn paint_order_children(scene: &Scene, parent: NodeId) -> Vec<NodeId> {
    let mut kids: Vec<NodeId> = match scene.nodes.get(parent.to_key()) {
        Some(n) => n.children.clone(),
        None => return Vec::new(),
    };
    kids.sort_by_key(|&c| {
        scene
            .nodes
            .get(c.to_key())
            .map(|n| n.style.z_index)
            .unwrap_or(0)
    });
    kids
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod repr_tests {
    use super::NodeKind;

    #[test]
    fn kind_as_u8_is_discriminant() {
        // repr(u8) 后 as u8 等于声明顺序的判别值；锁定几个关键值防漂移。
        assert_eq!(NodeKind::Container as u8, 0);
        assert_eq!(NodeKind::TextNode as u8, 1);
        assert_eq!(NodeKind::Button as u8, 3);
        assert_eq!(NodeKind::Image as u8, 4);
        assert_eq!(NodeKind::Template as u8, 18);
        assert_eq!(NodeKind::TabList as u8, 19);
        assert_eq!(NodeKind::Tab as u8, 20);
    }

    #[test]
    fn from_u8_roundtrip_all_variants() {
        let all = [
            NodeKind::Container,
            NodeKind::TextNode,
            NodeKind::TextElement,
            NodeKind::Button,
            NodeKind::Image,
            NodeKind::TextField,
            NodeKind::NumberField,
            NodeKind::Slider,
            NodeKind::Toggle,
            NodeKind::RadioButton,
            NodeKind::TextArea,
            NodeKind::Dropdown,
            NodeKind::OptionItem,
            NodeKind::ProgressBar,
            NodeKind::ListView,
            NodeKind::ListItem,
            NodeKind::Slot,
            NodeKind::CustomElement,
            NodeKind::Template,
            NodeKind::TabList,
            NodeKind::Tab,
        ];
        for &k in &all {
            assert_eq!(NodeKind::from_u8(k as u8), Some(k));
        }
        assert_eq!(NodeKind::from_u8(21), None); // 越界（Tab=20 是最后合法判别值）
        assert_eq!(NodeKind::from_u8(255), None);
    }

    #[test]
    fn tablist_tab_kind_roundtrip_and_container() {
        // T3：TabList=19、Tab=20 追加到 enum 末尾，判别值稳定（pkg 版本门保跨版本）。
        assert_eq!(NodeKind::TabList as u8, 19);
        assert_eq!(NodeKind::Tab as u8, 20);
        assert_eq!(NodeKind::from_u8(19), Some(NodeKind::TabList));
        assert_eq!(NodeKind::from_u8(20), Some(NodeKind::Tab));
        // TabList（持 tab 子，镜像 ListView）+ Tab（持 label 子，镜像 Button）都是容器。
        assert!(NodeKind::TabList.is_container());
        assert!(NodeKind::Tab.is_container());
        assert!(!NodeKind::TabList.is_leaf());
        assert!(!NodeKind::Tab.is_leaf());
    }
}
