//! @keyframes 动画类型定义（pkg 序列化 + runtime 共用）。
//!
//! 数据流：fence 解析 `@keyframes`/`animation`（pack-time）→ 打包器转成这里的类型
//! → pkg.bin v30 序列化 → instantiate 时组件 keyframes 合并进 `Scene.keyframes` 全局表
//! （CSS `@keyframes` 全局语义）→ KeyframePlayer 按 `AnimationSpec.name` 查表驱动。
//!
//! 类型划分：pkg 层（KeyframesRule 表）+ ResolvedStyle 层（AnimationSpec，在
//! `style/resolved.rs`，bincode 序列化）+ runtime 层（KeyframePlayer 时间轴推进，本文件）。

use serde::{Deserialize, Serialize};

use crate::input::EventRecord;
use crate::scene::node::{AnimTable, NodeId, Scene};
use crate::style::resolved::{
    AnimationDirection, AnimationFillMode, AnimationPlayState, AnimationSpec,
};
use crate::transform::{self, Affine2, Affine2Ext, LenPct};
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
    /// 该 stop 的 `animation-timing-function`（CSS per-keyframe timing：作用于本 stop
    /// 到下一 stop 的区段）。None = 用 spec 级 timing。
    pub timing: Option<Ease>,
    /// `/* @ikat-hook name */` 锚点：player 播放到该 stop 时发事件。None = 无锚点。
    pub hook: Option<String>,
}

/// `@keyframes <name> { ... }` 整体规则。stops 按 source 顺序保留（runtime 按 selector 插值）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyframesRule {
    pub name: String,
    pub stops: Vec<KeyframeStop>,
}

/// 可动画长度值的声明域（#10 layout 动画端点值域）。`value` 存 CSS 原始数值
/// （`25%`/`50vw` 的 25/50），域内线性插值、域间不可插（fence 拒，运行时兜底离散）；
/// px/pct 写回 taffy Dimension 原生形，vw/vh/vmin/vmax 在 solve sync 期按当帧
/// root_size 换算（动画中途 resize 自动重解析，保持比例正确）。
///
/// 判别值与 FFI/C# `LenDomain` 镜像对齐；bincode 进 pkg（variant index = 声明序），
/// 只从末尾追加。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum LenDomain {
    Px = 0,
    Pct = 1,
    Vw = 2,
    Vh = 3,
    Vmin = 4,
    Vmax = 5,
}

impl LenDomain {
    /// u32 → LenDomain（FFI 域码校验用）。越界 → None。
    pub fn try_from_code(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Px),
            1 => Some(Self::Pct),
            2 => Some(Self::Vw),
            3 => Some(Self::Vh),
            4 => Some(Self::Vmin),
            5 => Some(Self::Vmax),
            _ => None,
        }
    }
}

/// 带域的可动画长度（width/height 动画端点与逐帧 override 值）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AnimLen {
    pub domain: LenDomain,
    pub value: f32,
}

/// 单个 stop 声明的可动画属性（围栏动画子集，与 `NodeAnim` 通道一一对应）。
/// box-shadow 是变长列表（≤12 层）→ 本结构不可 Copy（Clone 即可；采样/写帧每帧
/// 至多一次 clone，仅动画中的节点付费）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AnimatableProps {
    pub opacity: Option<f32>,
    pub transform: Option<TransformAnim>,
    pub bg_color: Option<[f32; 4]>,
    pub text_color: Option<[f32; 4]>,
    pub width: Option<AnimLen>,
    pub height: Option<AnimLen>,
    pub flex_grow: Option<f32>,
    pub box_shadow: Option<Vec<crate::style::resolved::BoxShadow>>,
}

/// transform 的 TRS 分解存储（围栏 transform 子集只有 translate/rotate/scale，1:1 无信息
/// 丢失）。每帧分量级 lerp 合成矩阵，不做 CSS 矩阵插值。translate 分量是 `LenPct`
/// 混合长度（`translateX(-50%)` 相对自身尺寸，采样后写入期解析，#77）。
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct TransformAnim {
    pub translate: Option<[LenPct; 2]>,
    pub scale: Option<[f32; 2]>,
    /// radians
    pub rotate: Option<f32>,
}

/// Scene 级活跃 player 的 slotmap key（u64 稳定句柄 → 未来 C# Animation 句柄）。
pub type PlayerKey = slotmap::DefaultKey;

/// player 运行状态。`#[repr(u8)]` 保 FFI/序列化稳定，Default = Playing。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u8)]
pub enum PlayerPlayState {
    #[default]
    Playing = 0,
    Paused = 1,
    /// 完成（iteration 跑满 count）。fill forwards/both 持续写末值不回收；
    /// fill none/backwards 由消费方回收回退 base。
    Completed = 2,
    /// 显式 Stop：scene 层**终态**——update_all 下帧清通道并回收 player，
    /// PlayerKey 失效，不可恢复（勿当可恢复暂停）。
    Stopped = 3,
}

/// 一帧推进结果（`KeyframePlayer::advance` 返回值，纯数据）。
/// 消费方（tick 集成 / 事件层）拿它写 NodeAnim、发 START/END/ITERATION 事件。
/// Clone 非 Copy：AnimatableProps 含 box-shadow 变长列表。
#[derive(Debug, Clone, PartialEq, Default)]
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
/// （防 direction 用到旧 iteration）。
///
/// 字段可见性为 pub：`Scene.players` 是 pub 字段，clippy -D warnings 下更私有的类型会报
/// private_interfaces；集成测试也需要构造/断言。玩家结构仍是 core 内部实现细节，C# 只见
/// PlayerKey 句柄。
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
    /// 程序化 player（node.Play 建）：sync_animation_players 完全跳过
    /// （不受 class 声明管——声明消失不回收、同名不视为已启动），靠 Stop/句柄回收。
    /// class 声明触发的 player = false。
    pub programmatic: bool,
    /// FFI 注册的 OnKey 百分比阈值（事件层检测用）。
    pub on_key_percents: Vec<f32>,
    /// 已触发的阈值（防同 iteration 重复触发）。
    pub fired_keys: Vec<f32>,
    /// 已触发的 hook stop 百分比（防同 iteration 重复；与 fired_keys 分离——
    /// OnKey 与同百分点 hook 可各自独立触发）。
    pub fired_hooks: Vec<f32>,
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
            programmatic: false,
            on_key_percents: Vec::new(),
            fired_keys: Vec::new(),
            fired_hooks: Vec::new(),
            fired_start: false,
        }
    }

    /// 推进时间轴一帧（纯函数：不写 NodeAnim、不 emit 事件）。
    ///
    /// Paused 跳过推进（elapsed 不变，返回当前时刻帧，幂等）。Stopped 是 scene 层**终态**：
    /// 本函数仅不推进（幂等），消费方 update_all 下帧清通道 + 回收 player——不可恢复，
    /// 勿当"可恢复暂停"。
    pub fn advance(&mut self, dt: f32) -> PlayerFrame {
        if matches!(
            self.play_state,
            PlayerPlayState::Paused | PlayerPlayState::Stopped
        ) {
            return self.compute_frame();
        }
        self.elapsed += dt;
        self.compute_frame()
    }

    /// 从 `self.elapsed` 推导本帧取值 + 状态，回写 iteration/last_progress/play_state。
    /// Paused 时也走这里（elapsed 不变 → 幂等）。
    fn compute_frame(&mut self) -> PlayerFrame {
        let was_completed = self.play_state == PlayerPlayState::Completed;

        // delay 阶段：backwards/both fill 显首帧值，否则无 override。
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

        // 完成判定（sticky：fill forwards/both 的 Completed player 持续保持）。
        let completed =
            was_completed || matches!(self.spec.iteration_count, Some(n) if iteration >= n);

        // 迭代边界：本帧跨越到新迭代 → 报"刚结束的迭代序号"（0-based）。
        let iteration_boundary = if !was_completed && duration > 0.0 && iteration > prev_iteration {
            Some(iteration - 1)
        } else {
            None
        };

        // direction 应用到 progress（用本帧当前 iteration，非旧值）。
        let directed = apply_direction(self.spec.direction, iteration, progress);

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
/// alternate 偶正奇反、alternate-reverse 相反。
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

/// 在 keyframes stops 间定位 progress 落点并插值。
///
/// 语义：
/// - stops 按 selector percent 排序，同 percent 合并（后者胜，CSS 同位置 keyframe 后者优先；
///   fence 的 `from, 0%` 逗号多 stop 会展开出同位 stop）；
/// - p 落在段外（无 0%/100% stop）→ 保持最近端 stop 原始值（CSS：from/to 取首个/末个 keyframe）；
///   恰等于某 stop 位置也取原始值（不经 ease，避免 Step 边界跳变）；
/// - 段内 per-property lerp：opacity/颜色双端 Some 才插值（单端保持）；transform TRS
///   各分量 lerp（translate 是 LenPct 双域各自 lerp），缺分量用 identity
///   （translate 0 / scale [1,1] / rotate 0）；
/// - ease 取**段起始 stop** 的 per-stop timing（CSS per-keyframe animation-timing-function
///   作用于该 keyframe 到下一 keyframe 的区段），None 回落 spec 级 timing。
fn sample(keyframes: &KeyframesRule, progress: f32, default_ease: Ease) -> AnimatableProps {
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
        return first.props.clone();
    }
    let last = stops[stops.len() - 1];
    if progress >= last.selector.percent() {
        return last.props.clone();
    }
    for pair in stops.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (pa, pb) = (a.selector.percent(), b.selector.percent());
        if pb > pa && pa <= progress && progress <= pb {
            let local_t = (progress - pa) / (pb - pa);
            let ease = a.timing.unwrap_or(default_ease);
            let t = ease.evaluate(local_t, 1.0);
            return lerp_props(&a.props, &b.props, t);
        }
    }
    // 全 stop 同 percent（退化段）→ 去重后仅剩一个，直接取它。
    last.props.clone()
}

/// 段内 per-property 插值。opacity/颜色：双端 Some 才插值，单端保持（None = 该通道无
/// override）；transform：TRS 分量级 lerp，单端缺失分量用 identity（translate 0 /
/// scale [1,1] / rotate 0）；width/height：同域数值插值（异域为 fence 拒的兜底：
/// t<0.5 取 from、否则取 to 的离散跳变，与 box-shadow inset 不配对同一离散语义）；
/// box-shadow：短列表补透明零长空阴影逐对插值，任一配对 inset 不匹配 → 整表离散
/// （css-backgrounds-3 插值规则）。
fn lerp_props(from: &AnimatableProps, to: &AnimatableProps, t: f32) -> AnimatableProps {
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
        width: lerp_len_hold(from.width, to.width, t),
        height: lerp_len_hold(from.height, to.height, t),
        flex_grow: lerp_opt_hold(from.flex_grow, to.flex_grow, t),
        box_shadow: match (&from.box_shadow, &to.box_shadow) {
            (Some(a), Some(b)) => Some(crate::tween::lerp_shadow_list(a, b, t)),
            // 单端保持（稀疏 keyframes：未声明通道沿用，同颜色语义）。
            (a, b) => a.clone().or_else(|| b.clone()),
        },
    }
}

/// 带域长度插值：双端 Some 且同域 → 域内数值 lerp（vw/vh/vmin/vmax 各自域内等价，
/// 解析推迟到 solve sync）；异域/单端 → 保持语义（异域是 fence 拒后的运行时兜底，
/// 离散跳变而非静默混合出无意义数值）。
fn lerp_len_hold(a: Option<AnimLen>, b: Option<AnimLen>, t: f32) -> Option<AnimLen> {
    match (a, b) {
        (Some(x), Some(y)) if x.domain == y.domain => Some(AnimLen {
            domain: x.domain,
            value: x.value + (y.value - x.value) * t,
        }),
        (Some(x), Some(y)) => Some(if t < 0.5 { x } else { y }),
        (a, b) => a.or(b),
    }
}

/// TRS 分量级 lerp。缺分量用 identity（translate 0 / scale [1,1] / rotate 0）。
/// translate 是 LenPct：px/pct 两域各自线性 lerp（CSS calc 混合插值），解析在合成端。
fn lerp_transform(a: TransformAnim, b: TransformAnim, t: f32) -> TransformAnim {
    TransformAnim {
        translate: lerp_opt_len2(a.translate, b.translate, t),
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

/// LenPct 双分量插值（identity = LenPct::ZERO：px/pct 都归零；scale 的 [1,1] 对应物）。
fn lerp_opt_len2(a: Option<[LenPct; 2]>, b: Option<[LenPct; 2]>, t: f32) -> Option<[LenPct; 2]> {
    let lerp1 = |x: LenPct, y: LenPct| LenPct {
        px: x.px + (y.px - x.px) * t,
        pct: x.pct + (y.pct - x.pct) * t,
    };
    match (a, b) {
        (Some(x), Some(y)) => Some([lerp1(x[0], y[0]), lerp1(x[1], y[1])]),
        (Some(x), None) => Some([lerp1(x[0], LenPct::ZERO), lerp1(x[1], LenPct::ZERO)]),
        (None, Some(y)) => Some([lerp1(LenPct::ZERO, y[0]), lerp1(LenPct::ZERO, y[1])]),
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

/// PlayerKey → u64（slotmap `KeyData::as_ffi`：`(version << 32) | idx`）。
/// 事件 payload 编码（拆 2×u32 装 touch_id/x）+ FFI 的 C# ulong 句柄共用。
pub fn player_key_as_u64(key: PlayerKey) -> u64 {
    slotmap::Key::data(&key).as_ffi()
}

/// u64 → PlayerKey（`from_ffi` 逆变换；任意值安全但 unspecified，消费方用
/// `Scene.players.get` 校验——非法 key 查不到即 no-op）。
pub fn player_key_from_u64(v: u64) -> PlayerKey {
    PlayerKey::from(slotmap::KeyData::from_ffi(v))
}

/// FFI 注册 OnKey 百分比阈值（`animation_on_key`）：push 进
/// `player.on_key_percents`，update_all 每帧检测跨越。同 pct 重复注册去重。
pub fn register_on_key(scene: &mut Scene, key: PlayerKey, pct: f32) {
    if let Some(p) = scene.players.get_mut(key) {
        if !p.on_key_percents.contains(&pct) {
            p.on_key_percents.push(pct);
        }
    }
}

/// 程序化启动 @keyframes player（`node.Play` FFI 用）。
///
/// 查 `Scene.keyframes` 全局表（CSS `@keyframes` 全局语义）建 **programmatic** player
/// （`sync_animation_players` 完全跳过：声明消失不回收、同名不视为已启动；回收靠
/// Stop/句柄，或下次 Play 的入口接管——同名与同通道旧 player 先回收，见函数体注释）。
/// C# `Play(name)` 无时长参数 → 默认：**1s / 无 delay / 单次迭代 / normal / fill both /
/// ease（CSS 默认，精确 bezier(0.25,0.1,0.25,1)；#9 前用 cubic-out 近似）**。
/// fill both = 播放结束停终态；改默认须同步 C# 侧测试。
///
/// 立即算首帧写 NodeAnim（不等下一帧 update_all，防 delay 期闪 base）。
/// 返 PlayerKey；name 未找到 / 节点悬空 → None（调用方转 FFI 无效 key）。
pub fn play_programmatic(scene: &mut Scene, node: NodeId, name: &str) -> Option<PlayerKey> {
    play_programmatic_with_duration(scene, node, name, 1.0)
}

/// 同 [`play_programmatic`]，显式指定时长（秒，覆盖 1s 默认）。C# `Play(name, duration)`
/// 走此入口——无 `animation:` 声明绑定的 keyframes 没有声明层时长可读，程序化播放的
/// 节奏由调用方给。duration ≤ 0 按 1s 默认处理（防手滑传 0 瞬完成）。
pub fn play_programmatic_with_duration(
    scene: &mut Scene,
    node: NodeId,
    name: &str,
    duration: f32,
) -> Option<PlayerKey> {
    if !scene.nodes.contains_key(node.to_key()) {
        return None;
    }
    let rule = scene.keyframes.get(name)?.clone();
    // 同节点上会被新动画取代的 programmatic player 先回收：同名者（重复 Play = 确定性
    // 从头重播，CSS 重新触发动画同义）+ 持有重叠通道者（不限状态、不限名字）。Completed
    // +fill-both 的旧 player 每帧续写末值且 sync 侧永不回收，Playing 中的旧 player 同样
    // 每帧写帧值——不回收则新旧叠加写同通道，最终值取决于 slotmap 槽序（旧 player 槽位
    // 靠后就静默压掉新动画的每一帧）。通道不相交的 player（如 transform 动画 + opacity
    // 动画）保留，支持并行组合。
    let new_channels = stops_channels(&rule.stops);
    let stale: Vec<PlayerKey> = scene
        .players
        .iter()
        .filter(|(_, p)| {
            p.programmatic
                && p.node == node
                && (p.spec.name == name
                    || owned_channels(p)
                        .iter()
                        .zip(new_channels.iter())
                        .any(|(a, b)| *a && *b))
        })
        .map(|(k, _)| k)
        .collect();
    for k in stale {
        if let Some(p) = scene.players.remove(k) {
            remove_player_clearing_channels(scene, p);
        }
    }
    let spec = AnimationSpec {
        name: name.to_string(),
        duration: if duration.is_finite() && duration > 0.0 {
            duration
        } else {
            1.0
        },
        delay: 0.0,
        iteration_count: Some(1),
        direction: AnimationDirection::Normal,
        fill_mode: AnimationFillMode::Both,
        timing_function: Ease::CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.25,
            y2: 1.0,
        },
        play_state: AnimationPlayState::Running,
    };
    let mut player = KeyframePlayer::new(node, spec.clone(), rule);
    player.programmatic = true;
    let first = player.advance(0.0);
    let key = scene.players.insert(player);
    if matches!(
        spec.fill_mode,
        AnimationFillMode::Backwards | AnimationFillMode::Both
    ) {
        let size = node_size(scene, node);
        write_frame(&mut scene.anim, node, first.props, size);
    }
    Some(key)
}

/// 节点布局尺寸（write_frame 解析 LenPct 百分比用）。悬空节点不可达
/// （调用方已校验 live），防御性返 0×0（百分比分量归零，px 分量照常）。
fn node_size(scene: &Scene, node: NodeId) -> [f32; 2] {
    scene
        .get(node)
        .map(|n| [n.layout_rect.w, n.layout_rect.h])
        .unwrap_or([0.0, 0.0])
}

/// 阈值跨越判定：prev→cur 单调扫过 pct（双向——reverse/alternate 反向迭代也触发）。
fn crossed(prev: f32, cur: f32, pct: f32) -> bool {
    (prev < pct && cur >= pct) || (prev > pct && cur <= pct)
}

/// 每 tick 推进所有活跃 player 并写 NodeAnim。
///
/// 在 `TweenManager.update` **之后**调用——写入顺序即优先级：animation 同通道覆盖
/// transition。
///
/// 状态处理：
/// - Playing/Paused：写本帧帧值（Paused elapsed 不推进，位置保持）；
/// - Completed + fill forwards/both：每帧续写末值，**不回收**（player 是"动画已结束"的
///   标记，sync_animation_players 靠它防止声明仍存在时重启）；
/// - Completed + fill none/backwards：**完成转变帧一次性**清掉本 player 自己持有的通道
///   （NodeAnim 回 None → 下帧 tween/base 接管），其后 player 惰性保留在 Completed 态
///   （不再写也不再清，回收由 sync/Stop 负责）。完成清带掩码（own ∩ ¬同节点其他活跃
///   player 持有）——多 animation 共享通道时保留他人本帧已写的值，防一帧闪 base；
/// - Stopped（显式 Stop）：清通道 + 从 players 表移除；
/// - 悬空 NodeId（节点已删）：同 Stopped 移除（与 tween.rs 同款双保险）。
///
/// 事件（advance 后按 PlayerFrame + player 状态判定，emit 进 `out`）：
/// - START：首帧 advance 一次（`fired_start` 防重）；
/// - ITERATION：iteration 边界跨越（`PlayerFrame.iteration_boundary`，报刚结束的 0-based
///   迭代序号）。完成帧不发 ITERATION——CSS：最后一次 iteration 结束只发 END
///   （animationiteration 不因最后一次 iteration 触发）；非完成边界跨越才发。
/// - END：完成转变帧一次（`frame.completed && !was_completed`；fill 续写帧不重发）；
/// - KEY：`on_key_percents` 跨越（`fired_keys` 防同 iteration 重；iteration 边界清表，
///   下一 iteration 重新可发）；
/// - HOOK：keyframes stop 的 `hook` 锚点跨越（`fired_hooks` 同 KEY 语义）。0% (From)
///   hook 不触发——crossing 语义下起点即 0.0 恒无跨越，与 100% 不对称，已知取舍。
///
/// KEY/HOOK 跨越基准 prev 的选取：首帧/出 delay 帧从 t=0 的 directed progress 起算
/// （reverse/alternate-reverse 起点是 1.0，用 last_progress 初始 0.0 会首帧误发）；
/// 迭代边界帧从新 iteration 起点（`apply_direction(direction, iteration, 0)`）起算
/// （progress 回绕不误判为跨越）；其余帧用上帧 sampled progress。delay 期 progress
/// 冻结（elapsed < delay），不发 KEY/HOOK。
pub fn update_all(scene: &mut Scene, dt: f32, out: &mut Vec<EventRecord>) {
    if scene.players.is_empty() {
        return;
    }
    // 完成清通道的掩码要查"同节点其他 player 的推进后状态"（谁本帧完成、谁仍活跃），
    // 而推进必须按槽序交错写/清（写入顺序即优先级）——拆两阶段：
    // 1) 按槽序推进全部 player，原位写帧、原位清 Stopped/悬空通道，记录 fill-none 完成转变；
    // 2) 全部推进后，按其他 player 的最终状态算清掩码（与 sync 回收侧同款，见 dynamic.rs）；
    // 3) 回收 Stopped/悬空。全程不 drain/重插 slotmap——PlayerKey 稳定（C# 侧持句柄）。
    let keys: Vec<PlayerKey> = scene.players.keys().collect();
    let mut remove_keys: Vec<PlayerKey> = Vec::new();
    // 本帧 fill-none 完成转变的 player：(node, 自有通道掩码)。
    // （不需要 key：清掩码按"同节点其他活跃 player 的推进后状态"算，状态足以排除自己——
    //   完成者已是 Completed-fill-none，`holds_channels` 为 false。）
    let mut completions: Vec<(NodeId, ChannelMask)> = Vec::new();
    for k in keys {
        let Some(p) = scene.players.get_mut(k) else {
            continue; // 防御：key 集合快照内理论不可达
        };
        if p.play_state == PlayerPlayState::Stopped || !scene.nodes.contains_key(p.node.to_key()) {
            clear_owned_channels(&mut scene.anim, p);
            remove_keys.push(k);
            continue;
        }
        // 完成转变检测（一次性）：本帧前非 Completed、本帧到达完成态。
        let was_completed = p.play_state == PlayerPlayState::Completed;
        // 事件判定快照（advance 覆写 last_progress/iteration/play_state）。
        let prev_sample = p.last_progress;
        let was_in_delay = p.elapsed < p.spec.delay;
        let frame = p.advance(dt);

        let first_advance = !p.fired_start;
        p.fired_start = true;
        let completion = frame.completed && !was_completed;
        if first_advance {
            out.push(crate::event::animation_start(
                &mut scene.event_strs,
                p.node,
                k,
                &p.spec.name,
            ));
        }
        if let Some(i) = frame.iteration_boundary {
            // CSS：animationiteration 不因最后一次 iteration 触发——完成帧只发 END，
            // 非完成 iteration 边界跨越才发 ITERATION（count=1 完成帧同理只发 END）。
            if !completion {
                out.push(crate::event::animation_iteration(
                    &mut scene.event_strs,
                    p.node,
                    k,
                    &p.spec.name,
                    i,
                ));
            }
        }
        // KEY/HOOK 跨越检测（delay 期 progress 冻结，不发）。
        if p.elapsed >= p.spec.delay {
            let seed = if first_advance || was_in_delay {
                apply_direction(p.spec.direction, 0, 0.0)
            } else if frame.iteration_boundary.is_some() {
                apply_direction(p.spec.direction, p.iteration, 0.0)
            } else {
                prev_sample
            };
            let cur = p.last_progress;
            if frame.iteration_boundary.is_some() {
                p.fired_keys.clear();
                p.fired_hooks.clear();
            }
            for &pct in &p.on_key_percents {
                if crossed(seed, cur, pct) && !p.fired_keys.contains(&pct) {
                    p.fired_keys.push(pct);
                    out.push(crate::event::animation_key(
                        &mut scene.event_strs,
                        p.node,
                        k,
                        &p.spec.name,
                        pct,
                    ));
                }
            }
            for stop in &p.keyframes.stops {
                if let Some(hook) = &stop.hook {
                    let hp = stop.selector.percent();
                    if crossed(seed, cur, hp) && !p.fired_hooks.contains(&hp) {
                        p.fired_hooks.push(hp);
                        out.push(crate::event::animation_hook(
                            &mut scene.event_strs,
                            p.node,
                            k,
                            &p.spec.name,
                            hook,
                        ));
                    }
                }
            }
        }
        if completion {
            // END 在 KEY/HOOK 之后：完成帧上阈值跨越发生在完成之前的进度段，时序更接近真实。
            out.push(crate::event::animation_end(
                &mut scene.event_strs,
                p.node,
                k,
                &p.spec.name,
            ));
        }
        if frame.completed
            && !was_completed
            && !matches!(
                p.spec.fill_mode,
                AnimationFillMode::Forwards | AnimationFillMode::Both
            )
        {
            // fill none/backwards 完成：帧 props 已全 None，清掉本 player 持有的通道，
            // 下帧起 tween/base 接管。player 保留 Completed 态（防 sync 重启）。
            // 清动作推迟到全部推进后（掩码须按他人推进后状态算）。
            completions.push((p.node, owned_channels(p)));
        } else if !frame.completed
            || matches!(
                p.spec.fill_mode,
                AnimationFillMode::Forwards | AnimationFillMode::Both
            )
        {
            // nodes 与 players 是不相交字段，p 的可变借用下可直取 nodes（避整 scene 不可变借）。
            let size = scene
                .nodes
                .get(p.node.to_key())
                .map(|n| [n.layout_rect.w, n.layout_rect.h])
                .unwrap_or([0.0, 0.0]);
            write_frame(&mut scene.anim, p.node, frame.props, size);
        }
        // 其余（Completed + fill none 的后续 tick）：惰性，不写不清。
    }
    // 完成清通道（掩码镜像 sync 回收侧 dynamic.rs）：others = 同节点其他"活跃"player
    // （播放中 / Paused / Completed+forwards 每帧写值者，`holds_channels`）的持有并集。
    // 只清"本 player 持有且无活跃他人持有"的通道：共享通道保留他人本帧已写的值（不闪
    // base）；同帧全部完成时（他人已变 Completed-fill-none 惰性）通道回 None（base 接管）。
    for (node, own) in completions {
        let mut others = [false; 8];
        for q in scene.players.values() {
            if q.node == node && holds_channels(q) {
                others = mask_or(others, owned_channels(q));
            }
        }
        let mut clear = [false; 8];
        for i in 0..8 {
            clear[i] = own[i] && !others[i];
        }
        clear_channels(&mut scene.anim, node, clear);
    }
    // 回收（Stopped / 悬空）：通道已在槽序原位清过（与写入交错，语义同 retain）。
    for k in remove_keys {
        scene.players.remove(k);
    }
}

/// 该 player 是否"活跃持有"通道（完成清通道掩码的"其他活跃 player"判定）：播放中 /
/// Paused 每帧写帧值；Completed + fill forwards/both 每帧续写末值——这些 player 的通道
/// 不可清（清了丢值/闪 base）。Completed + fill none/backwards 是惰性结束标记（不写不清，
/// 通道由 base 接管）→ 不算持有；Stopped 本帧即回收 → 不算持有。
fn holds_channels(p: &KeyframePlayer) -> bool {
    match p.play_state {
        PlayerPlayState::Playing | PlayerPlayState::Paused => true,
        PlayerPlayState::Completed => matches!(
            p.spec.fill_mode,
            AnimationFillMode::Forwards | AnimationFillMode::Both
        ),
        PlayerPlayState::Stopped => false,
    }
}

/// 按帧值写 NodeAnim 全通道。通道 None = 本帧无 override（不动该通道）。
/// `node_size = [w, h]`：transform translate 的 LenPct 百分比在此解析（#77——
/// 采样端保持描述符，尺寸解析统一在写入期）。width/height 保持 AnimLen 描述符
/// （px/pct/vw 的解析统一推迟到 solve sync 覆写期，见 layout/mod.rs）。
/// `pub(crate)`：sync_animation_players 启动时立即写首帧（backwards fill）用。
pub(crate) fn write_frame(
    anim: &mut AnimTable,
    node: NodeId,
    props: AnimatableProps,
    node_size: [f32; 2],
) {
    let a = anim.ensure(node);
    if let Some(v) = props.opacity {
        a.opacity = Some(v);
    }
    if let Some(m) = props
        .transform
        .and_then(|ta| compose_transform(ta, node_size[0], node_size[1]))
    {
        a.transform = Some(m);
    }
    if let Some(v) = props.bg_color {
        a.bg_color = Some(v);
    }
    if let Some(v) = props.text_color {
        a.text_color = Some(v);
    }
    if let Some(v) = props.width {
        a.width = Some(v);
    }
    if let Some(v) = props.height {
        a.height = Some(v);
    }
    if let Some(v) = props.flex_grow {
        a.flex_grow = Some(v);
    }
    if let Some(v) = props.box_shadow {
        a.box_shadow = Some(v);
    }
}

/// TransformAnim TRS → Affine2（SRT：点先 scale 再 rotate 再 translate，缩放旋转绕自身
/// 原点，图形学标准）。缺分量用 identity（translate 0 / scale [1,1] / rotate 0）。
/// translate LenPct 按 `w`/`h` 解析（x 用 w、y 用 h——CSS 百分比 translate 相对自身盒）。
/// 全 None → None（不 override base transform）。
fn compose_transform(ta: TransformAnim, w: f32, h: f32) -> Option<Affine2> {
    if ta.translate.is_none() && ta.scale.is_none() && ta.rotate.is_none() {
        return None;
    }
    let t = ta.translate.unwrap_or([LenPct::ZERO, LenPct::ZERO]);
    let s = ta.scale.unwrap_or([1.0, 1.0]);
    let r = ta.rotate.unwrap_or(0.0);
    Some(
        transform::from_translate(t[0].resolve(w), t[1].resolve(h))
            .mul(transform::from_rotate(r))
            .mul(transform::from_scale(s[0], s[1])),
    )
}

/// 通道掩码（player 持有/回收清通道的按位集）。顺序固定：
/// [opacity, transform, bg_color, text_color, width, height, flex_grow, box_shadow]。
pub type ChannelMask = [bool; 8];

fn mask_or(acc: ChannelMask, m: ChannelMask) -> ChannelMask {
    let mut out = acc;
    for (o, b) in out.iter_mut().zip(m) {
        *o |= b;
    }
    out
}

/// 该 player 的 keyframes 声明的通道掩码（stops props 的 Some 通道并集）。
/// `pub(crate)`：sync_animation_players（dynamic.rs）回收 player 时算"谁还持有该通道"用。
pub(crate) fn owned_channels(p: &KeyframePlayer) -> ChannelMask {
    stops_channels(&p.keyframes.stops)
}

/// stops 声明的通道掩码（props 的 Some 通道并集）。顺序见 [`ChannelMask`]。
fn stops_channels(stops: &[KeyframeStop]) -> ChannelMask {
    let mut mask = [false; 8];
    for stop in stops {
        let props = &stop.props;
        for (i, held) in [
            props.opacity.is_some(),
            props.transform.is_some(),
            props.bg_color.is_some(),
            props.text_color.is_some(),
            props.width.is_some(),
            props.height.is_some(),
            props.flex_grow.is_some(),
            props.box_shadow.is_some(),
        ]
        .into_iter()
        .enumerate()
        {
            mask[i] |= held;
        }
    }
    mask
}

/// 重启子树内全部声明式（class 触发）动画：programmatic player（node.Play 句柄持有）
/// 不受影响。实现 = 按通道回收语义移除既有 player——下一帧 `sync_animation_players`
/// 依 base_style.animation 声明原样重建（backwards/both 立即写首帧，delay 重新计时）。
/// 与「销毁重实例化」的差别：节点身份、滚动位置、控件值、事件订阅全保留。
pub fn restart_animations(scene: &mut Scene, root: NodeId) {
    // 收集子树节点集（含 root 自身）。
    let mut subtree = std::collections::HashSet::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if !subtree.insert(id) {
            continue;
        }
        if let Some(n) = scene.nodes.get(id.to_key()) {
            stack.extend(n.children.iter().copied());
        }
    }
    // 移除子树内全部非 programmatic player（通道清理由 survivors 再持有的通道保留）。
    let remove: Vec<_> = scene
        .players
        .iter()
        .filter(|(_, p)| !p.programmatic && subtree.contains(&p.node))
        .map(|(k, _)| k)
        .collect();
    for k in remove {
        let Some(p) = scene.players.remove(k) else {
            continue;
        };
        remove_player_clearing_channels(scene, p);
    }
}

/// 按通道掩码清 NodeAnim（None = 回退 tween/base）。全 false 掩码 no-op；清空后整条
/// anim 条目移除。`pub(crate)`：sync_animation_players 回收 player 时按"持有 ∩ 无剩余
/// 持有"掩码调用（多 player 共享通道时只清真正没人写的）。
pub(crate) fn clear_channels(anim: &mut AnimTable, node: NodeId, mask: ChannelMask) {
    if !mask.iter().any(|&b| b) {
        return;
    }
    if let Some(a) = anim.0.get_mut(&node) {
        if mask[0] {
            a.opacity = None;
        }
        if mask[1] {
            a.transform = None;
        }
        if mask[2] {
            a.bg_color = None;
        }
        if mask[3] {
            a.text_color = None;
        }
        if mask[4] {
            a.width = None;
        }
        if mask[5] {
            a.height = None;
        }
        if mask[6] {
            a.flex_grow = None;
        }
        if mask[7] {
            a.box_shadow = None;
        }
        if a.is_empty() {
            anim.0.remove(&node);
        }
    }
}

/// 清该 player 的 keyframes 声明的通道（stops props 的 Some 通道并集）。只动自己持有的
/// 通道——动画没声明的通道（tween/base 在写）不动。
fn clear_owned_channels(anim: &mut AnimTable, p: &KeyframePlayer) {
    clear_channels(anim, p.node, owned_channels(p));
}

/// 从 players 表移除一个已摘出的 player，并按「持有 ∩ 无幸存者持有」掩码清其通道：
/// 同节点仍在表中的 player 写着的通道不清（防丢值/闪 base）。调用方须先 `players.remove`
/// 摘出该 player（掩码计算不得把被移除者自己算作幸存者）。
fn remove_player_clearing_channels(scene: &mut Scene, p: KeyframePlayer) {
    let own = owned_channels(&p);
    let remaining = scene
        .players
        .values()
        .filter(|q| q.node == p.node)
        .fold([false; 8], |acc, q| mask_or(acc, owned_channels(q)));
    let mut clear = [false; 8];
    for i in 0..8 {
        clear[i] = own[i] && !remaining[i];
    }
    clear_channels(&mut scene.anim, p.node, clear);
}

#[cfg(test)]
mod restart_tests {
    use super::*;
    use crate::scene::node::{Node, NodeKind, Scene};
    use crate::style::resolved::{
        AnimationDirection, AnimationFillMode, AnimationPlayState, AnimationSpec,
    };

    fn spec() -> AnimationSpec {
        AnimationSpec {
            name: "fade".into(),
            duration: 0.5,
            delay: 0.0,
            iteration_count: Some(1),
            direction: AnimationDirection::Normal,
            fill_mode: AnimationFillMode::Both,
            timing_function: crate::tween::Ease::Linear,
            play_state: AnimationPlayState::Running,
        }
    }

    fn rule() -> KeyframesRule {
        KeyframesRule {
            name: "fade".into(),
            stops: vec![
                KeyframeStop {
                    selector: KeyframeStopSelector::From,
                    props: AnimatableProps {
                        opacity: Some(0.0),
                        ..Default::default()
                    },
                    timing: None,
                    hook: None,
                },
                KeyframeStop {
                    selector: KeyframeStopSelector::To,
                    props: AnimatableProps {
                        opacity: Some(1.0),
                        ..Default::default()
                    },
                    timing: None,
                    hook: None,
                },
            ],
        }
    }

    fn scene_with_animated_nodes(n: usize) -> (Scene, Vec<crate::scene::node::NodeId>) {
        let mut nodes: Vec<Node> = Vec::new();
        for _ in 0..n {
            let mut node = Node::default();
            node.kind = NodeKind::Container;
            node.style.animation = vec![spec()];
            nodes.push(node);
        }
        let scene = Scene::from_nodes(nodes, vec![]);
        let ids = scene.roots.clone();
        (scene, ids)
    }

    /// #77：百分比 translate 采样 → write_frame 按节点布局尺寸解析成矩阵。
    /// from translateX(0) to translateX(-50%)，w=200：半程 = -25% = -50px。
    #[test]
    fn percent_translate_resolves_against_node_size_at_write() {
        use crate::transform::LenPct;
        let (mut scene, ids) = scene_with_animated_nodes(1);
        let node = ids[0];
        scene.get_mut(node).unwrap().layout_rect.w = 200.0;
        scene.get_mut(node).unwrap().layout_rect.h = 10.0;
        // 声明名对齐 keyframes（helper 的 spec 叫 "fade"）+ 时长 1s（半程 0.25s）。
        let mut slide_spec = spec();
        slide_spec.name = "slide".into();
        slide_spec.duration = 1.0;
        scene.get_mut(node).unwrap().style.animation = vec![slide_spec];
        scene.keyframes.insert(
            "slide".into(),
            KeyframesRule {
                name: "slide".into(),
                stops: vec![
                    KeyframeStop {
                        selector: KeyframeStopSelector::From,
                        props: AnimatableProps {
                            transform: Some(TransformAnim {
                                translate: Some([LenPct::ZERO, LenPct::ZERO]),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                        timing: None,
                        hook: None,
                    },
                    KeyframeStop {
                        selector: KeyframeStopSelector::To,
                        props: AnimatableProps {
                            transform: Some(TransformAnim {
                                translate: Some([
                                    LenPct {
                                        px: 0.0,
                                        pct: -50.0,
                                    },
                                    LenPct::ZERO,
                                ]),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                        timing: None,
                        hook: None,
                    },
                ],
            },
        );
        crate::style::dynamic::sync_animation_players(&mut scene);
        crate::scene::animation::update_all(&mut scene, 0.25, &mut Vec::new());
        let m = scene
            .anim
            .get(node)
            .and_then(|a| a.transform)
            .expect("transform override written");
        assert!(
            (m[4] - (-25.0)).abs() < 1e-4,
            "p=0.25 → -50%×0.25 = -12.5% × w200 = -25px，got tx={}",
            m[4]
        );
    }

    /// per-stop timing（#9）：段起始 stop 的 animation-timing-function 作用于本段。
    /// 0%→50% linear、50%→100% step-end：p=0.75 落第二段（step-end 保持段首值 0.5 位移）。
    #[test]
    fn per_stop_timing_applies_to_segment() {
        let kf = KeyframesRule {
            name: "mix".into(),
            stops: vec![
                KeyframeStop {
                    selector: KeyframeStopSelector::From,
                    props: AnimatableProps {
                        opacity: Some(0.0),
                        ..Default::default()
                    },
                    timing: Some(Ease::Linear),
                    hook: None,
                },
                KeyframeStop {
                    selector: KeyframeStopSelector::Percent(50),
                    props: AnimatableProps {
                        opacity: Some(0.5),
                        ..Default::default()
                    },
                    timing: Some(Ease::Step { start: false }),
                    hook: None,
                },
                KeyframeStop {
                    selector: KeyframeStopSelector::To,
                    props: AnimatableProps {
                        opacity: Some(1.0),
                        ..Default::default()
                    },
                    timing: None,
                    hook: None,
                },
            ],
        };
        // p=0.25：第一段（linear）local=0.5 → opacity 0.25。
        let mid1 = sample(&kf, 0.25, Ease::Linear);
        assert!((mid1.opacity.unwrap() - 0.25).abs() < 1e-5);
        // p=0.75：第二段 step-end（local=0.5，未到段末）→ 保持段首 0.5。
        let mid2 = sample(&kf, 0.75, Ease::Linear);
        assert!(
            (mid2.opacity.unwrap() - 0.5).abs() < 1e-5,
            "step-end 段内保持段首值，got {}",
            mid2.opacity.unwrap()
        );
        // p=1.0：恰在 stop 位置 → 取原始值 1.0（不经 ease）。
        let end = sample(&kf, 1.0, Ease::Linear);
        assert!((end.opacity.unwrap() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn restart_removes_players_and_sync_rebuilds_from_zero() {
        let (mut scene, ids) = scene_with_animated_nodes(1);
        scene.keyframes.insert("fade".into(), rule());
        crate::style::dynamic::sync_animation_players(&mut scene);
        assert_eq!(scene.players.len(), 1, "sync 建 player");
        crate::scene::animation::update_all(&mut scene, 1.0, &mut Vec::new());
        assert_eq!(
            scene.players.len(),
            1,
            "Completed player 保留（fill both 持末值）"
        );

        restart_animations(&mut scene, ids[0]);
        assert_eq!(scene.players.len(), 0, "restart 清掉声明式 player");

        crate::style::dynamic::sync_animation_players(&mut scene);
        assert_eq!(scene.players.len(), 1, "sync 依声明重建");
        let p = scene.players.values().next().unwrap();
        assert_eq!(p.elapsed, 0.0, "重建后 elapsed 归零（delay 重计）");
        assert_eq!(p.play_state, PlayerPlayState::Playing);
    }

    #[test]
    fn restart_scoped_to_subtree() {
        let (mut scene, ids) = scene_with_animated_nodes(2);
        scene.keyframes.insert("fade".into(), rule());
        crate::style::dynamic::sync_animation_players(&mut scene);
        assert_eq!(scene.players.len(), 2);
        restart_animations(&mut scene, ids[0]); // 只重启 ids[0] 子树
        assert_eq!(scene.players.len(), 1, "子树外 player 保留");
        assert_eq!(scene.players.values().next().unwrap().node, ids[1]);
    }

    // —— #10 layout/box-shadow keyframes 采样 ——

    #[test]
    fn sample_interpolates_width_same_domain() {
        let kf = KeyframesRule {
            name: "grow".into(),
            stops: vec![
                KeyframeStop {
                    selector: KeyframeStopSelector::From,
                    props: AnimatableProps {
                        width: Some(AnimLen {
                            domain: LenDomain::Px,
                            value: 0.0,
                        }),
                        ..Default::default()
                    },
                    timing: None,
                    hook: None,
                },
                KeyframeStop {
                    selector: KeyframeStopSelector::To,
                    props: AnimatableProps {
                        width: Some(AnimLen {
                            domain: LenDomain::Px,
                            value: 200.0,
                        }),
                        ..Default::default()
                    },
                    timing: None,
                    hook: None,
                },
            ],
        };
        let mid = sample(&kf, 0.5, crate::tween::Ease::Linear);
        let w = mid.width.expect("width 通道采样");
        assert_eq!(w.domain, LenDomain::Px);
        assert!((w.value - 100.0).abs() < 1e-5, "px 同域半程 = 100");
    }

    #[test]
    fn sample_discrete_jump_on_cross_domain_width() {
        // 异域（px→%）是围栏硬拒后的运行时兜底：离散跳变（t<0.5 from / ≥0.5 to），
        // 不静默混合出无意义数值。
        let kf = KeyframesRule {
            name: "mixed".into(),
            stops: vec![
                KeyframeStop {
                    selector: KeyframeStopSelector::From,
                    props: AnimatableProps {
                        width: Some(AnimLen {
                            domain: LenDomain::Px,
                            value: 100.0,
                        }),
                        ..Default::default()
                    },
                    timing: None,
                    hook: None,
                },
                KeyframeStop {
                    selector: KeyframeStopSelector::To,
                    props: AnimatableProps {
                        width: Some(AnimLen {
                            domain: LenDomain::Pct,
                            value: 50.0,
                        }),
                        ..Default::default()
                    },
                    timing: None,
                    hook: None,
                },
            ],
        };
        let before = sample(&kf, 0.49, crate::tween::Ease::Linear);
        assert_eq!(
            before.width.unwrap().domain,
            LenDomain::Px,
            "t<0.5 取 from 域"
        );
        let after = sample(&kf, 0.5, crate::tween::Ease::Linear);
        assert_eq!(
            after.width.unwrap().domain,
            LenDomain::Pct,
            "t≥0.5 取 to 域"
        );
    }

    #[test]
    fn sample_pads_box_shadow_lists_per_browser_semantics() {
        // 1 层→2 层：短列表补透明零长阴影，第二层中点 alpha=0.5（规范语义，非跳现）。
        use crate::style::resolved::BoxShadow;
        let one = vec![BoxShadow {
            ox: 0.0,
            oy: 4.0,
            spread: 0.0,
            blur: 8.0,
            color: [0.0, 0.0, 0.0, 1.0],
            inset: false,
        }];
        let two = vec![
            BoxShadow {
                ox: 0.0,
                oy: 4.0,
                spread: 0.0,
                blur: 8.0,
                color: [0.0, 0.0, 0.0, 1.0],
                inset: false,
            },
            BoxShadow {
                ox: 0.0,
                oy: 8.0,
                spread: 0.0,
                blur: 16.0,
                color: [1.0, 1.0, 1.0, 1.0],
                inset: false,
            },
        ];
        let kf = KeyframesRule {
            name: "sh".into(),
            stops: vec![
                KeyframeStop {
                    selector: KeyframeStopSelector::From,
                    props: AnimatableProps {
                        box_shadow: Some(one),
                        ..Default::default()
                    },
                    timing: None,
                    hook: None,
                },
                KeyframeStop {
                    selector: KeyframeStopSelector::To,
                    props: AnimatableProps {
                        box_shadow: Some(two),
                        ..Default::default()
                    },
                    timing: None,
                    hook: None,
                },
            ],
        };
        let mid = sample(&kf, 0.5, crate::tween::Ease::Linear);
        let list = mid.box_shadow.expect("box-shadow 通道采样");
        assert_eq!(list.len(), 2, "补齐空阴影成对");
        assert!(
            (list[1].color[3] - 0.5).abs() < 1e-5,
            "新增层透明淡入 alpha=0.5"
        );
    }
}
