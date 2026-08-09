//! 防漂移门：schema 表的 CSS 属性 default 值经 core `apply_decl` 解析后，
//! 必须与 `ResolvedStyle::default()` 一致。
//!
//! 单一真相源本应是一处，但 schema（`CSS_PROPS`，fence）与解析产物默认态
//! （`ResolvedStyle::default()`，core）各自硬编码，存在双向漂移风险：改了
//! schema default 没同步 resolved.rs（或反之）会让两边描述的「CSS 初始值」
//! 不再一致。本测试是这层弱保证：对每个 CSS_PROP，在空 ResolvedStyle 上 apply
//! 它的 default 值，结果应仍等于全 default 态（因为 CSS 初始值应用到默认态应保持
//! 默认态）。
//!
//! 锁的强度：
//! - 主断言对每个**非例外**属性做整结构体相等（强锁）。
//! - 已知「表示不同但语义等价」的属性进 skip 列表，逐条标注根因（见
//!   `REPRESENTATION_DIFF_PROPS`）。改 schema default 使之不再落入该语义 →
//!   测试红，强制 review。
//! - apply_decl 不识别其 default 值的属性（如 `resize`，core 本就 noop）进
//!   `UNCONSUMED_DEFAULT_PROPS`，集合被锁——新增此类属性须显式登记，
//!   避免悄悄累积「schema 接受但 core 丢弃」的缺口。
//! - 关键属性（background/border/overflow/color/opacity 等）另做单字段精确断言，
//!   作为 skip 推理错误的第二道防线。

use loomgui_core::style::mapping::apply_decl;
use loomgui_core::style::resolved::{BorderStyle, OverflowMode, ResolvedStyle, TextAlign};
use loomgui_fence::schema::css::CSS_PROPS;

/// 已知「表示不同但语义等价」的属性：apply 其 schema default 后结构体 ≠ 全 default，
/// 但差异是表示形式（Some-vs-None / 级联标记 / 惰性 spec），不是语义漂移。
///
/// 逐条根因：
/// - `display`：schema default `block`，apply 置 Block；`ResolvedStyle::default` 是
///   Flex（= taffy 自身 DEFAULT）。这是**有意为之**——默认态仅对手搓测试 fixture
///   有意义，真实节点 display 由 tag 的 `DisplayDefault`（打包期 css_resolve）或
///   `default_display_for_kind`（运行期 create_node）覆盖（见 resolved.rs 注释）。
///   换言之 schema default `block` 描述的是 DOM 默认行为，resolved 默认态是
///   taffy 对齐值，二者服务的对象不同。
/// - `justify-content`/`align-items`/`align-content`/`align-self`：apply 显式值产
///   `Some(...)`，而 `ResolvedStyle::default` 的对应 taffy 字段是 `None`（= auto）。
///   taffy 的 `None`（auto）在 flex 容器里解析为 CSS 初始值（flex-start / stretch /
///   auto），与 schema default 语义等价，仅表示形式不同。
/// - `font-family`：schema default `inherit`（级联标记），apply 置 `Some("inherit")`；
///   resolved 默认 `None` = 未声明 → 走系统默认 / 经 inherited_set bitmask 继承。
///   schema 的 "inherit" 是级联指令，resolved 的 None 是「未设」标记，二者不对应到
///   同一 apply 路径——属表示差异（注：若某路径真把 "inherit" 当字面字体名消费会是
///   bug，但当前无此消费方；见报告顾虑）。
/// - `transition`：schema default `none`，apply 产 1 条惰性 TransitionSpec
///   （prop=None/duration=0），resolved 默认是空 Vec。duration=0 的 spec 是行为
///   no-op（瞬完），语义等价于「无过渡」。表示差异，非语义漂移。
const REPRESENTATION_DIFF_PROPS: &[&str] = &[
    "display",
    "justify-content",
    "align-items",
    "align-content",
    "align-self",
    "font-family",
    "transition",
];

/// apply_decl 不消费其 schema default 值的属性（apply 返 false，结构体不变）。
/// 分两类（均非遗漏 default 漂移）：
/// - **合理的 noop default**：default 本就是「无」语义，apply 不存值但字段默认态已
///   正确（None / 空）：
///   - `border-image-slice`/`background-image`/`text-shadow`/
///     `-webkit-text-stroke`/`font-effect`：default `none`/`0 transparent` → None，
///     与 resolved 默认 None 一致。
///   - `box-shadow` default `none` 现由 core apply_decl 正确消费（apply=true，
///     清空 Vec == resolved 默认），走主断言，不在此列表。
///   - `animation`/`resize`：fence 注册并做语法校验。`animation` 已由 apply_decl
///     "animation" arm 消费（M2：class 规则 → computed style → sync_animation_players，
///     default `none` → 空 Vec == resolved 默认，走主断言）；`resize` 仍不消费。
/// - **apply 完整性缺口（独立 bug，非 default 漂移，见报告顾虑）**：default 值本应
///   被 apply 识别却返 false：
///   - `background-size`：schema 广告 `stretch` 为合法值，但 apply 仅认 `100%`，
///     `stretch` 被静默拒（schema 接受 + core 丢弃的反模式）。
///   - margin-top/right/bottom/left 已补单边 longhand 臂（与 padding 对称），不再在此列表。
const UNCONSUMED_DEFAULT_PROPS: &[&str] = &[
    "border-image-slice",
    "background-image",
    "background-size",
    "resize",
    "text-shadow",
    "-webkit-text-stroke",
    "font-effect",
];

/// 对每个非例外 CSS_PROP：apply 其 default 值到空 ResolvedStyle，断言结果仍 == 全
/// default（CSS 初始值不改默认态）。例外（表示差异）与未被消费的 default 各自
/// 进独立锁集合，防止新增属性悄悄漂移而无 review。
#[test]
fn schema_default_applied_matches_resolved_default() {
    for spec in CSS_PROPS {
        let mut s = ResolvedStyle::default();
        let applied = apply_decl(&mut s, spec.name, spec.default);

        let is_rep_diff = REPRESENTATION_DIFF_PROPS.contains(&spec.name);
        let is_unconsumed = UNCONSUMED_DEFAULT_PROPS.contains(&spec.name);

        if !applied {
            // apply 返 false → 结构体未变，必 == default。但它必须登记在
            // UNCONSUMED_DEFAULT_PROPS（否则是未经 review 的新缺口）。
            assert!(
                is_unconsumed,
                "`{}` default `{}` 未被 apply_decl 消费（apply=false），但未登记在 \
                 UNCONSUMED_DEFAULT_PROPS——要么补 apply_decl，要么显式登记并注明原因",
                spec.name, spec.default,
            );
            // 结构体未变，必然 == default，无需再断言。
            continue;
        }

        if is_rep_diff {
            // 已知表示差异：apply 返 true 但结构体 ≠ default。这里不做 == 断言
            // （会假阳性失败），仅验证「apply 不 panic + 被识别」——真正的锁是
            // 该属性必须出现在 REPRESENTATION_DIFF_PROPS（否则下面 else 分支的
            // == 断言会捕获它）。
            continue;
        }

        // 非例外且 apply 成功：结构体必须仍 == 全 default。
        assert_eq!(
            s,
            ResolvedStyle::default(),
            "`{}`: apply schema default `{}` 后结构体漂离 ResolvedStyle::default()。\
             若是表示差异（如 Some-vs-None），加入 REPRESENTATION_DIFF_PROPS 并注明\
             根因；若是真语义漂移，对齐 schema 或 resolved.rs。",
            spec.name,
            spec.default,
        );
    }

    // 反向锁：两个例外集合本身须稳定——改它们要过 review，避免「为了让测试绿而
    // 把新缺口塞进 skip」的偷懒路径。集合成员变动 = 显式 diff = 强制阅读注释。
    assert_no_unlisted_prop(REPRESENTATION_DIFF_PROPS, "REPRESENTATION_DIFF_PROPS");
    assert_no_unlisted_prop(UNCONSUMED_DEFAULT_PROPS, "UNCONSUMED_DEFAULT_PROPS");
    // 两集合互斥：rep_diff 要求 apply=true，unconsumed 要求 apply=false，一个属性
    // 不可能同时满足——若两集合都含某属性说明分类自相矛盾。
    let overlap: Vec<&&str> = REPRESENTATION_DIFF_PROPS
        .iter()
        .filter(|n| UNCONSUMED_DEFAULT_PROPS.contains(n))
        .collect();
    assert!(
        overlap.is_empty(),
        "属性同时出现在两个 skip 集合（互斥分类自相矛盾）: {overlap:?}"
    );
}

/// 关键属性精确字段断言（第二道防线）。
///
/// 主循环只检查整结构体 == default（不透明——不知是哪个字段对上了）。这里对一组
/// 有明确 default、字段可独立比对的属性做单字段精确断言，把「default 值解析到正确
/// 字段」的意图文档化。任一 schema default 被改成语义不同的值（如 background-color
/// 默认从 transparent 改成 red）会被这里直接抓到，且失败信息直指具体字段。
#[test]
fn key_prop_defaults_resolve_to_expected_fields() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "background-color", "transparent"));
    assert_eq!(
        s.background_color, None,
        "background-color transparent → None"
    );

    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "border-color", "transparent"));
    assert_eq!(s.border_color, None, "border-color transparent → None");

    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "border-style", "none"));
    assert_eq!(
        s.border_style,
        BorderStyle::None,
        "border-style none → None"
    );

    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "overflow-x", "visible"));
    assert_eq!(s.overflow_x, OverflowMode::Visible);

    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "overflow-y", "visible"));
    assert_eq!(s.overflow_y, OverflowMode::Visible);

    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "color", "#000000"));
    assert_eq!(s.color, [0.0, 0.0, 0.0, 1.0], "color #000000 → 不透明黑");

    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "opacity", "1"));
    assert_eq!(s.opacity, 1.0);

    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "font-size", "16px"));
    assert_eq!(s.font_size, 16.0);

    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "font-weight", "400"));
    assert_eq!(s.font_weight, 400);

    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "text-align", "left"));
    assert_eq!(s.text_align, TextAlign::Left);

    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "pointer-events", "auto"));
    assert!(s.touchable, "pointer-events auto → touchable true");

    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "transform", "none"));
    assert!(s.transform.is_identity(), "transform none → identity");

    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "filter", "none"));
    assert_eq!(s.color_filter, None, "filter none → None");

    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "flex-wrap", "nowrap"));
    assert_eq!(s.taffy_style.flex_wrap, taffy::FlexWrap::NoWrap);

    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "flex-grow", "0"));
    assert_eq!(s.taffy_style.flex_grow, 0.0);
}

/// 验证：skip 集合里每个属性都真实存在于 CSS_PROPS（防止拼写错 / 删属性后 skip
/// 残留），且 apply 其 default 确实不满足「== default 且 consumed」。
#[test]
fn skip_lists_are_accurate() {
    for &name in REPRESENTATION_DIFF_PROPS {
        let spec = find_prop_or_panic(name);
        let mut s = ResolvedStyle::default();
        let applied = apply_decl(&mut s, name, spec.default);
        assert!(
            applied,
            "`{}` 在 REPRESENTATION_DIFF_PROPS 但 apply 其 default 返 false——\
             应移到 UNCONSUMED_DEFAULT_PROPS",
            name,
        );
        assert_ne!(
            s,
            ResolvedStyle::default(),
            "`{}` 在 REPRESENTATION_DIFF_PROPS（预期 != default）但实际 == default——\
             可从 skip 移除，让它走主断言",
            name,
        );
    }
    for &name in UNCONSUMED_DEFAULT_PROPS {
        let spec = find_prop_or_panic(name);
        let mut s = ResolvedStyle::default();
        let applied = apply_decl(&mut s, name, spec.default);
        assert!(
            !applied,
            "`{}` 在 UNCONSUMED_DEFAULT_PROPS（预期 apply=false）但实际 apply=true——\
             分类过时，需更新",
            name,
        );
        assert_eq!(
            s,
            ResolvedStyle::default(),
            "`{}` apply=false 但结构体被改动（不应发生）",
            name,
        );
    }
}

fn find_prop_or_panic(name: &str) -> &'static loomgui_fence::schema::css::CssPropSpec {
    CSS_PROPS
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("`{}` 不在 CSS_PROPS", name))
}

/// 反向锁：skip 集合不得含未注册的属性名（防拼写错 / 删属性后 skip 残留为僵尸条目）。
fn assert_no_unlisted_prop(skip: &[&str], set_name: &str) {
    for &name in skip {
        assert!(
            CSS_PROPS.iter().any(|p| p.name == name),
            "`{}` 在 {set_name} 但不在 CSS_PROPS（拼写错或属性已删，清理 skip）",
            name,
        );
    }
}
