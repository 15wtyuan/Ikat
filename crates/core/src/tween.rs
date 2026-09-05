//! GTween-lite：tween 引擎（TweenManager + Ease/TweenSpec/TweenValue）。
//! 动 opacity / transform(translate·scale·rotate) / 颜色(bg·text)；支持 delay /
//! repeat+yoyo 多轮播放；active/pool 双池复用（完成/被杀槽回炉，spawn 零分配稳态）。
//! replace-override：动画值覆盖 ResolvedStyle 读取点（None 退回 CSS）。
//! `TweenValue`/`lerp_n` 是 tween 与 keyframes 共享的插值原语（#9 统一）。

use serde::{Deserialize, Serialize};

/// 可动属性。u8 值与 FFI / C# enum 对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum TweenProp {
    Opacity = 0,
    Translate = 1,
    Scale = 2,
    Rotation = 3,
    BgColor = 4,
    TextColor = 5,
    /// CSS `transition: transform` 的复合通道：整矩阵 TRS 分解后单 tween 插值
    ///（不拆 Translate/Scale/Rotation 三 tween——apply 各臂整矩阵覆写会互踩）。
    /// 追加到末尾保持既有判别值稳定。
    Transform = 6,
    /// #10 layout 动画通道。Width/Height 载荷 = [value, domain_code]（域码为
    /// LenDomain 判别值，start/end 必须同域——同域保证下 lerp 域码恒等）；
    /// solve sync 覆写链最末位消费（base → viewport → anim）。
    Width = 7,
    Height = 8,
    /// flex-grow 标量（无域）。
    FlexGrow = 9,
    /// box-shadow 列表通道：值不走 TweenValue（≤12 层×9 分量远超 8 槽），
    /// 载荷在 `TweenSpec.shadow`（None 视为无效提交，tween() 拒收）。
    BoxShadow = 10,
}

impl TweenProp {
    /// u32 → TweenProp（FFI 校验用）。越界 → None。判别值与 C# enum / FFI u32 对齐。
    pub fn try_from(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Opacity),
            1 => Some(Self::Translate),
            2 => Some(Self::Scale),
            3 => Some(Self::Rotation),
            4 => Some(Self::BgColor),
            5 => Some(Self::TextColor),
            6 => Some(Self::Transform),
            7 => Some(Self::Width),
            8 => Some(Self::Height),
            9 => Some(Self::FlexGrow),
            10 => Some(Self::BoxShadow),
            _ => None,
        }
    }
}

/// 每个 prop 的 lerp 分量数（start/end 前 N 个分量有效；Width/Height 的第 2 槽是
/// 域码——同域保证下 lerp 恒等，apply 端从结果槽位读回）。BoxShadow = 0（列表载荷
/// 在 shadow 字段，TweenValue 不参与）。
pub fn prop_value_size(prop: TweenProp) -> u8 {
    match prop {
        TweenProp::Opacity | TweenProp::Rotation | TweenProp::FlexGrow => 1,
        TweenProp::Translate | TweenProp::Scale | TweenProp::Width | TweenProp::Height => 2,
        TweenProp::BgColor | TweenProp::TextColor => 4,
        TweenProp::Transform => 5,
        TweenProp::BoxShadow => 0,
    }
}

/// CSS 标准 easing 关键字的精确 bezier 等价（CSS Easing Functions Level 1）。
/// 早期版本用 Quad/Cubic 幂函数近似这些 keyword，现统一为标准 bezier。
pub const EASE_BEZIER: [f32; 4] = [0.25, 0.1, 0.25, 1.0];
pub const EASE_IN_BEZIER: [f32; 4] = [0.42, 0.0, 1.0, 1.0];
pub const EASE_OUT_BEZIER: [f32; 4] = [0.0, 0.0, 0.58, 1.0];
pub const EASE_IN_OUT_BEZIER: [f32; 4] = [0.42, 0.0, 0.58, 1.0];

/// easing 函数全集（CSS 标准 + yio 超集）。u8 判别值与 FFI / C# enum 对齐。
///
/// 变体追加纪律：**只从末尾追加**，既有判别值（0..9 keyword + Step）稳定——Ease 走
/// bincode 进 pkg（variant index = 声明序），中途插入会错读旧包。
///
/// - CSS 标准：`linear` / `ease` / `ease-in` / `ease-out` / `ease-in-out`（精确 bezier）、
///   `cubic-bezier(x1,y1,x2,y2)`（x∈[0,1] 由 parse 侧校验）、`steps` 单步（start/end）。
/// - yio 超集（fence 认收的非标 keyword，游戏 UI 刚需）：`ease-{in,out,in-out}-{back,
///   elastic,bounce}` 固定系数族——参数化 elastic（amplitude/period）不做，DSL 侧用
///   cubic-bezier 近似或运行时 API 表达。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Ease {
    Linear = 0,
    QuadIn = 1,
    QuadOut = 2,
    QuadInOut = 3,
    CubicIn = 4,
    CubicOut = 5,
    CubicInOut = 6,
    BackIn = 7,
    BackOut = 8,
    BackInOut = 9,
    /// CSS `steps(start|end)` 阶跃函数。start=true → steps(start)（跳变在区间起点），
    /// false → steps(end)（末尾跳变）。
    Step {
        start: bool,
    },
    /// CSS `cubic-bezier(x1,y1,x2,y2)`。x 须 ∈[0,1]（y 不限，可 overshoot）；
    /// 运行时 Newton 迭代求逆（见 `evaluate`）。
    CubicBezier {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    },
    ElasticIn,
    ElasticOut,
    ElasticInOut,
    BounceIn,
    BounceOut,
    BounceInOut,
}

/// FFI 侧 Ease 描述（`YioTweenSpec.ease_kind` 值域）。core enum 的带数据变体
/// （Step/CubicBezier）拆成 kind+params 平面形；与 core 判别值**有意不对齐**
/// （数据变体无法用单 u32 表达），转换在 `ease_from_ffi`。
pub mod ease_ffi {
    pub const LINEAR: u32 = 0;
    pub const QUAD_IN: u32 = 1;
    pub const QUAD_OUT: u32 = 2;
    pub const QUAD_IN_OUT: u32 = 3;
    pub const CUBIC_IN: u32 = 4;
    pub const CUBIC_OUT: u32 = 5;
    pub const CUBIC_IN_OUT: u32 = 6;
    pub const BACK_IN: u32 = 7;
    pub const BACK_OUT: u32 = 8;
    pub const BACK_IN_OUT: u32 = 9;
    pub const STEP_END: u32 = 10;
    pub const STEP_START: u32 = 11;
    /// params = [x1, y1, x2, y2]
    pub const CUBIC_BEZIER: u32 = 12;
    pub const ELASTIC_IN: u32 = 13;
    pub const ELASTIC_OUT: u32 = 14;
    pub const ELASTIC_IN_OUT: u32 = 15;
    pub const BOUNCE_IN: u32 = 16;
    pub const BOUNCE_OUT: u32 = 17;
    pub const BOUNCE_IN_OUT: u32 = 18;
}

/// FFI kind+params → core Ease。越界/NaN 参数 → None（FFI 校验用）。
pub fn ease_from_ffi(kind: u32, params: [f32; 4]) -> Option<Ease> {
    use ease_ffi::*;
    Some(match kind {
        LINEAR => Ease::Linear,
        QUAD_IN => Ease::QuadIn,
        QUAD_OUT => Ease::QuadOut,
        QUAD_IN_OUT => Ease::QuadInOut,
        CUBIC_IN => Ease::CubicIn,
        CUBIC_OUT => Ease::CubicOut,
        CUBIC_IN_OUT => Ease::CubicInOut,
        BACK_IN => Ease::BackIn,
        BACK_OUT => Ease::BackOut,
        BACK_IN_OUT => Ease::BackInOut,
        STEP_END => Ease::Step { start: false },
        STEP_START => Ease::Step { start: true },
        CUBIC_BEZIER => {
            let [x1, y1, x2, y2] = params;
            if !(0.0..=1.0).contains(&x1) || !(0.0..=1.0).contains(&x2) {
                return None;
            }
            Ease::CubicBezier { x1, y1, x2, y2 }
        }
        ELASTIC_IN => Ease::ElasticIn,
        ELASTIC_OUT => Ease::ElasticOut,
        ELASTIC_IN_OUT => Ease::ElasticInOut,
        BOUNCE_IN => Ease::BounceIn,
        BOUNCE_OUT => Ease::BounceOut,
        BOUNCE_IN_OUT => Ease::BounceInOut,
        _ => return None,
    })
}

impl Ease {
    /// u32 → Ease（FFI 校验用）。越界 → None。判别值与 C# enum / FFI u32 对齐。
    /// 只覆盖无数据变体（0..9）；Step/CubicBezier 走 `ease_from_ffi`。
    pub fn try_from(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Linear),
            1 => Some(Self::QuadIn),
            2 => Some(Self::QuadOut),
            3 => Some(Self::QuadInOut),
            4 => Some(Self::CubicIn),
            5 => Some(Self::CubicOut),
            6 => Some(Self::CubicInOut),
            7 => Some(Self::BackIn),
            8 => Some(Self::BackOut),
            9 => Some(Self::BackInOut),
            _ => None,
        }
    }
}

const OVERSHOOT: f32 = 1.70158;

impl Ease {
    /// t∈[0,dur] → [0,1]。dur<=0 直接返 1（调用方已钳 tt<=dur，这里防除零）。
    pub fn evaluate(self, t: f32, dur: f32) -> f32 {
        if dur <= 0.0 {
            return 1.0;
        }
        match self {
            Ease::Linear => t / dur,
            Ease::QuadIn => {
                let t = t / dur;
                t * t
            }
            Ease::QuadOut => {
                let t = t / dur;
                -t * (t - 2.0)
            }
            Ease::QuadInOut => {
                let t = t / (dur * 0.5);
                if t < 1.0 {
                    0.5 * t * t
                } else {
                    let t = t - 1.0;
                    -0.5 * (t * (t - 2.0) - 1.0)
                }
            }
            Ease::CubicIn => {
                let t = t / dur;
                t * t * t
            }
            Ease::CubicOut => {
                let t = t / dur - 1.0;
                t * t * t + 1.0
            }
            Ease::CubicInOut => {
                let t = t / (dur * 0.5);
                if t < 1.0 {
                    0.5 * t * t * t
                } else {
                    let t = t - 2.0;
                    0.5 * (t * t * t + 2.0)
                }
            }
            Ease::BackIn => {
                let t = t / dur;
                t * t * ((OVERSHOOT + 1.0) * t - OVERSHOOT)
            }
            Ease::BackOut => {
                let t = t / dur - 1.0;
                t * t * ((OVERSHOOT + 1.0) * t + OVERSHOOT) + 1.0
            }
            Ease::BackInOut => {
                let s = OVERSHOOT * 1.525;
                let t = t / (dur * 0.5);
                if t < 1.0 {
                    0.5 * (t * t * ((s + 1.0) * t - s))
                } else {
                    let t = t - 2.0;
                    0.5 * (t * t * ((s + 1.0) * t + s) + 2.0)
                }
            }
            // CSS steps()：单步阶跃。steps(start) → t=0 即 1.0；steps(end) → 保持 0.0
            // 直到 t>=dur 跳 1.0（evaluate 入口已处理 dur<=0 返 1.0）。
            Ease::Step { start } => {
                if start || t >= dur {
                    1.0
                } else {
                    0.0
                }
            }
            Ease::CubicBezier { x1, y1, x2, y2 } => {
                let p = (t / dur).clamp(0.0, 1.0);
                cubic_bezier_y(p, x1, y1, x2, y2)
            }
            Ease::ElasticIn => {
                let p = t / dur;
                if p == 0.0 || p == 1.0 {
                    p
                } else {
                    -(2.0f32.powf(10.0 * p - 10.0))
                        * ((p * 10.0 - 10.75) * (std::f32::consts::TAU / 3.0)).sin()
                }
            }
            Ease::ElasticOut => {
                let p = t / dur;
                if p == 0.0 || p == 1.0 {
                    p
                } else {
                    2.0f32.powf(-10.0 * p)
                        * ((p * 10.0 - 0.75) * (std::f32::consts::TAU / 3.0)).sin()
                        + 1.0
                }
            }
            Ease::ElasticInOut => {
                let p = t / dur;
                if p == 0.0 || p == 1.0 {
                    p
                } else if p < 0.5 {
                    -(2.0f32.powf(20.0 * p - 10.0))
                        * ((20.0 * p - 11.125) * (std::f32::consts::TAU / 4.5)).sin()
                } else {
                    2.0f32.powf(-20.0 * p + 10.0)
                        * ((20.0 * p - 11.125) * (std::f32::consts::TAU / 4.5)).sin()
                        + 1.0
                }
            }
            Ease::BounceIn => 1.0 - bounce_out(1.0 - t / dur),
            Ease::BounceOut => bounce_out(t / dur),
            Ease::BounceInOut => {
                let p = t / dur;
                if p < 0.5 {
                    (1.0 - bounce_out(1.0 - 2.0 * p)) / 2.0
                } else {
                    (1.0 + bounce_out(2.0 * p - 1.0)) / 2.0
                }
            }
        }
    }
}

/// easeOutBounce（Penner 标准分段，BounceIn/InOut 由它组合）。
fn bounce_out(p: f32) -> f32 {
    const N1: f32 = 7.5625;
    const D1: f32 = 2.75;
    if p < 1.0 / D1 {
        N1 * p * p
    } else if p < 2.0 / D1 {
        let p = p - 1.5 / D1;
        N1 * p * p + 0.75
    } else if p < 2.5 / D1 {
        let p = p - 2.25 / D1;
        N1 * p * p + 0.9375
    } else {
        let p = p - 2.625 / D1;
        N1 * p * p + 0.984375
    }
}

/// cubic-bezier 曲线 y(p)：给定 x 进度 p，解三次 bezier 的 x(u)=p 得参数 u，返 y(u)。
///
/// bezier 控制点 (x1,y1)/(x2,y2)（端点固定 (0,0)/(1,1)）。x 单调性由 x1,x2∈[0,1]
/// 保证（CSS 有效性约束），可用二分兜底。Newton 4 轮 + 二分收敛，足够 f32 精度。
fn cubic_bezier_y(p: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    if p <= 0.0 {
        return 0.0;
    }
    if p >= 1.0 {
        return 1.0;
    }
    // bezier 分量导数：3(1-u)²(x1-0) + 6(1-u)u(x2-x1) + 3u²(1-x2)
    let dx = |u: f32| {
        3.0 * (1.0 - u) * (1.0 - u) * x1
            + 6.0 * (1.0 - u) * u * (x2 - x1)
            + 3.0 * u * u * (1.0 - x2)
    };
    let bx =
        |u: f32| 3.0 * (1.0 - u) * (1.0 - u) * u * x1 + 3.0 * (1.0 - u) * u * u * x2 + u * u * u;
    let by =
        |u: f32| 3.0 * (1.0 - u) * (1.0 - u) * u * y1 + 3.0 * (1.0 - u) * u * u * y2 + u * u * u;
    // Newton 迭代
    let mut u = p;
    for _ in 0..4 {
        let x = bx(u) - p;
        let d = dx(u);
        if d.abs() < 1e-6 {
            break;
        }
        u -= x / d;
        if !(0.0..=1.0).contains(&u) {
            break; // 越界交二分兜底
        }
    }
    // 兜底二分（Newton 未收敛/越界时）
    if !(0.0..=1.0).contains(&u) || (bx(u) - p).abs() > 1e-4 {
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..24 {
            u = (lo + hi) * 0.5;
            if bx(u) < p {
                lo = u;
            } else {
                hi = u;
            }
        }
    }
    by(u)
}

/// tween/动画共用的定长插值缓冲（前 `prop_value_size` 个分量有效）。
/// 8 槽对齐：TRS+百分比扩展（7 分量）与颜色（4）都装得下，余量给未来通道。
pub type TweenValue = [f32; 8];

/// 定长逐分量 lerp（前 n 个）。tween apply 与 keyframes 通道插值共用（插值原语统一）。
pub fn lerp_n(a: &TweenValue, b: &TweenValue, t: f32, n: usize) -> TweenValue {
    let mut out = [0.0; 8];
    for i in 0..n.min(8) {
        out[i] = a[i] + (b[i] - a[i]) * t;
    }
    out
}

/// tween 提交单元（builder/FFI spec 的 core 内形态）。`shadow` 仅 BoxShadow 通道
/// 使用（其余通道 None）；prop=BoxShadow 且 shadow=None 是无效提交，`tween()` 拒收。
#[derive(Debug, Clone)]
pub struct TweenSpec {
    pub prop: TweenProp,
    pub start: TweenValue,
    pub end: TweenValue,
    pub ease: Ease,
    pub delay: f32,
    pub duration: f32,
    pub tag: u32,
    /// 额外重播次数（0 = 单次；总播放 = repeat+1 轮）。
    pub repeat: u32,
    /// 往返：偶数轮 start→end、奇数轮 end→start（CSS alternate 同义）。
    pub yoyo: bool,
    /// box-shadow 双端列表（BoxShadow 通道载荷；Box 包内嵌防 TweenSpec 膨胀）。
    pub shadow: Option<Box<ShadowPair>>,
}

impl TweenSpec {
    /// 便捷构造：单次、无 delay、linear、tag=0（builder 各字段覆写）。
    pub fn new(prop: TweenProp, start: TweenValue, end: TweenValue) -> Self {
        Self {
            prop,
            start,
            end,
            ease: Ease::Linear,
            delay: 0.0,
            duration: 0.3,
            tag: 0,
            repeat: 0,
            yoyo: false,
            shadow: None,
        }
    }
}

/// box-shadow 通道的双端列表（tween 与 transition 请求共用形态）。
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowPair {
    pub start: Vec<crate::style::resolved::BoxShadow>,
    pub end: Vec<crate::style::resolved::BoxShadow>,
}

/// transition 请求（rematch 检测 data-page 通道变化时推入 Scene.pending_transitions；
/// Stage tick drain 后 kill 旧 tween + 提交新 tween）。
#[derive(Debug, Clone)]
pub struct TransitionRequest {
    pub node: crate::scene::node::NodeId,
    pub prop: TweenProp,
    /// 通道载荷（前 prop_value_size 个分量有效；Transform = [tx,ty,sx,sy,rot]；
    /// Width/Height = [value, domain_code]）。
    pub start: [f32; 5],
    pub end: [f32; 5],
    pub ease: Ease,
    pub delay: f32,
    pub duration: f32,
    /// box-shadow 通道的双端列表（其余通道 None）。
    pub shadow: Option<Box<ShadowPair>>,
}

use crate::input::{EventRecord, EVT_TWEEN_COMPLETE};
use crate::scene::node::{AnimTable, NodeId, Scene};
use crate::transform::{self};

/// 一个进行中的 tween（`TweenManager.active`/`pool` 复用槽）。
/// pub(crate) 字段供测试断言（kill 后验全 killed、transition drain 验 start/end/tag）+
/// Stage 联动查（killed/node）。
#[derive(Debug, Clone)]
pub(crate) struct Tween {
    pub(crate) node: NodeId,
    pub(crate) prop: TweenProp,
    pub(crate) start: TweenValue,
    pub(crate) end: TweenValue,
    ease: Ease,
    delay: f32,
    duration: f32,
    repeat: u32,
    yoyo: bool,
    elapsed: f32,
    pub(crate) tag: u32,
    started: bool,
    pub(crate) killed: bool,
    /// BoxShadow 通道载荷（其余通道 None；与 start/end 并列存储）。
    shadow: Option<Box<ShadowPair>>,
}

impl Tween {
    /// 池化复用：覆写全部可变字段，回炉当新 tween（省 per-spawn 分配）。
    fn recycle(&mut self, node: NodeId, spec: TweenSpec) {
        self.node = node;
        self.prop = spec.prop;
        self.start = spec.start;
        self.end = spec.end;
        self.ease = spec.ease;
        self.delay = spec.delay;
        self.duration = spec.duration;
        self.repeat = spec.repeat;
        self.yoyo = spec.yoyo;
        self.elapsed = 0.0;
        self.tag = spec.tag;
        self.started = false;
        self.killed = false;
        self.shadow = spec.shadow;
    }
}

/// Tween 引擎：active 池推进每 tick、完成/被杀回 pool 复用，写 scene.anim，
/// 完成时产 EVT_TWEEN_COMPLETE。
#[derive(Debug, Default)]
pub struct TweenManager {
    /// pub(crate) 供测试断言（kill_node 后验全 killed）+ Stage 联动查。
    pub(crate) active: Vec<Tween>,
    /// 回收池：update 末尾死 tween（完成/被杀/悬空）入池，tween() 优先复用。
    pub(crate) pool: Vec<Tween>,
}

impl TweenManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// 清空所有 tween（load 重建 scene 时调，防残留指向失效 node_id）。
    /// pool 一并清——池内槽同样可能持过期 node 语义（tag/ease 等由 recycle 全覆写，
    /// 清不清皆可，但保持「clear = 全归零」的直觉一致）。
    pub fn clear(&mut self) {
        self.active.clear();
        self.pool.clear();
    }

    /// 杀该节点所有 tween（remove_node 联动调）。标 killed，update 末尾回池。
    /// 与 `kill(node, prop)`（单 prop）不同——此杀该 node 全部 prop 的 tween。
    pub fn kill_node(&mut self, node: NodeId) {
        for t in &mut self.active {
            if t.node == node && !t.killed {
                t.killed = true;
            }
        }
    }

    /// 注册一个 tween（优先复用池槽）。越界 node 由 update 跳过。
    /// prop=BoxShadow 须带 shadow 载荷（缺 = 无效提交，拒收）。
    pub fn tween(&mut self, node: NodeId, spec: TweenSpec) {
        if spec.prop == TweenProp::BoxShadow && spec.shadow.is_none() {
            return;
        }
        if let Some(mut t) = self.pool.pop() {
            t.recycle(node, spec);
            self.active.push(t);
        } else {
            self.active.push(Tween {
                node,
                prop: spec.prop,
                start: spec.start,
                end: spec.end,
                ease: spec.ease,
                delay: spec.delay,
                duration: spec.duration,
                repeat: spec.repeat,
                yoyo: spec.yoyo,
                elapsed: 0.0,
                tag: spec.tag,
                started: false,
                killed: false,
                shadow: spec.shadow,
            });
        }
    }

    /// 停该节点该 prop 的 tween（killed，override 保留末值）。
    pub fn kill(&mut self, node: NodeId, prop: TweenProp) {
        for t in &mut self.active {
            if t.node == node && t.prop == prop && !t.killed {
                t.killed = true;
            }
        }
    }

    /// 每 tick：推进 active tween，写 scene.anim，产 complete 事件；
    /// 死槽（完成/被杀/悬空）原位分流回 pool（保活序稳定）。
    pub fn update(&mut self, dt: f32, scene: &mut Scene, out: &mut Vec<EventRecord>) {
        if self.active.is_empty() {
            return;
        }
        for t in &mut self.active {
            if t.killed {
                continue;
            }
            // 悬空/无效 NodeId（不在 scene.nodes）→ 标 killed 跳过 apply + 不产 complete。
            // 双保险：remove_node 联动 kill_node 主动杀；此处兜底——若 tween 残留指向已删 node
            // （HashMap 对任意 NodeId 都能插条目，故须显式校验 live，防悬空 tween 写幽灵槽）。
            if scene.get(t.node).is_none() {
                t.killed = true;
                continue;
            }
            t.elapsed += dt;
            if t.elapsed < t.delay {
                continue;
            }
            t.started = true;
            let tt = t.elapsed - t.delay;
            // 多轮播放：cycles 轮 × duration；yoyo 奇偶轮反向（末值↔首值往返）。
            let cycles = t.repeat.saturating_add(1) as f32;
            let total = t.duration * cycles;
            let (norm, done) = if tt >= total {
                // 完成帧：取最后一轮的终态（yoyo 且末轮为奇数轮 → 回到 start）。
                let last_forward = !t.yoyo || t.repeat % 2 == 0;
                (if last_forward { 1.0 } else { 0.0 }, true)
            } else {
                let cycle = if t.duration > 0.0 {
                    (tt / t.duration) as u32
                } else {
                    0
                };
                let local = tt - cycle as f32 * t.duration;
                let clamped = if local >= t.duration {
                    t.duration
                } else {
                    local
                };
                let mut n = t.ease.evaluate(clamped, t.duration);
                if t.yoyo && cycle % 2 == 1 {
                    n = 1.0 - n;
                }
                (n, false)
            };
            apply(
                &mut scene.anim,
                t.node,
                t.prop,
                &t.start,
                &t.end,
                t.shadow.as_deref(),
                norm,
            );
            if done {
                t.killed = true;
                out.push(EventRecord {
                    node_id: t.node.0,
                    event_type: EVT_TWEEN_COMPLETE,
                    click_count: t.prop as u8, // 复用：prop 枚举值
                    pad: [0, 0],
                    touch_id: t.tag as i32, // 复用：调用方 tag
                    x: 0.0,
                    y: 0.0,
                    dx: 0.0,
                    dy: 0.0,
                });
            }
        }
        // 死槽回池：活槽稳定前移（保序），死槽从尾部 pop 入 pool。
        let mut w = 0usize;
        for r in 0..self.active.len() {
            if !self.active[r].killed {
                self.active.swap(w, r);
                w += 1;
            }
        }
        while self.active.len() > w {
            let t = self.active.pop().expect("len > w 保证有槽");
            self.pool.push(t);
        }
    }
}

/// 逐分量 lerp start→end 写入 anim 对应通道（n=已算的 normalized；TweenValue 共享原语）。
/// 经 AnimTable::ensure(node) 取可变 NodeAnim（HashMap entry，缺则插 default）。
/// Width/Height 的域码在载荷第 2 槽（同域保证下 lerp 恒等，apply 端读回）。
/// pub(crate)：transition drain 提交 tween 时以 n=0 预写起始值（首帧 solve 读
/// override 而非级联终点，消掉端点一帧闪现；Stage tick ⑥.5 调）。
pub(crate) fn apply(
    anim: &mut AnimTable,
    node: NodeId,
    prop: TweenProp,
    start: &TweenValue,
    end: &TweenValue,
    shadow: Option<&ShadowPair>,
    n: f32,
) {
    let a = anim.ensure(node);
    let v = lerp_n(start, end, n, prop_value_size(prop) as usize);
    match prop {
        TweenProp::Opacity => a.opacity = Some(v[0]),
        TweenProp::Translate => a.transform = Some(transform::from_translate(v[0], v[1])),
        TweenProp::Scale => a.transform = Some(transform::from_scale(v[0], v[1])),
        TweenProp::Rotation => a.transform = Some(transform::from_rotate(v[0])),
        // TRS 五元组逐分量 lerp 后 SRT 合成（与 keyframe 的 transform 插值同一语义）。
        TweenProp::Transform => {
            a.transform = Some(transform::from_trs(v[0], v[1], v[2], v[3], v[4]))
        }
        TweenProp::BgColor => a.bg_color = Some([v[0], v[1], v[2], v[3]]),
        TweenProp::TextColor => a.text_color = Some([v[0], v[1], v[2], v[3]]),
        TweenProp::Width | TweenProp::Height => {
            let Some(domain) = crate::scene::animation::LenDomain::try_from_code(v[1] as u32)
            else {
                return; // 防御：域码非整数/越界（FFI 已拦，这里不 panic）
            };
            let len = crate::scene::animation::AnimLen {
                domain,
                value: v[0],
            };
            match prop {
                TweenProp::Width => a.width = Some(len),
                _ => a.height = Some(len),
            }
        }
        TweenProp::FlexGrow => a.flex_grow = Some(v[0]),
        TweenProp::BoxShadow => {
            if let Some(pair) = shadow {
                a.box_shadow = Some(lerp_shadow_list(&pair.start, &pair.end, n));
            }
        }
    }
}

/// box-shadow 列表插值（css-backgrounds-3 / MDN 语义，tween 与 keyframes 共用）：
/// 短列表末尾补「透明色、零偏移/模糊/spread」空阴影后逐对插值（补齐阴影继承配对
/// 方的 inset——空阴影无自身几何，inset 由配对实影决定）；**任一实配对 inset 不匹配
/// → 整表离散**（t<0.5 取 start、否则取 end）。
pub fn lerp_shadow_list(
    start: &[crate::style::resolved::BoxShadow],
    end: &[crate::style::resolved::BoxShadow],
    t: f32,
) -> Vec<crate::style::resolved::BoxShadow> {
    use crate::style::resolved::BoxShadow;
    let mismatch = start
        .iter()
        .zip(end.iter())
        .any(|(a, b)| a.inset != b.inset);
    if mismatch {
        return (if t < 0.5 { start } else { end }).to_vec();
    }
    let n = start.len().max(end.len());
    let null = |inset: bool| BoxShadow {
        ox: 0.0,
        oy: 0.0,
        spread: 0.0,
        blur: 0.0,
        color: [0.0; 4],
        inset,
    };
    let pick = |list: &[BoxShadow], i: usize| list.get(i).copied();
    (0..n)
        .map(|i| {
            let (a, b) = (pick(start, i), pick(end, i));
            match (a, b) {
                (Some(mut x), Some(y)) => {
                    x.ox += (y.ox - x.ox) * t;
                    x.oy += (y.oy - x.oy) * t;
                    x.spread += (y.spread - x.spread) * t;
                    x.blur += (y.blur - x.blur) * t;
                    x.color = lerp_arr4(x.color, y.color, t);
                    x
                }
                // 单端存在 → 与补齐空阴影插值（透明淡入/淡出，浏览器同语义）。
                (Some(mut x), None) => {
                    let e = null(x.inset);
                    x.ox += (e.ox - x.ox) * t;
                    x.oy += (e.oy - x.oy) * t;
                    x.spread += (e.spread - x.spread) * t;
                    x.blur += (e.blur - x.blur) * t;
                    x.color = lerp_arr4(x.color, e.color, t);
                    x
                }
                (None, Some(mut y)) => {
                    let s = null(y.inset);
                    y.ox = s.ox + (y.ox - s.ox) * t;
                    y.oy = s.oy + (y.oy - s.oy) * t;
                    y.spread = s.spread + (y.spread - s.spread) * t;
                    y.blur = s.blur + (y.blur - s.blur) * t;
                    y.color = lerp_arr4(s.color, y.color, t);
                    y
                }
                (None, None) => null(false),
            }
        })
        .collect()
}

fn lerp_arr4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(prop: TweenProp, start: [f32; 5], end: [f32; 5]) -> TweenSpec {
        TweenSpec {
            prop,
            start: pad5(start),
            end: pad5(end),
            ease: Ease::Linear,
            delay: 0.0,
            duration: 1.0,
            tag: 0,
            repeat: 0,
            yoyo: false,
            shadow: None,
        }
    }

    fn pad5(v: [f32; 5]) -> TweenValue {
        let mut out = [0.0; 8];
        out[..5].copy_from_slice(&v);
        out
    }

    #[test]
    fn prop_value_size_mapping() {
        assert_eq!(prop_value_size(TweenProp::Opacity), 1);
        assert_eq!(prop_value_size(TweenProp::Rotation), 1);
        assert_eq!(prop_value_size(TweenProp::Translate), 2);
        assert_eq!(prop_value_size(TweenProp::Scale), 2);
        assert_eq!(prop_value_size(TweenProp::BgColor), 4);
        assert_eq!(prop_value_size(TweenProp::TextColor), 4);
        assert_eq!(prop_value_size(TweenProp::Transform), 5);
        // #10 layout/box-shadow 通道：Width/Height = 值 + 域码双槽；FlexGrow 标量；
        // BoxShadow 列表载荷在 shadow 字段（TweenValue 0 槽）。
        assert_eq!(prop_value_size(TweenProp::Width), 2);
        assert_eq!(prop_value_size(TweenProp::Height), 2);
        assert_eq!(prop_value_size(TweenProp::FlexGrow), 1);
        assert_eq!(prop_value_size(TweenProp::BoxShadow), 0);
    }

    #[test]
    fn lerp_n_leading_components_only() {
        let a = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0];
        let b = [10.0; 8];
        let v = lerp_n(&a, &b, 0.5, 3);
        assert!((v[0] - 5.0).abs() < 1e-6);
        assert!((v[1] - 5.5).abs() < 1e-6);
        assert!((v[2] - 6.0).abs() < 1e-6);
        assert_eq!(v[3], 0.0, "n 之外分量不动（保持 0 槽）");
    }

    #[test]
    fn ease_endpoints_are_0_and_1() {
        let dur = 1.0;
        let beziers = [
            Ease::CubicBezier {
                x1: 0.25,
                y1: 0.1,
                x2: 0.25,
                y2: 1.0,
            },
            Ease::CubicBezier {
                x1: 0.42,
                y1: 0.0,
                x2: 1.0,
                y2: 1.0,
            },
            Ease::CubicBezier {
                x1: 0.0,
                y1: 0.0,
                x2: 0.58,
                y2: 1.0,
            },
        ];
        for ease in [
            Ease::Linear,
            Ease::QuadIn,
            Ease::QuadOut,
            Ease::QuadInOut,
            Ease::CubicIn,
            Ease::CubicOut,
            Ease::CubicInOut,
            Ease::BackIn,
            Ease::BackOut,
            Ease::BackInOut,
            Ease::ElasticIn,
            Ease::ElasticOut,
            Ease::ElasticInOut,
            Ease::BounceIn,
            Ease::BounceOut,
            Ease::BounceInOut,
        ]
        .into_iter()
        .chain(beziers)
        {
            assert!((ease.evaluate(0.0, dur)).abs() < 1e-4, "{:?}@0 != 0", ease);
            assert!(
                (ease.evaluate(dur, dur) - 1.0).abs() < 1e-4,
                "{:?}@dur != 1",
                ease
            );
        }
    }

    #[test]
    fn ease_dur_zero_returns_1() {
        assert_eq!(Ease::Linear.evaluate(0.5, 0.0), 1.0);
    }

    #[test]
    fn ease_step_jumps_at_start_or_end() {
        // CSS steps()：steps(start) → t=0 即 1.0；steps(end) → 0.0 直到 t>=dur 跳 1.0。
        let dur = 1.0;
        assert_eq!(Ease::Step { start: true }.evaluate(0.0, dur), 1.0);
        assert_eq!(Ease::Step { start: true }.evaluate(0.5, dur), 1.0);
        assert_eq!(Ease::Step { start: false }.evaluate(0.0, dur), 0.0);
        assert_eq!(Ease::Step { start: false }.evaluate(0.5, dur), 0.0);
        assert_eq!(Ease::Step { start: false }.evaluate(dur, dur), 1.0);
        // dur<=0 入口统一返 1（与其它 ease 一致，防除零）
        assert_eq!(Ease::Step { start: false }.evaluate(0.0, 0.0), 1.0);
    }

    #[test]
    fn cubic_bezier_matches_css_reference_values() {
        // CSS ease（bezier .25,.1,.25,1）的中点参考值 ≈ 0.8024（浏览器实现共识，
        // Newton+二分混合解在 1e-3 内一致即可——f32 曲线求逆不追求位级对齐）。
        let ease = Ease::CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.25,
            y2: 1.0,
        };
        let mid = ease.evaluate(0.5, 1.0);
        assert!((mid - 0.8024).abs() < 2e-3, "ease@0.5 ≈ 0.8024, got {mid}");
        // linear bezier 恒等：bezier(0,0,1,1) == linear
        let lin = Ease::CubicBezier {
            x1: 0.0,
            y1: 0.0,
            x2: 1.0,
            y2: 1.0,
        };
        for i in 0..=10 {
            let p = i as f32 / 10.0;
            assert!((lin.evaluate(p, 1.0) - p).abs() < 1e-3);
        }
    }

    #[test]
    fn cubic_bezier_overshoot_y_outside_unit() {
        // y 分量可越 [0,1]（back 类 overshoot 由 y2>1 表达）。
        let e = Ease::CubicBezier {
            x1: 0.3,
            y1: 1.5,
            x2: 0.7,
            y2: 1.5,
        };
        let mut max = 0.0f32;
        for i in 0..=100 {
            max = max.max(e.evaluate(i as f32 / 100.0, 1.0));
        }
        assert!(max > 1.0, "bezier y 越界 overshoot，got max {max}");
    }

    #[test]
    fn elastic_bounce_shapes() {
        // ElasticOut 中段超 1（弹过冲）；BounceOut 值域 [0,1] 且有分段弹跳（非单调）。
        let mut over = false;
        for i in 0..100 {
            if Ease::ElasticOut.evaluate(i as f32 / 100.0, 1.0) > 1.0 {
                over = true;
            }
        }
        assert!(over, "ElasticOut 中段须 >1");
        let mut prev = 0.0f32;
        let mut bounced = false;
        for i in 1..=100 {
            let p = i as f32 / 100.0;
            let v = Ease::BounceOut.evaluate(p, 1.0);
            assert!((0.0..=1.0).contains(&v), "BounceOut 值域 [0,1]，got {v}");
            if v < prev {
                bounced = true; // 出现回落 = 弹跳
            }
            prev = v;
        }
        assert!(bounced, "BounceOut 须有回落段");
    }

    #[test]
    fn ease_from_ffi_roundtrip_kinds() {
        use ease_ffi::*;
        assert_eq!(ease_from_ffi(LINEAR, [0.0; 4]), Some(Ease::Linear));
        assert_eq!(
            ease_from_ffi(STEP_START, [0.0; 4]),
            Some(Ease::Step { start: true })
        );
        assert_eq!(
            ease_from_ffi(CUBIC_BEZIER, [0.25, 0.1, 0.25, 1.0]),
            Some(Ease::CubicBezier {
                x1: 0.25,
                y1: 0.1,
                x2: 0.25,
                y2: 1.0
            })
        );
        assert_eq!(
            ease_from_ffi(CUBIC_BEZIER, [-0.1, 0.0, 1.0, 1.0]),
            None,
            "x1 越界 [0,1] 拒"
        );
        assert_eq!(ease_from_ffi(999, [0.0; 4]), None, "未知 kind 拒");
    }

    #[test]
    fn cubic_in_below_linear_below_cubic_out_at_mid() {
        // t=0.5,dur=1：CubicIn(0.125) < Linear(0.5) < CubicOut(0.875)
        let lin = Ease::Linear.evaluate(0.5, 1.0);
        let cin = Ease::CubicIn.evaluate(0.5, 1.0);
        let cout = Ease::CubicOut.evaluate(0.5, 1.0);
        assert!(
            cin < lin && lin < cout,
            "CubicIn({}) < Linear({}) < CubicOut({})",
            cin,
            lin,
            cout
        );
        assert!((cin - 0.125).abs() < 1e-5);
        assert!((cout - 0.875).abs() < 1e-5);
    }

    #[test]
    fn back_out_overshoots_above_1_mid() {
        // BackOut 中段 >1（overshoot）；约 t≈0.6 处达峰 ~1.1
        let mut max_v = 0.0f32;
        for i in 0..100 {
            let t = i as f32 / 100.0;
            let v = Ease::BackOut.evaluate(t, 1.0);
            if v > max_v {
                max_v = v;
            }
        }
        assert!(
            max_v > 1.0,
            "BackOut 中段须 >1（overshoot），实达 {}",
            max_v
        );
    }

    #[test]
    fn back_in_undershoots_below_0_early() {
        // BackIn 初段 <0（反向 overshoot）
        let v = Ease::BackIn.evaluate(0.1, 1.0);
        assert!(v < 0.0, "BackIn 初段须 <0，得 {}", v);
    }

    use crate::input::EVT_TWEEN_COMPLETE;
    use crate::scene::node::{Node, NodeKind, Rect};

    fn one_node_scene() -> (Scene, NodeId) {
        // 返 (scene, node_id)——NodeId 由 slotmap 分配（首节点 idx=1, version=1）。
        let mut n = Node::default();
        n.kind = NodeKind::Container;
        n.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        let scene = Scene::from_nodes(vec![n], vec![]);
        let id = scene.roots[0];
        (scene, id)
    }

    #[test]
    fn update_writes_opacity_override_per_tick() {
        let (mut s, nid) = one_node_scene();
        let mut mgr = TweenManager::new();
        mgr.tween(
            nid,
            TweenSpec {
                tag: 42,
                ..spec(TweenProp::Opacity, [0.0; 5], [1.0, 0.0, 0.0, 0.0, 0.0])
            },
        );
        let mut out = Vec::new();
        // dt=0.5 → norm=0.5 → opacity=0.5
        mgr.update(0.5, &mut s, &mut out);
        assert!(
            (s.anim.0.get(&nid).unwrap().opacity.unwrap() - 0.5).abs() < 1e-5,
            "半程 opacity=0.5"
        );
        assert!(out.is_empty(), "未结束 → 无 complete 事件");
    }

    #[test]
    fn update_emits_complete_with_tag_and_prop() {
        let (mut s, nid) = one_node_scene();
        let mut mgr = TweenManager::new();
        mgr.tween(
            nid,
            TweenSpec {
                tag: 7,
                ..spec(
                    TweenProp::Scale,
                    [1.0, 1.0, 0.0, 0.0, 0.0],
                    [2.0, 3.0, 0.0, 0.0, 0.0],
                )
            },
        );
        let mut out = Vec::new();
        mgr.update(1.0, &mut s, &mut out); // 恰好结束
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].event_type, EVT_TWEEN_COMPLETE);
        assert_eq!(
            out[0].click_count,
            TweenProp::Scale as u8,
            "click_count 复用装 prop"
        );
        assert_eq!(out[0].touch_id, 7, "touch_id 复用装 tag");
        let m = s.anim.0.get(&nid).unwrap().transform.unwrap();
        assert!(
            (m[0] - 2.0).abs() < 1e-5 && (m[3] - 3.0).abs() < 1e-5,
            "末值 scale(2,3)"
        );
        let mut out2 = Vec::new();
        mgr.update(1.0, &mut s, &mut out2);
        assert!(out2.is_empty(), "完成后不再产事件");
    }

    #[test]
    fn update_repeat_plays_multiple_cycles() {
        // repeat=2（共 3 轮）× dur=1：t=0.5 → 第 1 轮中点 0.5；t=3.0 才完成。
        let (mut s, nid) = one_node_scene();
        let mut mgr = TweenManager::new();
        mgr.tween(
            nid,
            TweenSpec {
                repeat: 2,
                ..spec(TweenProp::Opacity, [0.0; 5], [1.0, 0.0, 0.0, 0.0, 0.0])
            },
        );
        let mut out = Vec::new();
        mgr.update(0.5, &mut s, &mut out);
        assert!(
            (s.anim.0.get(&nid).unwrap().opacity.unwrap() - 0.5).abs() < 1e-5,
            "第 1 轮中点 0.5"
        );
        mgr.update(2.5, &mut s, &mut out); // tt=3.0 = 完成
        assert_eq!(out.len(), 1, "3 轮跑满产 1 次 complete");
        assert!(
            (s.anim.0.get(&nid).unwrap().opacity.unwrap() - 1.0).abs() < 1e-5,
            "末值 = end"
        );
    }

    #[test]
    fn update_yoyo_alternates_direction() {
        // yoyo repeat=1（共 2 轮）：第 2 轮（奇数轮）end→start。t=1.5（第 2 轮中点）
        // → norm = 1-0.5 = 0.5（线性对称看不出）；用 ease 也不对称——线性下值=0.5。
        // 判据改用边界：t=2.0 完成 → 回到 start（奇数轮终态）。
        let (mut s, nid) = one_node_scene();
        let mut mgr = TweenManager::new();
        mgr.tween(
            nid,
            TweenSpec {
                repeat: 1,
                yoyo: true,
                ..spec(TweenProp::Opacity, [0.0; 5], [1.0, 0.0, 0.0, 0.0, 0.0])
            },
        );
        let mut out = Vec::new();
        mgr.update(2.0, &mut s, &mut out); // tt=2.0 完成
        assert_eq!(out.len(), 1);
        assert!(
            (s.anim.0.get(&nid).unwrap().opacity.unwrap() - 0.0).abs() < 1e-5,
            "yoyo 偶数次轮（repeat=1）末轮奇 → 终态回 start"
        );
        // 对照：非 yoyo repeat=1 完成在 end
        mgr.tween(
            nid,
            TweenSpec {
                repeat: 1,
                ..spec(TweenProp::Opacity, [0.0; 5], [1.0, 0.0, 0.0, 0.0, 0.0])
            },
        );
        let mut out2 = Vec::new();
        mgr.update(2.0, &mut s, &mut out2);
        assert!(
            (s.anim.0.get(&nid).unwrap().opacity.unwrap() - 1.0).abs() < 1e-5,
            "非 yoyo 末态 = end"
        );
    }

    #[test]
    fn update_transform_composes_srt_midway() {
        // Transform 通道半程：TRS 逐分量 lerp 后 SRT 合成。
        // start = T(0,0)·S(1,1)·R(0)（identity），end = T(10,4)·S(2,1)·R(π/2)。
        // n=0.5 → T(5,2)·S(1.5,1)·R(π/4)：矩阵 = [c·sx, s·sx, -s·sy, c·sy, tx, ty]。
        let (mut s, nid) = one_node_scene();
        let mut mgr = TweenManager::new();
        mgr.tween(
            nid,
            spec(
                TweenProp::Transform,
                [0.0, 0.0, 1.0, 1.0, 0.0],
                [10.0, 4.0, 2.0, 1.0, std::f32::consts::FRAC_PI_2],
            ),
        );
        let mut out = Vec::new();
        mgr.update(0.5, &mut s, &mut out);
        let m = s.anim.0.get(&nid).unwrap().transform.unwrap();
        let (sn, cs) = std::f32::consts::FRAC_PI_4.sin_cos();
        assert!((m[0] - cs * 1.5).abs() < 1e-5, "a {m:?}");
        assert!((m[1] - sn * 1.5).abs() < 1e-5, "b {m:?}");
        assert!((m[2] + sn).abs() < 1e-5, "c {m:?}");
        assert!((m[3] - cs).abs() < 1e-5, "d {m:?}");
        assert!(
            (m[4] - 5.0).abs() < 1e-5 && (m[5] - 2.0).abs() < 1e-5,
            "t {m:?}"
        );
        assert!(out.is_empty(), "未结束");
    }

    #[test]
    fn update_delay_gates_apply() {
        let (mut s, nid) = one_node_scene();
        let mut mgr = TweenManager::new();
        mgr.tween(
            nid,
            TweenSpec {
                delay: 1.0,
                ..spec(TweenProp::Opacity, [0.0; 5], [1.0, 0.0, 0.0, 0.0, 0.0])
            },
        );
        let mut out = Vec::new();
        mgr.update(0.5, &mut s, &mut out); // elapsed 0.5 < delay 1 → 不写
        assert!(
            !s.anim.0.contains_key(&nid),
            "delay 内不写 override（HashMap 无条目）"
        );
        assert!(out.is_empty());
        mgr.update(1.0, &mut s, &mut out); // elapsed 1.5，tt=0.5 → norm=0.5
        assert!(
            (s.anim.0.get(&nid).unwrap().opacity.unwrap() - 0.5).abs() < 1e-5,
            "越过 delay 后按 tt 插值"
        );
    }

    #[test]
    fn kill_stops_update_keeps_override() {
        let (mut s, nid) = one_node_scene();
        let mut mgr = TweenManager::new();
        mgr.tween(
            nid,
            spec(TweenProp::Opacity, [0.0; 5], [1.0, 0.0, 0.0, 0.0, 0.0]),
        );
        let mut out = Vec::new();
        mgr.update(0.3, &mut s, &mut out);
        let v = s.anim.0.get(&nid).unwrap().opacity.unwrap();
        mgr.kill(nid, TweenProp::Opacity);
        mgr.update(0.5, &mut s, &mut out); // kill 后不再推进
        assert_eq!(
            s.anim.0.get(&nid).unwrap().opacity.unwrap(),
            v,
            "kill 后 override 保留末值不变"
        );
        assert!(out.is_empty(), "kill 不产 complete");
    }

    #[test]
    fn update_skips_out_of_range_node() {
        let (mut s, _nid) = one_node_scene(); // 仅 1 节点
        let mut mgr = TweenManager::new();
        // 构造一个 index 越界的 NodeId：idx=99（远超 nodes.len()=1+1=2）
        let oob = NodeId(99 | (1 << 32)); // 新位型：index=99, gen=1
        mgr.tween(
            oob,
            spec(TweenProp::Opacity, [0.0; 5], [1.0, 0.0, 0.0, 0.0, 0.0]),
        );
        let mut out = Vec::new();
        mgr.update(1.0, &mut s, &mut out); // index 99 越界 → 跳过
        assert!(out.is_empty(), "越界 node 不产事件");
    }

    #[test]
    fn kill_node_kills_all_tweens_for_node() {
        // 2 节点 scene：nid 的 2 tween + other 的 1 tween。
        let (mut s, nid) = one_node_scene();
        // 加第二节点取其 id（不进 roots/children——仅需要一个 live NodeId 喂 tween + update）。
        let other = {
            let mut n = Node::default();
            n.kind = NodeKind::Container;
            n.layout_rect = Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            };
            let key = s.nodes.insert(n);
            NodeId::from_key(key)
        };
        let mut mgr = TweenManager::new();
        mgr.tween(
            nid,
            spec(TweenProp::Opacity, [0.0; 5], [1.0, 0.0, 0.0, 0.0, 0.0]),
        );
        mgr.tween(
            nid,
            spec(TweenProp::BgColor, [0.0; 5], [1.0, 0.0, 0.0, 0.0, 0.0]),
        );
        mgr.tween(
            other,
            spec(TweenProp::Opacity, [0.0; 5], [1.0, 0.0, 0.0, 0.0, 0.0]),
        );
        mgr.kill_node(nid);
        assert!(
            mgr.active.iter().all(|t| t.node != nid || t.killed),
            "kill_node 杀该 node 全 tween"
        );
        assert!(
            mgr.active.iter().any(|t| t.node == other && !t.killed),
            "其他 node tween 不被误杀"
        );
        // update 后 killed(nid) 的被分流回池；nid 不产 complete（被 kill 不推进）。
        // other 的 tween 会完成（dur=1.0,dt=1.0）→ 产 complete + 回池。
        let mut out = Vec::new();
        mgr.update(1.0, &mut s, &mut out);
        assert!(
            out.iter().all(|e| e.node_id != nid.0),
            "nid killed tween 不产 complete"
        );
        assert!(
            mgr.active.iter().all(|t| t.node != nid),
            "nid killed tween 被清出 active"
        );
    }

    #[test]
    fn completed_tweens_recycle_into_pool_and_reuse() {
        // 池化闭环：完成后槽入 pool；再 spawn 优先复用池槽（active+pool 总量不变）。
        let (mut s, nid) = one_node_scene();
        let mut mgr = TweenManager::new();
        mgr.tween(
            nid,
            spec(TweenProp::Opacity, [0.0; 5], [1.0, 0.0, 0.0, 0.0, 0.0]),
        );
        assert_eq!(mgr.pool.len(), 0);
        let mut out = Vec::new();
        mgr.update(1.0, &mut s, &mut out);
        assert_eq!(mgr.active.len(), 0, "完成后清出 active");
        assert_eq!(mgr.pool.len(), 1, "完成槽回池");
        // 复用：新 spawn 不再分配
        mgr.tween(
            nid,
            TweenSpec {
                tag: 9,
                duration: 2.0,
                ..spec(TweenProp::Opacity, [0.0; 5], [1.0, 0.0, 0.0, 0.0, 0.0])
            },
        );
        assert_eq!(mgr.pool.len(), 0, "spawn 复用池槽");
        assert_eq!(mgr.active.len(), 1);
        assert_eq!(mgr.active[0].tag, 9, "复用槽字段已覆写");
        assert!((mgr.active[0].duration - 2.0).abs() < 1e-6);
        // 活序稳定：三个 tween，中间一个完成，其余两个保序留在 active。
        let mut mgr2 = TweenManager::new();
        mgr2.tween(
            nid,
            TweenSpec {
                tag: 1,
                duration: 5.0,
                ..spec(TweenProp::Opacity, [0.0; 5], [1.0, 0.0, 0.0, 0.0, 0.0])
            },
        );
        mgr2.tween(
            nid,
            TweenSpec {
                tag: 2,
                duration: 1.0,
                ..spec(TweenProp::Scale, [1.0; 5], [2.0, 2.0, 0.0, 0.0, 0.0])
            },
        );
        mgr2.tween(
            nid,
            TweenSpec {
                tag: 3,
                duration: 5.0,
                ..spec(TweenProp::BgColor, [0.0; 5], [1.0, 0.0, 0.0, 0.0, 0.0])
            },
        );
        let mut out2 = Vec::new();
        mgr2.update(1.0, &mut s, &mut out2);
        let tags: Vec<u32> = mgr2.active.iter().map(|t| t.tag).collect();
        assert_eq!(tags, vec![1, 3], "活槽保序（tag2 完成回池）");
    }

    // —— #10 layout / box-shadow 通道 ——

    use crate::style::resolved::BoxShadow;

    fn shadow(
        ox: f32,
        oy: f32,
        blur: f32,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
        inset: bool,
    ) -> BoxShadow {
        BoxShadow {
            ox,
            oy,
            spread: 0.0,
            blur,
            color: [r, g, b, a],
            inset,
        }
    }

    #[test]
    fn lerp_shadow_list_pads_shorter_list_with_transparent_null() {
        // 1 层 → 2 层：浏览器语义（css-backgrounds-3）——短列表末尾补透明零长空阴影，
        // 新增层从透明淡入（非跳现）。t=0.5 时第二层 alpha = 0.5、几何 = 端点一半。
        let a = vec![shadow(0.0, 4.0, 8.0, 0.0, 0.0, 0.0, 1.0, false)];
        let b = vec![
            shadow(0.0, 4.0, 8.0, 0.0, 0.0, 0.0, 1.0, false),
            shadow(0.0, 8.0, 16.0, 1.0, 1.0, 1.0, 1.0, false),
        ];
        let mid = lerp_shadow_list(&a, &b, 0.5);
        assert_eq!(mid.len(), 2);
        assert!((mid[0].oy - 4.0).abs() < 1e-5, "既有层几何不变");
        assert!(
            (mid[1].color[3] - 0.5).abs() < 1e-5,
            "新增层 alpha 从 0 淡入"
        );
        assert!(
            (mid[1].oy - 4.0).abs() < 1e-5,
            "新增层几何半程（端点 8 的中点）"
        );
        assert!((mid[1].blur - 8.0).abs() < 1e-5, "新增层 blur 半程");
    }

    #[test]
    fn lerp_shadow_list_discrete_on_inset_mismatch() {
        // 配对 inset 不匹配 → 整表离散（t<0.5 start / t≥0.5 end）。
        let a = vec![shadow(0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, false)];
        let b = vec![shadow(0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, true)];
        let before = lerp_shadow_list(&a, &b, 0.49);
        assert!(
            !before[0].inset && (before[0].color[0] - 1.0).abs() < 1e-5,
            "t<0.5 取 start"
        );
        let after = lerp_shadow_list(&a, &b, 0.5);
        assert!(
            after[0].inset && (after[0].color[1] - 1.0).abs() < 1e-5,
            "t≥0.5 取 end"
        );
    }

    #[test]
    fn lerp_shadow_list_fades_out_to_empty() {
        // → 空列表（box-shadow:none 端点）：既有层向空阴影插值 = alpha 衰减淡出。
        let a = vec![shadow(0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, false)];
        let mid = lerp_shadow_list(&a, &[], 0.5);
        assert_eq!(mid.len(), 1);
        assert!((mid[0].color[3] - 0.5).abs() < 1e-5, "alpha 半程衰减");
    }

    #[test]
    fn update_writes_width_override_with_domain() {
        // Width tween：载荷 [value, domain_code]（域码 lerp 恒等——同域保证下双端相等）。
        let (mut s, nid) = one_node_scene();
        let mut mgr = TweenManager::new();
        mgr.tween(
            nid,
            TweenSpec {
                start: [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                end: [50.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ..spec(TweenProp::Width, [0.0; 5], [0.0; 5])
            },
        );
        let mut out = Vec::new();
        mgr.update(0.5, &mut s, &mut out);
        let w = s.anim.0.get(&nid).unwrap().width.unwrap();
        assert!((w.value - 25.0).abs() < 1e-5, "半程 width=25");
        assert_eq!(w.domain, crate::scene::LenDomain::Pct, "域码 = 载荷第 2 槽");
    }

    #[test]
    fn update_writes_box_shadow_override() {
        let (mut s, nid) = one_node_scene();
        let mut mgr = TweenManager::new();
        mgr.tween(
            nid,
            TweenSpec {
                shadow: Some(Box::new(ShadowPair {
                    start: vec![shadow(0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, false)],
                    end: vec![shadow(0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, false)],
                })),
                ..spec(TweenProp::BoxShadow, [0.0; 5], [0.0; 5])
            },
        );
        let mut out = Vec::new();
        mgr.update(0.5, &mut s, &mut out);
        let list = s.anim.0.get(&nid).unwrap().box_shadow.clone().unwrap();
        assert_eq!(list.len(), 1);
        assert!((list[0].color[3] - 0.5).abs() < 1e-5, "alpha 半程");
    }

    #[test]
    fn tween_rejects_boxshadow_without_payload() {
        let (mut s, nid) = one_node_scene();
        let mut mgr = TweenManager::new();
        mgr.tween(
            nid,
            spec(TweenProp::BoxShadow, [0.0; 5], [1.0, 0.0, 0.0, 0.0, 0.0]),
        );
        assert!(mgr.active.is_empty(), "BoxShadow 缺列表载荷 = 无效提交拒收");
        let mut out = Vec::new();
        mgr.update(1.0, &mut s, &mut out);
        assert!(out.is_empty() && !s.anim.0.contains_key(&nid));
    }
}
