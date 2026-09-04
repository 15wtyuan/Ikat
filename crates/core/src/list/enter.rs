use super::pool::encode_reuse_key;
use super::state::{Blueprint, ListState, INITIAL_SLOTS};
use super::templates::check_multi_template_selection;
use super::viewport::ancestor_pane;
use crate::scene::node::{NodeFlags, NodeId, NodeKind, Scene};

/// 进入数据驱动模式：备份全部模板蓝图（`<template>` 子逐个收养 + 兜底=第一个设计期 li）
/// + 建 spacer + 清空设计期子 + 建 ListState。
///
/// 多模板（多个 `<template>` 子）合法——须已给出选择（ItemTemplate override 或
/// TemplateSelector 的 per-item 映射，均经 `Scene::pending_lists` 缓冲到此消费）；
/// 多模板无选择 → Err（FFI 的 enter 前预检 `check_multi_template_selection` 同判，
/// 双保险——core 直调方（测试）也拦得住）。
///
/// 失败路径不留半态：pending 解析在清场**之前**完成，任一步 Err 时清理已克隆的游离
/// master 后返回，ul 子树原样。
///
/// ul 高度必须 auto（否则虚拟化无法撑出可滚内容）；非 auto → Err。祖先 flex 纵向拉伸
/// 同样钉死高度（warning 不 Err——短列表拉伸无害，见 [`ul_flex_stretch_warning`]）。
pub fn enter_data_driven(
    stage: &mut crate::stage::Stage,
    ul: NodeId,
    list_ordinal: u32,
) -> Result<(), String> {
    // 短期不可变借：校验 kind + height + 解析模板源（全部 <template> 子的首个 ListItem，
    // 兜底设计期 li）。不能跨 clone_subtree 持有 scene 借（clone_subtree 也要 &mut stage）。
    let (bp_sources, all_children, stretch_warn): (Vec<NodeId>, Vec<NodeId>, Option<String>) = {
        let scene = stage.scene.as_ref().ok_or("no scene")?;
        if scene.get(ul).map(|n| n.kind) != Some(NodeKind::ListView) {
            return Err("enter_data_driven: node is not a ListView".into());
        }
        check_ul_height_auto(scene, ul)?;
        check_multi_template_selection(scene, ul)?;
        let stretch_warn = ul_flex_stretch_warning(scene, ul);
        let ul_node = scene.get(ul).unwrap();
        // 每个 <template> 子（NodeKind::Template）贡献一个蓝图：其内首个 ListItem
        //（packer 保留 template 子树，缩进空白 TextNode 被 find-by-kind 自然跳过）。
        let mut bp_sources: Vec<NodeId> = Vec::new();
        for &c in &ul_node.children {
            if scene.get(c).map(|cn| cn.kind) != Some(NodeKind::Template) {
                continue;
            }
            let Some(tn) = scene.get(c) else { continue };
            if let Some(li) = tn
                .children
                .iter()
                .copied()
                .find(|&gc| scene.get(gc).map(|gcn| gcn.kind) == Some(NodeKind::ListItem))
            {
                bp_sources.push(li);
            }
        }
        // 兜底：ul 直接 ListItem 子（设计期 li 写法）。有 <template> 时模板优先，设计期 li 不收养。
        if bp_sources.is_empty() {
            if let Some(li) = ul_node
                .children
                .iter()
                .copied()
                .find(|&c| scene.get(c).map(|cn| cn.kind) == Some(NodeKind::ListItem))
            {
                bp_sources.push(li);
            }
        }
        (bp_sources, ul_node.children.clone(), stretch_warn)
    };
    // 拉伸警告（一次性，enter 每列表只跑一次）推运行时警告通道。
    if let Some(msg) = stretch_warn {
        if let Some(scene) = stage.scene.as_mut() {
            scene.warnings.push(msg);
        }
    }
    if bp_sources.is_empty() {
        return Err("ListView 无模板来源：无 <template>、无设计期 li、未设 ItemTemplate".into());
    }
    // 蓝图收养：逐源 clone 到游离 master。src_key = 源 li id——与 C# GetTemplate 返回的
    // UITemplate._srcNodeId 同一节点，pending 映射按它命中（源死后的重推也按它查表：
    // NodeId 带 generation，不会误撞）。
    let mut blueprints: Vec<Blueprint> = Vec::with_capacity(bp_sources.len());
    let mut bp_by_src: std::collections::HashMap<NodeId, u32> = std::collections::HashMap::new();
    let mut default_bp = 0u32;
    // pending 消费（ItemTemplate override + per-item 映射）在清场前解析：映射源要么已
    // 注册（HTML 模板 li），要么是场景内活节点（Instantiate 得到的游离子树）。失败 →
    // 清理全部已克隆 master 返回 Err，ul 原样。
    let pending = stage
        .scene
        .as_mut()
        .ok_or("no scene")?
        .pending_lists
        .remove(&ul);
    let resolve = |stage: &mut crate::stage::Stage,
                   blueprints: &mut Vec<Blueprint>,
                   bp_by_src: &mut std::collections::HashMap<NodeId, u32>,
                   src: NodeId|
     -> Result<u32, String> {
        if let Some(&idx) = bp_by_src.get(&src) {
            return Ok(idx);
        }
        if stage
            .scene
            .as_ref()
            .map(|s| s.get(src).is_none())
            .unwrap_or(true)
        {
            return Err(format!(
                "template source node {} is not alive and was never adopted (stale UITemplate?)",
                src.0
            ));
        }
        let master = stage.clone_subtree(src)?;
        let idx = blueprints.len() as u32;
        bp_by_src.insert(src, idx);
        blueprints.push(Blueprint {
            root: master,
            src_key: src,
            estimate: 0.0,
        });
        Ok(idx)
    };
    let resolve_result: Result<(u32, Vec<u16>), String> = (|| {
        let mut template_ids: Vec<u16> = Vec::new();
        if let Some(pending) = &pending {
            if let Some(src) = pending.override_src {
                default_bp = resolve(stage, &mut blueprints, &mut bp_by_src, src)?;
            }
            if pending.has_map && !pending.item_templates.is_empty() {
                template_ids = vec![default_bp as u16; pending.item_templates.len()];
                for (i, src) in pending.item_templates.iter().enumerate() {
                    if let Some(src) = src {
                        template_ids[i] =
                            resolve(stage, &mut blueprints, &mut bp_by_src, *src)? as u16;
                    }
                }
            }
        }
        Ok((default_bp, template_ids))
    })();
    let (default_bp, template_ids) = match resolve_result {
        Ok(v) => v,
        Err(e) => {
            for bp in &blueprints {
                stage.remove_node(bp.root);
            }
            return Err(e);
        }
    };
    // 全源收养（HTML 模板 li 在 pending 解析后统一收养——顺序保证 override/map 的已注册
    // 源命中，未涉及的模板 li 各占一蓝图，doc 序决定下标）。
    for src in bp_sources {
        resolve(stage, &mut blueprints, &mut bp_by_src, src)?;
    }
    // 清空 ul 全部设计期子（adopted <template> 子树 + 设计期 li + 标签间空白 TextNode），
    // 使 ul 仅剩 spacer+slot。
    for child in &all_children {
        stage.remove_node(*child);
    }
    let head = stage.create_node("div", "")?;
    let tail = stage.create_node("div", "")?;
    configure_spacer(stage, head);
    configure_spacer(stage, tail);
    stage.append_child(ul, head)?;
    stage.append_child(ul, tail)?;
    let default_master = blueprints[default_bp as usize].root;
    // 预分配初始 batch：INITIAL_SLOTS 个 slot 现在就克隆好、挂在 head/tail spacer 之间，
    // 全部 parked（display:none）。slot 从此永驻 ul 子树，只翻 display + 换绑，永不 detach
    // ——后端 GO 随稳定 reuse_key 永驻，滞后一帧的重建闪烁随之消失。
    let mut slots = Vec::with_capacity(INITIAL_SLOTS);
    for ordinal in 0..INITIAL_SLOTS {
        let node = stage.clone_subtree(default_master)?;
        stage.insert_before(ul, node, tail)?;
        let scene = stage.scene.as_mut().ok_or("no scene")?;
        // LOOKUP_SCOPE（不打 SCOPE_ROOT：slot 根 CSS 规则仍按页面根 scope 匹配）。
        if let Some(n) = scene.get_mut(node) {
            n.interaction.flags.insert(NodeFlags::LOOKUP_SCOPE);
        }
        // reuse_key 出生即定（ordinal = slots 下标，slots 只增不减 → key 永不旋转）。
        crate::scene::dynamic::set_reuse_key(scene, node, encode_reuse_key(list_ordinal, ordinal));
        crate::scene::dynamic::set_inline_override(scene, node, "display:none")?;
        slots.push(super::state::Slot {
            node,
            item_index: 0,
            parked: true,
            template_idx: default_bp as u16,
        });
    }
    let ls = ListState {
        item_count: 0,
        blueprints,
        bp_by_src,
        default_bp,
        template_ids,
        heights: super::state::HeightCache::new(0),
        slots,
        visible: 0..0,
        head_spacer: head,
        tail_spacer: tail,
        pending_binds: Vec::new(),
        list_ordinal,
        anchoring_active: false,
        dirty: true,
        grid: false,
        columns: 0,
        row_pitch: 0.0,
        warned_no_pane: false,
        plans_seen: 0,
    };
    stage.scene.as_mut().unwrap().lists.0.insert(ul, ls);
    Ok(())
}

/// ul 高度必须 auto（虚拟化靠 spacer 撑出可滚高度，ul 自身被滚动容器裁切）。
/// taffy 0.12 的 size.height 是 `Dimension`，用 `is_auto()` 检测。
fn check_ul_height_auto(scene: &Scene, ul: NodeId) -> Result<(), String> {
    let n = scene.get(ul).ok_or("ul not found")?;
    if !n.base_style.taffy_style.size.height.is_auto() {
        return Err("数据驱动 ListView 高度必须为 auto（否则虚拟化无法撑出可滚内容）".into());
    }
    Ok(())
}

/// ul 被直接父容器 flex 纵向拉伸的检测（显式 height 非 auto 之外的失效同源路径：
/// ul 高度被钉死 = 视口高 → content_size==viewport 永远不能滚）。只查直接父级——
/// flex 拉伸只作用于直接子级。拉伸检测：
/// - 纵向主轴（父 flex column/column-reverse）：ul `flex-grow > 0`；
/// - 纵向交叉轴（父 flex row/row-reverse）：父 `align-items` 生效值为 stretch
///   （None = CSS 初始值 stretch）且 ul 未用 `align-self` 覆盖成非 stretch。
/// 自滚模式（ul 自身带 ScrollPane）不算——拉伸只是定 ul 尺寸，滚动发生在 ul 内部。
/// 拉伸在视口 ≥ 内容高时无害（短列表常见），故警告级不 Err。
fn ul_flex_stretch_warning(scene: &Scene, ul: NodeId) -> Option<String> {
    let ul_node = scene.get(ul)?;
    if scene.scroll.get(ul).is_some() {
        return None; // 自滚模式：拉伸无害。
    }
    // 无 pane：已退化全量渲染，拉伸无所谓。
    ancestor_pane(scene, ul)?;
    let parent = ul_node.parent?;
    let ps = &scene.get(parent)?.style.taffy_style;
    if !matches!(ps.display, taffy::Display::Flex) {
        return None;
    }
    let cause = match ps.flex_direction {
        taffy::FlexDirection::Column | taffy::FlexDirection::ColumnReverse => {
            if ul_node.style.taffy_style.flex_grow <= 0.0 {
                return None;
            }
            "flex-grow on a flex-column parent"
        }
        taffy::FlexDirection::Row | taffy::FlexDirection::RowReverse => {
            let effective = ps
                .align_items
                .map(|a| a.keyword)
                .unwrap_or(taffy::AlignItemsKeyword::Stretch);
            if ul_node
                .style
                .taffy_style
                .align_self
                .map(|a| a.keyword)
                .unwrap_or(effective)
                != taffy::AlignItemsKeyword::Stretch
            {
                return None;
            }
            "align-items:stretch (the default) on a flex-row parent"
        }
    };
    Some(format!(
        "ListView node {}: its height is stretched by the parent flex container ({cause}) — \
         a stretched list is pinned to the viewport height and can never scroll. \
         Fix: height:auto on the list, or align-self:flex-start, or remove the flex-grow",
        ul.0
    ))
}

/// spacer 初始样式：flex-shrink:0（不被压缩）+ height:0 + padding-top:0.01px（阻断 margin collapsing）。
/// 直接改 base_style.taffy_style（运行时 create_node 的 css 参数虽经 apply_decl，但直接赋值更明确，
/// 避免 padding shorthand 的多值解析路径）。
fn configure_spacer(stage: &mut crate::stage::Stage, spacer: NodeId) {
    let scene = stage.scene.as_mut().unwrap();
    let n = scene.get_mut(spacer).unwrap();
    n.base_style.taffy_style.flex_shrink = 0.0;
    // padding 字段是 LengthPercentage（非 Auto）；size.height 是 Dimension（含 Auto 变体）。
    // taffy 0.12 用小写构造函数 `length(val)` / `auto()`。
    n.base_style.taffy_style.padding.top = taffy::style::LengthPercentage::length(0.01);
    n.base_style.taffy_style.size.height = taffy::style::Dimension::length(0.0);
    n.style = n.base_style.clone();
    n.dirty_mesh = true;
}
