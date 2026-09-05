//! ListView 多模板面：蓝图收养（adopt）+ per-item 模板映射 + enter 前配置缓冲。
//!
//! 数据流：C# `TemplateSelector`（纯 `Func<int, UITemplate>`）在 ItemCount set / Notify*
//! 时对受影响区间求值，把源 NodeId 数组批量推给 core（FFI `yio_list_set_item_templates`）。
//! core 侧零回调——克隆仍完全在内部 execute 阶段发生，选择结果经本模块的 per-item 映射
//! 提前送达。enter 前到达的推送缓冲进 `Scene::pending_lists`，enter 收养蓝图后一并消费。

use crate::scene::node::{NodeId, NodeKind, Scene};

/// 收养模板源为蓝图，返回蓝图下标。
///
/// - `bp_by_src` 命中 → 直接返回（源节点死后仍有效——NodeId 带 24bit generation，
///   slotmap 槽位复用不会撞出同 id；这正是 Notify* 后续重推能命中已收养蓝图的依据）。
/// - 源活着 → `clone_node_recursive` 产游离 master（源保持原样：HTML `<template>` 子树
///   由 enter 清场负责删，Instantiate 得到的游离源由业务持有寿命）→ 注册进蓝图表。
/// - 源死且未注册 → Err（陈旧 UITemplate——典型：enter 前取的 GetTemplate 模板被
///   重复 enter 或节点树被业务删过）。
///
/// 借用注意：ListState 挂在 scene.lists 里，本函数不能同时收 `&mut Scene` + `&mut ListState`
/// （别名借）——统一按 ul 自查。
pub(super) fn adopt_blueprint(scene: &mut Scene, ul: NodeId, src: NodeId) -> Result<u32, String> {
    if let Some(ls) = scene.lists.get(ul) {
        if let Some(&idx) = ls.bp_by_src.get(&src) {
            return Ok(idx);
        }
    }
    // 借用拆分：clone 需独占 scene（不能持 ls 借）——先探活，再克隆，再回借登记。
    if scene.get(src).is_none() {
        return Err(format!(
            "template source node {} is not alive and was never adopted (stale UITemplate?)",
            src.0
        ));
    }
    let master = crate::scene::dynamic::clone_node_recursive(scene, src);
    let Some(ls) = scene.lists.get_mut(ul) else {
        // 不可达（调用方保证 ul 已进数据驱动态，clone_node_recursive 不动 lists 表）——
        // 防御分支，master 游离不注册即弃。
        return Err("ListView disappeared during adopt".into());
    };
    let idx = ls.blueprints.len() as u32;
    if idx >= u16::MAX as u32 {
        return Err("blueprint count overflow (u16 template_ids)".into());
    }
    ls.blueprints.push(super::state::Blueprint {
        root: master,
        src_key: src,
        estimate: 0.0,
    });
    ls.bp_by_src.insert(src, idx);
    Ok(idx)
}

/// per-item 模板映射推送（FFI `yio_list_set_item_templates` 的 core 实现）。
///
/// enter 前：缓冲进 `pending_lists`（vec 按 start+len 扩容，缺省 None=default）。
/// enter 后：逐源收养解析；模板变了的 item 清已测高度（旧高度属旧蓝图），active slot
/// 与新蓝图不匹配的就地 park（display:none），下帧 plan/execute 用正确蓝图重新物化；
/// 已入队但被 park 的 slot 从 pending_binds 剔除（防 C# 对隐形 slot 跑 BindItem）。
pub fn set_item_templates(
    scene: &mut Scene,
    ul: NodeId,
    start: usize,
    src: &[NodeId],
) -> Result<(), String> {
    if scene.lists.get(ul).is_none() {
        // 缓冲：enter 时 consume_pending 解析（源此刻必须活着——enter 马上清场 ul 子树）。
        let cfg = scene.pending_lists.entry(ul).or_default();
        if cfg.item_templates.len() < start + src.len() {
            cfg.item_templates.resize(start + src.len(), None);
        }
        for (k, s) in src.iter().enumerate() {
            cfg.item_templates[start + k] = Some(*s);
        }
        cfg.has_map = true;
        return Ok(());
    }
    // 解析阶段（adopt 内部自查借用，逐源进行）。
    let mut resolved: Vec<u32> = Vec::with_capacity(src.len());
    for s in src {
        resolved.push(adopt_blueprint(scene, ul, *s)?);
    }
    // 应用阶段：写 template_ids + 清换蓝图项的已测高度。
    let changed: Vec<usize> = {
        let Some(ls) = scene.lists.get_mut(ul) else {
            return Ok(());
        };
        if ls.template_ids.len() < start + resolved.len() {
            let fill = ls.default_bp as u16;
            ls.template_ids.resize(start + resolved.len(), fill);
        }
        let mut changed = Vec::new();
        for (k, idx) in resolved.iter().enumerate() {
            let i = start + k;
            let v = *idx as u16;
            if ls.template_ids[i] != v {
                ls.template_ids[i] = v;
                ls.heights.clear(i);
                changed.push(i);
            }
        }
        if !changed.is_empty() {
            ls.dirty = true;
        }
        changed
    };
    if changed.is_empty() {
        return Ok(());
    }
    // park 阶段：模板变了的 item 的 active slot 就地休眠（复用过滤会拒绝跨模板复用，
    // 下帧 plan 把这些 item 重新进 to_bind → execute 以正确蓝图克隆/唤醒）。
    let changed_set: std::collections::HashSet<usize> = changed.into_iter().collect();
    let to_park: std::collections::HashSet<NodeId> = {
        let Some(ls) = scene.lists.get(ul) else {
            return Ok(());
        };
        ls.slots
            .iter()
            .filter(|s| !s.parked && changed_set.contains(&s.item_index))
            .map(|s| s.node)
            .collect()
    };
    if to_park.is_empty() {
        return Ok(());
    }
    if let Some(ls) = scene.lists.get_mut(ul) {
        for s in ls.slots.iter_mut() {
            if to_park.contains(&s.node) {
                s.parked = true;
            }
        }
        // 剔除本帧已入队但被 park 的 bind（对 display:none 的 slot 跑 BindItem 是白写）。
        ls.pending_binds.retain(|(n, _)| !to_park.contains(n));
    }
    for node in &to_park {
        let _ = crate::scene::dynamic::set_inline_override(scene, *node, "display:none");
    }
    Ok(())
}

/// 单模板 override 设定（FFI `yio_list_set_template` 的 core 实现，ItemTemplate 用）。
///
/// enter 前：缓冲进 pending（修复旧路径「无 ListState 返 -1 被静默丢」的缺陷）。
/// enter 后：收养为新蓝图并设为 default；template_ids 里跟随旧 default 的隐式项改指新
/// default（map 显式推过的项不动——显式选择优先于默认）。
pub fn set_list_template(scene: &mut Scene, ul: NodeId, src: NodeId) -> Result<(), String> {
    if scene.lists.get(ul).is_none() {
        scene.pending_lists.entry(ul).or_default().override_src = Some(src);
        return Ok(());
    }
    let idx = adopt_blueprint(scene, ul, src)?;
    let Some(ls) = scene.lists.get_mut(ul) else {
        return Ok(());
    };
    let old = ls.default_bp as u16;
    ls.default_bp = idx;
    let new = idx as u16;
    for t in ls.template_ids.iter_mut() {
        if *t == old {
            *t = new;
        }
    }
    Ok(())
}

/// ul 下 `<template>` 子数（FFI set_item_count 的 enter 前多模板预检用）。
pub fn template_child_count(scene: &Scene, ul: NodeId) -> usize {
    scene
        .get(ul)
        .map(|n| {
            n.children
                .iter()
                .filter(|&c| scene.get(*c).map(|cn| cn.kind) == Some(NodeKind::Template))
                .count()
        })
        .unwrap_or(0)
}

/// enter 前预检：多模板且未给出选择（无 override、无 map）→ Err。
/// FFI 把本 Err 映射为专用返回码（C# 转 UIContractException——契约：有多个模板却没说怎么选）。
pub fn check_multi_template_selection(scene: &Scene, ul: NodeId) -> Result<(), String> {
    if template_child_count(scene, ul) <= 1 {
        return Ok(());
    }
    let selection_given = scene
        .pending_lists
        .get(&ul)
        .map(|p| p.override_src.is_some() || p.has_map)
        .unwrap_or(false);
    if selection_given {
        return Ok(());
    }
    Err("multiple <template> under the list but no ItemTemplate/TemplateSelector given".into())
}
