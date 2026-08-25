use crate::scene::node::NodeId;

/// 预渲染缓冲项数（可见区前后各 BUFFER 项提前克隆 + bind，吸收滚动速度）。
pub const BUFFER: usize = 2;
/// 冷启动（首帧 layout_rect 全 0 → viewport.h=0）时实例化的项数。
pub const INITIAL_SLOTS: usize = 1 + 2 * BUFFER;

/// 每项高度缓存。未测项用 estimate（已测均值；无已测时=模板首次布局高）。
/// sum = 已测部分精确和 + 未测数 × estimate。朴素 O(n)，触发换 Fenwick 判据：sum 占 tick > 5%。
#[derive(Debug, Clone)]
pub struct HeightCache {
    pub known: Vec<Option<f32>>,
    pub estimate: f32,
}

impl HeightCache {
    pub fn new(item_count: usize, initial_estimate: f32) -> Self {
        Self {
            known: vec![None; item_count],
            estimate: initial_estimate,
        }
    }

    /// 改变项数。旧已知项保留（不收缩则 resize 填 None）。`initial_estimate` 仅在已测项清空时生效。
    pub fn resize(&mut self, item_count: usize, initial_estimate: f32) {
        self.known.resize(item_count, None);
        if self.known.is_empty() {
            self.estimate = initial_estimate;
        }
    }

    pub fn height_of(&self, i: usize) -> f32 {
        self.known
            .get(i)
            .copied()
            .flatten()
            .unwrap_or(self.estimate)
    }

    pub fn set(&mut self, i: usize, h: f32) {
        if i < self.known.len() {
            self.known[i] = Some(h);
        }
        self.recompute_estimate();
    }

    /// 求和 [start..end)。已测精确 + 未测 × estimate。
    pub fn sum(&self, range: std::ops::Range<usize>) -> f32 {
        let mut total = 0.0;
        for i in range {
            total += self.height_of(i);
        }
        total
    }

    fn recompute_estimate(&mut self) {
        let known: Vec<f32> = self.known.iter().filter_map(|v| *v).collect();
        if !known.is_empty() {
            self.estimate = known.iter().sum::<f32>() / known.len() as f32;
        }
    }
}

impl Default for HeightCache {
    fn default() -> Self {
        Self {
            known: Vec::new(),
            estimate: 0.0,
        }
    }
}

/// 单个虚拟列表 slot（克隆出的实例根 + 它当前绑定的 item 序号）。
///
/// slot 从 `enter_data_driven` 预分配起永驻 ul 子树——离开可见区不 detach，只标 `parked`
/// （置 display:none 便签），保住 NodeId / parent / reuse_key，后端 GO 随之永驻不重建。
#[derive(Debug, Clone, Copy)]
pub struct Slot {
    pub node: NodeId,
    /// 当前绑的 item；parked 时保留上次值（stale，仅作复用参考）。
    pub item_index: usize,
    /// true = 休眠（display:none override 已置，不占布局、不渲染）。
    pub parked: bool,
}

/// ListView 运行时虚拟化状态。每 ul（NodeKind::ListView）一个槽。
///
/// - `slots`：已实例化的 slot（克隆出的 li），高水位——只增不减，永不 detach。
/// - `visible`：上帧算出的可见 item 区间 [start, end)。
/// - `pending_binds`：本帧新绑定的 (slot_node, item_index)，由 bind 阶段消费。
/// - `anchoring_active` / `dirty`：anchoring / 静默刷新标记（预留）。
#[derive(Debug, Clone)]
pub struct ListState {
    pub item_count: usize,
    /// 克隆模板的游离根（enter_data_driven 备份的设计期 li）。
    pub template_root: Option<NodeId>,
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
}

impl Default for ListState {
    fn default() -> Self {
        Self {
            item_count: 0,
            template_root: None,
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
