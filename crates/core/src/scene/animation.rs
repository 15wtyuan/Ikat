//! @keyframes 动画类型定义（pkg 序列化 + runtime 共用）。
//!
//! 数据流：fence 解析 `@keyframes`/`animation`（pack-time）→ 打包器转成这里的类型
//! → pkg.bin v30 序列化 → instantiate 时组件 keyframes 合并进 `Scene.keyframes` 全局表
//! （CSS `@keyframes` 全局语义，spec §3.5）→ KeyframePlayer 按 `AnimationSpec.name` 查表驱动。
//!
//! 类型划分：pkg 层（KeyframesRule 表）+ ResolvedStyle 层（AnimationSpec，在
//! `style/resolved.rs`，bincode 序列化）+ runtime 层（KeyframePlayer 占位，T5 填逻辑）。

use serde::{Deserialize, Serialize};

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

/// 一个进行中的 @keyframes player（Scene.players 条目）。
///
/// **占位空 struct**：字段定义 + update 时间轴逻辑在后续 task 填入（T5）；本 task 只需
/// 让 `Scene.players` 的类型存在并编译。spec §4.2 定义了完整字段
/// （node/spec/keyframes/current_time/elapsed/iteration/play_state/on_key_percents 等）。
/// visibility 为 pub（spec §4.2 写 pub(crate)，但 §4.3 的 `Scene.players` 是 pub 字段，
/// clippy -D warnings 下更私有的类型会报 private_interfaces——取 §4.3 并放宽类型可见性；
/// 玩家结构仍是 core 内部实现细节，C# 只见 PlayerKey 句柄）。
#[derive(Debug, Clone)]
pub struct KeyframePlayer;
