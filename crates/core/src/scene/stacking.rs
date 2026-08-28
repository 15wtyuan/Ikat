//! Stacking context 全局画序（CSS Appendix E painting order，#100）。
//!
//! 画序不是「逐父兄弟排序」而是**每个 stacking context 内的全局分层**：SC 的所有
//! 后代（任意深度）按属性归入 负 z SC → static 树序 → z0 层 → 正 z SC 四段绘制。
//! 嵌套在 static 子树里的 opacity<1 / transform / filter / 定位+声明 z 元素会被
//! **上提**出所在 static 层，进入所属 SC 的 z0/正负 z 层——浏览器把 opacity<1 的
//! static 元素「当作 z-index:0 的 positioned 元素绘制」（CSS Color §opacity），这是
//! #100 的根因：static 顶栏里的半透明图标在浏览器里浮在 absolute z0 底图之上、
//! 文本留在 static 层被盖，而逐父排序的实现让整个 static 子树（含图标）沉底。
//!
//! 消费点三处共用本模块同一份序（语义单一真相源）：render 主 DFS（batch.rs
//! `assign_sort_keys` 画序 pass）、open popup 追加循环（render/mod.rs）、hit
//! （hit.rs 逆序遍历）。

use super::node::{Node, NodeId, Scene};
use crate::style::resolved::{DisplayMode, PositionDeclared};

/// 节点在所属 stacking context 里的归类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StackClass {
    /// static 内容：留在当前层的树序段里（与所在 positioned-z:auto 组或 SC 根绑定）。
    Static,
    /// positioned 且未声明 z（z:auto）：画在 z0 层，自身成组（static 后代跟组走），
    /// 但其后代 SC / positioned 会上提到所属 SC 的层（CSS App E step 8：treat as
    /// if it created a new stacking context, but positioned descendants ... part of
    /// the parent stacking context）。
    PositionedAuto,
    /// 真 stacking context：整棵子树作为单元在对应层一次画完（负 z / z0 / 正 z）。
    /// 子树内部再按本算法递归分层——「子树整体移动」不变量的载体。
    Context { z: i32 },
}

/// 归类判定（当帧有效值：opacity/transform 吃动画覆写）。
///
/// SC 判据（CSS）：
/// - positioned + 声明 z（z≠0 或 `z_declared`）→ `Context{z}`；
/// - positioned z:auto → `PositionedAuto`；但叠加 opacity<1 / transform / filter
///   时仍成原子 `Context{0}`（这些属性无条件创建 SC，定位与否只决定画在哪层）；
/// - **非定位** + opacity<1 / transform≠identity / filter → `Context{0}`（CSS Color
///   §opacity：as if positioned with z-index: 0）；
/// - **非定位** + 声明 z（flex item 上 z 即使 static 也生效；运行时直改 z 的
///   逃生舱路径同此）→ `Context{z}`。**已知口径分歧**：浏览器对非定位、非 flex
///   item 的声明 z 视而不见，core 恒生效（fgui 血统的运行时语义）——围栏侧避免
///   该写法即可（见 fence.md §z-index）。
fn classify(scene: &Scene, n: &Node) -> StackClass {
    let s = &n.style;
    let positioned = s.position_declared != PositionDeclared::Static;
    // z 是否生效：作者声明（含 flex item 的 z:0）或运行时直改非 0（缺省恒 0，
    // 非 0 即被改过的证据）。
    let z_active = s.z_declared || s.z_index != 0;
    // opacity<1 / transform / filter：无条件 SC（动画覆写优先于 CSS 声明）。
    let opacity_sc = scene
        .anim
        .get(n.id)
        .and_then(|a| a.opacity)
        .unwrap_or(s.opacity)
        < 1.0;
    let transform_sc = match scene.anim.get(n.id).and_then(|a| a.transform) {
        Some(m) => !crate::transform::is_identity(&m),
        None => !s.transform.is_identity(),
    };
    let filter_sc = s.color_filter.is_some();
    if z_active {
        StackClass::Context { z: s.z_index }
    } else if opacity_sc || transform_sc || filter_sc {
        // CSS：非定位时画在 z0 层；定位 z:auto 时本非 SC，但叠加这些属性即原子组。
        StackClass::Context { z: 0 }
    } else if positioned {
        StackClass::PositionedAuto
    } else {
        StackClass::Static
    }
}

/// `root` 子树的扁平绘制序（含 root 本身在首位）。`include` 为 false 的节点整棵
/// 子树剪掉——render 侧传 `id_to_pos` 含有性（display:none / popup 剪枝口径），
/// hit 侧传 world_transforms 缺席守卫（1 帧延迟语义）。
///
/// 输出性质：父恒先于子（statics 段天然树序；SC/组先发自身再发内容）；跨节点则
/// 按 SC 分层语义（statics 全部先于 z0 层，负 z 沉底，正 z 按 (z 升, 树序)）。
pub fn paint_order(scene: &Scene, root: NodeId, include: &dyn Fn(NodeId) -> bool) -> Vec<NodeId> {
    let mut out = Vec::new();
    if !include(root) {
        return out;
    }
    sc_paint(scene, root, include, &mut out);
    out
}

/// z0 层条目：positioned-z:auto 组（自身 + 其 static 段）或 z=0 SC，按树序混排。
enum ZeroEntry {
    /// positioned z:auto：node 自身先画，statics 段（groups[group]）紧随。
    Group(NodeId, usize),
    /// z=0 的 SC：子树整体在 emit 时递归 [`sc_paint`]。
    Sc(NodeId),
}

/// 画一个 stacking context：root 先画，后代分四段——负 z SC（z 升, 树序）→
/// root 的 statics（树序平铺，层 3/5 的近似合并）→ z0 层（树序）→ 正 z SC
/// （z 升, 树序）。
fn sc_paint(scene: &Scene, root: NodeId, include: &dyn Fn(NodeId) -> bool, out: &mut Vec<NodeId>) {
    out.push(root);
    // statics 段 arena：groups[0] = 本 SC 根的 statics；后续 = 各 positioned-z:auto 组的。
    let mut groups: Vec<Vec<NodeId>> = vec![Vec::new()];
    let mut zeros: Vec<ZeroEntry> = Vec::new();
    let mut neg: Vec<(i32, u32, NodeId)> = Vec::new();
    let mut pos: Vec<(i32, u32, NodeId)> = Vec::new();
    let mut seq: u32 = 0;
    walk(
        scene,
        root,
        include,
        &mut groups,
        &mut zeros,
        &mut neg,
        &mut pos,
        &mut seq,
        0,
    );
    neg.sort_by_key(|&(z, s, _)| (z, s));
    for &(_, _, sc) in &neg {
        sc_paint(scene, sc, include, out);
    }
    out.extend(groups[0].iter().copied());
    for entry in zeros {
        match entry {
            ZeroEntry::Group(n, gi) => {
                out.push(n);
                out.extend(groups[gi].iter().copied());
            }
            ZeroEntry::Sc(n) => sc_paint(scene, n, include, out),
        }
    }
    pos.sort_by_key(|&(z, s, _)| (z, s));
    for &(_, _, sc) in &pos {
        sc_paint(scene, sc, include, out);
    }
}

/// 树序走查 `parent` 的后代并归类。statics 追加进 `groups[group]`；SC 节点只登记
/// 不深入（emit 时整树递归）；positioned-z:auto 开新组并把其 static 后代挂进新组，
/// 其 SC/positioned 后代继续上提到本 SC 的层。
///
/// 兄弟访问序 = CSS order-modified tree order：flex 父（围栏缺省 display）下按
/// `order` 升序稳定排（等值保 DOM 序）——浏览器 painting 用同序，`order` 只影响
/// flex item（block 父纯 DOM 序）。
fn walk(
    scene: &Scene,
    parent: NodeId,
    include: &dyn Fn(NodeId) -> bool,
    groups: &mut Vec<Vec<NodeId>>,
    zeros: &mut Vec<ZeroEntry>,
    neg: &mut Vec<(i32, u32, NodeId)>,
    pos: &mut Vec<(i32, u32, NodeId)>,
    seq: &mut u32,
    group: usize,
) {
    let Some(pn) = scene.get(parent) else {
        return;
    };
    let kids = children_in_tree_order(scene, pn);
    for c in kids {
        if !include(c) {
            continue;
        }
        let Some(cn) = scene.get(c) else {
            continue; // 死 id 防御（children 恒 live 的不变量外的兜底）
        };
        match classify(scene, cn) {
            StackClass::Static => {
                groups[group].push(c);
                walk(scene, c, include, groups, zeros, neg, pos, seq, group);
            }
            StackClass::PositionedAuto => {
                groups.push(Vec::new());
                let gi = groups.len() - 1;
                zeros.push(ZeroEntry::Group(c, gi));
                walk(scene, c, include, groups, zeros, neg, pos, seq, gi);
            }
            StackClass::Context { z } => {
                *seq += 1;
                let s = *seq - 1;
                if z < 0 {
                    neg.push((z, s, c));
                } else if z == 0 {
                    zeros.push(ZeroEntry::Sc(c));
                } else {
                    pos.push((z, s, c));
                }
                // 子树不深入：emit 时 sc_paint 整树递归（SC 边界包含其后代）。
            }
        }
    }
}

/// 兄弟树序：flex 父按 `order` 升序稳定排（CSS order-modified tree order），其余
/// 纯 DOM 序。
fn children_in_tree_order(scene: &Scene, pn: &Node) -> Vec<NodeId> {
    let mut kids = pn.children.clone();
    if pn.style.display_mode == DisplayMode::Flex {
        kids.sort_by_key(|&c| scene.get(c).map(|n| n.style.order).unwrap_or(0));
    }
    kids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::node::{Node, Scene};
    use crate::style::resolved::{DisplayMode, LocalTransform, PositionDeclared};

    /// from_nodes 无删除：values() 迭代序 = 插入序，node.id 即构造时的 vec 序。
    fn ids_in_order(scene: &Scene) -> Vec<NodeId> {
        scene.nodes.values().map(|n| n.id).collect()
    }

    /// #100 精确形状：absolute z0 底图 + static 顶栏（文本 static、图标 opacity .65）。
    /// 浏览器：图标（opacity SC，z0 层树序在底图后）画在底图之上、文本沉 static 层。
    /// 逐父排序的旧实现：整个 static 顶栏（含图标）沉底——两端分歧实锤。
    #[test]
    fn opacity_descendant_hoists_above_positioned_underlay() {
        let mut paper = Node::default();
        paper.style.position_declared = PositionDeclared::Absolute; // z 未声明 → z:auto
        let topbar = Node::default(); // static
        let text = Node::default(); // static
        let mut icon = Node::default();
        icon.style.opacity = 0.65; // opacity<1 → SC，画 z0 层
        let scene = Scene::from_nodes(
            vec![Node::default(), paper, topbar, text, icon],
            vec![(0, 1), (0, 2), (2, 3), (2, 4)],
        );
        let v = ids_in_order(&scene);
        let order = paint_order(&scene, scene.roots[0], &|_| true);
        assert_eq!(
            order,
            vec![v[0], v[2], v[3], v[1], v[4]],
            "#100 形状完整序：screen → topbar → text → paper → icon（图标浮在底图上）"
        );
    }

    /// 嵌套在 static 子树里的 positioned 后代同样上提（任意深度，不限直接子级）。
    #[test]
    fn positioned_descendant_of_static_subtree_hoists() {
        let mut underlay = Node::default();
        underlay.style.position_declared = PositionDeclared::Absolute;
        let wrapper = Node::default(); // static 中间层
        let mut rel_child = Node::default();
        rel_child.style.position_declared = PositionDeclared::Relative; // z:auto
        let scene = Scene::from_nodes(
            vec![Node::default(), underlay, wrapper, rel_child],
            vec![(0, 1), (0, 2), (2, 3)],
        );
        let v = ids_in_order(&scene);
        let order = paint_order(&scene, scene.roots[0], &|_| true);
        assert_eq!(
            order,
            vec![v[0], v[2], v[1], v[3]],
            "static wrapper 先画，positioned 后代（任意深度）上提到 z0 层、树序在底图后"
        );
    }

    /// SC 边界包含：opacity 组内的 z:5 子不越过组边界（子树整体移动不变量）——
    /// 组画在 z0 层，外部 z:1 兄弟照常在正 z 层盖住整组。
    #[test]
    fn stacking_context_contains_inner_positive_z() {
        let mut fade_group = Node::default();
        fade_group.style.opacity = 0.5;
        let mut inner_high_z = Node::default();
        inner_high_z.style.z_index = 5;
        let mut sibling_z1 = Node::default();
        sibling_z1.style.z_index = 1;
        let scene = Scene::from_nodes(
            vec![
                Node::default(),
                Node::default(),
                fade_group,
                inner_high_z,
                sibling_z1,
            ],
            vec![(0, 1), (0, 2), (2, 3), (0, 4)],
        );
        let v = ids_in_order(&scene);
        let order = paint_order(&scene, scene.roots[0], &|_| true);
        assert_eq!(
            order,
            vec![v[0], v[1], v[2], v[3], v[4]],
            "opacity 组整体在 z0 层（含内部 z:5），外部 z:1 在正 z 层盖上"
        );
    }

    /// 负 z 的 SC 沉到全部 static 之下（任意深度嵌套同理）。
    #[test]
    fn negative_z_sinks_below_statics() {
        let wrapper = Node::default(); // static
        let mut neg = Node::default();
        neg.style.position_declared = PositionDeclared::Relative;
        neg.style.z_index = -1;
        let scene = Scene::from_nodes(
            vec![
                Node::default(),
                Node::default(),
                wrapper,
                neg,
                Node::default(),
            ],
            vec![(0, 1), (0, 2), (2, 3), (0, 4)],
        );
        let v = ids_in_order(&scene);
        let order = paint_order(&scene, scene.roots[0], &|_| true);
        assert_eq!(
            order,
            vec![v[0], v[3], v[1], v[2], v[4]],
            "负 z SC 最先画（static wrapper 之下），statics 按树序"
        );
    }

    /// z 排序矩阵迁移自 paint_order_children 时代：z 升序稳定、#96 形状
    /// （positioned 盖 static）、声明 z:0 抬层。
    #[test]
    fn z_sort_matrix_and_css_tiers() {
        // (a) z 升序稳定：a(0) b(2) c(1) → statics[a] 后 pos[c, b]
        let scene = Scene::from_nodes(
            {
                let mut b = Node::default();
                b.style.z_index = 2;
                let mut c = Node::default();
                c.style.z_index = 1;
                vec![Node::default(), Node::default(), b, c]
            },
            vec![(0, 1), (0, 2), (0, 3)],
        );
        let v = ids_in_order(&scene);
        let order = paint_order(&scene, scene.roots[0], &|_| true);
        assert_eq!(order, vec![v[0], v[1], v[3], v[2]]);

        // (b) #96 形状：absolute 底图（z 未声明）盖住 static 内容
        let scene = Scene::from_nodes(
            {
                let mut paper = Node::default();
                paper.style.position_declared = PositionDeclared::Absolute;
                vec![Node::default(), paper, Node::default()]
            },
            vec![(0, 1), (0, 2)],
        );
        let v = ids_in_order(&scene);
        let order = paint_order(&scene, scene.roots[0], &|_| true);
        assert_eq!(order, vec![v[0], v[2], v[1]]);

        // (c) 声明 z:0（未定位）抬到 static 之上
        let scene = Scene::from_nodes(
            {
                let mut declared0 = Node::default();
                declared0.style.z_declared = true;
                vec![Node::default(), declared0, Node::default()]
            },
            vec![(0, 1), (0, 2)],
        );
        let v = ids_in_order(&scene);
        let order = paint_order(&scene, scene.roots[0], &|_| true);
        assert_eq!(order, vec![v[0], v[2], v[1]]);
    }

    /// `order` 属性改 flex 兄弟的画序（order-modified tree order，浏览器同序）；
    /// block 父不受影响。
    #[test]
    fn order_property_reorders_flex_siblings_only() {
        let a = Node::default();
        let mut b = Node::default();
        b.style.order = -1;
        let scene = Scene::from_nodes(
            vec![Node::default(), a, b], // 缺省 display = Flex
            vec![(0, 1), (0, 2)],
        );
        let v = ids_in_order(&scene);
        let order = paint_order(&scene, scene.roots[0], &|_| true);
        assert_eq!(
            order,
            vec![v[0], v[2], v[1]],
            "flex 父下 order:-1 的 b 先画"
        );

        let mut root = Node::default();
        root.style.display_mode = DisplayMode::Block;
        let a = Node::default();
        let mut b = Node::default();
        b.style.order = -1;
        let scene = Scene::from_nodes(vec![root, a, b], vec![(0, 1), (0, 2)]);
        let v = ids_in_order(&scene);
        let order = paint_order(&scene, scene.roots[0], &|_| true);
        assert_eq!(
            order,
            vec![v[0], v[1], v[2]],
            "block 父下 order 不生效（CSS：order 只作用于 flex/grid item）"
        );
    }

    /// transform / filter 也创建 SC 上提 z0 层（与 opacity 同一档）。
    #[test]
    fn transform_and_filter_hoist_like_opacity() {
        let mut root = Node::default();
        root.style.display_mode = DisplayMode::Block;
        let mut underlay = Node::default();
        underlay.style.position_declared = PositionDeclared::Absolute;
        let wrapper = Node::default();
        let mut scaled = Node::default();
        scaled.style.transform = LocalTransform {
            matrix: [2.0, 0.0, 0.0, 2.0, 0.0, 0.0],
        };
        let mut filtered = Node::default();
        filtered.style.color_filter = Some([1.0; 20]);
        let scene = Scene::from_nodes(
            vec![root, underlay, wrapper, scaled, filtered],
            vec![(0, 1), (0, 2), (2, 3), (2, 4)],
        );
        let v = ids_in_order(&scene);
        let order = paint_order(&scene, scene.roots[0], &|_| true);
        assert_eq!(
            order,
            vec![v[0], v[2], v[1], v[3], v[4]],
            "transform/filter 后代上提 z0 层（树序在底图后），wrapper 留 statics"
        );
    }

    /// include 剪枝：false 的节点整棵子树不进画序（render 侧 display:none/popup
    /// 口径，hit 侧 world_transforms 缺席守卫）。
    #[test]
    fn include_filter_prunes_subtree() {
        let scene = Scene::from_nodes(
            vec![
                Node::default(),
                Node::default(), // 1 visible
                Node::default(), // 2 剪掉（含子 3）
                Node::default(),
            ],
            vec![(0, 1), (0, 2), (2, 3)],
        );
        let v = ids_in_order(&scene);
        let order = paint_order(
            &scene,
            scene.roots[0],
            &|id| id != v[2], // 剪掉 hidden_subtree 整棵（含 child）
        );
        assert_eq!(
            order,
            vec![v[0], v[1]],
            "include=false 的子树整体剪掉（含其后代）"
        );
    }

    /// 动画覆写的 opacity 也驱动 SC 判定（fade-in 到 1.0 的那一帧起不再上提）。
    #[test]
    fn animated_opacity_drives_stacking() {
        let mut scene = Scene::from_nodes(
            {
                let mut underlay = Node::default();
                underlay.style.position_declared = PositionDeclared::Absolute;
                let mut icon = Node::default();
                icon.style.opacity = 1.0; // CSS 声明不透明
                vec![Node::default(), underlay, icon]
            },
            vec![(0, 1), (0, 2)],
        );
        let v = ids_in_order(&scene);
        // 无动画：icon static，沉底图之下
        let order = paint_order(&scene, scene.roots[0], &|_| true);
        assert_eq!(order, vec![v[0], v[2], v[1]]);
        // 动画覆写 0.5：icon 成 SC 上提
        scene.anim.ensure(v[2]).opacity = Some(0.5);
        let order = paint_order(&scene, scene.roots[0], &|_| true);
        assert_eq!(order, vec![v[0], v[1], v[2]]);
    }
}
