//! ListView 虚拟化内核：HeightCache + 可见区算法 + slot 池 + spacer 撑高 + anchoring。
//! side table 模式（照 scroll.rs / EditState），不塞进 Node。

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

/// 计算可见项区间 [start, end)（含 BUFFER）。viewport.h==0 → 冷启动返 INITIAL_SLOTS。
/// top = scroll_pos.y - listview_offset（ul 相对 pane 的偏移）。
pub fn compute_visible_range(
    item_count: usize,
    scroll_pos_y: f32,
    listview_offset: f32,
    viewport_h: f32,
    heights: &HeightCache,
) -> std::ops::Range<usize> {
    if item_count == 0 {
        return 0..0;
    }
    if viewport_h <= 0.0 {
        return 0..INITIAL_SLOTS.min(item_count);
    } // 冷启动
    let top = scroll_pos_y - listview_offset;
    // first = 首个顶边超过 top 的项（累积后判）。若全项顶边 ≤ top（内容短于视口），
    // 循环不 break，first 保持 0 → start 经 BUFFER 回退到 0，整列可见。
    let mut acc = 0.0;
    let mut first = 0usize;
    for i in 0..item_count {
        acc += heights.height_of(i);
        if acc > top {
            first = i;
            break;
        }
    }
    let target = top + viewport_h;
    let mut acc2 = 0.0;
    let mut last = item_count;
    for j in 0..item_count {
        acc2 += heights.height_of(j);
        if acc2 >= target {
            last = j + 1;
            break;
        }
    }
    let start = first.saturating_sub(BUFFER);
    let end = (last + BUFFER).min(item_count);
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_cache_sum_with_mixed_known_estimate() {
        let mut hc = HeightCache::new(3, 20.0);
        hc.set(0, 10.0);
        hc.set(2, 30.0);
        approx_eq(hc.sum(0..3), 60.0);
    }

    #[test]
    fn height_cache_estimate_updates_to_known_mean() {
        let mut hc = HeightCache::new(5, 40.0);
        hc.set(0, 10.0);
        hc.set(1, 30.0);
        approx_eq(hc.estimate, 20.0);
        approx_eq(hc.sum(0..5), 100.0);
    }

    #[test]
    fn height_cache_sum_empty_range_zero() {
        let hc = HeightCache::new(10, 50.0);
        approx_eq(hc.sum(5..5), 0.0);
    }

    #[test]
    fn visible_range_basic() {
        let r = compute_visible_range(100, 0.0, 0.0, 100.0, &uniform_heights(100, 10.0));
        assert_eq!(r, 0..12);
    }

    #[test]
    fn visible_range_scrolled_mid() {
        let r = compute_visible_range(100, 50.0, 0.0, 100.0, &uniform_heights(100, 10.0));
        assert_eq!(r, 3..17);
    }

    #[test]
    fn visible_range_clamps_to_count() {
        let r = compute_visible_range(5, 50.0, 0.0, 100.0, &uniform_heights(5, 10.0));
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 5);
    }

    #[test]
    fn visible_range_empty_count() {
        let r = compute_visible_range(0, 0.0, 0.0, 100.0, &HeightCache::new(0, 10.0));
        assert_eq!(r, 0..0);
    }

    #[test]
    fn visible_range_cold_start_viewport_zero() {
        let r = compute_visible_range(1000, 0.0, 0.0, 0.0, &uniform_heights(1000, 10.0));
        assert_eq!(r, 0..INITIAL_SLOTS);
    }

    fn uniform_heights(n: usize, h: f32) -> HeightCache {
        let mut hc = HeightCache::new(n, h);
        for i in 0..n {
            hc.set(i, h);
        }
        hc
    }

    fn approx_eq(a: f32, b: f32) {
        assert!((a - b).abs() < 0.01, "{a} != {b}");
    }
}
