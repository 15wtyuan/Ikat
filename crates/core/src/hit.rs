//! 命中测试：输入 design 坐标点 → 返回命中 NodeId。
//! 逆等效绘制序遍历（顶层优先），layout_rect AABB + clip 门控 + pointer-events。
//! 命中几何 = layout_rect 经累计 world_matrix 逆变换回节点本地空间（transform 动画
//! 生效；world_transforms 用上帧值，1 帧延迟语义见 hit_subtree bounds guard）。

use crate::scene::node::{ControlState, NodeId, Rect, Scene};

/// 点是否在 Rect 内（含边界，design 坐标）。
pub(crate) fn point_in_rect(point: (f32, f32), r: Rect) -> bool {
    point.0 >= r.x && point.0 <= r.x + r.w && point.1 >= r.y && point.1 <= r.y + r.h
}

// 绘制序不再本地推导：hit 与 render 共用 crate::scene::stacking::paint_order
//（stacking context 全局分层，#100——嵌套 static 子树里的 opacity/transform/
// 定位+声明 z 后代会上提层，逆序遍历即顶层优先，语义单一真相源）。flex `order`
// 的兄弟序也由该走查统一（order-modified tree order）。

/// 命中合成 scrollbar thumb → (container_id, axis: 0=v 1=h)。None 不命中。
/// scrollbar 最上层——遍历所有容器 check v/h thumb rect。
pub fn hit_scrollbar_grip(scene: &Scene, point: (f32, f32)) -> Option<(NodeId, u8)> {
    for (_key, n) in &scene.nodes {
        let nid = n.id;
        if let Some(r) = crate::scroll::v_thumb_rect(scene, nid) {
            if point_in_rect(point, r) {
                return Some((nid, 0));
            }
        }
        if let Some(r) = crate::scroll::h_thumb_rect(scene, nid) {
            if point_in_rect(point, r) {
                return Some((nid, 1));
            }
        }
    }
    None
}

/// 命中 open Dropdown 的 popup 子树。open popup 浮层渲染在所有正常内容之上（
/// mask=0、末尾追加 DFS），故命中优先级高于正常内容——在主 roots DFS 前测。
///
/// 与 [`hit_scrollbar_grip`] 的优先级：scrollbar grip 仍先于此（grip 返 sentinel NodeId）。
/// 理由：popup 自身可滚（长 option 列表）时其 scrollbar grip 不能被 popup 命中遮蔽——
/// grip 必须可抓。grip 与正常 dropdown popup 几何重叠极少（grip 在滚动容器边缘、popup
/// 绝对定位浮层），两者互不冲突。返真节点（option/popup）的 NodeId。
fn hit_open_popups(scene: &Scene, point: (f32, f32)) -> Option<NodeId> {
    // 先收集所有 open Dropdown 的 listbox 根（不可变借），再逐个 hit_subtree（不可变借）。
    // 两阶段分开避免边迭代 controls 边递归 scene 的复杂借用。收集口径与 render 层
    // `collect_open_popup_roots` 一致（同源：open Dropdown → role=listbox 子节点，递归定位）。
    let mut popups: Vec<NodeId> = Vec::new();
    for n in scene.nodes.values() {
        let is_open_dropdown = matches!(
            scene.controls.get(n.id),
            Some(ControlState::Dropdown { open: true, .. })
        );
        if !is_open_dropdown {
            continue;
        }
        if let Some(popup) = crate::scene::control::find_child_by_role_recursive(
            scene,
            n.id,
            crate::scene::control::ROLE_LISTBOX,
        ) {
            popups.push(popup);
        }
    }
    for popup in popups {
        if let Some(hit) = hit_subtree(scene, popup, point) {
            return Some(hit);
        }
    }
    None
}

/// 命中测试。逆等效绘制序遍历，第一个命中即返回（顶层优先）。
/// scrollbar thumb 最上层，前置 check。
pub fn hit_test(scene: &Scene, point: (f32, f32)) -> Option<NodeId> {
    if let Some((container, axis)) = hit_scrollbar_grip(scene, point) {
        let flag = if axis == 0 {
            crate::scroll::V_THUMB_FLAG
        } else {
            crate::scroll::H_THUMB_FLAG
        };
        return Some(NodeId(container.0 | flag));
    }
    // open popup 浮层渲染在所有正常内容之上 → 命中优先于正常内容。
    // 顺序在 scrollbar grip 之后（见 [`hit_open_popups`] 文档：grip 须可抓）。
    if let Some(hit) = hit_open_popups(scene, point) {
        return Some(hit);
    }
    // 多 root：渲染序后 root 画在上层（roots 序追加 = 顶层）→ 命中先测后 root。
    for &root in scene.roots.iter().rev() {
        if let Some(hit) = hit_subtree(scene, root, point) {
            return Some(hit);
        }
    }
    None
}

/// 测某子树：绘制序取 [`crate::scene::stacking::paint_order`]（render 同源，
/// #100：嵌套 static 子树里的 opacity/transform/定位+声明 z 后代会上提层），逆序
/// 遍历 = 顶层优先。父恒先于子绘制 → 逆序子先测，父自然成为子的 fallback。
/// 逐节点独立检查：box/clip 门/touchable 在 [`hit_node`]；clip 门沿祖先链求值
/// （见 [`clip_gate_passed`]），与逐父递归时代的「祖先 gate 失败剪整子树」等价。
fn hit_subtree(scene: &Scene, id: NodeId, point: (f32, f32)) -> Option<NodeId> {
    // include = world_transforms 缺席守卫（bounds guard 的子树粒度版：结构变更帧
    // 新增节点本帧 transforms 未算 / 首帧空 → 整子树不进画序，1 帧延迟语义）。
    let order = crate::scene::stacking::paint_order(scene, id, &|n| {
        scene.world_transforms.get(n.index()).is_some()
    });
    // clipper 祖先门求值缓存：同一次命中查询内，同一点对同一 clipper 只算一次
    //（多后代共享祖先 gate；逆逆变换不便宜）。
    let mut gate_cache: std::collections::HashMap<NodeId, bool> = std::collections::HashMap::new();
    for nid in order.iter().rev() {
        if let Some(hit) = hit_node(scene, *nid, point, &mut gate_cache) {
            return Some(hit);
        }
    }
    None
}

/// clip 门控：节点未被任何 clipper 祖先（含自身 `clip_rect`）挡住。
///
/// gate(A) = 点在 A 的页面坐标里落进 A.clip_rect。页面点 = 逆世界变换（已含
/// 滚动逆变换）回 A 本地 + A.layout_rect 偏移——与 clip 同空间（页面绝对坐标、
/// 不含滚动；拿屏幕点直接比会在祖先滚动下失配，嵌套滚动整树穿透）。递归时代的
/// 语义是「A 的 gate 失败 → A 整棵子树跳过」；扁平序下等价于「后代逐个沿祖先
/// 链问 gate」，结果一致但顺序无关。
fn clip_gate_passed(
    scene: &Scene,
    id: NodeId,
    point: (f32, f32),
    cache: &mut std::collections::HashMap<NodeId, bool>,
) -> bool {
    let node = match scene.get(id) {
        Some(n) => n,
        None => return true, // 死 id 防御：无 gate 可挡
    };
    if let Some(&passed) = cache.get(&id) {
        return passed;
    }
    let mut passed = true;
    if let Some(clip) = node.clip_rect {
        // world_transforms 缺席 → bounds guard 语义：本 gate 挡下（paint_order 的
        // include 已按子树剪过，这里到不了；防御分支）。
        if let Some(wm) = scene.world_transforms.get(id.index()) {
            let inv = crate::transform::inverse(wm);
            let (lx, ly) = crate::transform::apply_point(&inv, point.0, point.1);
            let lr = node.layout_rect;
            passed = point_in_rect((lx + lr.x, ly + lr.y), clip);
        }
    }
    // 自身 gate 过了还要过祖先的（祖先 gate 挡 = 整子树不可命中）。
    if passed {
        if let Some(p) = node.parent {
            passed = clip_gate_passed(scene, p, point, cache);
        }
    }
    cache.insert(id, passed);
    passed
}

/// 单节点命中检查：world 逆变换 → 本地 box → clip 门（祖先链）→ touchable。
/// rich-text-block 命中细化到 inline 流 source（span 事件归属契约）。
fn hit_node(
    scene: &Scene,
    id: NodeId,
    point: (f32, f32),
    gate_cache: &mut std::collections::HashMap<NodeId, bool>,
) -> Option<NodeId> {
    let node = scene.get_live(id, "hit/hit_node");
    // bounds guard（子树版已在 paint_order include 剪掉；此处防御）。
    let wm = scene.world_transforms.get(id.index())?;
    let inv = crate::transform::inverse(wm);
    // 点逆投到节点本地空间（box 判定的 (0,0,w,h) 系）。
    let (lx, ly) = crate::transform::apply_point(&inv, point.0, point.1);
    let lr = node.layout_rect;
    // clip 门控：自身 + 祖先链（见 [`clip_gate_passed`]）。
    if !clip_gate_passed(scene, id, point, gate_cache) {
        return None;
    }
    if node.interaction.touchable && lx >= 0.0 && lx <= lr.w && ly >= 0.0 && ly <= lr.h {
        // rich-text-block：命中容器后细化到 inline 流的 source 节点（span / TextNode /
        // Image）——事件归属契约（公共树保留 span，订阅 span 的 click 命中 span 本身，
        // 非容器）。坐标同空间（box 判定的 (0,0,w,h) 系；hit_test_rich 自扣
        // border/padding）。source 不可触摸 / 未细化中（无 text_layout，首帧）→ 回落
        // 容器（HTML 语义：纯文本段的点击目标也是宿主元素）。
        if node.rich_text_block {
            if let Some(src) = crate::text::hit_test::hit_test_rich(scene, id, (lx, ly)) {
                let src_touchable = scene.get(src).is_some_and(|sn| sn.interaction.touchable);
                if src_touchable {
                    return Some(src);
                }
            }
        }
        return Some(id);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::node::{Node, NodeFlags, NodeId, NodeKind, Rect, Scene};
    use crate::scene::transform::compute_world_transforms;
    use crate::style::resolved::LocalTransform;
    use crate::transform;
    use crate::transform::Affine2Ext;

    /// #44：rich-text-block 容器命中细化到 run.source（span 事件归属契约：
    /// 公共树保留 span，订阅 span 的 click 命中 span 本身，非容器）。
    /// 命中 span 区域 → span；text_layouts 缺席（首帧）→ 回落容器。
    #[test]
    fn hit_test_resolves_rich_block_to_span_source() {
        use crate::layout::solve;
        use crate::style::resolved::ResolvedStyle;
        use crate::text::layout::FontTable;
        use std::collections::HashMap;
        let font_path = format!(
            "{}/tests/fixtures/DejaVuSans.ttf",
            env!("CARGO_MANIFEST_DIR")
        );
        let Ok(bytes) = std::fs::read(&font_path) else {
            return; // 字体 fixture 缺席的环境跳过（同 text/hit_test.rs 口径）
        };
        let mut ft = FontTable::new();
        ft.register("default", bytes, true).unwrap();

        let mut root_s = ResolvedStyle::default();
        root_s.taffy_style.size.width = taffy::style::Dimension::length(200.0);
        let mut div_s = ResolvedStyle::default();
        div_s.taffy_style.size.width = taffy::style::Dimension::length(100.0);
        div_s.font_size = 16.0;
        let entries = [
            (
                None,
                NodeKind::Container,
                root_s,
                Vec::new(),
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
                div_s,
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(1),
                NodeKind::TextNode,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                Some("text ".into()),
                None,
            ),
            (
                Some(1),
                NodeKind::TextElement,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
                None,
            ),
            (
                Some(3),
                NodeKind::TextNode,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
                Some("x".into()),
                None,
            ),
        ];
        let mut scene = Scene::build(&entries);
        let div = scene.get(scene.roots[0]).unwrap().children[0];
        let span = scene.get(div).unwrap().children[1];
        scene.get_mut(div).unwrap().rich_text_block = true;
        solve(&mut scene, &ft, (200.0, 1000.0), &HashMap::new());
        compute_world_transforms(&mut scene);

        // span run 区域中心 → 命中 span（细化生效）。
        let span_center = {
            let layout = scene.text_layouts[div.index()]
                .as_ref()
                .expect("solve 填 text_layouts");
            let r = layout.run_rects.iter().find(|r| r.source == span).unwrap();
            (r.x + r.w / 2.0, r.y + r.h / 2.0)
        };
        assert_eq!(
            hit_test(&scene, span_center),
            Some(span),
            "span 区域命中细化到 span source"
        );

        // text_layouts 缺席（首帧 lazy 前）→ 细化不中，回落容器。
        scene.text_layouts[div.index()] = None;
        assert_eq!(
            hit_test(&scene, span_center),
            Some(div),
            "无 layout 时回落容器（HTML 语义：文本段的点击目标是宿主元素）"
        );
    }

    /// #74：rich-text-block 里的 `<a>` 文本命中细化到 a 节点（run.source=a）——
    /// 事件路由归链接，不归内部匿名 TextNode、也不归容器。
    #[test]
    fn hit_test_resolves_link_run_to_a_node() {
        use crate::layout::solve;
        use crate::style::resolved::ResolvedStyle;
        use crate::text::layout::FontTable;
        use std::collections::HashMap;

        let font_path = format!(
            "{}/tests/fixtures/DejaVuSans.ttf",
            env!("CARGO_MANIFEST_DIR")
        );
        let Ok(bytes) = std::fs::read(&font_path) else {
            return; // 字体 fixture 缺席的环境跳过（同上口径）
        };
        let mut ft = FontTable::new();
        ft.register("default", bytes, true).unwrap();

        let mut root_s = ResolvedStyle::default();
        root_s.taffy_style.size.width = taffy::style::Dimension::length(200.0);
        let mut div_s = ResolvedStyle::default();
        div_s.taffy_style.size.width = taffy::style::Dimension::length(150.0);
        div_s.font_size = 16.0;
        // entries: 0:root 1:div(rich) 2:TextNode "看" 3:a(Link) 4:TextNode "商店"(in a)
        let entries = [
            (
                None,
                NodeKind::Container,
                root_s,
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
                div_s,
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
                NodeKind::TextNode,
                ResolvedStyle::default(),
                vec![],
                None,
                false,
                None,
                None,
                Some("看 ".into()),
                None,
            ),
            (
                Some(1),
                NodeKind::Link,
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
                Some(3),
                NodeKind::TextNode,
                ResolvedStyle::default(),
                vec![],
                None,
                false,
                None,
                None,
                Some("商店".into()),
                None,
            ),
        ];
        let mut scene = Scene::build(&entries);
        let div = scene.get(scene.roots[0]).unwrap().children[0];
        let a = scene.get(div).unwrap().children[1];
        scene.get_mut(div).unwrap().rich_text_block = true;
        solve(&mut scene, &ft, (200.0, 1000.0), &HashMap::new());
        compute_world_transforms(&mut scene);

        // 链接 run 区域中心 → 命中 a（run.source=a 的细化）。
        let link_center = {
            let layout = scene.text_layouts[div.index()]
                .as_ref()
                .expect("solve 填 text_layouts");
            let r = layout
                .run_rects
                .iter()
                .find(|r| r.source == a)
                .expect("链接 run 的 source 应为 a");
            (r.x + r.w / 2.0, r.y + r.h / 2.0)
        };
        assert_eq!(
            hit_test(&scene, link_center),
            Some(a),
            "链接文本命中细化到 <a> 节点（事件归链接）"
        );
    }

    /// 构造两兄弟子节点的 scene：root + child_a + child_b，都 100x100，
    /// child_a 在 (0,0)，child_b 在 (50,50)（与 a 重叠右下角）。
    /// children 顺序 [a, b] → 等效序 b 顶层（后绘制）。
    fn overlap_scene() -> Scene {
        let mut root = Node::default();
        root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        };
        let mut a = Node::default();
        a.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        let mut b = Node::default();
        b.layout_rect = Rect {
            x: 50.0,
            y: 50.0,
            w: 100.0,
            h: 100.0,
        };
        // edges: (0,1)=root→a, (0,2)=root→b；root parent=None 自动成 root。
        let s = Scene::from_nodes(vec![root, a, b], vec![(0, 1), (0, 2)]);
        // 返回前 node id 由 slotmap 分配（首节点 = NodeId((1<<12)|1)）。
        s
    }

    /// 返回 overlap_scene 的 id 三元组 (root_id, a_id, b_id)。
    fn overlap_ids(s: &Scene) -> (NodeId, NodeId, NodeId) {
        let root_id = s.roots[0];
        let a_id = s.get(root_id).unwrap().children[0];
        let b_id = s.get(root_id).unwrap().children[1];
        (root_id, a_id, b_id)
    }

    #[test]
    fn hit_test_returns_none_on_empty_scene() {
        let mut s = Scene::from_nodes(vec![], vec![]);
        compute_world_transforms(&mut s);
        assert_eq!(hit_test(&s, (10.0, 10.0)), None);
    }

    #[test]
    fn hit_test_hits_topmost_child() {
        let mut s = overlap_scene();
        compute_world_transforms(&mut s);
        let (_root, _a, b) = overlap_ids(&s);
        // 点 (75,75) 在 a 和 b 重叠区——b 顶层（后绘制）应命中
        assert_eq!(hit_test(&s, (75.0, 75.0)), Some(b));
    }

    #[test]
    fn hit_test_hits_only_child_when_no_overlap() {
        let mut s = overlap_scene();
        compute_world_transforms(&mut s);
        let (_root, a, _b) = overlap_ids(&s);
        // 点 (10,10) 只在 a 内
        assert_eq!(hit_test(&s, (10.0, 10.0)), Some(a));
    }

    #[test]
    fn hit_test_skips_pointer_events_none_but_tests_children() {
        let mut s = overlap_scene();
        compute_world_transforms(&mut s);
        let (root, a, _b) = overlap_ids(&s);
        // root touchable=false，但子 a 仍应命中（CSS 语义：none 不挡子）
        s.get_mut(root).unwrap().interaction.touchable = false;
        // 点 (10,10) 在 a 内——root 不命中但子 a 命中
        assert_eq!(hit_test(&s, (10.0, 10.0)), Some(a));
        // 点 (160,160) 在 root AABB 但不在 a/b（a=[0,100], b=[50,150]）
        // ——root touchable=false → None
        assert_eq!(hit_test(&s, (160.0, 160.0)), None);
    }

    #[test]
    fn hit_test_clip_rect_excludes_subtree() {
        let mut s = overlap_scene();
        compute_world_transforms(&mut s);
        let (root, _a, b) = overlap_ids(&s);
        // root 加 clip_rect (0,0,80,80)——点 (90,90) 在 root AABB 但 clip 外
        s.get_mut(root).unwrap().clip_rect = Some(Rect {
            x: 0.0,
            y: 0.0,
            w: 80.0,
            h: 80.0,
        });
        // 点 (90,90) 在 b 的 AABB (50,50,100,100) 但在 root clip 外 → 子树不命中
        assert_eq!(hit_test(&s, (90.0, 90.0)), None);
        // 点 (70,70) 在 clip 内 + 在 b 内 → 命中 b
        assert_eq!(hit_test(&s, (70.0, 70.0)), Some(b));
    }

    #[test]
    fn hit_test_clip_survives_ancestor_scroll() {
        // 嵌套滚动回归：root 是滚动容器（自身 clip=视口），子 inner 是
        // clip 节点（页面绝对坐标 far 处）。root 滚动后，inner 出现在屏幕上——屏幕点
        // 必须命中 inner 子树；旧实现拿屏幕点直接比页面绝对 clip（无滚动补偿）→ 整棵
        // 子树不可命中，滚轮/点击穿透到外层容器。
        let mut root = Node::default();
        root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        };
        root.clip_rect = Some(Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        });
        let mut inner = Node::default();
        // inner 页面绝对位置 y=1000；root 滚动 900 后出现在屏幕 y≈100。
        inner.layout_rect = Rect {
            x: 10.0,
            y: 1000.0,
            w: 100.0,
            h: 100.0,
        };
        inner.clip_rect = Some(Rect {
            x: 10.0,
            y: 1000.0,
            w: 100.0,
            h: 100.0,
        });
        inner.interaction.touchable = true;
        let mut s = Scene::from_nodes(vec![root, inner], vec![(0, 1)]);
        let root_id = s.roots[0];
        let inner_id = s.get_mut(root_id).unwrap().children[0];
        s.scroll.ensure(root_id).scroll_pos = (0.0, 900.0);
        compute_world_transforms(&mut s);
        // 屏幕点 (60, 1050)：无滚动时 page=(60,1050) 在 inner 内——root 滚 900 后
        // inner 的屏幕位置 = 1000-900=100..200 → 屏幕点 (60, 150) 命中 inner。
        assert_eq!(hit_test(&s, (60.0, 150.0)), Some(inner_id));
        // 屏幕点在 root 视口但不在 inner（inner 屏幕 y 100..200，取 y=50）→ 不命中 inner。
        assert_ne!(hit_test(&s, (60.0, 50.0)), Some(inner_id));
    }

    #[test]
    fn hit_test_respects_order() {
        let mut s = overlap_scene();
        compute_world_transforms(&mut s);
        let (_root, a, b) = overlap_ids(&s);
        // a 设 order=2（顶层），b order=0——等效序 a 在前
        s.get_mut(a).unwrap().style.order = 2;
        s.get_mut(b).unwrap().style.order = 0;
        // 点 (75,75) 重叠区——a 顶层应命中
        assert_eq!(hit_test(&s, (75.0, 75.0)), Some(a));
    }

    #[test]
    fn hit_test_disabled_node_still_target() {
        // disabled 仍参与命中（active/click 抑制在状态机层，hit_test 只返回几何命中）
        let mut s = overlap_scene();
        compute_world_transforms(&mut s);
        let (_root, _a, b) = overlap_ids(&s);
        s.get_mut(b)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::DISABLED);
        // 点 (75,75) 在 b 内——b 仍命中（disabled 不跳过）
        assert_eq!(hit_test(&s, (75.0, 75.0)), Some(b));
    }

    #[test]
    fn hit_rotated_parent_catches_child_via_world_to_local() {
        // parent rotate(90°) at (0,0,100,100)；child identity at (0,0,10,10)。
        // parent 绕 center(50,50) 转 90°：child(在 parent 左上角) 视觉转到 parent 右上区域。
        // 命中点取 child 旋转后的中心附近。
        let mut s = overlap_scene_rotated();
        compute_world_transforms(&mut s);
        let root_id = s.roots[0];
        let parent_id = s.get(root_id).unwrap().children[0];
        let child_id = s.get(parent_id).unwrap().children[0];
        // child world == parent world（identity 子继承）。parent 旋转后 child box 在新位置。
        // 用 child box center 经 parent.world 变换得世界中心，命中应返 child。
        let child_wm = s.world_transforms[child_id.index()];
        let (cx, cy) = child_wm.apply_point(5.0, 5.0); // child 本地中心
        assert_eq!(
            hit_test(&s, (cx, cy)),
            Some(child_id),
            "点在旋转后 child 上 → 命中 child"
        );
    }

    fn overlap_scene_rotated() -> Scene {
        let mut root = Node::default();
        root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        };
        let mut parent = Node::default();
        parent.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        parent.style.transform = LocalTransform {
            matrix: transform::from_rotate(std::f32::consts::FRAC_PI_2),
        };
        let mut child = Node::default();
        child.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        Scene::from_nodes(vec![root, parent, child], vec![(0, 1), (1, 2)])
    }

    fn scroll_scene_with_thumb() -> Scene {
        use crate::style::resolved::{OverflowMode, ResolvedStyle};
        let mut scroll_style = ResolvedStyle::default();
        scroll_style.overflow_y = OverflowMode::Scroll;
        let entries: Vec<(
            Option<usize>,
            NodeKind,
            ResolvedStyle,
            Vec<String>,
            Option<String>,
            bool,
            Option<i32>,
            Option<String>,
            Option<String>,
            Option<String>,
        )> = vec![
            (
                None,
                NodeKind::Container,
                scroll_style.clone(),
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
        ];
        let mut s = Scene::build(&entries);
        // build 按 entries 插入序分配 id：roots[0]=容器，其 children=[entry1, entry2]。
        let container_id = s.roots[0];
        let inner_id = s.get(container_id).unwrap().children[0];
        let content_id = s.get(container_id).unwrap().children[1];
        s.get_mut(container_id).unwrap().layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        s.get_mut(inner_id).unwrap().layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 40.0,
            h: 40.0,
        };
        s.get_mut(content_id).unwrap().layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 30.0,
            h: 200.0,
        }; // content_y=200 > viewport=100
        crate::scroll::refresh_content_sizes(&mut s);
        compute_world_transforms(&mut s);
        s
    }

    #[test]
    fn hit_scrollbar_grip_returns_container() {
        let s = scroll_scene_with_thumb();
        let container_id = s.roots[0];
        // thumb 右边缘 (x=92..100, y=0..50)，取 center (96, 25)
        let result = hit_scrollbar_grip(&s, (96.0, 25.0));
        assert!(result.is_some(), "thumb 内一点应命中");
        let (container, axis) = result.unwrap();
        assert_eq!(container, container_id, "返容器 id（slotmap 分配）");
        assert_eq!(axis, 0, "垂直 thumb axis=0");
    }

    #[test]
    fn hit_scrollbar_grip_returns_none_outside_thumb() {
        let s = scroll_scene_with_thumb();
        // 点在容器左上角 (10,10) 非 thumb 区
        assert!(
            hit_scrollbar_grip(&s, (10.0, 10.0)).is_none(),
            "非 thumb 区 → None"
        );
    }

    #[test]
    fn hit_scrollbar_grip_no_scroll_no_thumb() {
        let s = overlap_scene(); // 无 scroll 容器
        compute_world_transforms(&mut s.clone());
        assert!(
            hit_scrollbar_grip(&s, (50.0, 50.0)).is_none(),
            "无 scroll 容器 → None"
        );
    }

    #[test]
    fn hit_test_returns_sentinel_for_thumb() {
        let s = scroll_scene_with_thumb();
        let container_id = s.roots[0];
        // thumb 内一点 → hit_test 应返 sentinel（含 V_THUMB_FLAG）
        let hit = hit_test(&s, (96.0, 25.0));
        assert!(hit.is_some(), "thumb 区 hit_test 命中");
        let raw = hit.unwrap().0;
        assert!(
            raw & crate::scroll::V_THUMB_FLAG != 0,
            "sentinel 含 V_THUMB_FLAG"
        );
        assert_eq!(
            raw & !crate::scroll::V_THUMB_FLAG,
            container_id.0,
            "flag off → container id"
        );
    }

    /// 建 open Dropdown 场景：root > select(Dropdown,open,120x30 @(10,10))，
    /// select 的 listbox(80x60 @(10,40)) 内含两个 option（各 80x20，垂直堆叠）。
    /// 复刻生产运行时结构（combobox > [data-slot=value, role=listbox > [option...]]）。
    /// 返回 (select_id, popup_id, opt0_id, opt1_id)。点 opt0 用 (50,50)（opt0 区 40..60）。
    fn open_dropdown_scene() -> (Scene, NodeId, NodeId, NodeId, NodeId) {
        use crate::asset::ControlInit;
        use crate::scene::control::ROLE_LISTBOX;
        use crate::scene::dynamic::create_node_from_template;
        use crate::scene::node::{ControlState, RoleInfo};
        use crate::style::resolved::ResolvedStyle;

        let mut root = Node::default();
        root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 300.0,
        };
        let mut s = Scene::from_nodes(vec![root], vec![]);
        let root_id = s.roots[0];

        // select（Dropdown 控件）—— 作者自写结构（core 不再注入）。
        let select = create_node_from_template(
            &mut s,
            NodeKind::Dropdown,
            ResolvedStyle::default(),
            Some(ControlInit::Dropdown {
                selected_index: 0,
                option_values: Vec::new(),
            }),
        );
        crate::scene::dynamic::append_child(&mut s, root_id, select).unwrap();
        // 设 open=true（create 默认 open=false）。
        if let Some(ControlState::Dropdown { open, .. }) = s.controls.get_mut(select) {
            *open = true;
        }
        s.get_mut(select).unwrap().layout_rect = Rect {
            x: 10.0,
            y: 10.0,
            w: 120.0,
            h: 30.0,
        };

        // listbox role 子（作者写的弹出列表容器）。登记 role 进 RoleTable。
        let listbox =
            create_node_from_template(&mut s, NodeKind::Container, ResolvedStyle::default(), None);
        crate::scene::dynamic::append_child(&mut s, select, listbox).unwrap();
        s.roles.insert(
            listbox,
            RoleInfo {
                role: Some(ROLE_LISTBOX.to_string()),
                slots: Default::default(),
                aria_controls: None,
            },
        );
        // 两个 option 直接挂 listbox（作者正确结构）。
        let opt0 =
            create_node_from_template(&mut s, NodeKind::OptionItem, ResolvedStyle::default(), None);
        let opt1 =
            create_node_from_template(&mut s, NodeKind::OptionItem, ResolvedStyle::default(), None);
        crate::scene::dynamic::append_child(&mut s, listbox, opt0).unwrap();
        crate::scene::dynamic::append_child(&mut s, listbox, opt1).unwrap();

        let popup = listbox;
        // popup 浮在 select 下方（absolute，相对 select 定位）。
        s.get_mut(popup).unwrap().layout_rect = Rect {
            x: 10.0,
            y: 40.0,
            w: 80.0,
            h: 60.0,
        };
        s.get_mut(opt0).unwrap().layout_rect = Rect {
            x: 10.0,
            y: 40.0,
            w: 80.0,
            h: 20.0,
        };
        s.get_mut(opt1).unwrap().layout_rect = Rect {
            x: 10.0,
            y: 60.0,
            w: 80.0,
            h: 20.0,
        };
        compute_world_transforms(&mut s);
        (s, select, popup, opt0, opt1)
    }

    #[test]
    fn hit_inside_open_popup_returns_option() {
        // open dropdown，点击落在 opt0 的 layout_rect 内 → 返回 opt0 NodeId（不是 select）。
        let (s, _select, _popup, opt0, _opt1) = open_dropdown_scene();
        // opt0 区 (10,40,80,20) → center (50,50)
        assert_eq!(
            hit_test(&s, (50.0, 50.0)),
            Some(opt0),
            "点击 open popup 内 option → 返回该 option"
        );
    }

    #[test]
    fn hit_inside_open_popup_second_option_returns_it() {
        // opt1 区 (10,60,80,20) → center (50,70)
        let (s, _select, _popup, _opt0, opt1) = open_dropdown_scene();
        assert_eq!(
            hit_test(&s, (50.0, 70.0)),
            Some(opt1),
            "点击第二个 option → 返回 opt1"
        );
    }

    #[test]
    fn hit_open_popup_beats_normal_content_on_top() {
        // 优先级：一个在正常 DFS 序里「顶层」的节点（后挂的 root 子节点）几何覆盖 opt0 区。
        // open=true 时 popup 前置命中应赢——返回 opt0 而非 cover。
        // 这验证 popup 前置 check 在主 roots DFS 之前。
        use crate::scene::dynamic::create_node_from_template;
        use crate::scene::node::ControlState;
        use crate::style::resolved::ResolvedStyle;

        let (mut s, select, _popup, opt0, _opt1) = open_dropdown_scene();
        let root_id = s.roots[0];
        // cover：全覆盖 300x300，作为 root 的后挂子节点（后绘制=顶层）。在正常 DFS 里它会
        // 先于 select 子树被测（顶层优先），点 (50,50) 应命中 cover。
        let cover =
            create_node_from_template(&mut s, NodeKind::Container, ResolvedStyle::default(), None);
        crate::scene::dynamic::append_child(&mut s, root_id, cover).unwrap();
        s.get_mut(cover).unwrap().layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 300.0,
        };
        compute_world_transforms(&mut s);
        assert_eq!(
            hit_test(&s, (50.0, 50.0)),
            Some(opt0),
            "open 时 popup 前置命中赢过正常顶层内容"
        );
        if let Some(ControlState::Dropdown { open, .. }) = s.controls.get_mut(select) {
            *open = false;
        }
        assert_eq!(
            hit_test(&s, (50.0, 50.0)),
            Some(cover),
            "closed 时正常 DFS，顶层 cover 赢（popup 不前置）"
        );
    }

    #[test]
    fn hit_test_prefers_higher_z_index_sibling() {
        // a 先出现在 DOM、b 后（默认 b 顶层）。给 a z=10 后 z 翻转绘制序——
        // 重叠区 (75,75) 应命中 a（z 大者顶层，与 render DFS 的 z 升序绘制镜像）。
        let mut s = overlap_scene();
        let a_id = overlap_ids(&s).1;
        s.get_mut(a_id).unwrap().style.z_index = 10;
        compute_world_transforms(&mut s);
        assert_eq!(hit_test(&s, (75.0, 75.0)), Some(a_id));
    }

    #[test]
    fn hit_test_negative_z_sinks_below_dom_later_sibling() {
        // a z=-1、b z=0：负 z 沉底，重叠区命中 b（默认 DOM 后者）。
        let mut s = overlap_scene();
        let a_id = overlap_ids(&s).1;
        s.get_mut(a_id).unwrap().style.z_index = -1;
        compute_world_transforms(&mut s);
        let (_root, _a, b) = overlap_ids(&s);
        assert_eq!(hit_test(&s, (75.0, 75.0)), Some(b));
    }
}
