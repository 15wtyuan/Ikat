//! 动态树操作：运行时删/建/改节点。
//!
//! `remove_node`（递归删子 + 联动清 anim/scroll/tween + slotmap remove）+
//! 动态建树/改树 API：`kind_from_tag` / `apply_css` / `create_node` / `create_root`
//! / `append_child` / `insert_before` / `remove_child`（摘除不删）/ `set_text` / `set_src`
//! / `set_inline_override` / `unset_inline_override`（便签层 inline override，rematch 最高优先级）。
//!
//! **设计要点**（spec §5.3 + §7 + §8）：
//! - 删节点联动清持久附属 map（anim/scroll remove + tween kill），防悬空 NodeId 残留
//!   写幽灵槽（HashMap 对任意 NodeId 都能插条目，须显式 remove）。
//! - 递归删子先 clone children 再递归（避免边迭代边改 slotmap 的借用冲突）。
//! - slotmap remove 后 NodeId 失效（gen++，Scene::get 返 None），槽位可复用。
//! - 动态建树复用 `mapping::apply_decl`（runtime 可用，不依赖 parse feature）做 CSS 声明应用，
//!   复用 dom.rs 围栏白名单语义做 tag→NodeKind（`kind_from_tag`）。
//! - create_node 填 base_style（源）+ style=base_style.clone()（派生），下帧 rematch 从 base 起算。

use crate::asset::ControlInit;
use crate::scene::node::{
    ControlState, EditState, Node, NodeFlags, NodeId, NodeInteraction, NodeKind, Rect, Scene,
};
use crate::style::dynamic::{inline_bit, InlineSet};
use crate::style::mapping::apply_decl;
use crate::style::resolved::{DisplayMode, OverflowMode, ResolvedStyle};
use crate::tween::TweenManager;

/// tag 字符串 → NodeKind（复用 dom.rs 围栏白名单语义，runtime 可用，不依赖 parse feature）。
/// 围栏白名单：div→Container, button→Button, img→Image, span→Text。
/// 未识别 tag → Err（动态建树 API 的 kind 入参由调用方负责，不像 parse 层有白名单兜底）。
pub fn kind_from_tag(tag: &str) -> Result<NodeKind, String> {
    match tag {
        "div" => Ok(NodeKind::Container),
        "button" => Ok(NodeKind::Button),
        "img" => Ok(NodeKind::Image),
        "span" => Ok(NodeKind::TextNode),
        other => Err(format!(
            "unknown kind tag: {}（围栏白名单：div/button/img/span）",
            other
        )),
    }
}

/// 运行时 create_node 支持的 tag 子集（div/button/img/span）的默认 display 铺底。
///
/// 复刻打包器 `fence::css_resolve` 对 tag DisplayDefault 的处理：block tag → Block，
/// inline tag → Flex（taffy 兼容）。运行时动态建的节点（不经 css_resolve）必须同样铺底，
/// 否则未声明 display 的元素会拿到 `ResolvedStyle::default()` 的 Flex（旧范式残留）。
///
/// 注意：这只是 NodeKind 级映射（create_node 仅 4 tag），不是完整 schema——
/// 完整 31 tag 表真相源仍在 fence（打包期消费）。两套不漂移：NodeKind↔display 是固定的。
fn default_display_for_kind(k: NodeKind) -> (DisplayMode, taffy::Display) {
    match k {
        // block tag：div（Container）→ 真 CSS block 流。
        NodeKind::Container => (DisplayMode::Block, taffy::Display::Block),
        // inline tag：button/img/span（运行时映射 Button/Image/TextNode）→ Flex（taffy 无 inline flow）。
        // 与 fence css_resolve 的 DisplayDefault::Inline → Flex 一致。
        _ => (DisplayMode::Flex, taffy::Display::Flex),
    }
}

/// CSS 声明串（"width:100px;background:#f00"）→ 应用到 ResolvedStyle。
/// 极简分割（split(';') + split_once(':')），逐条调 `mapping::apply_decl`。
/// 不识别的声明静默忽略（apply_decl 返 false）；格式错（无冒号）的声明跳过。
/// runtime 可用，不依赖 parse feature（apply_decl 是 mapping.rs 默认编译的公共函数）。
pub fn apply_css(style: &mut ResolvedStyle, css: &str) {
    for decl in css.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        if let Some((prop, val)) = decl.split_once(':') {
            apply_decl(style, prop.trim(), val.trim());
        }
    }
}

/// slotmap insert 后若 capacity 增长，resize parallel arrays 对齐新容量。
/// parallel arrays（text_layouts）按 NodeId.index() 索引，
/// 必须至少为 capacity+1（1 基索引，idx 0 占位），否则索引越界 panic。
fn resize_parallel_arrays(scene: &mut Scene) {
    let need = scene.nodes.capacity() + 1;
    if scene.text_layouts.len() < need {
        scene.text_layouts.resize(need, None);
    }
}

/// 建节点：kind_from_tag + apply_css 填 base_style + slotmap insert + 回填 node.id。
/// base_style = apply_css 结果（源），style 初始 = base_style.clone()（派生，下帧 rematch 从 base 起算）。
/// clip_rect 按 overflow_x/y（非 Visible）派生 Some(占位)（值由 layout/render 填）。
/// anim/scroll 不预填（HashMap 懒初始化，ensure 时填）。返回新 NodeId。
pub fn create_node(scene: &mut Scene, kind: &str, css: &str) -> Result<NodeId, String> {
    let k = kind_from_tag(kind)?;
    let mut base_style = ResolvedStyle::default();
    // schema display 铺底（block tag→Block，inline tag→Flex）：复刻打包器 css_resolve，
    // 让运行时动态建的节点默认 display 正确（旧范式 default 是 Flex，div 会误成 Flex）。
    // 在 apply_css 之前——显式 inline display 声明仍胜出（apply_css 后覆盖）。
    let (dm, td) = default_display_for_kind(k);
    base_style.display_mode = dm;
    base_style.taffy_style.display = td;
    apply_css(&mut base_style, css);
    let touchable = base_style.touchable;
    let clip = if base_style.overflow_x != OverflowMode::Visible
        || base_style.overflow_y != OverflowMode::Visible
    {
        Some(Rect::default())
    } else {
        None
    };
    let dirty_text = matches!(k, NodeKind::TextNode);
    let node = Node {
        id: NodeId::INVALID, // 临时，insert 后回填
        parent: None,
        kind: k,
        style: base_style.clone(),
        base_style,
        taffy_id: None,
        layout_rect: Rect::default(),
        clip_rect: clip,
        children: Vec::new(),
        dirty_mesh: true,
        dirty_text,
        classes: Vec::new(),
        id_attr: None,
        interaction: NodeInteraction {
            flags: NodeFlags::empty(),
            touchable,
            draggable: false,
            tabindex: None,
        },
        reuse_key: 0,
        inline_override: ResolvedStyle::default(),
        inline_set: InlineSet(0),
        user_transform: crate::transform::NodeTransform::default(),
    };
    let key = scene.nodes.insert(node);
    resize_parallel_arrays(scene);
    let id = NodeId::from_key(key);
    scene.nodes.get_mut(key).unwrap().id = id; // 回填
    if k == NodeKind::TextNode {
        scene.text_contents.insert(id, String::new());
    } else if k == NodeKind::Image {
        scene.image_srcs.insert(id, String::new());
    }
    Ok(id)
}

/// 建根节点：create_node + roots.push(id)。
pub fn create_root(scene: &mut Scene, kind: &str, css: &str) -> Result<NodeId, String> {
    let id = create_node(scene, kind, css)?;
    scene.roots.push(id);
    // 文档根 = 顶层作用域根（main-design §5.4 / public-api §2.3）。全局规则（scope_root=INVALID）
    // 跨作用域命中；文档根作为外层作用域，其直接子树（未嵌套其他实例根时）归属此作用域。
    if let Some(n) = scene.get_mut(id) {
        n.interaction
            .flags
            .insert(NodeFlags::SCOPE_ROOT | NodeFlags::LOOKUP_SCOPE);
    }
    Ok(id)
}

/// 建节点（从已 bake 的 kind + base_style）：instantiate 用。
/// 与 `create_node` 同构的节点构造（clip_rect 派生 / dirty_text / slotmap insert / id 回填），
/// 不涉及 CSS 解析——style 已在 ComponentTemplate.nodes[i].style 烘焙好（打包期产物）。
/// 直接用传入 style 作 base_style（源）+ style.clone() 作 style 初始（派生，下帧 rematch 从 base 起算）。
/// classes/id_attr/draggable/tabindex 由调用方在返回 NodeId 后填（与 create_node 一致——
/// （同 create_node：classes/id_attr 等由调用方在返回 NodeId 后填）。
/// `control_init` 为 `Some` 时按变体映射填 `Scene.controls` side table（Slider 补
/// 运行时独有 `dragging:false`）；`None` = 非控件节点，不建槽。
pub fn create_node_from_template(
    scene: &mut Scene,
    kind: NodeKind,
    base_style: ResolvedStyle,
    control_init: Option<ControlInit>,
) -> NodeId {
    let touchable = base_style.touchable;
    let clip = if base_style.overflow_x != OverflowMode::Visible
        || base_style.overflow_y != OverflowMode::Visible
    {
        Some(Rect::default())
    } else {
        None
    };
    let dirty_text = matches!(kind, NodeKind::TextNode);
    let node = Node {
        id: NodeId::INVALID, // 临时，insert 后回填
        parent: None,
        kind,
        style: base_style.clone(),
        base_style,
        taffy_id: None,
        layout_rect: Rect::default(),
        clip_rect: clip,
        children: Vec::new(),
        dirty_mesh: true,
        dirty_text,
        classes: Vec::new(),
        id_attr: None,
        interaction: NodeInteraction {
            flags: NodeFlags::empty(),
            touchable,
            draggable: false,
            tabindex: None,
        },
        reuse_key: 0,
        inline_override: ResolvedStyle::default(),
        inline_set: InlineSet(0),
        user_transform: crate::transform::NodeTransform::default(),
    };
    let key = scene.nodes.insert(node);
    resize_parallel_arrays(scene);
    let id = NodeId::from_key(key);
    scene.nodes.get_mut(key).unwrap().id = id; // 回填
    if kind == NodeKind::TextNode {
        scene.text_contents.insert(id, String::new());
    } else if kind == NodeKind::Image {
        scene.image_srcs.insert(id, String::new());
    }
    // 控件状态：按 ControlInit 变体映射填 ControlState（Slider 补运行时独有 dragging:false）。
    // 非 control 节点 control_init=None，不建槽（get 返 None，渲染/交互按无控件处理）。
    //
    // **sanitize（坑：clamp panic）**：ControlInit 的 min/max/value 来自打包期 HTML 属性
    // （`<progress max="-5">`、`<input min="100" max="0">`），无 schema 约束。下游所有
    // clamp 调用（指针交互 slider_pos_to_value/set_slider_value、FFI set_control_value）都用
    // f32::clamp(min,max)，它在 min>max 时 debug 断言 abort——FFI 边界 panic = 杀宿主进程。
    // 在此单一入口建立不变量（max≥0、min≤max、value∈[lo,hi]、step≥0），下游方可无守卫 clamp。
    if let Some(init) = control_init {
        let state = match init {
            ControlInit::Progress {
                value,
                max,
                indeterminate,
            } => {
                let max = max.max(0.0);
                ControlState::Progress {
                    value: value.clamp(0.0, max),
                    max,
                    indeterminate,
                }
            }
            ControlInit::Toggle { checked } => ControlState::Toggle { checked },
            ControlInit::Radio { checked, name } => ControlState::Radio { checked, name },
            ControlInit::Slider {
                value,
                min,
                max,
                step,
            } => {
                let max = max.max(min);
                ControlState::Slider {
                    value: value.clamp(min, max),
                    min,
                    max,
                    step: step.max(0.0),
                    dragging: false,
                }
            }
            ControlInit::TextField(e) => ControlState::TextField(EditState::from_init(
                e.value.clone(),
                e.placeholder.clone(),
                e.max_length,
                e.readonly,
            )),
            ControlInit::TextArea(e) => ControlState::TextArea(EditState::from_init(
                e.value.clone(),
                e.placeholder.clone(),
                e.max_length,
                e.readonly,
            )),
            ControlInit::Dropdown { selected_index } => ControlState::Dropdown {
                selected_index: selected_index as usize,
                open: false,
                value_lock: false,
                open_selected_index: None,
            },
            ControlInit::NumberField {
                edit,
                min,
                max,
                step,
            } => ControlState::NumberField {
                edit: EditState::from_init(
                    edit.value,
                    edit.placeholder,
                    edit.max_length,
                    edit.readonly,
                ),
                min,
                max,
                step,
            },
        };
        scene.controls.ensure(id, state);
        // 控件即容器：instantiate 后注入框架内部视觉子节点（.loom-fill/.loom-track/...）。
        // 紧跟 side table 填充之后——控件状态先就位，再挂视觉结构。非控件节点不走此分支。
        crate::scene::control::inject_control_children(scene, id, kind);
    }
    id
}

/// 递归克隆子树：返回游离新根（parent=None，不挂树，调用方 append_child 挂载）。
///
/// side table 判定（list spec §6）：
/// 拷贝：kind/classes/id_attr/base_style/text_contents/image_srcs（模板化数据，克隆出新实例）。
/// 不拷贝：scroll/anim/tweens/EditState/text_layouts/focused_node/事件订阅（运行时状态——
/// 克隆是干净模板，由调用方按需重设）。
///
/// 控件初值传 None：create_node_from_template 的 control_init 分支建控件视觉子树 + ControlState。
/// 列表 slot 场景下，控件值由 driver bind 后 `set_control_value` 显式设（slot 复用时 reset）。
pub(crate) fn clone_node_recursive(scene: &mut Scene, src: NodeId) -> NodeId {
    // 先取出源节点的不可变快照（kind/base_style/classes/id_attr/text/image_srcs），
    // drop 借后再可变借建新节点——避免边读边写 scene 的借用冲突。
    let (kind, base_style, classes, id_attr, content, src_path) = {
        let n = scene.get(src).expect("live src");
        (
            n.kind,
            n.base_style.clone(),
            n.classes.clone(),
            n.id_attr.clone(),
            scene.text_contents.get(&src).cloned(),
            scene.image_srcs.get(&src).cloned(),
        )
    };
    let new_id = create_node_from_template(scene, kind, base_style, None);
    {
        let n = scene.get_mut(new_id).unwrap();
        n.classes = classes;
        n.id_attr = id_attr;
    }
    if let Some(c) = content {
        scene.text_contents.insert(new_id, c);
    }
    if let Some(sp) = src_path {
        scene.image_srcs.insert(new_id, sp);
    }
    // 递归克隆子（先 clone children，避免边迭代边改 slotmap 的借用冲突）。
    let children = scene.get(src).expect("live src").children.clone();
    for child in children {
        let new_child = clone_node_recursive(scene, child);
        scene.get_mut(new_id).unwrap().children.push(new_child);
        scene.get_mut(new_child).unwrap().parent = Some(new_id);
    }
    new_id
}

/// 挂子：parent.children 末尾追加 + child.parent = Some(parent)。
/// child 必须当前无父（先 remove_child 摘除当前父）。重复挂同一父子对幂等（已含则 no-op）。
pub fn append_child(scene: &mut Scene, parent: NodeId, child: NodeId) -> Result<(), String> {
    // 先做存在性 + 无父检查（不可变借），drop 后再可变借写。
    {
        let p = scene.get(parent).ok_or("parent not live")?;
        if p.children.contains(&child) {
            return Ok(()); // 幂等：已挂同一父子对
        }
        if scene.get(child).and_then(|c| c.parent).is_some() {
            return Err("child already has parent（先 remove_child 摘除当前父）".into());
        }
    }
    scene.get_mut(parent).unwrap().children.push(child);
    scene.get_mut(child).unwrap().parent = Some(parent);
    Ok(())
}

/// 插子：在 parent.children 中 ref_id 之前插入 child。ref_id=INVALID → 末尾追加（同 append_child）。
/// child 必须当前无父。ref_id 必须在 parent.children 中。
pub fn insert_before(
    scene: &mut Scene,
    parent: NodeId,
    child: NodeId,
    ref_id: NodeId,
) -> Result<(), String> {
    if !ref_id.is_valid() {
        return append_child(scene, parent, child);
    }
    if scene.get(child).and_then(|c| c.parent).is_some() {
        return Err("child already has parent（先 remove_child 摘除当前父）".into());
    }
    let p = scene.get_mut(parent).ok_or("parent not live")?;
    let pos = p
        .children
        .iter()
        .position(|&c| c == ref_id)
        .ok_or("ref_id not in parent.children")?;
    p.children.insert(pos, child);
    scene.get_mut(child).unwrap().parent = Some(parent);
    Ok(())
}

/// 摘子：从 parent.children 移除 child + child.parent = None。
/// 与 remove_node 不同——节点不删（slotmap 槽保留，NodeId 仍 live），可再挂到别处。
///
/// **直系子校验**：child 的真实 parent 必须是传入的 parent，否则 Err。
/// 原实现无校验——`retain` 对非直系子无效（child 不在 parent.children）但仍清 child.parent，
/// 误断 child 与其真实 parent 的关系。调用方应先调 `append_child`/`insert_before` 摘除再挂。
pub fn remove_child(scene: &mut Scene, parent: NodeId, child: NodeId) -> Result<(), String> {
    let actual_parent = scene.get(child).and_then(|c| c.parent);
    if actual_parent != Some(parent) {
        return Err("remove_child: child is not a direct child of parent".into());
    }
    let p = scene.get_mut(parent).ok_or("parent not live")?;
    p.children.retain(|&c| c != child);
    scene.get_mut(child).unwrap().parent = None;
    Ok(())
}

/// 改 Text 节点 content + 标 dirty_text。非 Text 节点 → Err。
pub fn set_text(scene: &mut Scene, node: NodeId, text: &str) -> Result<(), String> {
    match scene.get(node).map(|n| n.kind) {
        None => return Err("set_text: node not live".into()),
        Some(NodeKind::TextNode) => {}
        Some(_) => return Err("set_text 只对 Text 节点生效".into()),
    }
    scene.text_contents.insert(node, text.into());
    scene.get_mut(node).unwrap().dirty_text = true;
    Ok(())
}

/// 改 Image 节点 src + 标 dirty_mesh。非 Image 节点 → Err。
pub fn set_src(scene: &mut Scene, node: NodeId, src: &str) -> Result<(), String> {
    match scene.get(node).map(|n| n.kind) {
        None => return Err("set_src: node not live".into()),
        Some(NodeKind::Image) => {}
        Some(_) => return Err("set_src 只对 Image 节点生效".into()),
    }
    scene.image_srcs.insert(node, src.into());
    scene.get_mut(node).unwrap().dirty_mesh = true;
    Ok(())
}

/// 写 inline override（便签层）：把 CSS 声明应用到 `inline_override` 字段，并把每个
/// 成功 apply 的 prop 对应 bit OR 进 `inline_set`。下帧 rematch 在动态规则之后应用，
/// 故 inline 优先级最高（> 动态规则 > base_style）。node 不 live → Err。
///
/// 复用 `apply_decl`（apply_css 同路径，不依赖 parse feature）。多次 set 同 prop 累加
/// （bit 幂等 OR，值覆盖）。
///
/// **bit 检查前置（review I1 修复）：** 不在 `inline_bit` 表的 prop（transform/filter/
/// border/padding-top/flex-grow/background-image/order/pointer-events/aspect-ratio 等
/// 约 20 个——它们走别的运行时路径或不在 NodeStyle 表面）**完全不写** `inline_override`，
/// 避免 ghost state（写字段但不置 bit → rematch `apply_inline_override` 不拷该字段 →
/// override 静默丢失；若后续 set 同族 longhand 置 bit，还会读到写字段时的旧 ghost 值）。
/// 语义上这些 prop 对便签层"不可表达"，等价于 apply_decl 返 false——不进 inline_override，
/// 不污染字段。
pub fn set_inline_override(scene: &mut Scene, node: NodeId, css: &str) -> Result<(), String> {
    let n = scene.get_mut(node).ok_or("node not live")?;
    for decl in css.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        if let Some((prop, val)) = decl.split_once(':') {
            let prop = prop.trim();
            // bit 检查前置：只对 inline_bit 表内的 prop apply。表外 prop 跳过 apply_decl，
            // 连字段都不写 inline_override，杜绝 ghost state。
            if let Some(bit) = inline_bit(prop) {
                if apply_decl(&mut n.inline_override, prop, val.trim()) {
                    n.inline_set.0 |= bit;
                }
            }
        }
    }
    n.dirty_mesh = true;
    Ok(())
}

/// 清 inline override 的某 prop bit（值保留在 `inline_override`，但下次 rematch 不再应用）。
/// 下帧 rematch 回落到 base_style / 动态规则值。prop 不可 inline（不在 `inline_bit` 表）
/// 时为 no-op（仍返 Ok，便于调用方无需判）。node 不 live → Err。
pub fn unset_inline_override(scene: &mut Scene, node: NodeId, prop: &str) -> Result<(), String> {
    let n = scene.get_mut(node).ok_or("node not live")?;
    if let Some(bit) = inline_bit(prop) {
        n.inline_set.0 &= !bit;
    }
    n.dirty_mesh = true;
    Ok(())
}

/// 写节点用户态 Transform（public-api Transform API 的 core 端落点）。
///
/// 写 `node.user_transform`，不触发 layout solve——`compute_world_transforms` 在世界矩阵
/// 累计时并入（同 CSS transform：渲染/命中层）。供高频拖拽（slider thumb）等运行时定位用：
/// 每帧写一次、下帧 compute 读取，避开 solve 开销。node 不 live → Err。
pub fn set_user_transform(
    scene: &mut Scene,
    node: NodeId,
    t: crate::transform::NodeTransform,
) -> Result<(), String> {
    let n = scene.get_mut(node).ok_or("node not live")?;
    n.user_transform = t;
    n.dirty_mesh = true;
    Ok(())
}

/// 读节点子节点数。node 不 live（已删 / 未建）→ None。
///
/// 给 C# 投影层 Container.Children / Get<T> 提供只读遍历入口。
pub fn get_child_count(scene: &Scene, node: NodeId) -> Option<usize> {
    scene.get(node).map(|n| n.children.len())
}

/// 读节点子节点列表（clone，调用方拿到独立 Vec）。node 不 live → None。
/// 叶子节点 → Some(vec![])（空 Vec，不是 None——区分"节点存在但无子" vs "节点不存在"）。
///
/// 给 C# 投影层 Container.Children / Get<T> 提供只读遍历入口。
pub fn get_children(scene: &Scene, node: NodeId) -> Option<Vec<NodeId>> {
    scene.get(node).map(|n| n.children.clone())
}

/// 加 class。重复名不重复 push。node 不 live → Err。标 dirty_mesh 触发下帧 rematch。
///
/// 给 C# 投影层 ClassList.Add 铺路（class 变化影响 cascade，须触发 rematch）。
pub fn add_class(scene: &mut Scene, node: NodeId, name: &str) -> Result<(), String> {
    let n = scene.get_mut(node).ok_or("node not live")?;
    if !n.classes.iter().any(|c| c == name) {
        n.classes.push(name.to_string());
    }
    n.dirty_mesh = true;
    Ok(())
}

/// 移除 class（全部匹配）。node 不 live → Err。标 dirty_mesh。
///
/// 给 C# 投影层 ClassList.Remove 铺路（class 变化影响 cascade，须触发 rematch）。
pub fn remove_class(scene: &mut Scene, node: NodeId, name: &str) -> Result<(), String> {
    let n = scene.get_mut(node).ok_or("node not live")?;
    n.classes.retain(|c| c != name);
    n.dirty_mesh = true;
    Ok(())
}

/// 查询 class 是否存在。node 不 live → None。
///
/// 给 C# 投影层 ClassList.Contains 铺路（只读查询，不改 dirty）。
pub fn has_class(scene: &Scene, node: NodeId, name: &str) -> Option<bool> {
    scene.get(node).map(|n| n.classes.iter().any(|c| c == name))
}

/// 设渲染复用键（虚拟列表 slot 用）。node 无效 → no-op（不 panic）。
pub fn set_reuse_key(scene: &mut Scene, node: NodeId, key: u32) {
    if let Some(n) = scene.get_mut(node) {
        n.reuse_key = key;
    }
}

/// 删节点：递归删子 → 从父/roots 摘除 → 联动清 anim/scroll/tween → slotmap remove。
///
/// NodeId 此后失效（slotmap gen++，Scene::get 返 None）。子树递归删。
/// anim/scroll/tween 联动清（HashMap remove / tween kill），防悬空残留。
/// 槽位可复用（slotmap remove 释放槽，下次 insert 复用 + gen++）。
///
/// 调用方须保证 `id` 为 live NodeId（已删节点 no-op：scene.get 返 None 直接返回）。
pub fn remove_node(scene: &mut Scene, tweens: &mut TweenManager, id: NodeId) {
    // 0. 已删/无效节点 → no-op（防重复删或悬空 id 调用 panic）。
    //    先取 children + parent（持有不可变借），drop 后再递归/可变借。
    let (children, parent_id, was_css_scope, _was_lookup_scope) = match scene.get(id) {
        Some(n) => (
            n.children.clone(),
            n.parent,
            n.interaction.flags.contains(NodeFlags::SCOPE_ROOT),
            n.interaction.flags.contains(NodeFlags::LOOKUP_SCOPE),
        ),
        None => return,
    };
    // 1. 递归删子（先 clone 了 children，避免边迭代边改 slotmap）。
    for c in children {
        remove_node(scene, tweens, c);
    }
    // 2. 从父摘除（或 roots）
    match parent_id {
        Some(pid) => {
            if let Some(p) = scene.get_mut(pid) {
                p.children.retain(|&c| c != id);
            }
        }
        None => scene.roots.retain(|&r| r != id),
    }
    // 3. 联动清持久附属 map（HashMap remove + tween kill），防悬空残留。
    scene.anim.clear_node(id);
    scene.scroll.remove(id);
    scene.controls.remove(id);
    scene.text_contents.remove(&id);
    scene.image_srcs.remove(&id);
    tweens.kill_node(id);
    // pending_transitions 不清：每帧首由 Stage drain/clear（stage.rs），瞬态，非持久泄漏；
    // 消费方对悬空 NodeId 有 None-check 兜底。
    // 3b. focused_node 联动清：删焦点节点后 focused_node 不应悬空（否则 FOCUS_OUT 带 stale node_id）。
    //     全局单一焦点，== Some(id) 检查对每个被删节点都做（递归删子时若子是焦点同样清）。
    if scene.focused_node == Some(id) {
        scene.focused_node = None;
    }
    // 3c. PointerState（Stage 层）的 down_node/hovered_chain/drag_target 等不在此清：
    //     消费点（input.rs）全有 scene.get None-check 兜底，悬空 NodeId 仅向已删节点发 stale 事件
    //     （RollOut/DRAG_MOVE），无 panic；强清需把 pointer_state 传进 remove_node（改签名），YAGNI。
    // 4. slotmap remove（gen++，NodeId 失效，槽位可复用）。
    //    经 key_for(NodeId) 桥接到 DefaultKey。
    scene.nodes.remove(scene.key_for(id));
    // 5. 作用域根销毁 → 连带清理其贡献的 dynamic_rules（scope_root == id）。
    //    防规则跨页残留污染（坑：旧实现不清理，切页后规则只增不减，跨组件同名 class 互相覆盖）。
    //    remove_node 递归删子，子作用域根的规则在此同样被清（每层递归各自清自己的 scope）。
    was_css_scope.then(|| {
        scene.dynamic_rules.entries.retain(|sr| sr.scope_root != id);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::node::NodeKind;
    use crate::style::resolved::ResolvedStyle;
    use crate::tween::{Ease, TweenProp};

    /// 建 3 层树：root → child → grandchild。用 Scene::build（不依赖动态建树 API）。
    fn build_3level() -> (Scene, NodeId, NodeId, NodeId) {
        let entries: Vec<(
            Option<usize>,
            NodeKind,
            ResolvedStyle,
            Vec<String>,
            Option<String>,
            bool,
            Option<i32>,
            Option<String>, // data_controller
            Option<String>,
            Option<String>,
        )> = vec![
            (
                None,
                NodeKind::Container,
                ResolvedStyle::default(),
                vec![],
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::Container,
                ResolvedStyle::default(),
                vec![],
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(1),
                NodeKind::Container,
                ResolvedStyle::default(),
                vec![],
                None,
                false,
                None,
                None,
                None,
                None,
            ),
        ];
        let scene = Scene::build(&entries);
        let root = scene.roots[0];
        let child = scene.get(root).unwrap().children[0];
        let grand = scene.get(child).unwrap().children[0];
        (scene, root, child, grand)
    }

    // ── Spec-4a A4：get_children / get_child_count（只读子节点遍历）──

    #[test]
    fn get_children_returns_node_children() {
        // build_3level: root → child → grand。覆盖中间节点（1 子）/ 叶子（0 子）/ 不存在节点。
        let (scene, root, child, grand) = build_3level();
        // root 有 1 子（child）
        assert_eq!(get_child_count(&scene, root), Some(1));
        assert_eq!(get_children(&scene, root), Some(vec![child]));
        // child 有 1 子（grand）—— 中间节点
        assert_eq!(get_child_count(&scene, child), Some(1));
        assert_eq!(get_children(&scene, child), Some(vec![grand]));
        // grand 是叶子 → 空 Vec（不是 None）
        assert_eq!(get_child_count(&scene, grand), Some(0));
        assert_eq!(get_children(&scene, grand), Some(vec![]));
        // 不存在节点 → None（slotmap 查不到）
        assert_eq!(get_child_count(&scene, NodeId(0xFFFF_FFFF)), None);
        assert_eq!(get_children(&scene, NodeId(0xFFFF_FFFF)), None);
    }

    // ── Spec-4a A5：add_class / remove_class / has_class（操作 Node.classes）──

    #[test]
    fn class_ops_mutate_and_flag_dirty() {
        // 用现有 build_3level() helper（scene/dynamic.rs tests，root→child→grand）
        let (mut scene, root, _child, _grand) = build_3level();
        add_class(&mut scene, root, "active").unwrap();
        assert!(has_class(&scene, root, "active").unwrap());
        assert!(
            scene.get(root).unwrap().dirty_mesh,
            "add 标 dirty 触发 rematch"
        );
        remove_class(&mut scene, root, "active").unwrap();
        assert!(!has_class(&scene, root, "active").unwrap());
        // 重复 add 不重复 push
        add_class(&mut scene, root, "x").unwrap();
        add_class(&mut scene, root, "x").unwrap();
        assert_eq!(
            scene
                .get(root)
                .unwrap()
                .classes
                .iter()
                .filter(|c| **c == "x")
                .count(),
            1
        );
        // 不存在节点 → Err / None
        assert!(add_class(&mut scene, NodeId(0xFFFF_FFFF), "y").is_err());
        assert_eq!(has_class(&scene, NodeId(0xFFFF_FFFF), "y"), None);
    }

    #[test]
    fn remove_node_clears_anim_scroll_and_kills_tween() {
        let (mut scene, root, child, _grand) = build_3level();
        let mut tweens = TweenManager::new();
        // 给 child 灌 anim/scroll/tween
        scene.anim.ensure(child).opacity = Some(0.5);
        scene.scroll.ensure(child);
        tweens.tween(
            child,
            TweenProp::Opacity,
            [0.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            Ease::Linear,
            0.0,
            1.0,
            0,
        );
        // 删 child
        remove_node(&mut scene, &mut tweens, child);
        // 联动清
        assert!(scene.anim.get(child).is_none(), "anim 清");
        assert!(scene.scroll.get(child).is_none(), "scroll 清");
        assert!(
            tweens.tweens.iter().all(|t| t.node != child || t.killed),
            "tween killed"
        );
        assert!(
            scene.get(child).is_none(),
            "slotmap removed（被删 NodeId 失效）"
        );
        // root 仍在，且 root.children 不含 child
        assert!(scene.get(root).is_some(), "root 未删");
        assert!(
            !scene.get(root).unwrap().children.contains(&child),
            "child 从父摘除"
        );
    }

    #[test]
    fn remove_node_recurses_children() {
        let (mut scene, root, child, grand) = build_3level();
        let mut tweens = TweenManager::new();
        // 给 grand 灌 anim
        scene.anim.ensure(grand).opacity = Some(0.5);
        // 删 root → 递归删 child + grand
        remove_node(&mut scene, &mut tweens, root);
        assert!(scene.get(root).is_none(), "root 删");
        assert!(scene.get(child).is_none(), "子递归删");
        assert!(scene.get(grand).is_none(), "孙递归删");
        assert!(scene.anim.get(grand).is_none(), "孙 anim 联动清");
        assert!(scene.roots.is_empty(), "roots 摘除");
    }

    #[test]
    fn remove_node_from_middle_clears_subtree_and_keeps_siblings() {
        // root → [a, b, c]；删 b → a/c 保留，b 子树（b → bchild）递归删。
        let entries: Vec<(
            Option<usize>,
            NodeKind,
            ResolvedStyle,
            Vec<String>,
            Option<String>,
            bool,
            Option<i32>,
            Option<String>, // data_controller
            Option<String>,
            Option<String>,
        )> = vec![
            (
                None,
                NodeKind::Container,
                ResolvedStyle::default(),
                vec![],
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::Container,
                ResolvedStyle::default(),
                vec![],
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::Container,
                ResolvedStyle::default(),
                vec![],
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::Container,
                ResolvedStyle::default(),
                vec![],
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(2),
                NodeKind::Container,
                ResolvedStyle::default(),
                vec![],
                None,
                false,
                None,
                None,
                None,
                None,
            ),
        ];
        let mut scene = Scene::build(&entries);
        let mut tweens = TweenManager::new();
        let root = scene.roots[0];
        let kids = scene.get(root).unwrap().children.clone();
        let (a, b, c) = (kids[0], kids[1], kids[2]);
        let bchild = scene.get(b).unwrap().children[0];
        scene.anim.ensure(bchild).opacity = Some(0.5);
        // 删 b
        remove_node(&mut scene, &mut tweens, b);
        assert!(scene.get(a).is_some(), "兄弟 a 保留");
        assert!(scene.get(c).is_some(), "兄弟 c 保留");
        assert!(scene.get(b).is_none(), "b 删");
        assert!(scene.get(bchild).is_none(), "bchild 递归删");
        assert!(scene.anim.get(bchild).is_none(), "bchild anim 清");
        // root.children 不含 b，但含 a/c
        let new_kids = scene.get(root).unwrap().children.clone();
        assert!(!new_kids.contains(&b), "b 从父摘除");
        assert!(
            new_kids.contains(&a) && new_kids.contains(&c),
            "a/c 保留在父 children"
        );
        assert_eq!(new_kids.len(), 2, "父 children 从 3 → 2");
    }

    #[test]
    fn remove_node_already_removed_is_noop() {
        let (mut scene, root, child, _grand) = build_3level();
        let mut tweens = TweenManager::new();
        // 删 child 两次：第二次 no-op（不 panic）
        remove_node(&mut scene, &mut tweens, child);
        remove_node(&mut scene, &mut tweens, child);
        assert!(scene.get(child).is_none());
        assert!(scene.get(root).is_some(), "root 仍在");
    }

    #[test]
    fn remove_node_clears_focused_node() {
        // 删焦点节点后 focused_node 应联动清（防 FOCUS_OUT 带 stale node_id）。
        let (mut scene, root, child, _grand) = build_3level();
        let mut tweens = TweenManager::new();
        scene.focused_node = Some(child);
        remove_node(&mut scene, &mut tweens, child);
        assert_eq!(scene.focused_node, None, "删焦点节点 → focused_node 清");
        assert!(scene.get(child).is_none(), "child 已删");
        assert!(scene.get(root).is_some(), "root 仍在");
    }

    #[test]
    fn remove_node_keeps_focused_node_when_other_deleted() {
        // 焦点是 root，删非焦点 child → focused_node 不变（指向 root 仍 live）。
        let (mut scene, root, child, _grand) = build_3level();
        let mut tweens = TweenManager::new();
        scene.focused_node = Some(root);
        remove_node(&mut scene, &mut tweens, child);
        assert_eq!(
            scene.focused_node,
            Some(root),
            "删非焦点 → focused_node 不变"
        );
    }

    #[test]
    fn remove_node_recursion_clears_focused_child() {
        // 递归删子时，若子是焦点也要清（root 删 → grand 是焦点 → focused_node 清）。
        let (mut scene, root, _child, grand) = build_3level();
        let mut tweens = TweenManager::new();
        scene.focused_node = Some(grand);
        remove_node(&mut scene, &mut tweens, root);
        assert_eq!(scene.focused_node, None, "递归删焦点子 → focused_node 清");
    }

    #[test]
    fn remove_node_slot_reuse_invalidates_old_nodeid() {
        // 删后槽位可复用：被删 NodeId 失效（gen++），新 insert 复用槽位但 NodeId 不同。
        let (mut scene, root, child, _grand) = build_3level();
        let mut tweens = TweenManager::new();
        let child_id_old = child;
        remove_node(&mut scene, &mut tweens, child);
        assert!(
            scene.get(child_id_old).is_none(),
            "被删 NodeId 失效（gen++）"
        );
        // 新 insert（复用槽位）
        let new_key = scene.nodes.insert(crate::scene::node::Node::default());
        let new_id = crate::scene::node::NodeId::from_key(new_key);
        // child_id_old 与新 new_id 不同（gen 不同），被删 id 仍 None
        assert!(scene.get(child_id_old).is_none(), "被删 NodeId 仍失效");
        assert!(scene.get(new_id).is_some(), "新 NodeId live");
        // root 仍在
        assert!(scene.get(root).is_some());
    }

    // ---- 动态建树 API 单元测试（自由函数级，不依赖 Stage） ----

    fn empty_scene() -> Scene {
        Scene::default()
    }

    #[test]
    fn kind_from_tag_maps_fence_whitelist() {
        assert!(matches!(kind_from_tag("div").unwrap(), NodeKind::Container));
        assert!(matches!(kind_from_tag("button").unwrap(), NodeKind::Button));
        assert!(matches!(kind_from_tag("img").unwrap(), NodeKind::Image));
        assert!(matches!(kind_from_tag("span").unwrap(), NodeKind::TextNode));
    }

    #[test]
    fn kind_from_tag_unknown_returns_err() {
        assert!(kind_from_tag("ul").is_err());
        assert!(kind_from_tag("l-container").is_err()); // 不在围栏白名单内，与 div 同映射冗余
        assert!(kind_from_tag("").is_err());
    }

    #[test]
    fn apply_css_sets_width_and_background() {
        let mut s = ResolvedStyle::default();
        apply_css(&mut s, "width:100px;height:50px;background-color:#ff0000");
        use taffy::style::Dimension;
        assert_eq!(s.taffy_style.size.width, Dimension::length(100.0));
        assert_eq!(s.taffy_style.size.height, Dimension::length(50.0));
        assert_eq!(s.background_color, Some([1.0, 0.0, 0.0, 1.0]));
    }

    #[test]
    fn apply_css_ignores_empty_and_malformed() {
        let mut s = ResolvedStyle::default();
        // 空串 / 纯空白 / 无冒号 / 空声明 → 不 panic，不误改
        apply_css(&mut s, "");
        apply_css(&mut s, "   ;  ; ");
        apply_css(&mut s, "noscolon");
        apply_css(&mut s, "width:200px");
        use taffy::style::Dimension;
        assert_eq!(s.taffy_style.size.width, Dimension::length(200.0));
    }

    #[test]
    fn apply_css_unknown_decl_silently_ignored() {
        let mut s = ResolvedStyle::default();
        apply_css(&mut s, "unknown-prop:42px;width:100px");
        use taffy::style::Dimension;
        assert_eq!(
            s.taffy_style.size.width,
            Dimension::length(100.0),
            "known 声明生效"
        );
    }

    #[test]
    fn create_node_fills_base_style_and_id() {
        let mut scene = empty_scene();
        let id = create_node(&mut scene, "div", "width:100px;height:100px").unwrap();
        let n = scene.get(id).unwrap();
        assert_eq!(n.id, id, "id 回填");
        assert!(n.parent.is_none());
        use taffy::style::Dimension;
        assert_eq!(
            n.base_style.taffy_style.size.width,
            Dimension::length(100.0)
        );
        // style 初始 = base_style.clone()
        assert_eq!(n.style, n.base_style);
        assert!(n.dirty_mesh, "新建节点 dirty_mesh=true");
    }

    #[test]
    fn create_node_applies_schema_display_default() {
        // 新范式：未声明 display 的元素按 tag 的 DisplayDefault 铺底（对齐打包器 css_resolve）。
        // div 是 block 标签 → display_mode=Block + taffy display=Block（不是旧范式的 Flex）。
        // 运行时 create_node 必须复刻 css_resolve 的 schema 铺底，否则动态建的 div 会是 Flex。
        use crate::style::resolved::DisplayMode;
        let mut scene = empty_scene();
        let div = create_node(&mut scene, "div", "").unwrap();
        let n = scene.get(div).unwrap();
        assert_eq!(
            n.base_style.display_mode,
            DisplayMode::Block,
            "div（block tag）默认该是 Block，不是 Flex"
        );
        assert_eq!(
            n.base_style.taffy_style.display,
            taffy::Display::Block,
            "taffy display 也要 Block"
        );
        // button/img 是 inline tag → 运行时映射成 Flex（taffy 兼容，同 css_resolve）。
        let btn = create_node(&mut scene, "button", "").unwrap();
        assert_eq!(
            scene.get(btn).unwrap().base_style.display_mode,
            DisplayMode::Flex,
            "button（inline tag）默认映射成 Flex"
        );
    }

    #[test]
    fn create_node_text_marks_dirty_text() {
        let mut scene = empty_scene();
        let id = create_node(&mut scene, "span", "").unwrap();
        let n = scene.get(id).unwrap();
        assert!(n.dirty_text, "Text 节点 dirty_text=true");
        assert!(matches!(n.kind, NodeKind::TextNode));
    }

    #[test]
    fn create_node_clip_rect_for_overflow_hidden() {
        let mut scene = empty_scene();
        let id = create_node(&mut scene, "div", "overflow:hidden").unwrap();
        assert!(
            scene.get(id).unwrap().clip_rect.is_some(),
            "overflow:hidden → clip slot"
        );
        let id2 = create_node(&mut scene, "div", "").unwrap();
        assert!(
            scene.get(id2).unwrap().clip_rect.is_none(),
            "默认 Visible → 无 clip slot"
        );
    }

    #[test]
    fn create_node_from_template_uses_baked_style() {
        // instantiate 调用的节点构造（不含 bake 后的 kind+style），不涉及 CSS 解析。
        let mut scene = empty_scene();
        let mut style = ResolvedStyle::default();
        apply_css(
            &mut style,
            "width:100px;height:100px;overflow:hidden;background-color:#ff0000",
        );
        let id = create_node_from_template(&mut scene, NodeKind::Container, style.clone(), None);
        let n = scene.get(id).unwrap();
        assert_eq!(n.id, id, "id 回填");
        assert!(n.parent.is_none());
        assert_eq!(n.base_style, style, "base_style = 传入 baked style");
        assert_eq!(n.style, n.base_style, "style 初始 = base_style.clone()");
        assert!(n.dirty_mesh, "新建节点 dirty_mesh=true");
        assert!(
            n.clip_rect.is_some(),
            "overflow:hidden → clip slot（同 create_node）"
        );
    }

    #[test]
    fn create_node_from_template_text_marks_dirty_text() {
        let mut scene = empty_scene();
        let id = create_node_from_template(
            &mut scene,
            NodeKind::TextNode,
            ResolvedStyle::default(),
            None,
        );
        scene.text_contents.insert(id, "hi".into());
        let n = scene.get(id).unwrap();
        assert!(n.dirty_text, "Text 节点 dirty_text=true（同 create_node）");
        assert_eq!(scene.text_contents.get(&id).map(|s| s.as_str()), Some("hi"));
    }

    #[test]
    fn create_node_from_template_id_is_live() {
        let mut scene = empty_scene();
        let id = create_node_from_template(
            &mut scene,
            NodeKind::Container,
            ResolvedStyle::default(),
            None,
        );
        assert!(scene.get(id).is_some(), "返回的 NodeId live");
        assert_ne!(id, NodeId::INVALID);
    }

    #[test]
    fn create_node_from_template_control_init_injects_children() {
        // 传 control_init 的控件节点：side table 填充 + 视觉子节点注入都要发生。
        // 验子节点注入接线：create_node_from_template 内部调 inject_control_children。
        let mut scene = empty_scene();
        let id = create_node_from_template(
            &mut scene,
            NodeKind::ProgressBar,
            ResolvedStyle::default(),
            Some(ControlInit::Progress {
                value: 0.5,
                max: 1.0,
                indeterminate: false,
            }),
        );
        // side table 填了
        assert!(
            scene.controls.get(id).is_some(),
            "control side table filled"
        );
        // 视觉子节点注入了（ProgressBar → 1 个 loom-fill 子）
        let children = scene.get(id).unwrap().children.clone();
        assert_eq!(children.len(), 1, "ProgressBar injects fill child");
        assert!(scene
            .get(children[0])
            .unwrap()
            .classes
            .iter()
            .any(|c| c == "loom-fill"));
    }

    #[test]
    fn create_node_from_template_no_control_init_injects_nothing() {
        // control_init=None 的节点（即使是控件 kind）不注入子节点。
        // inject 只在 control_init.is_some() 分支触发。
        let mut scene = empty_scene();
        let id = create_node_from_template(
            &mut scene,
            NodeKind::ProgressBar,
            ResolvedStyle::default(),
            None,
        );
        assert!(scene.controls.get(id).is_none());
        assert!(
            scene.get(id).unwrap().children.is_empty(),
            "no control_init → no injected children"
        );
    }

    #[test]
    fn create_root_pushes_to_roots() {
        let mut scene = empty_scene();
        let r = create_root(&mut scene, "div", "").unwrap();
        assert_eq!(scene.roots, vec![r]);
    }

    #[test]
    fn append_child_links_parent_and_child() {
        let mut scene = empty_scene();
        let root = create_root(&mut scene, "div", "").unwrap();
        let child = create_node(&mut scene, "div", "").unwrap();
        append_child(&mut scene, root, child).unwrap();
        assert_eq!(scene.get(root).unwrap().children, vec![child]);
        assert_eq!(scene.get(child).unwrap().parent, Some(root));
    }

    #[test]
    fn append_child_idempotent_same_pair() {
        let mut scene = empty_scene();
        let root = create_root(&mut scene, "div", "").unwrap();
        let child = create_node(&mut scene, "div", "").unwrap();
        append_child(&mut scene, root, child).unwrap();
        // 二次挂同一对 → 幂等 no-op（不报错，children 不重复）
        append_child(&mut scene, root, child).unwrap();
        assert_eq!(scene.get(root).unwrap().children.len(), 1);
    }

    #[test]
    fn append_child_rejects_child_with_existing_parent() {
        let mut scene = empty_scene();
        let a = create_root(&mut scene, "div", "").unwrap();
        let b = create_node(&mut scene, "div", "").unwrap();
        let c = create_node(&mut scene, "div", "").unwrap();
        append_child(&mut scene, a, c).unwrap();
        // c 已有父 a → 挂到 b 应报错
        let err = append_child(&mut scene, b, c);
        assert!(err.is_err(), "child 已有父 → Err");
    }

    #[test]
    fn insert_before_inserts_in_middle() {
        let mut scene = empty_scene();
        let root = create_root(&mut scene, "div", "").unwrap();
        let a = create_node(&mut scene, "div", "").unwrap();
        let b = create_node(&mut scene, "div", "").unwrap();
        let c = create_node(&mut scene, "div", "").unwrap();
        append_child(&mut scene, root, a).unwrap();
        append_child(&mut scene, root, b).unwrap();
        // 在 a 之前插 c → [c, a, b]
        insert_before(&mut scene, root, c, a).unwrap();
        assert_eq!(scene.get(root).unwrap().children, vec![c, a, b]);
        assert_eq!(scene.get(c).unwrap().parent, Some(root));
    }

    #[test]
    fn insert_before_invalid_ref_appends() {
        let mut scene = empty_scene();
        let root = create_root(&mut scene, "div", "").unwrap();
        let a = create_node(&mut scene, "div", "").unwrap();
        let b = create_node(&mut scene, "div", "").unwrap();
        append_child(&mut scene, root, a).unwrap();
        // ref=INVALID → 末尾追加
        insert_before(&mut scene, root, b, NodeId::INVALID).unwrap();
        assert_eq!(scene.get(root).unwrap().children, vec![a, b]);
    }

    #[test]
    fn insert_before_missing_ref_returns_err() {
        let mut scene = empty_scene();
        let root = create_root(&mut scene, "div", "").unwrap();
        let a = create_node(&mut scene, "div", "").unwrap();
        append_child(&mut scene, root, a).unwrap();
        // 造一个 valid 但不在 root.children 的 NodeId 作 ref
        let orphan = create_node(&mut scene, "div", "").unwrap();
        let new_child = create_node(&mut scene, "div", "").unwrap();
        let err = insert_before(&mut scene, root, new_child, orphan);
        assert!(err.is_err(), "ref 不在 parent.children → Err");
    }

    #[test]
    fn remove_child_detaches_but_keeps_node() {
        let mut scene = empty_scene();
        let root = create_root(&mut scene, "div", "").unwrap();
        let child = create_node(&mut scene, "div", "").unwrap();
        append_child(&mut scene, root, child).unwrap();
        remove_child(&mut scene, root, child).unwrap();
        assert!(scene.get(root).unwrap().children.is_empty());
        assert!(scene.get(child).unwrap().parent.is_none(), "child 变孤立");
        assert!(
            scene.get(child).is_some(),
            "child 仍存活（未删 slotmap 槽）"
        );
    }

    #[test]
    fn remove_child_rejects_non_direct_child() {
        // 直系子校验：b 的真实 parent 是 root，remove_child(a, b) 应 Err，
        // 且不动 b.parent / a.children / root.children（防误断真实父子关系）。
        let mut scene = empty_scene();
        let root = create_root(&mut scene, "div", "").unwrap();
        let a = create_node(&mut scene, "div", "").unwrap();
        let b = create_node(&mut scene, "div", "").unwrap();
        append_child(&mut scene, root, a).unwrap();
        append_child(&mut scene, root, b).unwrap();
        // b 的 parent 是 root，不是 a → remove_child(a, b) 应 Err。
        let err = remove_child(&mut scene, a, b);
        assert!(err.is_err(), "remove_child on non-direct child must error");
        assert_eq!(
            scene.get(b).unwrap().parent,
            Some(root),
            "b.parent must stay root"
        );
        assert!(
            !scene.get(a).unwrap().children.contains(&b),
            "a.children unchanged"
        );
        assert!(
            scene.get(root).unwrap().children.contains(&b),
            "root.children still has b"
        );
    }

    #[test]
    fn set_text_changes_content_and_marks_dirty() {
        let mut scene = empty_scene();
        let t = create_node(&mut scene, "span", "").unwrap();
        // create_node 时 dirty_text=true（Text 节点），先清掉验 set_text 重标
        scene.get_mut(t).unwrap().dirty_text = false;
        set_text(&mut scene, t, "hello").unwrap();
        assert!(scene.get(t).unwrap().dirty_text);
        assert_eq!(
            scene.text_contents.get(&t).map(|s| s.as_str()),
            Some("hello")
        )
    }

    #[test]
    fn set_text_rejects_non_text() {
        let mut scene = empty_scene();
        let d = create_node(&mut scene, "div", "").unwrap();
        assert!(set_text(&mut scene, d, "x").is_err());
    }

    #[test]
    fn set_src_changes_src_and_marks_dirty_mesh() {
        let mut scene = empty_scene();
        let img = create_node(&mut scene, "img", "").unwrap();
        scene.get_mut(img).unwrap().dirty_mesh = false;
        set_src(&mut scene, img, "icon.png").unwrap();
        assert!(scene.get(img).unwrap().dirty_mesh);
        assert_eq!(
            scene.image_srcs.get(&img).map(|s| s.as_str()),
            Some("icon.png")
        )
    }

    #[test]
    fn set_src_rejects_non_image() {
        let mut scene = empty_scene();
        let d = create_node(&mut scene, "div", "").unwrap();
        assert!(set_src(&mut scene, d, "x").is_err());
    }

    #[test]
    fn create_node_id_is_live_via_get() {
        // slotmap insert 返回的 NodeId 经 from_key 转换，Scene::get 能查到（to_key roundtrip）
        let mut scene = empty_scene();
        let id = create_node(&mut scene, "div", "").unwrap();
        assert!(scene.get(id).is_some(), "create_node 返回的 NodeId live");
        assert_ne!(id, NodeId::INVALID);
    }

    #[test]
    fn append_child_builds_multi_level_tree() {
        let mut scene = empty_scene();
        let root = create_root(&mut scene, "div", "").unwrap();
        let c1 = create_node(&mut scene, "div", "").unwrap();
        let c2 = create_node(&mut scene, "div", "").unwrap();
        append_child(&mut scene, root, c1).unwrap();
        append_child(&mut scene, c1, c2).unwrap();
        assert_eq!(scene.get(root).unwrap().children, vec![c1]);
        assert_eq!(scene.get(c1).unwrap().children, vec![c2]);
        assert_eq!(scene.get(c2).unwrap().parent, Some(c1));
    }

    #[test]
    fn create_node_resizes_parallel_arrays_on_slotmap_expansion() {
        // 动态 create_node 后 parallel arrays (text_layouts)
        // 必须对齐 slotmap capacity，否则后续按 NodeId.index() 访问会越界 panic。
        let mut scene = empty_scene();
        // 创建大量节点确保触发 slotmap 容量增长（初始 capacity 较小）。
        let mut ids = Vec::new();
        for _ in 0..64 {
            let id = create_node(&mut scene, "div", "").unwrap();
            // 每个新 NodeId 的 index 应在 parallel arrays 范围内
            assert!(
                id.index() < scene.text_layouts.len(),
                "text_layouts must cover node index {} (len {})",
                id.index(),
                scene.text_layouts.len()
            );
            ids.push(id);
        }
        // 最终 arrays 至少为 capacity+1（1 基索引，idx 0 占位）
        let cap = scene.nodes.capacity();
        assert!(
            scene.text_layouts.len() > cap,
            "text_layouts len {} > capacity {}",
            scene.text_layouts.len(),
            cap
        );
    }

    #[test]
    fn create_node_from_template_resizes_parallel_arrays_on_slotmap_expansion() {
        let mut scene = empty_scene();
        for _ in 0..64 {
            let id = create_node_from_template(
                &mut scene,
                NodeKind::Container,
                ResolvedStyle::default(),
                None,
            );
            assert!(
                id.index() < scene.text_layouts.len(),
                "text_layouts must cover node index {} (len {})",
                id.index(),
                scene.text_layouts.len()
            );
        }
        let cap = scene.nodes.capacity();
        assert!(scene.text_layouts.len() > cap);
    }

    // ── clone_subtree：场景级子树深拷贝（list spec §6 side table 判定）──

    #[test]
    fn clone_subtree_copies_structure_text_image_classes() {
        // clone_subtree 返游离新根：深拷结构 + text_contents + image_srcs + classes，
        // 新根 parent=None（不挂树，调用方负责 append）。文本/图片内容映射到新 NodeId。
        let mut s = crate::stage::Stage::new_for_test();
        let root = s.create_root("div", "").unwrap();
        let img = s.create_node("img", "").unwrap();
        s.set_src(img, "icon.png").unwrap();
        s.append_child(root, img).unwrap();
        let txt = s.create_node("span", "").unwrap();
        s.set_text(txt, "hello").unwrap();
        s.append_child(root, txt).unwrap();

        let cloned = s.clone_subtree(root).unwrap();
        let scene = s.scene.as_ref().unwrap();
        assert!(
            scene.get(cloned).unwrap().parent.is_none(),
            "cloned root is detached"
        );
        assert_eq!(scene.get(cloned).unwrap().children.len(), 2);
        assert_ne!(cloned, root, "新 NodeId 不同于源");
        // image_srcs 映射到新 NodeId（按 kind 找到对应 img 子节点）
        let cloned_kids: Vec<_> = scene.get(cloned).unwrap().children.clone();
        let img_child = cloned_kids
            .iter()
            .copied()
            .find(|&c| scene.get(c).unwrap().kind == NodeKind::Image)
            .unwrap();
        assert_eq!(
            scene.image_srcs.get(&img_child).map(|s| s.as_str()),
            Some("icon.png")
        );
        // text_contents 映射到新 NodeId（按 kind 找到对应 span 子节点）
        let txt_child = cloned_kids
            .iter()
            .copied()
            .find(|&c| scene.get(c).unwrap().kind == NodeKind::TextNode)
            .unwrap();
        assert_eq!(
            scene.text_contents.get(&txt_child).map(|s| s.as_str()),
            Some("hello")
        );
    }

    #[test]
    fn clone_subtree_skips_runtime_side_tables() {
        // side table 判定（list spec §6）：运行时状态（scroll/anim/tween/EditState）
        // 不深拷——克隆根是干净模板，调用方按需重设。这里验 scroll 副表不残留：
        // 即使源根是 scroll 容器，克隆根无 ScrollPaneState 或 scroll_pos 归零。
        let mut s = crate::stage::Stage::new_for_test();
        let root = s.create_root("div", "overflow:auto").unwrap();
        let cloned = s.clone_subtree(root).unwrap();
        let scene = s.scene.as_ref().unwrap();
        let scroll_zero = scene
            .scroll
            .get(cloned)
            .map(|st| st.scroll_pos.1 == 0.0)
            .unwrap_or(true);
        assert!(scroll_zero, "scroll 运行时状态不得克隆");
    }
}
