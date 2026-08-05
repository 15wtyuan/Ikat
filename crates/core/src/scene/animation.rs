//! @keyframes 动画类型定义（pkg 序列化 + runtime 共用）。
//!
//! 数据流：fence 解析 `@keyframes`/`animation`（pack-time）→ 打包器转成这里的类型
//! → pkg.bin v30 序列化 → instantiate 时组件 keyframes 合并进 `Scene.keyframes` 全局表
//! （CSS `@keyframes` 全局语义，spec §3.5）→ KeyframePlayer 按 `AnimationSpec.name` 查表驱动。
//!
//! 类型划分：pkg 层（KeyframesRule 表）+ ResolvedStyle 层（AnimationSpec，在
//! `style/resolved.rs`，bincode 序列化）+ runtime 层（KeyframePlayer 时间轴推进，本文件）。

use serde::{Deserialize, Serialize};

use crate::scene::node::NodeId;
use crate::style::resolved::{AnimationDirection, AnimationFillMode, AnimationSpec};
use crate::tween::Ease;

/// `@keyframes` 一条 stop 的选择器位置。CSS 标准：`from`=`0%`，`to`=`100%`。
/// 带数据变体（Percent(u8)）与 `#[repr(u8)]` 不兼容，pkg 序列化时手动 match 写 u8
/// 判别值（0=From / 1=To / 2=Percent + payload）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyframeStopSelector {
    From,
    To,
    /// 0..=100（fence 子集只接受整数百分比，与 css_rules.rs 一致）。
    Percent(u8),
}

impl KeyframeStopSelector {
    /// stop 在时间轴上的位置（From=0.0 / To=1.0 / Percent(n)=n/100），player 插值定位用。
    pub fn percent(self) -> f32 {
        match self {
            KeyframeStopSelector::From => 0.0,
            KeyframeStopSelector::To => 1.0,
            KeyframeStopSelector::Percent(n) => f32::from(n) / 100.0,
        }
    }
}

/// `@keyframes` 内一条 stop：选择器位置 + 可动画属性值 + hook 锚点。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyframeStop {
    pub selector: KeyframeStopSelector,
    /// 该 stop 声明的可动画属性值（fence 解析声明块后提取，缺省字段 = None = 不参与插值）。
    pub props: AnimatableProps,
    /// `/* @loom-hook name */` 锚点：player 播放到该 stop 时发事件（spec §8.4）。None = 无锚点。
    pub hook: Option<String>,
}

/// `@keyframes <name> { ... }` 整体规则。stops 按 source 顺序保留（runtime 按 selector 插值）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyframesRule {
    pub name: String,
    pub stops: Vec<KeyframeStop>,
}

/// 单个 stop 声明的可动画属性（围栏动画子集，与 `NodeAnim` 通道一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct AnimatableProps {
    pub opacity: Option<f32>,
    pub transform: Option<TransformAnim>,
    pub bg_color: Option<[f32; 4]>,
    pub text_color: Option<[f32; 4]>,
}

/// transform 的 TRS 分解存储（围栏 transform 子集只有 translate/rotate/scale，1:1 无信息
/// 丢失，spec §3.6）。每帧分量级 lerp 合成矩阵，不做 CSS 矩阵插值。
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct TransformAnim {
    pub translate: Option<[f32; 2]>,
    pub scale: Option<[f32; 2]>,
    /// radians
    pub rotate: Option<f32>,
}

/// Scene 级活跃 player 的 slotmap key（u64 稳定句柄 → 未来 C# Animation 句柄）。
pub type PlayerKey = slotmap::DefaultKey;

/// player 运行状态（spec §5.4 生命周期）。`#[repr(u8)]` 保 FFI/序列化稳定，Default = Playing。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u8)]
pub enum PlayerPlayState {
    #[default]
    Playing = 0,
    Paused = 1,
    /// 完成（iteration 跑满 count）。fill forwards/both 持续写末值不回收；
    /// fill none/backwards 由消费方回收回退 base。
    Completed = 2,
    /// 显式 Stop：消费方回收。
    Stopped = 3,
}

/// 一帧推进结果（`KeyframePlayer::advance` 返回值，纯数据）。
/// 消费方（tick 集成 / 事件层）拿它写 NodeAnim、发 START/END/ITERATION 事件。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PlayerFrame {
    /// 本帧应取的属性 override（全 None = 无 override，回退 base）。
    pub props: AnimatableProps,
    /// 本帧到达完成态（iteration >= count）。fill forwards/both 持续为 true；
    /// fill none/backwards 消费方应收起 player。
    pub completed: bool,
    /// 本帧跨越迭代边界时，刚结束的迭代序号（0-based）。None = 未跨越。
    /// 大 dt 跨多迭代时报告最后一个完成的（事件层按需补发中间迭代）。
    pub iteration_boundary: Option<u32>,
    pub play_state: PlayerPlayState,
}

/// 一个进行中的 @keyframes player（Scene.players 条目）。时间轴推进见 `advance`。
///
/// 纯时间轴状态机：不持有场景引用、不写 NodeAnim、不发事件——`advance` 只推进
/// elapsed/iteration/last_progress 并返回 `PlayerFrame`，副作用由消费方（tick 集成、
/// 事件层）执行。**elapsed 是唯一时间源头**：iteration/current_time/progress 全从它推导
/// （spec §5.3 自审修正点，防 direction 用到旧 iteration）。
///
/// 字段可见性为 pub（spec §4.2 写 pub(crate)，但 §4.3 的 `Scene.players` 是 pub 字段，
/// clippy -D warnings 下更私有的类型会报 private_interfaces——取 §4.3 并放宽类型可见性；
/// 集成测试也需要构造/断言。玩家结构仍是 core 内部实现细节，C# 只见 PlayerKey 句柄）。
#[derive(Debug, Clone)]
pub struct KeyframePlayer {
    pub node: NodeId,
    pub spec: AnimationSpec,
    /// keyframes 值拷贝（表小，避免生命周期绑定；启动时从 Scene.keyframes 全局表复制）。
    pub keyframes: KeyframesRule,
    /// 累计时间（含 delay 计时），唯一时间源头。
    pub elapsed: f32,
    /// 上帧 directed progress（OnKey 跨越检测用，事件层消费）。
    pub last_progress: f32,
    /// 当前（0-based）迭代序号（每帧从 elapsed 推导后回写）。
    pub iteration: u32,
    pub play_state: PlayerPlayState,
    /// FFI 注册的 OnKey 百分比阈值（事件层检测用）。
    pub on_key_percents: Vec<f32>,
    /// 已触发的阈值（防同 iteration 重复触发）。
    pub fired_keys: Vec<f32>,
    pub fired_start: bool,
}

impl KeyframePlayer {
    /// 建新 player（elapsed 从 0 起，Playing）。
    pub fn new(node: NodeId, spec: AnimationSpec, keyframes: KeyframesRule) -> Self {
        Self {
            node,
            spec,
            keyframes,
            elapsed: 0.0,
            last_progress: 0.0,
            iteration: 0,
            play_state: PlayerPlayState::Playing,
            on_key_percents: Vec::new(),
            fired_keys: Vec::new(),
            fired_start: false,
        }
    }

    /// 推进时间轴一帧（spec §5.3 纯函数：不写 NodeAnim、不 emit 事件）。
    ///
    /// Paused/Stopped 跳过推进（elapsed 不变，返回当前时刻帧）。
    pub fn advance(&mut self, dt: f32) -> PlayerFrame {
        if matches!(
            self.play_state,
            PlayerPlayState::Paused | PlayerPlayState::Stopped
        ) {
            // 不推进 elapsed，返回当前时刻的帧（位置不变）。
            return self.compute_frame();
        }
        self.elapsed += dt;
        self.compute_frame()
    }

    /// 从 `self.elapsed` 推导本帧取值 + 状态（spec §5.3 step 3-8），回写
    /// iteration/last_progress/play_state。Paused 时也走这里（elapsed 不变 → 幂等）。
    fn compute_frame(&mut self) -> PlayerFrame {
        let was_completed = self.play_state == PlayerPlayState::Completed;

        // 3. delay 阶段：backwards/both fill 显首帧值，否则无 override。
        if self.elapsed < self.spec.delay {
            self.iteration = 0;
            self.last_progress = 0.0;
            let props = if matches!(
                self.spec.fill_mode,
                AnimationFillMode::Backwards | AnimationFillMode::Both
            ) {
                // 首帧值 = progress 0 处采样（无 0% stop 时保持首个 keyframe 值）。
                sample(&self.keyframes, 0.0, self.spec.timing_function)
            } else {
                AnimatableProps::default()
            };
            return PlayerFrame {
                props,
                completed: false,
                iteration_boundary: None,
                play_state: self.play_state,
            };
        }

        // 4. iteration / current_time / progress 全从 elapsed 推导（唯一时间源头）。
        let duration = self.spec.duration;
        let (iteration, progress) = if duration > 0.0 {
            let anim_time = self.elapsed - self.spec.delay;
            let iteration = (anim_time / duration) as u32;
            (iteration, (anim_time % duration) / duration)
        } else {
            // 0 时长：越过 delay 即到达终点。iteration 取 MAX 保证完成判定恒成立。
            (u32::MAX, 1.0)
        };
        let prev_iteration = self.iteration;
        self.iteration = iteration;

        // 5. 完成判定（sticky：fill forwards/both 的 Completed player 持续保持）。
        let completed =
            was_completed || matches!(self.spec.iteration_count, Some(n) if iteration >= n);

        // 迭代边界：本帧跨越到新迭代 → 报"刚结束的迭代序号"（0-based）。
        let iteration_boundary = if !was_completed && duration > 0.0 && iteration > prev_iteration {
            Some(iteration - 1)
        } else {
            None
        };

        // 6. direction 应用到 progress（用本帧当前 iteration，非旧值）。
        let directed = apply_direction(self.spec.direction, iteration, progress);

        // 7. 采样 + 完成态 fill。
        let (props, sample_progress) = if completed {
            if matches!(
                self.spec.fill_mode,
                AnimationFillMode::Forwards | AnimationFillMode::Both
            ) {
                // 末值按 direction：动画最后一轮迭代（count-1）结束点的 directed progress。
                let final_iter = self.spec.iteration_count.unwrap_or(0).saturating_sub(1);
                let end_p = apply_direction(self.spec.direction, final_iter, 1.0);
                (
                    sample(&self.keyframes, end_p, self.spec.timing_function),
                    end_p,
                )
            } else {
                // fill none/backwards：完成即回退 base（无 override）。
                (AnimatableProps::default(), directed)
            }
        } else {
            (
                sample(&self.keyframes, directed, self.spec.timing_function),
                directed,
            )
        };

        self.last_progress = sample_progress;
        if completed {
            self.play_state = PlayerPlayState::Completed;
        }

        PlayerFrame {
            props,
            completed,
            iteration_boundary,
            play_state: self.play_state,
        }
    }
}

/// direction 应用到 [0,1) progress。iteration 是当前（0-based）迭代序号：
/// alternate 偶正奇反、alternate-reverse 相反（spec §5.3 step 6）。
fn apply_direction(direction: AnimationDirection, iteration: u32, progress: f32) -> f32 {
    let reverse = match direction {
        AnimationDirection::Normal => false,
        AnimationDirection::Reverse => true,
        AnimationDirection::Alternate => !iteration.is_multiple_of(2),
        AnimationDirection::AlternateReverse => iteration.is_multiple_of(2),
    };
    if reverse {
        1.0 - progress
    } else {
        progress
    }
}

/// 在 keyframes stops 间定位 progress 落点并插值（spec §5.3 step 7）。
///
/// 语义：
/// - stops 按 selector percent 排序，同 percent 合并（后者胜，CSS 同位置 keyframe 后者优先；
///   fence 的 `from, 0%` 逗号多 stop 会展开出同位 stop）；
/// - p 落在段外（无 0%/100% stop）→ 保持最近端 stop 原始值（CSS：from/to 取首个/末个 keyframe）；
///   恰等于某 stop 位置也取原始值（不经 ease，避免 Step 边界跳变）；
/// - 段内 per-property lerp：opacity/颜色双端 Some 才插值（单端保持）；transform TRS
///   各分量 lerp，缺分量用 identity（translate 0 / scale [1,1] / rotate 0）；
/// - ease 应用于段内 local_t（整体 AnimationSpec.timing_function，per-stop ease 推 M2.5）。
fn sample(keyframes: &KeyframesRule, progress: f32, ease: Ease) -> AnimatableProps {
    if keyframes.stops.is_empty() {
        return AnimatableProps::default();
    }
    let mut sorted: Vec<&KeyframeStop> = keyframes.stops.iter().collect();
    sorted.sort_by(|a, b| {
        a.selector
            .percent()
            .partial_cmp(&b.selector.percent())
            .expect("stop percent 恒为有限值")
    });
    // 同 percent 去重：保留 source 顺序靠后的（pop+push = 后者覆盖前者）。
    let mut stops: Vec<&KeyframeStop> = Vec::with_capacity(sorted.len());
    for s in sorted {
        let same = stops
            .last()
            .is_some_and(|u| u.selector.percent() == s.selector.percent());
        if same {
            stops.pop();
        }
        stops.push(s);
    }

    let first = stops[0];
    if progress <= first.selector.percent() {
        return first.props;
    }
    let last = stops[stops.len() - 1];
    if progress >= last.selector.percent() {
        return last.props;
    }
    for pair in stops.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (pa, pb) = (a.selector.percent(), b.selector.percent());
        if pb > pa && pa <= progress && progress <= pb {
            let local_t = (progress - pa) / (pb - pa);
            let t = ease.evaluate(local_t, 1.0);
            return lerp_props(a.props, b.props, t);
        }
    }
    // 全 stop 同 percent（退化段）→ 去重后仅剩一个，直接取它。
    last.props
}

/// 段内 per-property 插值。opacity/颜色：双端 Some 才插值，单端保持（None = 该通道无
/// override）；transform：TRS 分量级 lerp，单端缺失分量用 identity（translate 0 /
/// scale [1,1] / rotate 0，spec §3.6）。
fn lerp_props(from: AnimatableProps, to: AnimatableProps, t: f32) -> AnimatableProps {
    AnimatableProps {
        opacity: lerp_opt_hold(from.opacity, to.opacity, t),
        bg_color: lerp_opt4_hold(from.bg_color, to.bg_color, t),
        text_color: lerp_opt4_hold(from.text_color, to.text_color, t),
        transform: match (from.transform, to.transform) {
            (Some(a), Some(b)) => normalize_transform(lerp_transform(a, b, t)),
            (Some(v), None) => normalize_transform(lerp_transform(v, TransformAnim::default(), t)),
            (None, Some(v)) => normalize_transform(lerp_transform(TransformAnim::default(), v, t)),
            (None, None) => None,
        },
    }
}

/// TRS 分量级 lerp。缺分量用 identity（translate [0,0] / scale [1,1] / rotate 0）。
fn lerp_transform(a: TransformAnim, b: TransformAnim, t: f32) -> TransformAnim {
    TransformAnim {
        translate: lerp_opt2(a.translate, b.translate, t, [0.0, 0.0]),
        scale: lerp_opt2(a.scale, b.scale, t, [1.0, 1.0]),
        rotate: lerp_opt(a.rotate, b.rotate, t, 0.0),
    }
}

/// 全分量 None 的 TransformAnim 无任何 override 通道 → 归一为 None。
fn normalize_transform(t: TransformAnim) -> Option<TransformAnim> {
    if t.translate.is_none() && t.scale.is_none() && t.rotate.is_none() {
        None
    } else {
        Some(t)
    }
}

/// Option 标量：双端 Some → 数值插值；单端 → 用 identity 补缺端后插值（transform 分量语义）。
/// 参数顺序与 lerp_opt2 一致：(a, b, t, identity)。
fn lerp_opt(a: Option<f32>, b: Option<f32>, t: f32, identity: f32) -> Option<f32> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x + (y - x) * t),
        (Some(x), None) => Some(x + (identity - x) * t),
        (None, Some(y)) => Some(identity + (y - identity) * t),
        (None, None) => None,
    }
}

/// 同上，[f32;2] 版本（identity 由调用方给：translate [0,0] / scale [1,1]）。
fn lerp_opt2(
    a: Option<[f32; 2]>,
    b: Option<[f32; 2]>,
    t: f32,
    identity: [f32; 2],
) -> Option<[f32; 2]> {
    match (a, b) {
        (Some(x), Some(y)) => Some(lerp_arr2(x, y, t)),
        (Some(x), None) => Some(lerp_arr2(x, identity, t)),
        (None, Some(y)) => Some(lerp_arr2(identity, y, t)),
        (None, None) => None,
    }
}

/// Option 数值（opacity / 颜色通道）：双端 Some → lerp；单端 → 保持（CSS 稀疏 keyframes：
/// 未声明属性沿用最近声明值；player 无 base style 可查，不能向 base 插值）。
fn lerp_opt_hold(a: Option<f32>, b: Option<f32>, t: f32) -> Option<f32> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x + (y - x) * t),
        _ => a.or(b),
    }
}

/// 同上，[f32;4] 版本（bg_color / text_color 逐通道 lerp）。
fn lerp_opt4_hold(a: Option<[f32; 4]>, b: Option<[f32; 4]>, t: f32) -> Option<[f32; 4]> {
    match (a, b) {
        (Some(x), Some(y)) => Some(lerp_arr4(x, y, t)),
        _ => a.or(b),
    }
}

fn lerp_arr2(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn lerp_arr4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}
