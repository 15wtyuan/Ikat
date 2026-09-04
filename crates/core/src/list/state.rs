use crate::scene::node::NodeId;

/// 预渲染缓冲项数（可见区前后各 BUFFER 项提前克隆 + bind，吸收滚动速度）。
pub const BUFFER: usize = 2;
/// 冷启动（首帧 layout_rect 全 0 → viewport.h=0）时实例化的项数。
pub const INITIAL_SLOTS: usize = 1 + 2 * BUFFER;

/// 每项高度缓存。未测项的估高由调用方按 item 的蓝图给（`height_of(i, fallback)`）——
/// 多模板下不同蓝图高度不同，全局均值会把 A/B 交替列表的 spacer 估歪。
/// sum = 已测部分精确和 + 未测数 × 各自蓝图 estimate。朴素 O(n)，触发换 Fenwick 判据：sum 占 tick > 5%。
#[derive(Debug, Clone, Default)]
pub struct HeightCache {
    pub known: Vec<Option<f32>>,
}

impl HeightCache {
    pub fn new(item_count: usize) -> Self {
        Self {
            known: vec![None; item_count],
        }
    }

    /// 改变项数。旧已知项保留（不收缩则 resize 填 None）。
    pub fn resize(&mut self, item_count: usize) {
        self.known.resize(item_count, None);
    }

    pub fn height_of(&self, i: usize, fallback: f32) -> f32 {
        self.known
            .get(i)
            .copied()
            .flatten()
            .unwrap_or(fallback.max(0.0))
    }

    /// 已测均值（跨蓝图全体）。蓝图自身尚无已测项时作次级 fallback——保证任一模板
    /// 测过一次后，未测项的估高就为正（估高 ≤0 会让累积和永不达阈值 → 误判整列可见）。
    pub fn mean_known(&self) -> Option<f32> {
        let mut n = 0usize;
        let mut sum = 0.0f32;
        for v in self.known.iter().flatten() {
            sum += *v;
            n += 1;
        }
        if n > 0 {
            Some(sum / n as f32)
        } else {
            None
        }
    }

    /// 是否存在任一已测项（冷启动判定：全未测 → 退化为 INITIAL_SLOTS 定数）。
    pub fn any_known(&self) -> bool {
        self.known.iter().any(|v| v.is_some())
    }

    pub fn set(&mut self, i: usize, h: f32) {
        if i < self.known.len() {
            self.known[i] = Some(h);
        }
    }

    /// 清单项已测高度（item 换蓝图时调——旧高度属旧模板，留着会污染新模板估高与 spacer 求和）。
    pub fn clear(&mut self, i: usize) {
        if i < self.known.len() {
            self.known[i] = None;
        }
    }

    /// 求和 [start..end)。已测精确 + 未测 × fallback(item)。
    pub fn sum(&self, range: std::ops::Range<usize>, fallback_of: impl Fn(usize) -> f32) -> f32 {
        let mut total = 0.0;
        for i in range {
            total += self.height_of(i, fallback_of(i));
        }
        total
    }
}

/// 单个 item 蓝图：克隆 master（游离子树根）+ 估高（本蓝图已测均值）+ 来源键。
///
/// `src_key` = 收养时的源节点 id（`<template>` 内首个 li，或运行时传入的场景内子树根）。
/// 源节点死后**仍作注册键**——NodeId 带 24bit generation（slotmap 版本位），删除后槽位
/// 复用也不会撞出相同 id，跨死亡查表安全。C# `TemplateSelector` 求值出的 UITemplate
/// 持有的正是这些源 id，Notify* 后续重推时经 `bp_by_src` 命中已收养蓝图。
#[derive(Debug, Clone)]
pub struct Blueprint {
    pub root: NodeId,
    pub src_key: NodeId,
    /// 本蓝图已测项高度均值（collect_heights 每帧重算；未测=0）。
    pub estimate: f32,
}

/// 单个虚拟列表 slot（克隆出的实例根 + 它当前绑定的 item 序号 + 蓝图）。
///
/// slot 从 `enter_data_driven` 预分配起永驻 ul 子树——离开可见区不 detach，只标 `parked`
/// （置 display:none 便签），保住 NodeId / parent / reuse_key，后端 GO 随之永驻不重建。
/// `template_idx`：本 slot 从哪个蓝图克隆（池按蓝图分组——A 蓝图的 slot 不能复用去绑
/// B 蓝图的 item，execute 复用时按它过滤）。
#[derive(Debug, Clone, Copy)]
pub struct Slot {
    pub node: NodeId,
    /// 当前绑的 item；parked 时保留上次值（stale，仅作复用参考）。
    pub item_index: usize,
    /// true = 休眠（display:none override 已置，不占布局、不渲染）。
    pub parked: bool,
    /// 本 slot 克隆自的蓝图下标（blueprints 索引）。
    pub template_idx: u16,
}

/// enter 前的 ListView 模板配置缓冲。C# 投影允许任意顺序配置属性（先 ItemTemplate /
/// 先 TemplateSelector、再 ItemCount 触发 enter），enter 前到达的模板设定缓存在此，
/// enter 时一并消费。修复旧路径「enter 前 set ItemTemplate 被静默丢弃」的缺陷。
#[derive(Debug, Clone, Default)]
pub struct PendingListCfg {
    /// ItemTemplate 设定（set_template 缓冲；None=未设）。
    pub override_src: Option<NodeId>,
    /// TemplateSelector 求值出的 per-item 模板源（按 item index；None=default）。
    /// `has_map` 独立于 vec 判定「选择已给出」——空列表（count=0）时 vec 为空但意图已在。
    pub item_templates: Vec<Option<NodeId>>,
    pub has_map: bool,
}

/// ListView 运行时虚拟化状态。每 ul（NodeKind::ListView）一个槽。
///
/// - `blueprints`：全部已收养蓝图（enter 收养 list 下全部 `<template>` + 运行时 adopt），
///   下标 0 起；`default_bp` 为未指定模板项所用（enter 时 = 首个 `<template>` 或 ItemTemplate）。
/// - `template_ids`：每 item 的蓝图下标（len == item_count）。单模板列表恒 default_bp。
/// - `slots`：已实例化的 slot（克隆出的 li），高水位——只增不减，永不 detach。
/// - `visible`：上帧算出的可见 item 区间 [start, end)。
/// - `pending_binds`：本帧新绑定的 (slot_node, item_index)，由 bind 阶段消费。
/// - `anchoring_active` / `dirty`：anchoring / 静默刷新标记（预留）。
#[derive(Debug, Clone)]
pub struct ListState {
    pub item_count: usize,
    pub blueprints: Vec<Blueprint>,
    /// 源节点 id → 蓝图下标（跨源死亡持久；运行时 adopt 追加）。
    pub bp_by_src: std::collections::HashMap<NodeId, u32>,
    pub default_bp: u32,
    pub template_ids: Vec<u16>,
    pub heights: HeightCache,
    pub slots: Vec<Slot>,
    pub visible: std::ops::Range<usize>,
    pub head_spacer: NodeId,
    pub tail_spacer: NodeId,
    pub pending_binds: Vec<(NodeId, usize)>,
    pub list_ordinal: u32,
    pub anchoring_active: bool,
    pub dirty: bool,
    /// wrap 网格虚拟化：ul.style 为 flex-row+wrap 时 true，按行虚拟化、行内全量。
    /// 单列（含 flex-column、block）为 false，走原有 1D item 路径。
    pub grid: bool,
    /// 网格每行列数（grid=true 时首帧 solve 后测得；0=尚未测，退化为冷启动定数）。
    pub columns: usize,
    /// 行距 = item_h + gap_y（grid 测得 columns 时一并填）。单列不用。
    pub row_pitch: f32,
    /// warn-once 旗标：无滚动容器退化全量渲染已警告（每列表一次，防每帧刷屏）。
    pub warned_no_pane: bool,
    /// plan 次数计数（no-pane 警告宽限）：首个 plan 可能先于滚动容器 scroll 条目
    /// 物化（tick 序 plan < rematch < refresh_content_sizes），无窗首帧必 None——
    /// 第二次 plan 起仍无窗才确属配置错。防首帧竞态误报。
    pub plans_seen: u32,
}

impl ListState {
    /// item i 的蓝图下标（template_ids 越界/超界回落 default_bp——防御 C# 未推全量）。
    pub fn bp_of(&self, i: usize) -> u32 {
        let max = self.blueprints.len() as u32;
        if max == 0 {
            return 0;
        }
        self.template_ids
            .get(i)
            .map(|&t| t as u32)
            .unwrap_or(self.default_bp)
            .min(max - 1)
    }

    /// item i 的估高 fallback：本蓝图已测均值；未测时全体已测均值（保证测过任一模板后为正）。
    pub fn bp_estimate_of(&self, i: usize) -> f32 {
        let bp = self.bp_of(i) as usize;
        let own = self.blueprints.get(bp).map(|b| b.estimate).unwrap_or(0.0);
        if own > 0.0 {
            own
        } else {
            self.heights.mean_known().unwrap_or(0.0)
        }
    }

    /// item i 的高度（已测精确 / 未测按蓝图估高）。
    pub fn item_height(&self, i: usize) -> f32 {
        self.heights.height_of(i, self.bp_estimate_of(i))
    }

    /// 区间高度求和（spacer / scroll 目标偏移共用）。
    pub fn sum_heights(&self, range: std::ops::Range<usize>) -> f32 {
        self.heights.sum(range, |i| self.bp_estimate_of(i))
    }
}

impl Default for ListState {
    fn default() -> Self {
        Self {
            item_count: 0,
            blueprints: Vec::new(),
            bp_by_src: std::collections::HashMap::new(),
            default_bp: 0,
            template_ids: Vec::new(),
            heights: HeightCache::default(),
            slots: Vec::new(),
            visible: 0..0,
            head_spacer: NodeId::INVALID,
            tail_spacer: NodeId::INVALID,
            pending_binds: Vec::new(),
            list_ordinal: 0,
            anchoring_active: false,
            dirty: true,
            grid: false,
            columns: 0,
            row_pitch: 0.0,
            warned_no_pane: false,
            plans_seen: 0,
        }
    }
}

/// 每 ListView 节点的虚拟化状态表（`HashMap<NodeId, ListState>`）。运行时态，不进 pkg。
/// 结构与访问约定同 `ScrollTable`/`AnimTable`（NodeId 不能直接当
/// SecondaryMap Key，用 HashMap 便租用 / 零转换）。enter_data_driven 填、remove_node 联动清。
#[derive(Debug, Clone, Default)]
pub struct ListTable(pub std::collections::HashMap<NodeId, ListState>);

impl ListTable {
    pub fn get(&self, id: NodeId) -> Option<&ListState> {
        self.0.get(&id)
    }
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut ListState> {
        self.0.get_mut(&id)
    }
    /// 删该节点 ListState（remove_node 联动调，防悬空 NodeId 残留）。
    pub fn remove(&mut self, id: NodeId) {
        self.0.remove(&id);
    }
}
