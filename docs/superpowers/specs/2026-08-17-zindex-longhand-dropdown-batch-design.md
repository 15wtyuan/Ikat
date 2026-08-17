# z-index / animation 长划 / dropdown 视口翻转 批设计（2026-08-17）

状态：DONE（同日实现）。

## 背景

公共底座 NE 批（调度/生命周期/option）收口后，本轮选三件：z-index（主）、
animation 长划子属性 + dropdown popup 视口感知定位（配）。z-index 的进入判据
（T1 视觉层推进）由卡牌 dogfood 提前触发预判：hover 盖牌 / 拖拽 ghost / tooltip
全是 z-index 经典场景。

## 决策

### z-index：兄弟间绘制/命中序，子树整体移动

- **语义**：`z-index: <integer>`（默认 0，负数合法）只改**同级兄弟间的绘制与
  命中顺序**——同级按 (z_index 升, DOM 序) 绘制，z 大者盖上；子树整体移动
  （父的 z 决定整棵子树在哪一层，子树内部再按自身 z 排）。**不改 flex 排列**
  （那是 `order` 的职责，两者正交）。
- **不做嵌套 stacking context**（CSS 的 positioned/z-index 上下文树）。每个
  父节点天然是排序边界。模型与「AI 能预测渲染」一致：兄弟排序可局部推理。
- **存储**：`ResolvedStyle.z_index: i32`（紧邻 `order`）→ base_style 进 pkg，
  **格式 v38**。
- **运行时 inline**：InlineSet 位图 u32 → **u64**（bits 0-31 原样，z-index =
  bit 32）。InlineSet 是 runtime transient 不进 pkg，升级零格式影响。C# 走现成
  `set_inline_override` 字符串通道（`"z-index:5"`），**无新 FFI**；getter 走
  mirror round-trip（与 Width 同约定）。
- **生效点三处**（必须同步，hit「逆等效绘制序」不变量）：
  1. render DFS 子迭代（batch.rs）：children 稳定排 z（等 z 保 DOM 序）；
  2. open popup 末尾追加循环（render/mod.rs）：同排序后逆序入栈；
  3. hit `effective_draw_order`：反转 + 稳定排 `-order` 后再稳定排 `-z`
    （z 主键；等 z 时保持既有 order 行为）。
  z 全 0 时三处逐位等于改动前行为。`reorder_for_batching` 按 sort_key 稳定
  分组重编号，不破坏 DFS 相对序。
- **fence**：入 CSS_PROPS（Integer parser），撤 `unsupported_hint` 的 z-index
  引导文案（围栏内合法）。`auto` 不收（整数值域，缺失即默认 0）。

### animation-* 长划（8 个）：广播写既有 spec

- `animation-name/duration/timing-function/delay/iteration-count/direction/
  fill-mode/play-state`，**单值**（逗号列表不收，fence 报错引导用简写）。
- 语义：写入 `style.animation` 的**全部既有 spec**（简写先声明、长划改字段）；
  无既有 spec 时创建一条默认 spec（name 空 = 不播放，`animation-name` 到位才
  启动——与 CSS「无 name 不播」一致）。简写在长划后出现则整体替换。
- 无格式变化（落进既有 `AnimationSpec`）。fence 校验委托 core 解析器（同一
  真相源，同 `parse_animation` 的 fence 委托模式）。

### dropdown popup 视口感知定位

- solve 后置钩子（每帧、廉价）：对每个 open dropdown，取 select/popup 上帧
  solve 的 layout_rect——下方放得下 → 不动（作者 CSS `top:100%`）；下方放不下
  且上方放得下 → inline `top:-(popup.h)px` 上翻；两向都放不下 → 钉到视口底
  （top = viewport_h - select.y - popup.h）+ `max-height` 收缩 + `overflow-y:
  auto`（接既有滚动机制）。收起时 unset 覆写回落作者 CSS。
- 上翻在 open 后第 2 帧可见（覆写需再 solve）：错位帧的几何在视口外，实际
  不可见；收缩场景半可见帧可接受，真机反馈再议。
- 每帧重估覆盖祖先滚动 / 视口 resize 下的 select 位移。

## 交付

core（z_index 字段/arm/u64 位图/三处序/翻转钩子 + 测试）、fence（z-index +
8 长划 + fence.md/skill 副本 + 测试）、pkg v38（fixtures 重打）、C#
（NodeStyle.ZIndex 接 mirror + headless 测试）、showcase（lab z-index 标本 +
m2 长划标本 + form 底部 dropdown 翻转场景）、dll/bindings/GUI exe 同步链。

## 风险与边界

- z-index 与 flex `order` 同时使用：render 按 (z, DOM) 绘制（CSS 正确——
  flex order 不改绘制序）；hit 的 order 近似排序保留为次键（既有行为，本批
  不动，登记在案）。
- `auto` 值拒收是围栏收紧（CSS 有 auto），报错文案引导省略（默认即 0）。
