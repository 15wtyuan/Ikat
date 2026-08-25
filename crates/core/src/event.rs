//! 动画事件：事件类型常量 + EventRecord 构造 +
//! 事件字符串表。
//!
//! 数据流：`update_all`（scene/animation.rs）advance 后检测阈值 → 本模块构造 EventRecord
//! 推入 `out` → stage tick 汇入 last_events → C# `borrow_events` demux。START/END/
//! ITERATION 走 node.EventBus 全局路由，KEY/HOOK 走 player_key 句柄私有路由。
//!
//! **payload 编码**（复用 EventRecord 现有字段，参考 EVT_TWEEN_COMPLETE 复用模式，不扩
//! struct、ABI 安全）：
//! - `node_id`：动画节点；
//! - `event_type`：本模块 5 常量之一；
//! - `click_count + pad[0..2]`：动画 **name** 的 [`EventStrTable`] 索引（24-bit 小端）。
//!   EventRecord 无字符串槽，name/hook_name 走字符串表，C# 侧按索引读回；
//! - `touch_id`：PlayerKey u64 低 32 位（slotmap `KeyData::as_ffi`，见
//!   [`player_key_as_u64`]）；
//! - `x`：PlayerKey u64 高 32 位（f32 bit pattern）；
//! - `y`：事件载荷——KEY=percent(f32) / ITERATION=迭代序号(f32 bits) /
//!   HOOK=hook_name 的字符串表索引(f32 bits) / START·END=0。

use crate::input::EventRecord;
use crate::scene::animation::{player_key_as_u64, PlayerKey};
use crate::scene::node::NodeId;

/// 动画启动（player 首次 update；class 触发 + node.Play 都发）。
/// 值对齐 C# `EventType.AnimationStart`（LoomGUI.EventType.cs，公共 API 冻结值）。
pub const EVT_ANIMATION_START: u8 = 18;
/// 迭代结束（每个 iteration 边界；完成帧不发——CSS：最后一次 iteration 结束只发 END，
/// animationiteration 不因最后一次 iteration 触发）。
/// 值对齐 C# `EventType.AnimationIteration`。
pub const EVT_ANIMATION_ITERATION: u8 = 19;
/// 动画完成（PlayerFrame.completed 转变帧一次；fill forwards/both 持续完成态不重发）。
/// 值对齐 C# `EventType.AnimationEnd`。
pub const EVT_ANIMATION_END: u8 = 20;
/// OnKey 百分比跨越（句柄私有，不广播 EventBus）。
pub const EVT_ANIMATION_KEY: u8 = 27;
/// @loom-hook stop 跨越（句柄私有）。
pub const EVT_ANIMATION_HOOK: u8 = 28;

/// 事件字符串表：动画事件 payload 的 name/hook_name 载体（EventRecord 20B 扁平 POD
/// 无字符串槽，事件携带 24-bit 索引，C# demux 按索引读回）。
///
/// 持久 intern：同名跨 tick 索引稳定（表只增不减，量级 = 动画名 + hook 名，内存可忽略）。
/// 运行时态，不进 pkg。
#[derive(Debug, Clone, Default)]
pub struct EventStrTable {
    /// intern 表序 = 索引序（索引即 Vec 下标）。
    strings: Vec<String>,
    index: std::collections::HashMap<String, u32>,
}

impl EventStrTable {
    /// intern：已存在返旧索引，否则追加新索引。索引须装进 24-bit 槽
    /// （click_count+pad），表项数远小于上限。
    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&i) = self.index.get(s) {
            return i;
        }
        let i = self.strings.len() as u32;
        self.strings.push(s.to_owned());
        self.index.insert(s.to_owned(), i);
        i
    }

    /// 索引 → 字符串（越界返 None，防御——正常路径索引恒由 intern 产生）。
    pub fn get(&self, idx: u32) -> Option<&str> {
        self.strings.get(idx as usize).map(String::as_str)
    }
}

/// 公共构造：node + player_key + name + 载荷 → EventRecord。
/// name 经字符串表 intern，索引 24-bit 装 click_count+pad（小端）。
fn animation_event(
    strs: &mut EventStrTable,
    node: NodeId,
    key: PlayerKey,
    ty: u8,
    name: &str,
    payload: f32,
) -> EventRecord {
    let name_idx = strs.intern(name);
    let ffi = player_key_as_u64(key);
    EventRecord {
        node_id: node.0,
        event_type: ty,
        click_count: (name_idx & 0xFF) as u8,
        pad: [
            ((name_idx >> 8) & 0xFF) as u8,
            ((name_idx >> 16) & 0xFF) as u8,
        ],
        touch_id: (ffi & 0xFFFF_FFFF) as u32 as i32,
        x: f32::from_bits((ffi >> 32) as u32),
        y: payload,
        dx: 0.0,
        dy: 0.0,
    }
}

/// EVT_ANIMATION_START 构造（payload 无）。
pub fn animation_start(
    strs: &mut EventStrTable,
    node: NodeId,
    key: PlayerKey,
    name: &str,
) -> EventRecord {
    animation_event(strs, node, key, EVT_ANIMATION_START, name, 0.0)
}

/// EVT_ANIMATION_END 构造（payload 无）。
pub fn animation_end(
    strs: &mut EventStrTable,
    node: NodeId,
    key: PlayerKey,
    name: &str,
) -> EventRecord {
    animation_event(strs, node, key, EVT_ANIMATION_END, name, 0.0)
}

/// EVT_ANIMATION_ITERATION 构造。payload = 刚结束的迭代序号（0-based，f32 bits）。
pub fn animation_iteration(
    strs: &mut EventStrTable,
    node: NodeId,
    key: PlayerKey,
    name: &str,
    iteration: u32,
) -> EventRecord {
    animation_event(
        strs,
        node,
        key,
        EVT_ANIMATION_ITERATION,
        name,
        f32::from_bits(iteration),
    )
}

/// EVT_ANIMATION_KEY 构造。payload = 跨越的百分比阈值（f32）。
pub fn animation_key(
    strs: &mut EventStrTable,
    node: NodeId,
    key: PlayerKey,
    name: &str,
    percent: f32,
) -> EventRecord {
    animation_event(strs, node, key, EVT_ANIMATION_KEY, name, percent)
}

/// EVT_ANIMATION_HOOK 构造。payload = hook_name 的字符串表索引（f32 bits）。
pub fn animation_hook(
    strs: &mut EventStrTable,
    node: NodeId,
    key: PlayerKey,
    name: &str,
    hook: &str,
) -> EventRecord {
    let hook_idx = strs.intern(hook);
    animation_event(
        strs,
        node,
        key,
        EVT_ANIMATION_HOOK,
        name,
        f32::from_bits(hook_idx),
    )
}
