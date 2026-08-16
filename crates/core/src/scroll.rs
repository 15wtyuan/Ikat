//! ScrollPane 状态 + 物理。transient（不进 pkg）。
//!
//! 本模块持数据模型：
//! - `ScrollPaneState`：每滚动容器几何（content/viewport/overlap）+ 物理状态（pos/velocity/tween）。
//! - `ScrollTable`：per-node 槽（`Vec<Option<ScrollPaneState>>`，NodeId 索引），镜像 `AnimTable` 模式。
//! - `refresh_content_sizes(&mut Scene)`：layout solve 后填 content_size/viewport/overlap。
//! - `capable` / `effective` helper。
//!
//! core 无 Vec2 类型——几何用 `(f32, f32)` 元组（照 `transform::apply_point`）。

use crate::scene::node::{Node, NodeId, Rect, Scene};
use crate::style::resolved::OverflowMode;

/// 滚轮输入事件（FFI POD）。C# set_wheel_input 推一组；core apply_wheel_to_hit
/// 沿祖先找最近 effective 滚动容器 → apply_wheel。
/// 16B：x@0 + y@4 + delta_x@8 + delta_y@12（4×f32 紧凑 ABI 断言）。
/// （x,y)=指针 design 坐标（hit_test 用）；(delta_x,delta_y)=滚轮增量（apply_wheel 吃）。
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct WheelEvent {
    pub x: f32,
    pub y: f32,
    pub delta_x: f32,
    pub delta_y: f32,
}
const _: () = {
    assert!(std::mem::size_of::<WheelEvent>() == 16);
}; // ABI 断言

// ── 物理常量 ─────────────────────────────────────────────
// 滚动触发阈值（px）：鼠标/触摸移动超此才认拖拽。
// mouse 8 / touch 20。
pub const SCROLL_THRESHOLD_MOUSE: f32 = 8.0;
pub const SCROLL_THRESHOLD_TOUCH: f32 = 20.0;
/// 惯性减速系数（每 1/60s 速度衰减比）。
pub const DECELERATION_RATE: f64 = 0.967;
/// 速度指数平滑系数（drag_follow 写 velocity 用）。
pub const VELOCITY_SMOOTH: f32 = 10.0;
/// 惯性触发阈值（px/s 线性 |v|）：PC。判 `|v|*scale < thresh`。
pub const INERTIA_THRESH_PC: f32 = 500.0;
/// 惯性触发阈值（px/s 线性 |v|）：触摸。
pub const INERTIA_THRESH_TOUCH: f32 = 1000.0;
/// 惯性位移系数（change = v*dur*0.4）。
pub const INERTIA_DIST_COEFF: f32 = 0.4;
/// 默认补间时长（s）：set_pos/wheel/bounce。
pub const TWEEN_TIME_DEFAULT: f32 = 0.3;
/// 越界打折比（drag_follow 越界位移 × 0.5）。
pub const PULL_RATIO: f32 = 0.5;
/// 回弹触发阈值（越界 abs > 20 才回弹，否则 snap）。
pub const BOUNCE_THRESHOLD: f32 = 20.0;
/// 滚轮步进（每 delta 单位位移 design px）。对齐浏览器/OS 惯例：一格 ≈ 3 行文本
/// ≈ 100px @1920×1080 设计分辨率；旧值 25 在千像素级滚动页上体感极慢。
pub const SCROLL_STEP: f32 = 100.0;
/// scrollbar 轨道厚度（px）。
pub const SCROLLBAR_TRACK_THICKNESS: f32 = 8.0;
/// scrollbar thumb 最小尺寸（px，防 content 过长时 thumb 缩到不可见）。
pub const MIN_THUMB_SIZE: f32 = 20.0;

/// 合成 scrollbar thumb 的 sentinel node_id flag。
/// 合成 RenderNode 的 node_id = container_id.0 as u32 | flag（高位，真实 NodeId 小，复用稳定）。
pub const V_THUMB_FLAG: u32 = 0x4000_0000;
pub const H_THUMB_FLAG: u32 = 0x2000_0000;

/// cubic-out 缓动：(t-1)^3 + 1，t∈[0,1]。advance tween 用。
fn cubic_out(t: f32) -> f32 {
    let u = t - 1.0;
    u * u * u + 1.0
}

/// 单滚动容器状态。`#[derive(Default)]`：几何全 0、物理全 0/false、tweening=0（无）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ScrollPaneState {
    /// 直接子 layout_rect 的 AABB 尺寸。
    pub content_size: (f32, f32),
    /// 本容器 content box 尺寸（layout_rect border box；padding 简化）。
    pub viewport_size: (f32, f32),
    /// (content - viewport).max(0) 每轴；负钳 0。
    pub overlap: (f32, f32),
    /// 当前滚动位置（content 坐标系偏移）。
    pub scroll_pos: (f32, f32),
    /// 惯性速度（px/s）。advance 写。
    pub velocity: (f32, f32),
    /// 0=无补间，1=set_pos 补间，2=惯性+回弹补间。advance 写。
    /// [ax0, ax1]：每轴独立 tweening 状态，避免单轴 bounce 误标全容器。
    pub tweening: [u8; 2],
    pub tween_start: (f32, f32),
    pub tween_change: (f32, f32),
    pub tween_time: (f32, f32),
    pub tween_duration: (f32, f32),
    /// refresh 后若 content_size 变化置 true（供 scrollbar 复布局用）。
    pub content_size_dirty: bool,
    /// driver 注入 content_size 标记。true 时 refresh_content_sizes 跳过
    /// （不覆盖子节点 AABB）。set_content_size 置 true；clear_content_size_override 置 false。
    pub content_size_overridden: bool,
}

/// 每节点滚动状态表（`HashMap<NodeId, ScrollPaneState>`）。仅滚动容器 ensure 后有值。
/// transient——不进 pkg（同 `anim` / `world_transforms`）。
///
/// 用 `HashMap<NodeId, ScrollPaneState>` 而非 `Vec<Option<...>>`（按 id.index() 索引），同 AnimTable
/// （见 node.rs AnimTable doc：slotmap Key 是 unsafe trait + KeyData 64bit 与 NodeId 32bit 不匹配，
/// NodeId 不能直接当 SecondaryMap Key）。
#[derive(Debug, Clone, Default)]
pub struct ScrollTable(pub std::collections::HashMap<NodeId, ScrollPaneState>);

impl ScrollTable {
    pub fn get(&self, id: NodeId) -> Option<&ScrollPaneState> {
        self.0.get(&id)
    }
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut ScrollPaneState> {
        self.0.get_mut(&id)
    }
    /// 确保该节点有 scroll 槽并返回可变状态（缺则插 default）。
    pub fn ensure(&mut self, id: NodeId) -> &mut ScrollPaneState {
        self.0.entry(id).or_default()
    }
    /// 删该节点 scroll 槽（remove_node 联动调，防悬空 NodeId 残留）。
    pub fn remove(&mut self, id: NodeId) {
        self.0.remove(&id);
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// 物理方法。per-axis 用 ax 0/1 分支，自维护可变 target tween
/// （不走 GTween）。tweening[ax]：0=无，1=set_pos/wheel，2=inertia/bounce。
impl ScrollPaneState {
    /// 两轴 tweening 全零（无 tween）。
    pub fn tweening_idle(&self) -> bool {
        self.tweening[0] == 0 && self.tweening[1] == 0
    }
    /// 任一轴 tweening 非零。
    pub fn tweening_any(&self) -> bool {
        self.tweening[0] != 0 || self.tweening[1] != 0
    }

    /// 拖拽跟手：scroll_pos += delta（越界 PULL_RATIO 打折）+ 记速度（exp 平滑）。
    pub fn drag_follow(&mut self, delta: (f32, f32), dt: f32) {
        // 速度记录（指数平滑：v += (Δ/dt - v) * smooth）
        if dt > 0.0 {
            let smoothing = (dt * VELOCITY_SMOOTH).clamp(0.0, 1.0);
            self.velocity.0 += (delta.0 / dt - self.velocity.0) * smoothing;
            self.velocity.1 += (delta.1 / dt - self.velocity.1) * smoothing;
        }
        for ax in 0..2u8 {
            let cur = if ax == 0 {
                self.scroll_pos.0
            } else {
                self.scroll_pos.1
            };
            let d = if ax == 0 { delta.0 } else { delta.1 };
            let ov = if ax == 0 {
                self.overlap.0
            } else {
                self.overlap.1
            };
            if ov <= 0.0 {
                continue;
            } // 无 overlap 轴不动（防 overflow-y 容器斜拖 x 抖）
            let vp = if ax == 0 {
                self.viewport_size.0
            } else {
                self.viewport_size.1
            };
            let mut np = cur + d;
            let lo = 0.0f32;
            let hi = ov;
            if np < lo {
                // 越界单打折（min(位移*PULL_RATIO, vp*PULL_RATIO)）：最大越界 vp*PULL_RATIO。
                let dampened = ((lo - np) * PULL_RATIO).min(vp * PULL_RATIO);
                np = lo - dampened;
            } else if np > hi {
                let dampened = ((np - hi) * PULL_RATIO).min(vp * PULL_RATIO);
                np = hi + dampened;
            }
            if ax == 0 {
                self.scroll_pos.0 = np;
            } else {
                self.scroll_pos.1 = np;
            }
        }
        self.tweening = [0, 0]; // 拖拽中无 tween
    }

    /// Up 后松手物理（启 tween→tweening=2，否则 0）。is_touch 选阈值。
    /// 1. **越界**（start<0 或 >overlap）→ 直接 bounce 回边界（不论 velocity，不 inertia）。
    /// 2. **界内 + ratio>0** → inertia tween（target **不 clamp**，越界由 advance 运行时截断）。
    /// 3. **界内 + 低速（v2≤thresh）** → ratio=0，不设 tween（停）。
    ///
    /// v2 = |v|*scale（scale 默认 1）= 线性 |v|（"v2" 误导成 v²）；
    /// dur = |log(60/v2_eff)/log(DECELERATION_RATE)|/60。
    ///
    /// 手感对齐：① 越界松手 → bounce（不 snap）；② 二次 ratio `((v2-thresh)/thresh)²`
    /// 削弱低速；③ inertia target 不 clamp，advance 运行时 >20px 截断 + 回弹（弹性过冲回弹）。
    pub fn begin_inertia(&mut self, is_touch: bool) {
        let thresh = if is_touch {
            INERTIA_THRESH_TOUCH
        } else {
            INERTIA_THRESH_PC
        };
        self.tweening = [0, 0];
        for ax in 0..2u8 {
            let v = if ax == 0 {
                self.velocity.0
            } else {
                self.velocity.1
            };
            let ov = if ax == 0 {
                self.overlap.0
            } else {
                self.overlap.1
            };
            let start = if ax == 0 {
                self.scroll_pos.0
            } else {
                self.scroll_pos.1
            };
            // 分支 1：越界 → bounce 回边界（越界必平滑回弹，不 snap）
            let over_lo = start < 0.0;
            let over_hi = ov > 0.0 && start > ov;
            if over_lo || over_hi {
                let boundary = if over_lo { 0.0 } else { ov };
                let change = boundary - start;
                if ax == 0 {
                    self.tween_start.0 = start;
                    self.tween_change.0 = change;
                    self.tween_duration.0 = TWEEN_TIME_DEFAULT;
                    self.tween_time.0 = 0.0;
                } else {
                    self.tween_start.1 = start;
                    self.tween_change.1 = change;
                    self.tween_duration.1 = TWEEN_TIME_DEFAULT;
                    self.tween_time.1 = 0.0;
                }
                self.tweening[ax as usize] = 2;
                continue;
            }
            // 分支 3：界内低速或无 overlap → 停（ratio=0 不 inertia）
            let v2 = v.abs(); // v2 = |v|*scale = 线性 |v|（非 v²）
            if ov <= 0.0 || v2 <= thresh {
                continue;
            }
            // 二次 ratio 削弱低速：ratio = ((v2-thresh)/thresh)²，clamp ≤1；v2 与 v 同乘 ratio。
            let ratio = (((v2 - thresh) / thresh).powi(2)).min(1.0);
            let v2_eff = v2 * ratio;
            let v_eff = v * ratio;
            // 分支 2：界内 inertia。dur = |log(60/v2_eff)/log(DECEL)|/60。
            // change = v_eff*dur*0.4（经验公式）。**不 clamp target**——越界由 advance
            // 运行时截断（>20px 越界 → 截断 + 回弹 = 弹性过冲回弹）。
            let dur = ((60.0f64 / v2_eff as f64).log(DECELERATION_RATE).abs() / 60.0) as f32;
            let dur = dur.max(TWEEN_TIME_DEFAULT);
            let change = v_eff * dur * INERTIA_DIST_COEFF;
            if ax == 0 {
                self.tween_start.0 = start;
                self.tween_change.0 = change;
                self.tween_duration.0 = dur;
                self.tween_time.0 = 0.0;
            } else {
                self.tween_start.1 = start;
                self.tween_change.1 = change;
                self.tween_duration.1 = dur;
                self.tween_time.1 = 0.0;
            }
            self.tweening[ax as usize] = 2;
        }
    }

    /// 回弹 tween（越界 > BOUNCE_THRESHOLD 才启，否则 snap 由 advance done 处理）。
    /// 每轴独立设置 tweening[ax]=2，不误标另一轴。
    pub fn begin_bounce(&mut self) {
        for ax in 0..2u8 {
            let cur = if ax == 0 {
                self.scroll_pos.0
            } else {
                self.scroll_pos.1
            };
            let ov = if ax == 0 {
                self.overlap.0
            } else {
                self.overlap.1
            };
            // 界内或小越界 → 不回弹
            let boundary = if cur < 0.0 {
                0.0
            } else if cur > ov {
                ov
            } else {
                continue;
            };
            if (cur - boundary).abs() < BOUNCE_THRESHOLD {
                continue;
            }
            let start = cur;
            let change = boundary - cur;
            if ax == 0 {
                self.tween_start.0 = start;
                self.tween_change.0 = change;
                self.tween_duration.0 = TWEEN_TIME_DEFAULT;
                self.tween_time.0 = 0.0;
            } else {
                self.tween_start.1 = start;
                self.tween_change.1 = change;
                self.tween_duration.1 = TWEEN_TIME_DEFAULT;
                self.tween_time.1 = 0.0;
            }
            self.tweening[ax as usize] = 2;
        }
    }

    /// 推进 tween（tweening≠0）。
    /// 每帧 cubic_out 推进 pos；**tweening==2 时运行时检测越界**——pos 越界 >20px（或
    /// inertia 完成 change==0 时仍越界）即截断当前 tween，启回弹 tween（弹性过冲回弹）。
    /// 两轴 tween_change 都归零 → done（clamp[0,overlap] + tweening=0）。
    pub fn advance(&mut self, dt: f32) {
        if self.tweening[0] == 0 && self.tweening[1] == 0 {
            return;
        }
        for ax in 0..2u8 {
            let dur = if ax == 0 {
                self.tween_duration.0
            } else {
                self.tween_duration.1
            };
            if dur <= 0.0 {
                continue;
            }
            let change = if ax == 0 {
                self.tween_change.0
            } else {
                self.tween_change.1
            };
            if change == 0.0 {
                continue; // 该轴 tween 已完成（change 归零），待 done
            }
            let start = if ax == 0 {
                self.tween_start.0
            } else {
                self.tween_start.1
            };
            let ov = if ax == 0 {
                self.overlap.0
            } else {
                self.overlap.1
            };
            // 推进
            if ax == 0 {
                self.tween_time.0 += dt;
            } else {
                self.tween_time.1 += dt;
            }
            let t = if ax == 0 {
                self.tween_time.0
            } else {
                self.tween_time.1
            };
            let pos = if t >= dur {
                let p = start + change;
                if ax == 0 {
                    self.tween_change.0 = 0.0;
                } else {
                    self.tween_change.1 = 0.0;
                }
                p
            } else {
                start + change * cubic_out(t / dur)
            };
            if ax == 0 {
                self.scroll_pos.0 = pos;
            } else {
                self.scroll_pos.1 = pos;
            }
            // 运行时越界截断（仅 tweening==2）。
            // 越顶（pos<0）：inertia 往顶（cc<0）冲过 0 超 20px，或完成（cc==0）时仍越顶 → 回弹到 0。
            // 越底（pos>ov）：对称。→ 弹性过冲回弹（不冲远空白再突然 snap）。
            if self.tweening[ax as usize] == 2 {
                let cc = if ax == 0 {
                    self.tween_change.0
                } else {
                    self.tween_change.1
                };
                let bounce = if (pos < -BOUNCE_THRESHOLD && cc < 0.0) || (pos < 0.0 && cc == 0.0) {
                    Some((0.0_f32, 0.0 - pos))
                } else if ov > 0.0
                    && ((pos > ov + BOUNCE_THRESHOLD && cc > 0.0) || (pos > ov && cc == 0.0))
                {
                    Some((ov, ov - pos))
                } else {
                    None
                };
                if let Some((_boundary, new_change)) = bounce {
                    if ax == 0 {
                        self.tween_start.0 = pos;
                        self.tween_change.0 = new_change;
                        self.tween_duration.0 = TWEEN_TIME_DEFAULT;
                        self.tween_time.0 = 0.0;
                    } else {
                        self.tween_start.1 = pos;
                        self.tween_change.1 = new_change;
                        self.tween_duration.1 = TWEEN_TIME_DEFAULT;
                        self.tween_time.1 = 0.0;
                    }
                }
            }
        }
        // done：两轴 tween_change 都归零
        if self.tween_change.0 == 0.0 && self.tween_change.1 == 0.0 {
            self.scroll_pos.0 = self.scroll_pos.0.clamp(0.0, self.overlap.0);
            self.scroll_pos.1 = self.scroll_pos.1.clamp(0.0, self.overlap.1);
            self.tweening = [0, 0];
        }
    }

    /// 滚轮：target = (cur - delta*SCROLL_STEP).clamp[0,overlap]，启 tweening=1。
    /// delta.y > 0 = 上滚（看上方）→ scroll_pos.y 减少。
    /// 仅对 delta≠0 的轴设 tweening[ax]=1。
    pub fn apply_wheel(&mut self, delta: (f32, f32)) {
        for ax in 0..2u8 {
            let d = if ax == 0 { delta.0 } else { delta.1 };
            if d == 0.0 {
                continue;
            }
            let cur = if ax == 0 {
                self.scroll_pos.0
            } else {
                self.scroll_pos.1
            };
            let ov = if ax == 0 {
                self.overlap.0
            } else {
                self.overlap.1
            };
            let target = (cur - d * SCROLL_STEP).clamp(0.0, ov);
            let start = cur;
            if ax == 0 {
                self.tween_start.0 = start;
                self.tween_change.0 = target - start;
                self.tween_duration.0 = TWEEN_TIME_DEFAULT;
                self.tween_time.0 = 0.0;
            } else {
                self.tween_start.1 = start;
                self.tween_change.1 = target - start;
                self.tween_duration.1 = TWEEN_TIME_DEFAULT;
                self.tween_time.1 = 0.0;
            }
            self.tweening[ax as usize] = 1;
        }
    }

    /// 编程滚动。animated=false 直接 snap+clamp+tweening=0；true 启 tweening=1。
    pub fn set_pos(&mut self, target: (f32, f32), animated: bool) {
        if !animated {
            self.scroll_pos = (
                target.0.clamp(0.0, self.overlap.0),
                target.1.clamp(0.0, self.overlap.1),
            );
            self.tweening = [0, 0];
            return;
        }
        for ax in 0..2u8 {
            let t = if ax == 0 {
                target.0.clamp(0.0, self.overlap.0)
            } else {
                target.1.clamp(0.0, self.overlap.1)
            };
            let start = if ax == 0 {
                self.scroll_pos.0
            } else {
                self.scroll_pos.1
            };
            if ax == 0 {
                self.tween_start.0 = start;
                self.tween_change.0 = t - start;
                self.tween_duration.0 = TWEEN_TIME_DEFAULT;
                self.tween_time.0 = 0.0;
            } else {
                self.tween_start.1 = start;
                self.tween_change.1 = t - start;
                self.tween_duration.1 = TWEEN_TIME_DEFAULT;
                self.tween_time.1 = 0.0;
            }
            self.tweening[ax as usize] = 1;
        }
    }
}

/// 该轴是否允许滚动（overflow ∈ {Scroll, Auto}）。
pub fn capable(ovf: OverflowMode) -> bool {
    matches!(ovf, OverflowMode::Scroll | OverflowMode::Auto)
}

/// 该轴实际可滚（capable 且 (Scroll 或 content > viewport)）。
/// Auto 仅当内容溢出才可滚；Scroll 无论溢出与否皆可滚。
pub fn effective(ovf: OverflowMode, content: f32, viewport: f32) -> bool {
    capable(ovf) && (ovf == OverflowMode::Scroll || content > viewport)
}

/// 垂直 thumb design-rect（容器 viewport 右边缘 track；thumb 大小/位置）。
/// 返 None 若 overlap_y <= 0（无溢出、无需 thumb）。
pub fn v_thumb_rect(scene: &Scene, id: NodeId) -> Option<Rect> {
    let s = scene.scroll.get(id)?;
    if s.overlap.1 <= 0.0 {
        return None;
    }
    let lr = scene.get(id).expect("live node").layout_rect;
    let track_w = SCROLLBAR_TRACK_THICKNESS;
    let track_h = lr.h;
    let thumb_h = (s.viewport_size.1 * (s.viewport_size.1 / s.content_size.1))
        .max(MIN_THUMB_SIZE)
        .min(track_h);
    let perc = if s.overlap.1 > 0.0 {
        (s.scroll_pos.1 / s.overlap.1).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let thumb_y = lr.y + (track_h - thumb_h) * perc;
    Some(Rect {
        x: lr.x + lr.w - track_w,
        y: thumb_y,
        w: track_w,
        h: thumb_h,
    })
}

/// 水平 thumb design-rect（容器 viewport 底边 track；thumb 大小/位置）。
pub fn h_thumb_rect(scene: &Scene, id: NodeId) -> Option<Rect> {
    let s = scene.scroll.get(id)?;
    if s.overlap.0 <= 0.0 {
        return None;
    }
    let lr = scene.get(id).expect("live node").layout_rect;
    let track_h = SCROLLBAR_TRACK_THICKNESS;
    let track_w = lr.w;
    let thumb_w = (s.viewport_size.0 * (s.viewport_size.0 / s.content_size.0))
        .max(MIN_THUMB_SIZE)
        .min(track_w);
    let perc = if s.overlap.0 > 0.0 {
        (s.scroll_pos.0 / s.overlap.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let thumb_x = lr.x + (track_w - thumb_w) * perc;
    Some(Rect {
        x: thumb_x,
        y: lr.y + lr.h - track_h,
        w: thumb_w,
        h: track_h,
    })
}

/// solve 后填 content_size/viewport/overlap。
/// 遍历节点：任一轴 overflow != Visible 即视为滚动容器，ensure 后写几何。
/// children clone 避借用冲突（遍历子 layout_rect 时也要借 scene.nodes）。
pub fn refresh_content_sizes(scene: &mut Scene) {
    // 收集 (nid, kids, viewport) 避免在借 scene.scroll 时再借 scene.nodes。
    let mut work: Vec<(NodeId, Vec<NodeId>, (f32, f32))> = Vec::new();
    for n in scene.nodes.values() {
        if n.style.overflow_x != OverflowMode::Visible
            || n.style.overflow_y != OverflowMode::Visible
        {
            let kids = n.children.clone();
            let viewport = content_box_size(n);
            work.push((n.id, kids, viewport));
        }
    }
    for (nid, kids, viewport) in work {
        // driver 注入 content_size 的容器（虚拟列表）跳过自动算。
        // 只更新 viewport（容器尺寸可能变）+ 重算 overlap（用已注入的 content_size），
        // 不覆盖 content_size、不遍历子节点 AABB。
        let overridden = scene
            .scroll
            .get(nid)
            .map(|s| s.content_size_overridden)
            .unwrap_or(false);
        // anchoring 豁免须在 mutable borrow scene.scroll 前取（读 scene.lists + nodes）。
        let anchoring = scene_is_anchoring(scene, nid);
        if overridden {
            let st = scene.scroll.ensure(nid);
            st.viewport_size = viewport;
            let new_overlap = (
                (st.content_size.0 - viewport.0).max(0.0),
                (st.content_size.1 - viewport.1).max(0.0),
            );
            let old_overlap = st.overlap;
            st.overlap = new_overlap;
            // content_size 变化补偿（最小）：overridden 容器 content_size 未变，
            // 但 overlap 可能因 viewport 变化而缩小 → scroll_pos 越界需 clamp。
            // 仅 overlap 变时才 clamp（drag 拖出界的 rubber-banding 不 clamp）。
            // 越界时若正在跑 tween 则取消（快照到新边界）。
            if new_overlap != old_overlap {
                let out_of_range = st.scroll_pos.0 < 0.0
                    || st.scroll_pos.0 > new_overlap.0
                    || st.scroll_pos.1 < 0.0
                    || st.scroll_pos.1 > new_overlap.1;
                if out_of_range {
                    st.scroll_pos.0 = st.scroll_pos.0.clamp(0.0, new_overlap.0);
                    st.scroll_pos.1 = st.scroll_pos.1.clamp(0.0, new_overlap.1);
                    // anchoring 期不清 tweening（几何变化源于虚拟化回填，tween 应继续）。
                    if !anchoring {
                        st.tweening = [0, 0];
                    }
                }
            }
            continue;
        }
        // content_size = 直接子节点 layout_rect AABB。
        // 跳过零尺寸子节点（w=0 且 h=0）：HTML 元素间的纯空白 TextNode 被 layout
        // 的 is_whitespace_only_text 滤出 taffy 树，layout_rect 保持默认 (0,0,0,0)。
        // 若计入 AABB，其 (0,0) 原点会撑出假的 content 范围 → 水平滚动容器被误开垂直轴。
        let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
        let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
        let mut counted = 0u32;
        for c in &kids {
            let r = scene.get(*c).expect("live node").layout_rect;
            if r.w == 0.0 && r.h == 0.0 {
                continue;
            }
            min_x = min_x.min(r.x);
            min_y = min_y.min(r.y);
            max_x = max_x.max(r.x + r.w);
            max_y = max_y.max(r.y + r.h);
            counted += 1;
        }
        let content = if counted == 0 {
            (0.0, 0.0)
        } else {
            ((max_x - min_x).max(0.0), (max_y - min_y).max(0.0))
        };
        // anchoring 豁免须在 mutable borrow scene.scroll 前取（读 scene.lists + nodes）。
        let anchoring = scene_is_anchoring(scene, nid);
        let st = scene.scroll.ensure(nid);
        st.content_size_dirty = st.content_size != content;
        st.content_size = content;
        st.viewport_size = viewport;
        let new_overlap = (
            (content.0 - viewport.0).max(0.0),
            (content.1 - viewport.1).max(0.0),
        );
        let old_overlap = st.overlap;
        st.overlap = new_overlap;
        // content_size 变化补偿（最小）：geometry 变了，若 scroll_pos 跑出新
        // [0, overlap] 范围，直接 clamp 并取消正在跑的 tween。
        // 仅 overlap 变时才 clamp（drag 拖出界的 rubber-banding 不 clamp）。
        // 完整补偿（按比例缩短 tween_change/tween_duration）defer——简化为 snap。
        if new_overlap != old_overlap {
            let out_of_range = st.scroll_pos.0 < 0.0
                || st.scroll_pos.0 > new_overlap.0
                || st.scroll_pos.1 < 0.0
                || st.scroll_pos.1 > new_overlap.1;
            if out_of_range {
                st.scroll_pos.0 = st.scroll_pos.0.clamp(0.0, new_overlap.0);
                st.scroll_pos.1 = st.scroll_pos.1.clamp(0.0, new_overlap.1);
                // anchoring 期不清 tweening（几何变化源于虚拟化回填，tween 应继续）。
                if !anchoring {
                    st.tweening = [0, 0];
                }
            }
        }
    }
}

/// content box 尺寸（border box 扣 padding）。滚动容器的 viewport 须用 content box：
/// content_size 是子节点 AABB extent（不含容器 padding），viewport 也须扣 padding 才能让
/// overlap = content − viewport 正确反映可滚动范围。历史上返回 border box（含 padding），
/// 导致带 padding 的滚动容器（如 form .body padding:40 64）overlap 偏小、底部内容滚不到。
fn content_box_size(node: &Node) -> (f32, f32) {
    let lr = node.layout_rect;
    let ts = &node.style.taffy_style;
    let pl = crate::render::resolve_lp(ts.padding.left);
    let pr = crate::render::resolve_lp(ts.padding.right);
    let pt = crate::render::resolve_lp(ts.padding.top);
    let pb = crate::render::resolve_lp(ts.padding.bottom);
    ((lr.w - pl - pr).max(0.0), (lr.h - pt - pb).max(0.0))
}

/// 该 pane 是否正被某个 anchoring 活跃的 ListView 补偿（其祖先链含 pane）。
/// refresh_content_sizes 的 clamp 分支据此豁免清 tweening——虚拟化回填导致的
/// overlap 变化不是真实内容突变，正在跑的 tween（如 ScrollToItem Smooth）应继续。
/// 读 scene.lists + scene.nodes（与 scene.scroll 的 mutable borrow 不冲突：不同字段）。
fn scene_is_anchoring(scene: &Scene, pane: NodeId) -> bool {
    scene
        .lists
        .0
        .iter()
        .any(|(ul, ls)| ls.anchoring_active && ancestor_chain_contains(scene, *ul, pane))
}

/// `target` 是否在 `start` 的祖先链上（含 start 自身）。
fn ancestor_chain_contains(scene: &Scene, start: NodeId, target: NodeId) -> bool {
    let mut cur = Some(start);
    while let Some(id) = cur {
        if id == target {
            return true;
        }
        cur = scene.get(id).and_then(|n| n.parent);
    }
    false
}

/// hit(x,y) → 沿 node.parent 链找最近 effective 滚动容器 → apply_wheel。
/// 无祖先（或无 effective）→ 丢弃（return）。effective 判定用 scene.scroll.get 取
/// content/viewport（无 state 视 0.0，effective 对 Scroll overflow 仍 true）。
pub fn apply_wheel_to_hit(scene: &mut Scene, w: WheelEvent) {
    let mut pane = crate::hit::hit_test(scene, (w.x, w.y));
    while let Some(id) = pane {
        // sentinel thumb_id → decode container_id（thumb covers container edge,
        // wheel on thumb = wheel on container）
        let id = if id.0 & 0x6000_0000 != 0 {
            NodeId(id.0 & !0x6000_0000)
        } else {
            id
        };
        if let Some(n) = scene.get(id) {
            let eff_y = effective(
                n.style.overflow_y,
                scene.scroll.get(id).map_or(0.0, |s| s.content_size.1),
                scene.scroll.get(id).map_or(0.0, |s| s.viewport_size.1),
            );
            let eff_x = effective(
                n.style.overflow_x,
                scene.scroll.get(id).map_or(0.0, |s| s.content_size.0),
                scene.scroll.get(id).map_or(0.0, |s| s.viewport_size.0),
            );
            if eff_y || eff_x {
                if let Some(s) = scene.scroll.get_mut(id) {
                    s.apply_wheel((w.delta_x, w.delta_y));
                }
                return;
            }
        } else {
            // defensive: invalid node id (shouldn't happen after sentinel decode)
            break;
        }
        pane = scene.get(id).expect("live node").parent;
    }
}

/// tick 推进所有活跃 scroll tween（tweening[ax]≠0）。
/// 遍历 scene.scroll（HashMap values_mut），每个 st 若任一轴 tweening≠0 调 st.advance(dt)。
/// tweening 全零的拖拽中/静止容器不 advance。
pub fn advance_all(dt: f32, scene: &mut Scene) {
    for st in scene.scroll.0.values_mut() {
        if st.tweening[0] != 0 || st.tweening[1] != 0 {
            st.advance(dt);
        }
    }
}

#[cfg(test)]
mod tests;
