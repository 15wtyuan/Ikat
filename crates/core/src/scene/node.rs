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
    pub fn from_key(k: DefaultKey) -> NodeId {
        let ffi = k.data().as_ffi();
        let idx = (ffi & 0xFFFF_FFFF) as u32;
        let version = (ffi >> 32) as u32;
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
    TextBlock,
    TextElement,
    LineBreak,
    Label,
    Button,
    Link,
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
    Canvas,
}

impl NodeKind {
    /// u8 判别值 → NodeKind（pkg.bin kind_tag read 用）。越界返 None。
    /// 变体只追加到 enum 末尾，保持既有判别值稳定。
    pub fn from_u8(b: u8) -> Option<NodeKind> {
        match b {
            0 => Some(NodeKind::Container),
            1 => Some(NodeKind::TextNode),
            2 => Some(NodeKind::TextBlock),
            3 => Some(NodeKind::TextElement),
            4 => Some(NodeKind::LineBreak),
            5 => Some(NodeKind::Label),
            6 => Some(NodeKind::Button),
            7 => Some(NodeKind::Link),
            8 => Some(NodeKind::Image),
            9 => Some(NodeKind::TextField),
            10 => Some(NodeKind::NumberField),
            11 => Some(NodeKind::Slider),
            12 => Some(NodeKind::Toggle),
            13 => Some(NodeKind::RadioButton),
            14 => Some(NodeKind::TextArea),
            15 => Some(NodeKind::Dropdown),
            16 => Some(NodeKind::OptionItem),
            17 => Some(NodeKind::ProgressBar),
            18 => Some(NodeKind::ListView),
            19 => Some(NodeKind::ListItem),
            20 => Some(NodeKind::Slot),
            21 => Some(NodeKind::CustomElement),
            22 => Some(NodeKind::Canvas),
            _ => None,
        }
    }

    /// Container content model: user-arrangeable children (div/button/a/p/span/ul/li/...).
    /// Single source of truth for container vs leaf classification — adding a new
    /// container variant only requires changing this method.
    pub fn is_container(self) -> bool {
        matches!(
            self,
            Self::Container
                | Self::TextBlock
                | Self::TextElement
                | Self::Label
                | Self::Button
                | Self::Link
                | Self::ListView
                | Self::ListItem
                | Self::Canvas
                | Self::Slot
                | Self::CustomElement
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
            | NodeKind::TextBlock
            | NodeKind::TextElement
            | NodeKind::LineBreak
            | NodeKind::Label
            | NodeKind::Button
            | NodeKind::Link
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
            | NodeKind::Canvas => {}
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
    pub interaction: NodeInteraction,
    pub reuse_key: u32,
    /// 运行时 inline override（便签层）。C# Style.X=v 经 set_inline_override 写入；
    /// rematch 在动态规则后应用（最高优先级）。默认空 = 无 inline override。
    /// 纯运行时 transient，不进 pkg.bin（设计期无 inline override 概念）。
    pub inline_override: ResolvedStyle,
    /// inline_override 里哪些字段被设了（继承属性复用 INH_* bit，非继承用 INLINE_*）。
    /// 默认 0 = 无任何 inline override。rematch 据此把 inline_override 字段拷进 style。
    pub inline_set: InlineSet,
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
            interaction: NodeInteraction {
                flags: NodeFlags::empty(),
                touchable: true,
                draggable: false,
                tabindex: None,
            },
            reuse_key: 0,
            inline_override: ResolvedStyle::default(),
            inline_set: InlineSet(0),
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

#[derive(Debug, Clone, Default)]
pub struct Scene {
    pub roots: Vec<NodeId>,
    /// 节点存储。Vec<Node> → SlotMap<DefaultKey, Node>（动态树 spec §4.1）。
    /// 应用层用 NodeId(u32) 句柄（FFI/C# 透明），经 `Scene::key_for`/`NodeId::to_key` 桥接到 DefaultKey。
    pub nodes: SlotMap<DefaultKey, Node>,
    /// 运行时伪类重匹配规则表。默认空；包加载填，inline 路径空。
    pub dynamic_rules: crate::style::dynamic::DynamicRuleTable,
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
    /// 每节点 text 测量结果（layout solve 填，render 复用——消除双测量不一致）。
    /// index = NodeId.index()，仅 Text 节点 Some。运行时态，不进 pkg。
    ///
    /// 根因：layout 闭包用 taffy 选定 max_width 测（短文本 intrinsic≈available → taffy 传 None
    /// 不换行；长文本 → Some(available) 换行），render 若用 rect.w（stretch 后的 available 整数宽）
    /// 重测，短文本因 intrinsic 亚像素超 available 误判换行。故 render 复用 layout 结果，不重测。
    pub text_layouts: Vec<Option<crate::text::layout::TextLayout>>,
    /// TextNode content (only TextNode nodes have entries).
    pub text_contents: std::collections::HashMap<NodeId, String>,
    /// Image src paths (only Image nodes have entries).
    pub image_srcs: std::collections::HashMap<NodeId, String>,
    /// 本帧 transition 请求（rematch 检测 data-page 通道变化时推入；Stage tick drain 后
    /// kill 旧 tween + 提交新 tween，见 Phase E）。运行时态，不进 pkg。
    pub pending_transitions: Vec<crate::tween::TransitionRequest>,
}

impl Scene {
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
            Option<String>, // data_controller（dead：Node 字段 P1.4 删、TemplateNode.data_controller P1.6 删；此参数留 P1.12 一并清）
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
                interaction: NodeInteraction {
                    flags: NodeFlags::empty(),
                    touchable: style.touchable,
                    draggable: *draggable,
                    tabindex: *tabindex,
                },
                reuse_key: 0,
                inline_override: ResolvedStyle::default(),
                inline_set: InlineSet(0),
            };
            let key = scene.nodes.insert(node);
            let id = NodeId::from_key(key);
            scene.nodes.get_mut(key).unwrap().id = id; // 回填
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
        // text_layouts 随槽位容量对齐（None 占位，layout::solve / render::build 填）。
        // **容量而非存活数**：按 id.index() 索引，remove_node 后 idx 不变但存活数减，
        // 按 len 分配会越界。capacity+1（1 基索引，idx 0 占位）。
        scene.text_layouts = vec![None; scene.nodes.capacity() + 1];
        scene
    }

    /// test helper：从节点列表 + (parent_idx, child_idx) 边建 Scene。替代 70+ 字面量。
    /// roots = 无 parent 的节点（按插入序）。
    pub fn from_nodes(nodes: Vec<Node>, edges: Vec<(usize, usize)>) -> Scene {
        let mut scene = Scene::default();
        let mut ids: Vec<NodeId> = Vec::with_capacity(nodes.len());
        for n in nodes {
            let key = scene.nodes.insert(n);
            let id = NodeId::from_key(key);
            scene.nodes.get_mut(key).unwrap().id = id;
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
        scene.text_layouts = vec![None; scene.nodes.capacity() + 1];
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

    /// 按 CSS id 属性查节点（首个匹配）。无匹配 / 空 id → None。
    /// 供 FFI find_node_by_id：业务用 id 注册 listener / 设 disabled，替代硬编码 build 序 id
    /// （auto Text 子会偏移 build 序，硬编码不可靠）。
    pub fn find_by_id_attr(&self, id: &str) -> Option<NodeId> {
        self.nodes
            .iter()
            .find(|(_, n)| n.id_attr.as_deref() == Some(id))
            .map(|(_, n)| n.id)
    }
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
        assert_eq!(NodeKind::Button as u8, 6);
        assert_eq!(NodeKind::Image as u8, 8);
        assert_eq!(NodeKind::Canvas as u8, 22);
    }

    #[test]
    fn from_u8_roundtrip_all_variants() {
        let all = [
            NodeKind::Container,
            NodeKind::TextNode,
            NodeKind::TextBlock,
            NodeKind::TextElement,
            NodeKind::LineBreak,
            NodeKind::Label,
            NodeKind::Button,
            NodeKind::Link,
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
            NodeKind::Canvas,
        ];
        for &k in &all {
            assert_eq!(NodeKind::from_u8(k as u8), Some(k));
        }
        assert_eq!(NodeKind::from_u8(23), None); // 越界
        assert_eq!(NodeKind::from_u8(255), None);
    }
}
