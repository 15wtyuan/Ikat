//! 动画面：tween 注册/停止/通道清理、@keyframes player 的程序化播放与
//! 暂停/恢复/停止/seek/状态查询/OnKey 阈值（PlayerKey 以 u64 跨 FFI）。

use loomgui_core::scene::animation::{
    player_key_as_u64, player_key_from_u64, register_on_key, PlayerPlayState,
};
use loomgui_core::scene::NodeId;

use crate::{ffi_guard, StageHandle};

/// 注册 tween。start/end 指向 ≥value_size 个 f32（value_size 由 prop 隐含）。
/// null 句柄/null 指针 → no-op。越界 node / duration<=0 由 core update 处理（跳过/立即 complete）。
#[no_mangle]
pub extern "C" fn loomgui_stage_tween(
    h: *mut StageHandle,
    node_id: u64,
    prop: u32,
    start: *const f32,
    end: *const f32,
    duration: f32,
    ease: u32,
    delay: f32,
    tag: u32,
) {
    ffi_guard((), || {
        if h.is_null() || start.is_null() || end.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        let prop = match loomgui_core::tween::TweenProp::try_from(prop) {
            Some(p) => p,
            None => return,
        };
        let ease = match loomgui_core::tween::Ease::try_from(ease) {
            Some(e) => e,
            None => return,
        };
        let sz = loomgui_core::tween::prop_value_size(prop) as usize;
        let st = unsafe { std::slice::from_raw_parts(start, sz) };
        let en = unsafe { std::slice::from_raw_parts(end, sz) };
        let mut s = [0.0f32; 5];
        let mut e = [0.0f32; 5];
        s[..sz].copy_from_slice(st);
        e[..sz].copy_from_slice(en);
        sh.stage
            .tween(NodeId(node_id), prop, s, e, ease, delay, duration, tag);
    })
}

/// 停该节点该 prop 的 tween（override 保留末值）。
#[no_mangle]
pub extern "C" fn loomgui_stage_kill_tween(h: *mut StageHandle, node_id: u64, prop: u32) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        if let Some(prop) = loomgui_core::tween::TweenProp::try_from(prop) {
            sh.stage.kill_tween(NodeId(node_id), prop);
        }
    })
}

/// 清该节点所有动画 override（回 CSS）。
#[no_mangle]
pub extern "C" fn loomgui_stage_clear_anim(h: *mut StageHandle, node_id: u64) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        sh.stage.clear_anim(NodeId(node_id));
    })
}

/// 清该节点某 prop 对应通道（回 CSS）。
#[no_mangle]
pub extern "C" fn loomgui_stage_clear_anim_prop(h: *mut StageHandle, node_id: u64, prop: u32) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        if let Some(prop) = loomgui_core::tween::TweenProp::try_from(prop) {
            sh.stage.clear_anim_prop(NodeId(node_id), prop);
        }
    })
}

// C# `node.Play(name)` → Animation 句柄。PlayerKey 以 u64 跨 FFI
// （slotmap `KeyData::as_ffi`，player_key_as_u64/from_u64 转换，0 = 恒无效 key）。
// 句柄控制直接操作 scene.players（既有 `loomgui_node_parent` 同款 scene 直取模式，
// 控制语义在 core 的 update_all/PlayerPlayState 层，FFI 保持薄包装）。

/// 程序化启动 @keyframes 动画（spec §7.3 `play_animation`）。
/// node = 目标节点 NodeId；name = UTF-8 字节（指针+len）。返 PlayerKey u64；失败返 0
/// （null 句柄 / 非 UTF-8 / 无 scene / 节点无效 / keyframes 表无此 name）。
///
/// 建 **programmatic** player（sync_animation_players 完全跳过，不受 class 声明管）：
/// spec 默认 = 1s / 无 delay / 单次迭代 / normal / fill both / cubic-out
/// （C# `Play(name)` 无时长参数，默认由 core `play_programmatic` 定）。
/// 立即写首帧（spec §5.2：不等下帧 step b，防 delay 期闪 base）。
#[no_mangle]
pub extern "C" fn loomgui_stage_play_animation(
    h: *mut StageHandle,
    node: u64,
    name: *const u8,
    name_len: usize,
) -> u64 {
    ffi_guard(u64::MAX, || {
        const INVALID: u64 = 0;
        if h.is_null() {
            return INVALID;
        }
        let sh = unsafe { &mut *h };
        // null/零长兜底为空串（from_raw_parts(null, 0) 是 UB）：空 name 查表失败 → INVALID。
        let name = if name.is_null() || name_len == 0 {
            ""
        } else {
            match std::str::from_utf8(unsafe { std::slice::from_raw_parts(name, name_len) }) {
                Ok(s) => s,
                Err(_) => return INVALID,
            }
        };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return INVALID;
        };
        match loomgui_core::scene::animation::play_programmatic(scene, NodeId(node), name) {
            Some(k) => player_key_as_u64(k),
            None => INVALID,
        }
    })
}

/// 同 `loomgui_stage_play_animation`，显式指定时长（秒）。duration_s ≤ 0 / NaN 按 1s
/// 默认。C# `Play(name, durationSeconds)` 重载走此入口——无 `animation:` 声明绑定的
/// keyframes 无声明层时长，程序化播放节奏由调用方给。
#[no_mangle]
pub extern "C" fn loomgui_stage_play_animation_dur(
    h: *mut StageHandle,
    node: u64,
    name: *const u8,
    name_len: usize,
    duration_s: f32,
) -> u64 {
    ffi_guard(u64::MAX, || {
        const INVALID: u64 = 0;
        if h.is_null() {
            return INVALID;
        }
        let sh = unsafe { &mut *h };
        let name = if name.is_null() || name_len == 0 {
            ""
        } else {
            match std::str::from_utf8(unsafe { std::slice::from_raw_parts(name, name_len) }) {
                Ok(s) => s,
                Err(_) => return INVALID,
            }
        };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return INVALID;
        };
        match loomgui_core::scene::animation::play_programmatic_with_duration(
            scene,
            NodeId(node),
            name,
            duration_s,
        ) {
            Some(k) => player_key_as_u64(k),
            None => INVALID,
        }
    })
}

/// 暂停 player（Playing → Paused，elapsed 冻结位置保持）。key 无效 / 非 Playing → no-op。
#[no_mangle]
pub extern "C" fn loomgui_stage_pause_animation(h: *mut StageHandle, key: u64) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return;
        };
        if let Some(p) = scene.players.get_mut(player_key_from_u64(key)) {
            if p.play_state == PlayerPlayState::Playing {
                p.play_state = PlayerPlayState::Paused;
            }
        }
    })
}

/// 恢复播放（Paused → Playing）。key 无效 / 非 Paused → no-op
/// （Completed 是粘性完成态、Stopped 是终态，均不可恢复）。
#[no_mangle]
pub extern "C" fn loomgui_stage_resume_animation(h: *mut StageHandle, key: u64) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return;
        };
        if let Some(p) = scene.players.get_mut(player_key_from_u64(key)) {
            if p.play_state == PlayerPlayState::Paused {
                p.play_state = PlayerPlayState::Playing;
            }
        }
    })
}

/// 停止 player（scene 层**终态**，不可恢复，勿当暂停）。
/// 只标记 Stopped：下帧 update_all 清本 player 通道 + 从 players 表移除，PlayerKey 失效。
/// 此后 get_animation_state 恒 255（无效）。key 无效 → no-op。
#[no_mangle]
pub extern "C" fn loomgui_stage_stop_animation(h: *mut StageHandle, key: u64) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return;
        };
        if let Some(p) = scene.players.get_mut(player_key_from_u64(key)) {
            p.play_state = PlayerPlayState::Stopped;
        }
    })
}

/// 读 player 时间轴位置（elapsed——含 delay 计时的唯一时间源头，spec §5.3）。
/// key 无效 / 无 scene → 0.0（不 panic）。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_animation_time(h: *const StageHandle, key: u64) -> f32 {
    ffi_guard(f32::NAN, || {
        if h.is_null() {
            return 0.0;
        }
        let sh = unsafe { &*h };
        match &sh.stage.scene {
            Some(scene) => scene
                .players
                .get(player_key_from_u64(key))
                .map(|p| p.elapsed)
                .unwrap_or(0.0),
            None => 0.0,
        }
    })
}

/// seek：设 player.elapsed，下一帧 step b 按新位置采样（C# `Animation.Time` setter）。
/// 时间源头单一是 elapsed，不校验范围（负值 = 仍在 delay 阶段之前）。key 无效 → no-op。
#[no_mangle]
pub extern "C" fn loomgui_stage_set_animation_time(h: *mut StageHandle, key: u64, time: f32) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return;
        };
        if let Some(p) = scene.players.get_mut(player_key_from_u64(key)) {
            p.elapsed = time;
        }
    })
}

/// 读 player 运行状态。Playing=0 / Paused=1 / Completed=2；Invalid=255（key 不存在 /
/// 无 scene / Stopped——Stopped 是终态，下帧即回收，语义等同无效）。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_animation_state(h: *const StageHandle, key: u64) -> u8 {
    ffi_guard(u8::MAX, || {
        const INVALID: u8 = 255;
        if h.is_null() {
            return INVALID;
        }
        let sh = unsafe { &*h };
        match &sh.stage.scene {
            Some(scene) => match scene.players.get(player_key_from_u64(key)) {
                Some(p) => match p.play_state {
                    PlayerPlayState::Playing => 0,
                    PlayerPlayState::Paused => 1,
                    PlayerPlayState::Completed => 2,
                    PlayerPlayState::Stopped => INVALID,
                },
                None => INVALID,
            },
            None => INVALID,
        }
    })
}

/// 注册 OnKey 百分比阈值（spec §7.3 `animation_on_key`；C# `Animation.OnKey(pct, cb)` 走此 FFI，
/// 回调本身留 C# 按 playerKey 匹配触发）。pct 应 ∈ [0,1]（progress 域外永不触发，注册无害）。
/// 重复注册同 pct 去重（register_on_key）。key 无效 → no-op。
#[no_mangle]
pub extern "C" fn loomgui_stage_animation_on_key(h: *mut StageHandle, key: u64, pct: f32) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return;
        };
        register_on_key(scene, player_key_from_u64(key), pct);
    })
}
