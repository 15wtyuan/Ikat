//! Scene 层：持久 Node 树（场景图）。
//!
//! 消费 `ElementTree` + `Vec<ResolvedStyle>`，构建一棵 `Node` 树。
//! layout 层后续往 `taffy_id`/`layout_rect` 写几何；render 层消费
//! `clip_rect`/`dirty_*`。本模块只管建树 + 初始脏标志。

#[cfg(feature = "parse")]
use crate::parse::dom::{ElementId, ElementTree};
use crate::style::resolved::{OverflowMode, ResolvedStyle};
use slotmap::{DefaultKey, Key, KeyData, SlotMap};

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
#[derive(Debug, Clone, Default)]
pub enum NodeKind {
    #[default]
    Container,
    Text {
        content: String,
    },
    /// src 原样存（不加载），render 层映射到 image_path（同 path 的图可合批）。
    /// src 取自元素的 `src` 属性（`<img src="...">`），不是文本内容。
    Image {
        src: String,
    },
    Button,
}

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
    /// 打包期 resolve_styles 产物（不变，rematch 基线）。style 是运行时 rematch 覆写值。
    pub base_style: ResolvedStyle,
    /// 运行时 class 列表（建树时从 ElementData.classes 填；供动态规则 class 选择器匹配）。
    pub classes: Vec<String>,
    /// 运行时 id（建树时从 ElementData.id 填；供动态规则 id 选择器匹配）。
    pub id_attr: Option<String>,
    /// pointer-events:auto=true / none=false（解析时落，建树时从 style.touchable 填）。
    pub touchable: bool,
    /// 当前帧命中（运行时，每帧命中 diff 更新）。
    pub hovered: bool,
    /// 指针按下且命中（运行时状态机）。
    pub active: bool,
    /// 业务设（set_node_disabled），伪类源 + active/click 抑制。
    pub disabled: bool,
    /// opt-in 可拖拽（HTML `draggable="true"` 属性）。drag 状态机据此发起 drag。
    pub draggable: bool,
    /// HTML tabindex 属性值。None=不可聚焦；Some(-1)=仅编程聚焦；
    /// Some(0)=DOM 序可聚焦；Some(N>0)=显式序可聚焦。
    pub tabindex: Option<i32>,
    /// 当前是否聚焦（运行时，:focus 伪类源）。仅 focused_node 链上节点 true。
    pub focused: bool,
    /// 渲染复用键。0=无复用（后端按 node_id keying）；>0=按 reuse_key 复用 GO
    /// （虚拟列表 slot 用：slot 换绑 item 时 NodeId 变但 reuse_key 不变 → 后端复用 GO）。
    /// 运行时字段（不进 pkg，打包期不存）。driver 设。
    pub reuse_key: u32,
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
            touchable: true,
            hovered: false,
            active: false,
            disabled: false,
            draggable: false,
            tabindex: None,
            focused: false,
            reuse_key: 0,
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

#[derive(Debug, Clone)]
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
}

impl Scene {
    /// 从扁平 entries（DFS 先序）建 Node 树。`parent_idx` 指向 entries 下标，`None` = 根。
    /// clip_rect slot / dirty 标志按 style.overflow_x/y（非 Visible 即 clip）/ kind 派生。
    /// parse 路径（build_scene）与包加载路径（read_package）共用。
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
        )],
    ) -> Scene {
        let mut scene = Scene {
            roots: Vec::new(),
            nodes: SlotMap::with_key(),
            dynamic_rules: crate::style::dynamic::DynamicRuleTable::default(),
            focused_node: None,
            world_transforms: Vec::new(),
            anim: Default::default(),
            scroll: Default::default(),
            text_layouts: Vec::new(),
            node_sort_keys: Vec::new(),
        };
        // 先 insert 所有节点，收集 slotmap 分配的 NodeId
        let mut ids: Vec<NodeId> = Vec::with_capacity(entries.len());
        for (_, kind, style, classes, id_attr, draggable, tabindex) in entries.iter() {
            let node = Node {
                id: NodeId::INVALID, // 临时，insert 后回填
                parent: None,        // 下一轮填
                kind: kind.clone(),
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
                dirty_text: matches!(kind, NodeKind::Text { .. }),
                classes: classes.clone(),
                id_attr: id_attr.clone(),
                touchable: style.touchable,
                hovered: false,
                active: false,
                disabled: false,
                draggable: *draggable,
                tabindex: *tabindex,
                focused: false,
                reuse_key: 0,
            };
            let key = scene.nodes.insert(node);
            let id = NodeId::from_key(key);
            scene.nodes.get_mut(key).unwrap().id = id; // 回填
            ids.push(id);
        }
        // 接 parent/children/roots（用 ids 映射 entries 下标 → NodeId）
        for (i, (parent_idx, _, _, _, _, _, _)) in entries.iter().enumerate() {
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
        // text_layouts 随槽位容量对齐（None 占位，layout::solve 填实际 TextLayout）。
        // **容量而非存活数**：按 id.index() 索引，remove_node 后 idx 不变但存活数减，
        // 按 len 分配会越界。capacity+1（1 基索引，idx 0 占位）。
        scene.text_layouts = vec![None; scene.nodes.capacity() + 1];
        scene
    }

    /// test helper：从节点列表 + (parent_idx, child_idx) 边建 Scene。替代 70+ 字面量。
    /// roots = 无 parent 的节点（按插入序）。
    pub fn from_nodes(nodes: Vec<Node>, edges: Vec<(usize, usize)>) -> Scene {
        let mut scene = Scene {
            roots: Vec::new(),
            nodes: SlotMap::with_key(),
            dynamic_rules: crate::style::dynamic::DynamicRuleTable::default(),
            focused_node: None,
            world_transforms: Vec::new(),
            anim: Default::default(),
            scroll: Default::default(),
            text_layouts: Vec::new(),
            node_sort_keys: Vec::new(),
        };
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

/// 从 ElementTree + ResolvedStyle 构建 Node 树（gather 后调 `Scene::build`）。
///
/// `styles` 必须与 `tree.nodes` 同长且同序（由 `style::cascade::resolve_styles` 保证）。
#[cfg(feature = "parse")]
pub fn build_scene(tree: &ElementTree, styles: &[ResolvedStyle]) -> Scene {
    let mut entries: Vec<(
        Option<usize>,
        NodeKind,
        ResolvedStyle,
        Vec<String>,
        Option<String>,
        bool,
        Option<i32>,
    )> = Vec::new();
    for root in &tree.roots {
        gather_rec(tree, styles, *root, None, &mut entries);
    }
    Scene::build(&entries)
}

#[cfg(feature = "parse")]
fn gather_rec(
    tree: &ElementTree,
    styles: &[ResolvedStyle],
    el_id: ElementId,
    parent_idx: Option<usize>,
    entries: &mut Vec<(
        Option<usize>,
        NodeKind,
        ResolvedStyle,
        Vec<String>,
        Option<String>,
        bool,
        Option<i32>,
    )>,
) -> usize {
    let el = &tree.nodes[el_id.0];
    let style = &styles[el_id.0];
    // tag→NodeKind 复用 runtime 的 `kind_from_tag`（dynamic.rs，不依赖 parse feature），
    // 消除两处 tag 白名单重复。parse 层已保证 tag 在围栏白名单内（div/span/img/button），
    // 故 kind_from_tag 在此必 Ok——Err 走 unreachable（parse/白名单契约破坏）。
    // kind_from_tag 对 img/span 返空 src/content（动态建树语义）；parse 路径需从元素属性/文本回填。
    // img 的 src 从属性取（`<img src="...">`），不是元素文本；
    // span 的文本是其自身 content（Text 叶子，无子节点）。
    let mut kind = crate::scene::dynamic::kind_from_tag(&el.tag).unwrap_or_else(|_| {
        unreachable!(
            "parse 层白名单已挡围栏外 tag，scene 不应见到 <{}>；这是 parse/scene 契约破坏",
            el.tag
        )
    });
    match &mut kind {
        NodeKind::Image { src } => {
            *src = el.attrs.get("src").cloned().unwrap_or_default();
        }
        NodeKind::Text { content } => {
            *content = el.text.clone().unwrap_or_default();
        }
        _ => {}
    }
    // draggable="true" → Node.draggable（HTML 原生属性）。
    // 非 "true" 一律 false（draggable="false"/缺省/任意值 → false，照 HTML truthy 语义简化）。
    let draggable = el
        .attrs
        .get("draggable")
        .map(|v| v == "true")
        .unwrap_or(false);
    // tabindex 属性 → Option<i32>。非数字 → None（照 DOM 容错：无效值忽略）。
    // 语义：None=不可聚焦；Some(-1)=仅编程；Some(0)=DOM 序；Some(N>0)=显式序。
    let tabindex = el.attrs.get("tabindex").and_then(|v| v.parse::<i32>().ok());
    let my_idx = entries.len();
    entries.push((
        parent_idx,
        kind.clone(),
        style.clone(),
        el.classes.clone(),
        el.id.clone(),
        draggable,
        tabindex,
    ));

    // Container/Button 的裸文本 → Text 子节点。文本子像无 class 的 <span>：
    // taffy_style 取 DEFAULT（由测量定尺寸），视觉/字体字段继承父值。
    // 不能直接克隆父 style——父若是 .h{height:30px} 会让文本子也高 30px，
    // 既不正确也压制了文本自然测量。
    if matches!(kind, NodeKind::Container | NodeKind::Button) {
        if let Some(text) = &el.text {
            let mut ts = ResolvedStyle::default();
            ts.color = style.color;
            ts.font_size = style.font_size;
            ts.font_family = style.font_family.clone();
            ts.font_weight = style.font_weight;
            ts.line_height = style.line_height;
            ts.letter_spacing = style.letter_spacing;
            ts.text_align = style.text_align;
            ts.white_space_nowrap = style.white_space_nowrap;
            entries.push((
                Some(my_idx),
                NodeKind::Text {
                    content: text.clone(),
                },
                ts,
                Vec::new(),
                None,
                false,
                None,
            ));
        }
    }

    if !el.children.is_empty() {
        for c in &el.children {
            gather_rec(tree, styles, *c, Some(my_idx), entries);
        }
    }
    my_idx
}

#[cfg(test)]
mod tests;

#[cfg(all(test, feature = "parse"))]
mod parse_tests;
