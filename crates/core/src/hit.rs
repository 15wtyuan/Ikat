//! 命中测试：输入 design 坐标点 → 返回命中 NodeId。
//! 逆等效绘制序遍历（顶层优先），layout_rect AABB + clip 门控 + pointer-events。
//! 不做 transform world_to_local（无动画故无影响）。

use crate::scene::node::{ControlState, NodeId, Rect, Scene};

/// 点是否在 Rect 内（含边界，design 坐标）。
pub(crate) fn point_in_rect(point: (f32, f32), r: Rect) -> bool {
    point.0 >= r.x && point.0 <= r.x + r.w && point.1 >= r.y && point.1 <= r.y + r.h
}

/// children 按 style.order 降序排（大值=顶层在前）；同 order 时后出现的子在前
/// （CSS flexbox `order` 语义：默认 order=0，DOM 序 = 绘制序，后者绘 = 顶层）。
/// 实现：先反转 children（让后者靠前），再按 `-order` 稳定排——stable 保反转后序，
/// 即同 order 下后者先测，与 hit_test"顶层优先"一致。
fn effective_draw_order(scene: &Scene, parent: NodeId) -> Vec<NodeId> {
    let mut kids: Vec<NodeId> = scene.get(parent).expect("live node").children.clone();
    kids.reverse();
    kids.sort_by_key(|&c| -scene.get(c).expect("live node").style.order); // 负号=降序
    kids
}

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

/// 命中 open Dropdown 的 popup 子树。open popup 浮层渲染在所有正常内容之上（Task 11：
/// mask=0、末尾追加 DFS），故命中优先级高于正常内容——在主 roots DFS 前测。
///
/// 与 [`hit_scrollbar_grip`] 的优先级：scrollbar grip 仍先于此（grip 返 sentinel NodeId）。
/// 理由：popup 自身可滚（长 option 列表）时其 scrollbar grip 不能被 popup 命中遮蔽——
/// grip 必须可抓。grip 与正常 dropdown popup 几何重叠极少（grip 在滚动容器边缘、popup
/// 绝对定位浮层），两者互不冲突。返真节点（option/popup）的 NodeId。
fn hit_open_popups(scene: &Scene, point: (f32, f32)) -> Option<NodeId> {
    // 先收集所有 open Dropdown 的 popup 根（不可变借），再逐个 hit_subtree（不可变借）。
    // 两阶段分开避免边迭代 controls 边递归 scene 的复杂借用。收集口径与 render 层
    // `collect_open_popup_roots` 一致（同源：open Dropdown → .loom-popup 子节点）。
    let mut popups: Vec<NodeId> = Vec::new();
    for n in scene.nodes.values() {
        let is_open_dropdown = matches!(
            scene.controls.get(n.id),
            Some(ControlState::Dropdown { open: true, .. })
        );
        if !is_open_dropdown {
            continue;
        }
        if let Some(popup) =
            crate::scene::control::find_child_by_class(scene, n.id, crate::scene::control::POPUP)
        {
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
    // scrollbar grip 最上层（先于所有 Scene 节点）
    if let Some((container, axis)) = hit_scrollbar_grip(scene, point) {
        let flag = if axis == 0 {
            crate::scroll::V_THUMB_FLAG
        } else {
            crate::scroll::H_THUMB_FLAG
        };
        return Some(NodeId(container.0 | flag));
    }
    // open popup 浮层（Task 11 渲染在所有正常内容之上）→ 命中优先于正常内容。
    // 顺序在 scrollbar grip 之后（见 [`hit_open_popups`] 文档：grip 须可抓）。
    if let Some(hit) = hit_open_popups(scene, point) {
        return Some(hit);
    }
    // 从 roots 逐棵 DFS。多个 root 按顺序，后 root 顶层（与渲染序一致）。
    for &root in &scene.roots {
        if let Some(hit) = hit_subtree(scene, root, point) {
            return Some(hit);
        }
    }
    None
}

/// 递归测某子树。先测子（逆等效序，顶层先），子命中返回子的；子都不命中→自身 fallback。
fn hit_subtree(scene: &Scene, id: NodeId, point: (f32, f32)) -> Option<NodeId> {
    let node = scene.get(id).expect("live node");
    // clip 门控：有 clip_rect 且点不在 clip 内 → 整个子树不命中
    if let Some(clip) = node.clip_rect {
        if !point_in_rect(point, clip) {
            return None;
        }
    }
    // 先测子（逆等效绘制序 = 顶层先）
    for &c in &effective_draw_order(scene, id) {
        if let Some(hit) = hit_subtree(scene, c, point) {
            return Some(hit);
        }
    }
    // 子都不命中 → 自身 fallback：touchable + 点经 world matrix 逆投到本地 box
    // world_to_local：点经 world matrix 逆投到本地，判本地 box (0,0,w,h)
    if node.interaction.touchable {
        // bounds guard：world_transforms 可能未对齐（结构变更帧新增节点本帧 world_transforms
        // 未算，或首帧 world_transforms 空）→ 越界返 None（1 帧延迟语义：本帧未命中）。
        // sentinel id（thumb flag）不会进 hit_subtree（hit_test 在 hit_scrollbar_grip 命中后
        // 早 return），故此处 id 必为 live 节点 NodeId，index() 不会因 flag bit 失真。
        let wm = scene.world_transforms.get(id.index())?;
        let inv = crate::transform::inverse(wm);
        let (lx, ly) = crate::transform::apply_point(&inv, point.0, point.1);
        let lr = node.layout_rect;
        if lx >= 0.0 && lx <= lr.w && ly >= 0.0 && ly <= lr.h {
            return Some(id);
        }
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
            .insert(NodeFlags::DISABLED); // b disabled
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
        // edges: root→parent, parent→child
        Scene::from_nodes(vec![root, parent, child], vec![(0, 1), (1, 2)])
    }

    // ── hit_scrollbar_grip ─────────────────────────────────

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
        // 去掉 flag 应得 container id（packed u32）
        assert_eq!(
            raw & !crate::scroll::V_THUMB_FLAG,
            container_id.0,
            "flag off → container id"
        );
    }

    // ── open popup 前置命中（Task 12）─────────────────────────

    /// 建 open Dropdown 场景：root > select(Dropdown,open,120x30 @(10,10))，
    /// select 的 .loom-popup(80x60 @(10,40)) 内含两个 option（各 80x20，垂直堆叠）。
    /// 复刻生产运行时结构（spec §4.1：select > [.loom-value, .loom-popup > [option...]]）。
    /// 返回 (select_id, popup_id, opt0_id, opt1_id)。点 opt0 用 (50,50)（opt0 区 40..60）。
    fn open_dropdown_scene() -> (Scene, NodeId, NodeId, NodeId, NodeId) {
        use crate::asset::ControlInit;
        use crate::scene::control::POPUP;
        use crate::scene::dynamic::create_node_from_template;
        use crate::scene::node::ControlState;
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

        // select（Dropdown 控件）—— create_node_from_template 会 inject .loom-value/.loom-popup。
        let select = create_node_from_template(
            &mut s,
            NodeKind::Dropdown,
            ResolvedStyle::default(),
            Some(ControlInit::Dropdown { selected_index: 0 }),
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

        // 两个 option 挂到 select，再 reparent 进 .loom-popup（同生产 instantiate 路径）。
        let opt0 =
            create_node_from_template(&mut s, NodeKind::OptionItem, ResolvedStyle::default(), None);
        let opt1 =
            create_node_from_template(&mut s, NodeKind::OptionItem, ResolvedStyle::default(), None);
        crate::scene::dynamic::append_child(&mut s, select, opt0).unwrap();
        crate::scene::dynamic::append_child(&mut s, select, opt1).unwrap();
        crate::scene::control::reparent_options_into_popup(&mut s, select);

        let popup = crate::scene::control::find_child_by_class(&s, select, POPUP).unwrap();
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
        // open → popup 前置赢
        assert_eq!(
            hit_test(&s, (50.0, 50.0)),
            Some(opt0),
            "open 时 popup 前置命中赢过正常顶层内容"
        );
        // 收起 popup → 正常 DFS，cover（顶层）赢
        if let Some(ControlState::Dropdown { open, .. }) = s.controls.get_mut(select) {
            *open = false;
        }
        assert_eq!(
            hit_test(&s, (50.0, 50.0)),
            Some(cover),
            "closed 时正常 DFS，顶层 cover 赢（popup 不前置）"
        );
    }
}
