//! FairyBatching：sort_key 分配 + 绘制序 + rect clip mask_context。
//!
//! 简化（明确不做的事，留作后续优化）：
//! - **sort_key = DFS 出现序**：单一全局计数器，自增即赋值；不做 AABB 重排合并。
//!   保序即正确意图（重排是性能优化，保序能跑通管线即可）。
//! - **mask_context**：clip_rect 的 Container 是 BatchingRoot，开新层级；
//!   子树继承。用「出现序 + 1」当层级 id（计数器+1），不维护真 stencil 层级栈。
//! - **BatchingRoot 边界**：不在 Root 处断批合（FairyGUI 真实策略按贴图/program 断，
//!   当前没有贴图集 / 多 program，断无可断；留待后续）。

use crate::render::node::{MaskContext, NodePayload, RenderNode};
use crate::render::ClipEntry;
use crate::scene::node::{NodeId, Rect, Scene};

/// AABB 交集：返回 intersected Rect；无重叠 → 零面积 `{x, y, w:0, h:0}`（x/y 取
/// max-min 处的边界值，w/h=0）。永远返回 Rect（不是 None），方便 clip 表直填。
///
/// - x = max(a.x, b.x), y = max(a.y, b.y)
/// - right = min(a.x+a.w, b.x+b.w), bottom = min(a.y+a.h, b.y+b.h)
/// - 若 right<=x 或 bottom<=y → 零面积（disjoint → empty）。
///
/// 嵌套 disjoint clip → 零面积 rect（shader safe-blank 处理）。
pub fn rect_intersect(a: Rect, b: Rect) -> Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = (a.x + a.w).min(b.x + b.w);
    let bottom = (a.y + a.h).min(b.y + b.h);
    let w = (right - x).max(0.0);
    let h = (bottom - y).max(0.0);
    Rect { x, y, w, h }
}

/// 是否可合并 Mesh（program=0 + 纯平移 + 非 box-shadow 合成节点）。
/// Text（program=1）/ 非纯平移 / box-shadow 合成节点不参与重排与合并。
fn is_mergeable_mesh(rn: &RenderNode) -> bool {
    matches!(&rn.payload, NodePayload::Mesh { program, .. } if *program == 0)
        && crate::transform::is_pure_translation(&rn.world_matrix)
        && !crate::render::is_shadow_synth(rn.node_id)
        && !crate::render::is_tf_edit_synth(rn.node_id)
}

/// 可合并 Mesh 的 DrawState = (image_path, mask_context)。
/// （program 已由 is_mergeable_mesh 保证 0；blend 仅 Normal 不入 key。）
/// image_path 作合并键（同 path 的图可合批）。
/// 非 mergeable Mesh / Text → None。
fn draw_state(rn: &RenderNode) -> Option<(Option<String>, u32)> {
    match &rn.payload {
        NodePayload::Mesh {
            image_path,
            program,
            ..
        } if *program == 0 => (image_path.clone(), rn.mask_context.0).into(),
        _ => None,
    }
}

/// AABB 是否重叠（交集非零面积）。复用 rect_intersect。
fn aabb_overlap(a: Rect, b: Rect) -> bool {
    let r = rect_intersect(a, b);
    r.w > 0.0 && r.h > 0.0
}

/// 一个重排单元内做 fgui 式稳定插入排序。
/// `unit` = 该单元内节点的 scene 索引（进入时为 DFS 序）；原地重排为 batch 聚拢后顺序。
fn reorder_unit(scene: &Scene, nodes: &[RenderNode], unit: &mut Vec<usize>) {
    let n = unit.len();
    if n < 2 {
        return;
    }
    // nodes 0 基位置 → scene NodeId（经 RenderNode.node_id 桥接）→ scene.get 取 layout_rect。
    // 合成 id（行内图 image mesh / text 跨页子页）不在 scene：零面积兜底，不参与
    // AABB 重叠判断，也绝不 panic（.expect 会 non-unwinding abort 拖垮宿主进程）。
    // 行内图位置已由 build 期 mesh verts 固定，reorder 只按 draw_state（image_path）归批。
    let aabb_of = |pos: usize| -> Rect {
        let nid = NodeId(nodes[pos].node_id);
        scene.get(nid).map(|n| n.layout_rect).unwrap_or(Rect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        })
    };
    for i in 1..n {
        let cur = unit[i];
        let cur_ds = match draw_state(&nodes[cur]) {
            Some(d) => d,
            None => continue, // 单元内应全是 mergeable；防御
        };
        let cur_aabb = aabb_of(cur);
        let mut k: Option<usize> = None; // 插入点（unit 内下标）
        let mut last_ds: Option<(Option<String>, u32)> = None;
        let mut m = i;
        for j in (0..i).rev() {
            let test = unit[j];
            let test_ds = draw_state(&nodes[test]).unwrap(); // 单元内必 mergeable
                                                             // draw_state 含 Option<String>（非 Copy），用 ref 比较避免 move。
            if last_ds.as_ref() != Some(&test_ds) {
                last_ds = Some(test_ds.clone());
                m = j + 1;
            }
            if cur_ds == test_ds {
                k = Some(m);
            }
            if aabb_overlap(cur_aabb, aabb_of(test)) {
                if k.is_none() {
                    k = Some(m);
                }
                break; // 相交保序，停止前扫
            }
        }
        if let Some(ki) = k {
            if ki != i {
                let item = unit.remove(i);
                unit.insert(ki, item);
            }
        }
    }
}

/// 裁剪链项（dfs 内部形）：context 无关的 entry 数据，开新 context 时按链序
/// 逐条展开进 clip 表（多 entry 语义见 [`ClipEntry`]）。
#[derive(Debug, Clone)]
struct ClipChainItem {
    inv_frame: crate::transform::Affine2,
    rect: Option<crate::render::ClipRectSpec>,
    shape: crate::style::resolved::ClipShape,
}

/// 超深 clip 链 warn-once（运行时 CSS 通道越 [`crate::render::MAX_CLIP_CHAIN`] 时；
/// authored 情形 fence 打包期已拒）。
static CLIP_CHAIN_OVERFLOW_WARNED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// 给所有 RenderNode 填 sort_key + mask_context，并产 clip 表（多 entry：每个
/// context = 该层级活跃的**整条**祖先 clip 链，不坍缩）。
///
/// 两 pass：①结构 pass（DOM DFS）算 mask_context + clip 表——树结构性质，与画序
/// 无关（被上提的节点仍按其**树祖先链**取 clip：CSS 里 overflow 裁剪不因画序分层
/// 失效）；②画序 pass 按 [`crate::scene::stacking::paint_order`]（stacking context
/// 全局分层，#100）赋 sort_key。`nodes` 不含 display:none 子树（由
/// `build_render_nodes` 剪掉）；`id_to_pos` 只映射存活 NodeId → nodes vec 位置，
/// 画序 pass 的 include 即 `id_to_pos` 含有性，剪掉整棵子树。返回的
/// `Vec<ClipEntry>` 含且仅含 mask_context>0 的层级（context==0 = 无 clip，不入表）。
///
/// clipper 判定 = `clip_rect.is_some()`（overflow 派生）**或** `style.clip_path`
/// 非空（声明即 clipper，web 原义）。开 context 时把链上全部 entry 以新
/// context_id 重复入表（后代引用一个 context 即得整条链，交集语义）。entry 几何
/// 存 clipper box-local + 世界矩阵逆——滚动/共享祖先变换在逆映射中自动消解，
/// 旧的 scroll_offset 补偿与交集坍缩逻辑已退役（详见 [`ClipEntry`] 文档）。
pub fn assign_sort_keys(
    scene: &Scene,
    nodes: &mut [RenderNode],
    id_to_pos: &std::collections::HashMap<NodeId, usize>,
    sort_keys: &mut [u32],
) -> (Vec<ClipEntry>, Vec<String>) {
    let mut clips: Vec<ClipEntry> = Vec::new();
    let mut warns: Vec<String> = Vec::new();
    let mut ctx_counter: u32 = 0;
    fn dfs_mask(
        scene: &Scene,
        nodes: &mut [RenderNode],
        id_to_pos: &std::collections::HashMap<NodeId, usize>,
        id: NodeId,
        ctx_counter: &mut u32,
        clips: &mut Vec<ClipEntry>,
        warns: &mut Vec<String>,
        parent_mask: MaskContext,
        chain: Vec<ClipChainItem>,
    ) {
        // pruned（display:none 子树）节点不在 id_to_pos → 不赋 mask，不递归子树。
        if !id_to_pos.contains_key(&id) {
            return;
        }
        let node = scene.get_live(id, "render/assign_sort_keys");
        // nodes 0 基位置：用 id_to_pos 映射（slotmap 删节点后有空洞，idx-1 ≠ 位置）。
        // remove_node 后 slotmap idx 不连续，须用 build_render_nodes 算的 id_to_pos。
        let pos = *id_to_pos.get(&id).expect("live node 在 id_to_pos 中");
        // clip-path 惰性解析成 box-local 几何（元素尺寸相关——消费点解析 = 增量
        // solve 下无陈旧风险，Node 不落存储字段）。
        let shape = node
            .style
            .clip_path
            .as_ref()
            .map(|d| d.resolve(node.layout_rect.w, node.layout_rect.h))
            .unwrap_or(crate::style::resolved::ClipShape::None);
        let is_clipping =
            node.clip_rect.is_some() || !matches!(shape, crate::style::resolved::ClipShape::None);
        let (mask, child_chain) = if is_clipping {
            // 链深上限：fence 拒 authored 超深；运行时 CSS 越界 warn-once 丢本
            // clipper（选「少裁」不「全裁」——全裁是黑屏级故障，少裁是渐进退化）。
            if chain.len() >= crate::render::MAX_CLIP_CHAIN {
                // 会话级 warn-once（core 无日志依赖，走 FrameData.warnings →
                // Scene::warnings → 宿主日志的既有通道）。
                if !CLIP_CHAIN_OVERFLOW_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    warns.push(format!(
                        "clip chain deeper than {} — innermost clipper dropped (authored cases are rejected at package time)",
                        crate::render::MAX_CLIP_CHAIN
                    ));
                }
                (parent_mask, chain)
            } else {
                // rect 测试仅在 overflow 裁剪时挂（纯 clip-path 不裁到 border box，
                // shape 可出框）；圆角随 rect 透传（border-radius 裁后代以 overflow
                // 为前提，CSS 语义），并随 entry 传播到后代 context（web 行为）。
                let rect = node.clip_rect.is_some().then(|| {
                    let (w, h) = (node.layout_rect.w, node.layout_rect.h);
                    let r = crate::style::resolved::clamp_corner_radii(
                        w,
                        h,
                        &node.style.border_radius.as_corners(w, h),
                    );
                    let all_zero = r.iter().all(|&(rx, ry)| rx <= 0.0 && ry <= 0.0);
                    crate::render::ClipRectSpec {
                        w,
                        h,
                        radii: if all_zero { None } else { Some(r) },
                    }
                });
                let item = ClipChainItem {
                    inv_frame: crate::transform::inverse(&nodes[pos].world_matrix),
                    rect,
                    shape,
                };
                let ctx = *ctx_counter + 1;
                *ctx_counter = ctx;
                let mut child_chain = chain.clone();
                child_chain.push(item);
                // 链上全部 entry 以新 context 入表——祖先 entry 重复出现 = 多 entry
                // 交集语义的数据面（不坍缩，见 ClipEntry 文档）。
                for it in &child_chain {
                    clips.push(ClipEntry {
                        context_id: ctx,
                        inv_frame: it.inv_frame,
                        rect: it.rect,
                        shape: it.shape.clone(),
                    });
                }
                (MaskContext(ctx), child_chain)
            }
        } else {
            (parent_mask, chain)
        };
        nodes[pos].mask_context = mask;
        for c in node.children.clone() {
            dfs_mask(
                scene,
                nodes,
                id_to_pos,
                c,
                ctx_counter,
                clips,
                warns,
                mask,
                child_chain.clone(),
            );
        }
    }
    for root in &scene.roots {
        dfs_mask(
            scene,
            nodes,
            id_to_pos,
            *root,
            &mut ctx_counter,
            &mut clips,
            &mut warns,
            MaskContext(0),
            Vec::new(),
        );
    }
    // 画序 pass：stacking context 全局分层序 → sort_key（#100：嵌套在 static 子树
    // 里的 opacity/transform/filter/定位+声明 z 后代上提到所属 SC 的层，浏览器同序）。
    let mut key: u32 = 0;
    for root in &scene.roots {
        let order =
            crate::scene::stacking::paint_order(scene, *root, &|id| id_to_pos.contains_key(&id));
        for id in order {
            let pos = *id_to_pos
                .get(&id)
                .expect("paint_order 只发 include 过的节点");
            nodes[pos].sort_key = key;
            sort_keys[id.index()] = key;
            key += 1;
        }
    }
    (clips, warns)
}

/// AABB 保序重排：按 BatchingRoot（mask_context）分段，段内对 program=0
/// Mesh 节点做 fgui 式稳定插入排序（同 DrawState + AABB 不相交才前移），重排后重赋
/// sort_key。Text（program=1）作为 batch break，不重排。
///
/// 前置：`assign_sort_keys` 已赋 mask_context + DFS 序 sort_key + clip 表。
/// 原地改写 `nodes[*].sort_key` 为重排后序。clips 表由 assign_sort_keys 产，不受影响。
pub fn reorder_for_batching(scene: &Scene, nodes: &mut [RenderNode]) {
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by_key(|&i| nodes[i].sort_key);

    // 一遍扫描：识别重排单元（连续 mergeable + 同 mask_context）→ 重排 → 重赋 sort_key。
    let mut next_key: u32 = 0;
    let mut i = 0;
    while i < order.len() {
        let idx = order[i];
        if is_mergeable_mesh(&nodes[idx]) {
            let ctx = nodes[idx].mask_context;
            let mut unit: Vec<usize> = vec![idx];
            let mut j = i + 1;
            while j < order.len()
                && is_mergeable_mesh(&nodes[order[j]])
                && nodes[order[j]].mask_context == ctx
            {
                unit.push(order[j]);
                j += 1;
            }
            reorder_unit(scene, nodes, &mut unit);
            for &uidx in &unit {
                nodes[uidx].sort_key = next_key;
                next_key += 1;
            }
            i = j;
        } else {
            // Text：break，不重排，顺序赋 sort_key。
            nodes[idx].sort_key = next_key;
            next_key += 1;
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::node::{BlendMode, ChangeLevel, NodePayload};
    use crate::scene::node::*;

    fn placeholder_rn(i: usize) -> RenderNode {
        RenderNode {
            mount_root_id: 0,
            node_id: i as u64,
            parent_id: if i == 0 { None } else { Some(0) },
            visible: true,
            alpha: 1.0,
            color_tint: [1.0; 4],
            world_matrix: crate::transform::IDENTITY,
            blend: BlendMode::Normal,
            mask_context: MaskContext(0),
            sort_key: 0,
            change_level: ChangeLevel::Full,
            reuse_key: 0,
            effect: crate::render::node::EffectBlock::default(),
            shadow_params: [0.0; 6],
            gradient: crate::render::gradient::GradientParams::default(),
            payload: NodePayload::Mesh {
                verts: vec![[0.0, 0.0]; 4],
                uvs: vec![[0.0, 0.0]; 4],
                colors: vec![[1.0; 4]; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
                image_path: None,
                program: 0,
                color_matrix: [0.0; 20],
            },
        }
    }

    /// 测 helper：从 scene 构 id_to_pos 映射（同 build_render_nodes 的算法——values() 0 基位置）。
    /// 无间隙时等价 id.index()-1；有间隙时仍正确（remove_node 后用）。
    fn id_to_pos_map(scene: &Scene) -> std::collections::HashMap<NodeId, usize> {
        scene
            .nodes
            .values()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect()
    }

    /// 测 helper：建 sort_keys buffer（capacity+1，对齐生产路径 build_render_nodes 的扩容）。
    fn sort_keys_buf(scene: &Scene) -> Vec<u32> {
        vec![0u32; scene.nodes.capacity() + 1]
    }

    /// 构造 root > [a, b]，全部 Container 无 clip。
    fn tree_root_two_kids() -> Scene {
        let mut root = Node::default();
        let mut a = Node::default();
        let mut b = Node::default();
        // edges (0→1), (0→2) 由 from_nodes 设 parent/children；这里只设 layout_rect 等字段。
        root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        };
        a.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        };
        b.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        };
        Scene::from_nodes(vec![root, a, b], vec![(0, 1), (0, 2)])
    }

    #[test]
    fn keys_monotonic() {
        let scene = tree_root_two_kids();
        let mut rns: Vec<RenderNode> = (0..3).map(placeholder_rn).collect();
        let mut sort_keys = sort_keys_buf(&scene);
        assign_sort_keys(&scene, &mut rns, &id_to_pos_map(&scene), &mut sort_keys);
        // DFS 树序：root(0) → a(1) → b(2)
        assert!(rns[0].sort_key < rns[1].sort_key);
        assert!(rns[1].sort_key < rns[2].sort_key);
        assert_eq!(rns[0].sort_key, 0);
        assert_eq!(rns[1].sort_key, 1);
        assert_eq!(rns[2].sort_key, 2);
    }

    #[test]
    fn no_clip_keeps_mask_zero() {
        let scene = tree_root_two_kids();
        let mut rns: Vec<RenderNode> = (0..3).map(placeholder_rn).collect();
        let mut sort_keys = sort_keys_buf(&scene);
        assign_sort_keys(&scene, &mut rns, &id_to_pos_map(&scene), &mut sort_keys);
        for rn in &rns {
            assert_eq!(rn.mask_context, MaskContext(0), "无 clip 应保持 mask=0");
        }
    }

    #[test]
    fn clip_node_opens_new_mask_layer() {
        // root(clip) > child：root 开新 mask 层，child 继承。
        let mut root = Node::default();
        root.clip_rect = Some(Rect::default()); // 开 clip
        let child = Node::default();
        let scene = Scene::from_nodes(vec![root, child], vec![(0, 1)]);

        let mut rns: Vec<RenderNode> = (0..2).map(placeholder_rn).collect();
        let mut sort_keys = sort_keys_buf(&scene);
        assign_sort_keys(&scene, &mut rns, &id_to_pos_map(&scene), &mut sort_keys);
        // root 是首个分配（counter=0），clip → MaskContext(0+1)=1
        assert_eq!(rns[0].mask_context, MaskContext(1), "clip root 开层级 1");
        assert_eq!(rns[1].mask_context, MaskContext(1), "child 继承父层级");
    }

    /// clipper 节点 border_radius 全零 → rect spec 的 radii=None（直角 AABB clip）。
    #[test]
    fn clip_node_zero_radius_yields_none_radii() {
        use crate::style::resolved::BorderRadius;
        let mut root = Node::default();
        root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        root.clip_rect = Some(Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        });
        root.style.border_radius = BorderRadius::default(); // 全零
        let scene = Scene::from_nodes(vec![root], vec![]);
        let mut rns: Vec<RenderNode> = (0..1).map(placeholder_rn).collect();
        let mut sort_keys = sort_keys_buf(&scene);
        let (clips, _warns) =
            assign_sort_keys(&scene, &mut rns, &id_to_pos_map(&scene), &mut sort_keys);
        assert_eq!(clips.len(), 1);
        assert!(
            clips[0].rect.as_ref().unwrap().radii.is_none(),
            "全零 border_radius → radii=None"
        );
        // rect spec = clipper 自身 box（box-local，(0,0) 起）。
        let r = clips[0].rect.unwrap();
        assert_eq!((r.w, r.h), (100.0, 100.0));
    }

    /// clipper 节点 border_radius 非零 → rect spec 带 radii，四角值按 clipper box
    /// 尺寸解析（as_corners(w,h)）。验 TL/TR/BR/BL 序 + 像素值保真。
    #[test]
    fn clip_node_nonzero_radius_carries_radii() {
        use crate::style::resolved::{BorderRadius, CornerRadius};
        use taffy::style::LengthPercentage;
        let mut root = Node::default();
        root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 100.0,
        };
        root.clip_rect = Some(Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 100.0,
        });
        // 四角不同半径（TL=10,15 TR=20,25 BR=30,35 BL=40,45）验序 + 各角独立。
        root.style.border_radius = BorderRadius {
            corners: [
                CornerRadius {
                    h: LengthPercentage::length(10.0),
                    v: LengthPercentage::length(15.0),
                },
                CornerRadius {
                    h: LengthPercentage::length(20.0),
                    v: LengthPercentage::length(25.0),
                },
                CornerRadius {
                    h: LengthPercentage::length(30.0),
                    v: LengthPercentage::length(35.0),
                },
                CornerRadius {
                    h: LengthPercentage::length(40.0),
                    v: LengthPercentage::length(45.0),
                },
            ],
        };
        let scene = Scene::from_nodes(vec![root], vec![]);
        let mut rns: Vec<RenderNode> = (0..1).map(placeholder_rn).collect();
        let mut sort_keys = sort_keys_buf(&scene);
        let (clips, _warns) =
            assign_sort_keys(&scene, &mut rns, &id_to_pos_map(&scene), &mut sort_keys);
        assert_eq!(clips.len(), 1);
        let r = clips[0]
            .rect
            .as_ref()
            .unwrap()
            .radii
            .expect("非零 border_radius → radii=Some");
        let expected = [(10.0, 15.0), (20.0, 25.0), (30.0, 35.0), (40.0, 45.0)];
        for i in 0..4 {
            assert!(
                (r[i].0 - expected[i].0).abs() < 1e-5 && (r[i].1 - expected[i].1).abs() < 1e-5,
                "corner[{}] 期望 ({},{}) 得 ({},{})",
                i,
                expected[i].0,
                expected[i].1,
                r[i].0,
                r[i].1
            );
        }
    }

    /// 测试辅助：entry 对世界点的包含判定——inv_frame 映回 clipper box-local 后
    /// 测 rect（多 entry 交集语义的最小实现，与 shader/hit 同构）。
    fn entry_contains(e: &ClipEntry, wx: f32, wy: f32) -> bool {
        let (lx, ly) = crate::transform::apply_point(&e.inv_frame, wx, wy);
        match &e.rect {
            None => true,
            Some(spec) => lx >= 0.0 && ly >= 0.0 && lx <= spec.w && ly <= spec.h,
        }
    }

    #[test]
    fn nested_clip_opens_distinct_layers() {
        // root(clip) > mid(clip) > leaf：root=层1，mid=层N（N>1），leaf=mid 层。
        let mut root = Node::default();
        root.clip_rect = Some(Rect::default());
        let mut mid = Node::default();
        mid.clip_rect = Some(Rect::default());
        let leaf = Node::default();
        let scene = Scene::from_nodes(vec![root, mid, leaf], vec![(0, 1), (1, 2)]);

        let mut rns: Vec<RenderNode> = (0..3).map(placeholder_rn).collect();
        let mut sort_keys = sort_keys_buf(&scene);
        let (_clips, _warns) =
            assign_sort_keys(&scene, &mut rns, &id_to_pos_map(&scene), &mut sort_keys);
        // root: counter=0 → mask(1)
        // mid:  counter=1 → mask(2)
        // leaf: counter=2 → 继承 mid mask(2)
        assert_eq!(rns[0].mask_context, MaskContext(1));
        assert_eq!(rns[1].mask_context, MaskContext(2));
        assert_eq!(rns[2].mask_context, MaskContext(2));
    }

    /// 滚动语义在新模型下的形态：entry 存 box-local 几何 + clipper 世界逆矩阵，
    /// 滚动偏移由 frame 映射消解（不再有 scroll_adjusted 折叠 rect）。世界点判定
    /// 走「链上全部 entry 都过」：滚出 viewport 顶部的区域被**祖先** entry 挡下。
    #[test]
    fn clip_entries_in_scroll_container_gate_via_ancestor_chain() {
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
        let mut child = Node::default();
        child.layout_rect = Rect {
            x: 10.0,
            y: 10.0,
            w: 80.0,
            h: 80.0,
        };
        child.clip_rect = Some(Rect {
            x: 10.0,
            y: 10.0,
            w: 80.0,
            h: 80.0,
        });
        let mut scene = Scene::from_nodes(vec![root, child], vec![(0, 1)]);
        let root_id = scene.roots[0];
        scene.scroll.ensure(root_id).scroll_pos = (0.0, 30.0);

        let mut rns: Vec<RenderNode> = (0..2).map(placeholder_rn).collect();
        // child 的 world = layout(10,10) − scroll(0,30)（transform.rs 注入 T(-scroll)）。
        rns[1].world_matrix = [1.0, 0.0, 0.0, 1.0, 10.0, -20.0];
        let mut sort_keys = sort_keys_buf(&scene);
        let (clips, _warns) =
            assign_sort_keys(&scene, &mut rns, &id_to_pos_map(&scene), &mut sort_keys);

        // root ctx(1)：一条 entry（自身）。
        let root_entries: Vec<&ClipEntry> = clips.iter().filter(|c| c.context_id == 1).collect();
        assert_eq!(root_entries.len(), 1, "root context 1 条 entry");
        // child ctx(2)：两条 entry（root + child——多 entry 交集语义）。
        let child_entries: Vec<&ClipEntry> = clips.iter().filter(|c| c.context_id == 2).collect();
        assert_eq!(child_entries.len(), 2, "child context = 祖先链 2 条 entry");

        // 世界点判定（链上全过才算可见）：
        // (10,10) 在可视区 → 两条 entry 都过。
        assert!(
            child_entries.iter().all(|e| entry_contains(e, 10.0, 10.0)),
            "可视点过整条链"
        );
        // (10,-5) 在 child box-local 内（(0,15)）但滚出 root viewport 顶 → 被 root
        // entry 挡下（frame 消解 scroll，祖先 gate 生效）。
        let passing: Vec<bool> = child_entries
            .iter()
            .map(|e| entry_contains(e, 10.0, -5.0))
            .collect();
        assert!(
            passing.iter().any(|&p| !p),
            "滚出 viewport 顶的点必须被链上某条 entry 挡下，得 {:?}",
            passing
        );
    }

    // 多 entry 交集语义（不坍缩）：链上全部 entry 都过才算可见。disjoint / overlap
    // 不再折零面积 rect——交集由链判定隐式表达。

    /// nested disjoint: outer [0,0,100,100] > inner [200,200,50,50]（不相交）> leaf。
    /// inner context 的 2 条 entry 无公共可见点：inner 盒内点过 inner 但挡在 outer，
    /// outer 盒内点反之——「有效裁剪 = 链交集」。
    #[test]
    fn nested_disjoint_clip_chain_has_no_common_point() {
        let mut outer = Node::default();
        outer.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        outer.clip_rect = Some(Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        });
        let mut inner = Node::default();
        inner.layout_rect = Rect {
            x: 200.0,
            y: 200.0,
            w: 50.0,
            h: 50.0,
        };
        inner.clip_rect = Some(Rect {
            x: 200.0,
            y: 200.0,
            w: 50.0,
            h: 50.0,
        });
        let leaf = Node::default();
        let scene = Scene::from_nodes(vec![outer, inner, leaf], vec![(0, 1), (1, 2)]);

        let mut rns: Vec<RenderNode> = (0..3).map(placeholder_rn).collect();
        rns[1].world_matrix = [1.0, 0.0, 0.0, 1.0, 200.0, 200.0];
        let mut sort_keys = sort_keys_buf(&scene);
        let (clips, _warns) =
            assign_sort_keys(&scene, &mut rns, &id_to_pos_map(&scene), &mut sort_keys);

        // mask_context: outer=1, inner=2, leaf 继承 inner=2。
        assert_eq!(rns[0].mask_context, MaskContext(1));
        assert_eq!(rns[1].mask_context, MaskContext(2));
        assert_eq!(rns[2].mask_context, MaskContext(2));

        let ctx2: Vec<&ClipEntry> = clips.iter().filter(|c| c.context_id == 2).collect();
        assert_eq!(ctx2.len(), 2, "inner context = 链 2 条 entry");

        // inner 盒心 (225,225)：过 inner entry、挡在 outer entry。
        let at_inner_center: Vec<bool> = ctx2
            .iter()
            .map(|e| entry_contains(e, 225.0, 225.0))
            .collect();
        assert!(
            at_inner_center.iter().any(|&p| !p),
            "inner 盒心必须被 outer entry 挡下（disjoint 交集空），得 {:?}",
            at_inner_center
        );
        // outer 盒心 (50,50)：过 outer entry、挡在 inner entry。
        let at_outer_center: Vec<bool> =
            ctx2.iter().map(|e| entry_contains(e, 50.0, 50.0)).collect();
        assert!(
            at_outer_center.iter().any(|&p| !p),
            "outer 盒心必须被 inner entry 挡下，得 {:?}",
            at_outer_center
        );
    }

    /// nested overlapping: outer [0,0,100,100] > inner [50,50,100,100] > leaf。
    /// 交集区 [50,50]–[100,100] 内的点过整条链；任一盒外点被对应 entry 挡下。
    #[test]
    fn nested_overlapping_clip_chain_gates_by_intersection() {
        let mut outer = Node::default();
        outer.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        outer.clip_rect = Some(Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        });
        let mut inner = Node::default();
        inner.layout_rect = Rect {
            x: 50.0,
            y: 50.0,
            w: 100.0,
            h: 100.0,
        };
        inner.clip_rect = Some(Rect {
            x: 50.0,
            y: 50.0,
            w: 100.0,
            h: 100.0,
        });
        let leaf = Node::default();
        let scene = Scene::from_nodes(vec![outer, inner, leaf], vec![(0, 1), (1, 2)]);

        let mut rns: Vec<RenderNode> = (0..3).map(placeholder_rn).collect();
        rns[1].world_matrix = [1.0, 0.0, 0.0, 1.0, 50.0, 50.0];
        let mut sort_keys = sort_keys_buf(&scene);
        let (clips, _warns) =
            assign_sort_keys(&scene, &mut rns, &id_to_pos_map(&scene), &mut sort_keys);

        let ctx2: Vec<&ClipEntry> = clips.iter().filter(|c| c.context_id == 2).collect();
        assert_eq!(ctx2.len(), 2);
        // 交集区心 (75,75) 过整条链。
        assert!(
            ctx2.iter().all(|e| entry_contains(e, 75.0, 75.0)),
            "交集区心过整条链"
        );
        // (25,25) 只在 outer 盒内 → 挡在 inner entry。
        assert!(
            ctx2.iter().any(|e| !entry_contains(e, 25.0, 25.0)),
            "outer-only 点被 inner entry 挡下"
        );
        // (125,125) 只在 inner 盒内 → 挡在 outer entry。
        assert!(
            ctx2.iter().any(|e| !entry_contains(e, 125.0, 125.0)),
            "inner-only 点被 outer entry 挡下"
        );
    }

    /// clip-path 声明即 clipper（无 overflow）：开新 context，entry 带 shape、
    /// 无 rect 测试（纯 clip-path 不裁到 border box，shape 可出框）。
    #[test]
    fn clip_path_declares_clipper_without_rect() {
        use crate::style::resolved::{ClipPathDecl, ClipShape};
        use taffy::style::LengthPercentage;
        let mut root = Node::default();
        root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        root.style.clip_path = Some(ClipPathDecl::Circle {
            radius: LengthPercentage::percent(0.5),
            cx: LengthPercentage::percent(0.5),
            cy: LengthPercentage::percent(0.5),
        });
        let child = Node::default();
        let scene = Scene::from_nodes(vec![root, child], vec![(0, 1)]);

        let mut rns: Vec<RenderNode> = (0..2).map(placeholder_rn).collect();
        let mut sort_keys = sort_keys_buf(&scene);
        let (clips, _warns) =
            assign_sort_keys(&scene, &mut rns, &id_to_pos_map(&scene), &mut sort_keys);
        assert_eq!(
            rns[0].mask_context,
            MaskContext(1),
            "clip-path 声明即 clipper"
        );
        assert_eq!(rns[1].mask_context, MaskContext(1), "子继承 ctx");
        assert_eq!(clips.len(), 1);
        assert!(clips[0].rect.is_none(), "纯 clip-path entry 无 rect 测试");
        match &clips[0].shape {
            ClipShape::Circle { cx, cy, r } => {
                assert!((*cx - 50.0).abs() < 1e-4 && (*cy - 50.0).abs() < 1e-4);
                assert!((*r - 50.0).abs() < 1e-4, "100×100 circle(50%) 内切");
            }
            other => panic!("entry shape 形错：{other:?}"),
        }
    }

    /// overflow:hidden + clip-path 同元素：单 entry 双测试并存（rect Some + shape），
    /// entry 链语义 = 两条都过（web 交集原义）。
    #[test]
    fn overflow_plus_clip_path_single_entry_dual_test() {
        use crate::style::resolved::{ClipPathDecl, ClipShape};
        use taffy::style::LengthPercentage;
        let mut root = Node::default();
        root.layout_rect = Rect {
            x: 10.0,
            y: 10.0,
            w: 100.0,
            h: 100.0,
        };
        root.clip_rect = Some(Rect {
            x: 10.0,
            y: 10.0,
            w: 100.0,
            h: 100.0,
        });
        root.style.clip_path = Some(ClipPathDecl::Polygon {
            points: vec![
                (
                    LengthPercentage::percent(0.5),
                    LengthPercentage::percent(0.0),
                ),
                (
                    LengthPercentage::percent(1.0),
                    LengthPercentage::percent(0.5),
                ),
                (
                    LengthPercentage::percent(0.5),
                    LengthPercentage::percent(1.0),
                ),
                (
                    LengthPercentage::percent(0.0),
                    LengthPercentage::percent(0.5),
                ),
            ],
        });
        let child = Node::default();
        let scene = Scene::from_nodes(vec![root, child], vec![(0, 1)]);

        let mut rns: Vec<RenderNode> = (0..2).map(placeholder_rn).collect();
        let mut sort_keys = sort_keys_buf(&scene);
        let (clips, _warns) =
            assign_sort_keys(&scene, &mut rns, &id_to_pos_map(&scene), &mut sort_keys);
        assert_eq!(clips.len(), 1, "同元素双声明 = 单 entry");
        let e = &clips[0];
        let rect = e.rect.as_ref().expect("overflow 挂 rect 测试");
        assert_eq!((rect.w, rect.h), (100.0, 100.0));
        assert!(matches!(e.shape, ClipShape::Polygon { .. }), "shape 并存");

        // 语义：菱形心（design (60,60)）过双测试；rect 角（design (15,15)）在
        // rect 内但菱形外 → 挡在 shape。
        let mut rns2: Vec<RenderNode> = (0..2).map(placeholder_rn).collect();
        rns2[0].world_matrix = [1.0, 0.0, 0.0, 1.0, 10.0, 10.0];
        let mut sort_keys2 = sort_keys_buf(&scene);
        let (clips2, _) =
            assign_sort_keys(&scene, &mut rns2, &id_to_pos_map(&scene), &mut sort_keys2);
        let e2 = &clips2[0];
        // 映 design (60,60) → box-local (50,50)：rect 过 + 菱形心过。
        let (lx, ly) = crate::transform::apply_point(&e2.inv_frame, 60.0, 60.0);
        let in_rect = lx >= 0.0 && ly >= 0.0 && lx <= 100.0 && ly <= 100.0;
        assert!(in_rect);
        assert!(e2.shape.contains(lx, ly), "菱形心过 shape");
        // design (15,15) → box-local (5,5)：rect 过、菱形外。
        let (lx, ly) = crate::transform::apply_point(&e2.inv_frame, 15.0, 15.0);
        assert!(lx >= 0.0 && ly >= 0.0, "rect 内");
        assert!(!e2.shape.contains(lx, ly), "菱形外挡下");
    }

    /// 构造 program=0 Mesh RenderNode（给 reorder_unit 直接喂 unit 索引对应的 nodes）。
    /// image_path（None=纯色，Some=图片 path）。
    fn mesh_rn(path: Option<&str>, rect: Rect, mask: u32) -> RenderNode {
        RenderNode {
            mount_root_id: 0,
            node_id: 0,
            parent_id: None,
            visible: true,
            alpha: 1.0,
            color_tint: [1.0; 4],
            world_matrix: crate::transform::IDENTITY,
            blend: BlendMode::Normal,
            mask_context: MaskContext(mask),
            sort_key: 0,
            change_level: ChangeLevel::Full,
            reuse_key: 0,
            effect: crate::render::node::EffectBlock::default(),
            shadow_params: [0.0; 6],
            gradient: crate::render::gradient::GradientParams::default(),
            payload: NodePayload::Mesh {
                verts: vec![
                    [rect.x, rect.y],
                    [rect.x + rect.w, rect.y],
                    [rect.x + rect.w, rect.y + rect.h],
                    [rect.x, rect.y + rect.h],
                ],
                uvs: vec![[0.0, 0.0]; 4],
                colors: vec![[1.0; 4]; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
                image_path: path.map(|s| s.to_string()),
                program: 0,
                color_matrix: [0.0; 20],
            },
        }
    }

    #[test]
    fn reorder_unit_same_drawstate_disjoint_gathers() {
        // [A(path a.png, x=0), B(path b.png, x=100), C(path a.png, x=200)] 全不相交 → C 前移到 A 旁。
        // reorder_unit 经 RenderNode.node_id 桥接回 scene NodeId 取 layout_rect。
        let mut a = Node::default();
        a.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        let mut b = Node::default();
        b.layout_rect = Rect {
            x: 100.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        let mut c = Node::default();
        c.layout_rect = Rect {
            x: 200.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        let scene = Scene::from_nodes(vec![a.clone(), b.clone(), c.clone()], vec![]);
        let ids: Vec<NodeId> = scene.nodes.values().map(|n| n.id).collect();
        let mut nodes = vec![
            mesh_rn(
                Some("a.png"),
                Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                },
                0,
            ),
            mesh_rn(
                Some("b.png"),
                Rect {
                    x: 100.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                },
                0,
            ),
            mesh_rn(
                Some("a.png"),
                Rect {
                    x: 200.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                },
                0,
            ),
        ];
        nodes[0].node_id = ids[0].0;
        nodes[1].node_id = ids[1].0;
        nodes[2].node_id = ids[2].0;
        let mut unit = vec![0usize, 1, 2];
        reorder_unit(&scene, &nodes, &mut unit);
        // A,C 同 path a.png 聚拢：[A(0), C(2), B(1)]
        assert_eq!(unit, vec![0, 2, 1], "同 DrawState 不相交 → C 前移到 A 旁");
    }

    #[test]
    fn reorder_unit_overlapping_keeps_order() {
        // A(a.png) B(b.png) C(a.png)，A 与 C AABB 相交 → C 仍前移到 A 旁（k=A 之后），
        // 但不越过 A（保 A→C 绘制序，防遮挡）。B(b.png) 被推后。
        // 注：fgui DoFairyBatching 语义非「相交=不动」，而是「向后扫到首个相交即停，
        // 但 k 已在相交前按同 material 聚拢点算出」——同 material 相交仍聚拢到紧邻。
        let mut a = Node::default();
        a.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 50.0,
            h: 50.0,
        };
        let mut b = Node::default();
        b.layout_rect = Rect {
            x: 100.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        let mut c = Node::default();
        c.layout_rect = Rect {
            x: 10.0,
            y: 10.0,
            w: 50.0,
            h: 50.0,
        };
        let scene = Scene::from_nodes(vec![a, b, c], vec![]);
        let ids: Vec<NodeId> = scene.nodes.values().map(|n| n.id).collect();
        let mut nodes = vec![
            mesh_rn(
                Some("a.png"),
                Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 50.0,
                    h: 50.0,
                },
                0,
            ),
            mesh_rn(
                Some("b.png"),
                Rect {
                    x: 100.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                },
                0,
            ),
            mesh_rn(
                Some("a.png"),
                Rect {
                    x: 10.0,
                    y: 10.0,
                    w: 50.0,
                    h: 50.0,
                },
                0,
            ), // 与 A 相交
        ];
        nodes[0].node_id = ids[0].0;
        nodes[1].node_id = ids[1].0;
        nodes[2].node_id = ids[2].0;
        let mut unit = vec![0usize, 1, 2];
        reorder_unit(&scene, &nodes, &mut unit);
        // C 同 path a.png 聚拢到 A 旁（k=A 之后=1），不越 A（保 A→C 序）；B 被推后。
        assert_eq!(
            unit,
            vec![0, 2, 1],
            "同 DrawState 相交 → 聚拢到紧邻，不越目标"
        );
    }

    /// helper：把 mesh_rn 包成 RenderNode 并设 node_id。
    fn mesh_rn_into_rn(id: usize, path: Option<&str>, _scene: &Scene) -> RenderNode {
        let mut r = mesh_rn(
            path,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            0,
        );
        r.node_id = id as u64;
        r
    }
    fn text_rn(id: usize) -> RenderNode {
        let mut r = placeholder_rn(id);
        r.node_id = id as u64;
        // text 现产 Mesh(program=1, image_path=合成 atlas path)。
        r.payload = NodePayload::Mesh {
            verts: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            colors: vec![[1.0; 4]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            image_path: Some("ikat://font-atlas/p0".into()),
            program: 1,
            color_matrix: [0.0; 20],
        };
        r
    }

    #[test]
    fn reorder_splits_at_text_break() {
        // root > [A(a.png), Text, B(a.png)]：AABB 全不相交。Text 断单元 →
        // A、B 分属两个单元，B 不能跨 Text 前移到 A 旁（保 Text 绘制序）。
        let mut root = Node::default();
        root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 50.0,
        };
        let mut a = Node::default();
        a.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        let mut t = Node::default();
        t.kind = NodeKind::TextNode;
        t.layout_rect = Rect {
            x: 100.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        let mut b = Node::default();
        b.layout_rect = Rect {
            x: 200.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        let scene = Scene::from_nodes(vec![root, a, t, b], vec![(0, 1), (0, 2), (0, 3)]);
        let _text_id = scene.roots[0]; // text node content now lives in scene.text_contents
        let ids: Vec<NodeId> = scene.nodes.values().map(|n| n.id).collect();
        // rns 顺 = scene.nodes.values() 顺（root, a, t, b）
        let mut rns: Vec<RenderNode> = vec![
            {
                let mut r = placeholder_rn(0);
                r.mask_context = MaskContext(0);
                r.node_id = ids[0].0;
                r
            },
            {
                let mut r = mesh_rn_into_rn(0, Some("a.png"), &scene);
                r.node_id = ids[1].0;
                r
            },
            {
                let mut r = text_rn(0);
                r.node_id = ids[2].0;
                r
            },
            {
                let mut r = mesh_rn_into_rn(0, Some("a.png"), &scene);
                r.node_id = ids[3].0;
                r
            },
        ];
        // 先赋 DFS 序 sort_key（模拟 assign_sort_keys 输出）+ mask_context。
        for (k, r) in rns.iter_mut().enumerate() {
            r.sort_key = k as u32;
            r.mask_context = MaskContext(0);
        }

        reorder_for_batching(&scene, &mut rns);
        // Text 必在 A 与 B 之间（保绘制序）。
        let sk = |id: u64| rns.iter().find(|r| r.node_id == id).unwrap().sort_key;
        assert!(sk(ids[1].0) < sk(ids[2].0), "A 在 Text 前");
        assert!(
            sk(ids[2].0) < sk(ids[3].0),
            "Text 在 B 前（B 不跨 Text 前移）"
        );
    }

    #[test]
    fn reorder_splits_at_mask_context_boundary() {
        // 两个 mask_context 的 Mesh 不跨边界重排（不同 DrawState）。
        // A(ctx0,a.png) B(ctx1,a.png) C(ctx0,a.png)：A、C 同 ctx0 但被 B(ctx1) 断开，
        // 且 AABB 不相交。C 不应跨 ctx 边界前移到 A 旁。
        let root = Node::default();
        let mut n1 = Node::default();
        n1.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        let mut n2 = Node::default();
        n2.layout_rect = Rect {
            x: 100.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        let mut n3 = Node::default();
        n3.layout_rect = Rect {
            x: 200.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        let scene = Scene::from_nodes(vec![root, n1, n2, n3], vec![(0, 1), (0, 2), (0, 3)]);
        let ids: Vec<NodeId> = scene.nodes.values().map(|n| n.id).collect();
        // rns 顺 = A, B, C（跳过 root，root 不参与 reorder 单元——这里测只放 3 个 mesh）。
        let mut rns: Vec<RenderNode> = vec![
            {
                let mut r = mesh_rn_into_rn(0, Some("a.png"), &scene);
                r.node_id = ids[1].0;
                r
            },
            {
                let mut r = mesh_rn_into_rn(0, Some("a.png"), &scene);
                r.node_id = ids[2].0;
                r
            },
            {
                let mut r = mesh_rn_into_rn(0, Some("a.png"), &scene);
                r.node_id = ids[3].0;
                r
            },
        ];
        // sort_key = DFS 序；mask_context: 0→ctx0, 1→ctx1, 2→ctx0（模拟跨 clip 边界）。
        rns[0].sort_key = 0;
        rns[0].mask_context = MaskContext(0);
        rns[1].sort_key = 1;
        rns[1].mask_context = MaskContext(1);
        rns[2].sort_key = 2;
        rns[2].mask_context = MaskContext(0);

        reorder_for_batching(&scene, &mut rns);
        // C(ctx0) 不跨 B(ctx1) 前移：B 的 sort_key 仍在 A、C 之间或 A 前，但 C 不越 B。
        let sk = |id: u64| rns.iter().find(|r| r.node_id == id).unwrap().sort_key;
        // C 不应跑到 B 前面（不同 ctx 不跨边界）。
        assert!(sk(ids[2].0) < sk(ids[3].0), "C(ctx0) 不跨 B(ctx1) 边界前移");
    }
}
