# Showcase 内容完善 · 设计

> 日期：2026-07-04
> 目的：showcase 多个演示卡内容太简陋（空容器/无按钮/反馈太弱/逻辑不清），浏览器预览照出了问题。本轮重做 §5 滚动、§7 动效(7.3/7.5)、亮灯机制(全局)、§4.7 路由，让每个案例"看得见内容、有明确交互、能判断对错"。
> 模式：**HTML + C# `LoomShowcaseDriver` + 预览 JS（`loom-preview.js`）三者同步改**——showcase 是包源，改 HTML 必同步 C# driver 订阅 + JS 预览逻辑，保证 Unity PlayMode 和浏览器预览表现一致。

## 0. 全局原则

1. **每个滚动容器必须有可见内容**（文字/图片/色块）——不能是空 filler。让用户一眼看出"这里有东西在滚"。
2. **凡加按钮或改交互逻辑的，C# `LoomShowcaseDriver` 必须同步改订阅**（HTML 加 id → C# `AddPageListener`/`PlayProp`/`SetScrollPos` 跟着加）。
3. **预览 JS 与 C# 行为对齐**——同 id、同触发逻辑、同反馈。浏览器不能演示的（惯性物理、合成滚动条、NativeHost）明确标注"Unity 限定"，不假装。
4. **亮灯反馈要明显**——点亮保持 + 计数，不闪一下回灰。

## 1. §5 滚动实验室重做（page_scroll.html）

每卡放可见内容，5.6 补真按钮。

| 卡 | 容器内容 | 演示 | C#/JS 改动 |
|---|---|---|---|
| **5.1 overflow 模式** | 三个 mini 框各放多行文字（"第1行"…"第8行"，行高 24px，总 ~200px > 120px 视口）。`scroll`/`auto`/`hidden` 三种 | overflow 三模式：scroll/auto 能滚看完 8 行，hidden 截断只能看前 5 行 | 纯 HTML（CSS `.filler` 改成带文字的多行块或加 `<span>` 子节点）。无 driver 逻辑 |
| **5.2 overflow-x/y** | 横滚条放 4 个带图标+标签的 item（🏠主页/📷图片/📝文本/⚙设置，每个 160px，总 640px > 视口） | 水平滚动（shift+滚轮或拖滚动条） | 纯 HTML。无 driver |
| **5.3 惯性/回弹** | 保持说明卡，文字明确"拖本页/滚轮体验"。加注"Unity 有惯性+边界回弹，浏览器原生滚动无" | 惯性物理（**Unity 限定**，预览不演） | 纯 HTML 文案 |
| **5.4 滚动条 + grip** | mini 框放带标签的色块序列（红/橙/青/绿/紫 5 块，每块 60px + 颜色名标签，总 300px > 120px）。拖右侧滚动条定位到不同色块 | 滚动条 thumb 可拖（浏览器原生滚动条 ≠ LoomGUI 合成 thumb，标注差异） | 纯 HTML |
| **5.5 嵌套+轴锁** | 内层竖滚框放：顶部横滚条(4 item)、中间 `drag me` 可拖块、底部多行文字 | 嵌套滚 + 轴锁 + drag 仲裁（拖 drag 块 item 跟动是 C# `OnDragHit` 演示，预览不演 item 跟动） | HTML；C# 已有 `OnDragHit` 不改；JS 不绑 item 跟动 |
| **5.6 SetScrollPos** | **加 3 按钮**：`跳顶`/`跳中`/`跳底`（id `scroll-top`/`scroll-mid`/`scroll-bottom`）。容器就是外层 `#page-scroll`（内容为 §5 各卡），跳转后能看见滚到哪 | 程序控制滚动定位 | **HTML 加 3 按钮**；**C# `SubscribeScroll` 订阅 3 click → 调 `_stage.SetScrollPos(#page-scroll, y)**（y=0/中/底，底=contentHeight-viewportHeight）；**JS 绑 3 click → `#page-scroll`.scrollTop = y`** |

### 5.6 C# 细节
```csharp
void SubscribeScroll() {
  SubscribeBackHome();
  uint pageScroll = _stage.FindNodeById("page-scroll");
  AddPageListener(_stage.FindNodeById("scroll-top"), EventType.Click,
    _ => _stage.SetScrollPos(pageScroll, 0));
  // 中/底需要 contentHeight：GetNodeLayoutRect 或硬编码（本页内容固定）。
  // 实现时查 LoomStage API 确定 SetScrollPos 签名 + 取 contentSize 的法子。
  ...
}
```

## 2. §7 动效 7.3 / 7.5（page_tween.html + LoomShowcaseDriver）

### 2.1 修 7.3 delay 错峰（预览 bug）
**根因**：JS 里 `d-0/1/2` 初始 inline `style.opacity='0'`，点 `delay-play` toggle `.play` 类 → CSS `#d-0.play{opacity:1}`。但 **inline style 优先级 > 类选择器**，opacity:1 加不上，块一直隐着。C# 侧 Rust tween delay 正确，不动。

**改法**：JS 不靠 CSS 类对撞。改用 JS 直接操作 style + setTimeout 实现错峰：
- 点 `delay-play`：先 3 块设 `opacity:0`，再用 `setTimeout` 按 0/200/400ms 依次把每块 `style.opacity='1'`（带 `transition: opacity .5s`）。
- 再点：3 块依次回 `opacity:0`（toggle 方向）。
- C# 不动（Rust tween delay 已对）。

### 2.2 7.5 kill/clear 加播放按钮（HTML + C# 同步）
**根因**：`kill-target` 进页就自动旋转（C# `SubscribeTween` 里 `PlayProp(kill-target, rotation, 0→360, 4s, loop)`），`kill-btn`/`clear-btn` 是控制按钮（停/重置）不是播放。无明确播放入口。

**改法**：加 `play-kill-target` 按钮，C# 改成"点播放才转"：
- HTML：在 kill-target 下加 `<div class="hit" id="play-kill-target">▶ 播放</div>`。
- C# `SubscribeTween`：**删掉**进页即 `PlayProp(kill-target, rotation loop)`；改为订阅 `play-kill-target` click → `PlayProp(kill-target, rotation, 0→360, 4s, Linear, loop)`。
- C# `OnKill`/`OnClear` 不变（kill-btn 停、clear-btn 清）。
- JS 同步：`play-kill-target` click → 给 kill-target 加 spin 动画；kill-btn pause；clear-btn 清。

## 3. 亮灯机制全局改（HTML + C# `LightLamp` + JS `pulseLamp`）

**根因**：现 `LightLamp`(C#)/`pulseLamp`(JS) 都是 opacity 1→0.3→1 闪 0.2s，14×14 灰块太弱。

**新机制——点亮保持 + 计数**：
每个 `lamp-{name}` 容器有 N 盏灯子块。触发一次：
1. 点亮**下一盏未亮的灯**（`background-color:#5fb2c4` 青，保持不熄）。
2. 旁边 `<span id="count-{name}">` 计数 +1 显示"已触发 X 次"。
3. 全亮后再触发 → 全灭重置重新数（循环）。

**视觉效果**：青灯一盏盏亮 + 数字涨——清楚看见触了几次。

**改动**：
- **HTML**：每个有灯的卡（7.4 `lamp-complete`、4.1-4.6 `lamp-click/hover/drag/longpress/key`、4.7 三组灯）旁加 `<span class="lamp-count" id="count-{name}">0</span>`。灯块已有不动。
- **C# `LightLamp(string name, int count)`**：改逻辑——找 `lamp-{name}` 的子节点，点亮第 `count` 盏（设 background-color），更新 `count-{name}` 文本。
  - **待查**：`LoomStage` 有无遍历子节点的 API（之前注释提过"无 get_children"）。若没有 → 降级方案：`LightLamp` 改为给整个 `lamp-{name}` 容器设 background 渐变青 + 更新计数（整容器变色，不按盏）。实现时查 LoomStage API 确定。
- **JS `pulseLamp(name)`**：DOM 子节点好遍历，直接点亮下一盏 + 更新计数 span。改名为 `lightNext(name)` 更准确。
- **C# 各 `OnClickHit` 等**：现有 `++_clickCount` 传给 `LightLamp`，逻辑不动（LightLamp 内部改）。

## 4. §4.7 路由 + pointer-events（page_interact.html + C# + JS）

**根因**：inner/outer 点击都只亮同一盏 `route` 灯，看不出 StopPropagation 效果。

**改法——反馈分开 + 穿透可视**：

| 元素 | 改法 | C# 回调 |
|---|---|---|
| `route-outer` | 点击 → 亮 `lamp-outer` + 计数 | `OnRouteOuter`（不动逻辑，改亮 lamp-outer） |
| `route-inner` | 点击 → 亮 `lamp-inner` + 计数 + `StopPropagation`。点 inner 只 inner 亮，outer 不亮 | `OnRouteInner`（已 stopProp，改亮 lamp-inner） |
| `route-pe-under`（下层可点块，新） | 点击 → 亮 `lamp-pe` + 计数 | `OnRoutePeUnder`（新，原 `OnRoutePe` 改名） |
| `pe-none`（上层，`position:absolute` 盖住 route-pe-under，pointer-events:none） | 点它穿透到下层 route-pe-under → 下层亮 | 无订阅（pointer-events:none 不命中） |

**HTML 改动**：
- 4.7 灯组从 `lamp-route`(3 盏) 改成三组独立：`lamp-outer`(3) + `lamp-inner`(3) + `lamp-pe`(3)，各带计数 span。
- `route-pe` 块改名 `route-pe-under`，加 `position:relative`。
- 新增 `pe-none` 块 `position:absolute;inset:0` 盖住 route-pe-under（v1.4-b absolute 支持）。

**C# `SubscribeInteract`**：
- `SubscribeLamp("route-outer", Click, OnRouteOuter)` 不变（回调内改亮 lamp-outer）。
- `SubscribeLamp("route-inner", Click, OnRouteInner)` 不变（亮 lamp-inner）。
- `SubscribeLamp("route-pe", ...)` → 改 `route-pe-under`，回调 `OnRoutePeUnder` 亮 lamp-pe。
- 删 `SubscribeLamp("route-pe", ...)` 旧订阅（id 变了）。

## 5. 实施顺序（给 writing-plans 的输入）

1. **§5 page_scroll.html 重做**（5.1-5.6 内容，纯 HTML 为主 + 5.6 加按钮 id）。
2. **§5.6 driver 同步**：C# `SubscribeScroll` 订阅 3 按钮 + JS 绑 3 按钮。
3. **§7.3 修预览 bug**：JS `delay-play` 重写（不靠 CSS 类）。
4. **§7.5 HTML + C# + JS**：加 `play-kill-target` 按钮，C# 改订阅逻辑，JS 同步。
5. **亮灯机制**：HTML 加计数 span（多页），C# `LightLamp` 改逻辑（先查 LoomStage 子节点 API），JS `pulseLamp`→`lightNext` 改逻辑。
6. **§4.7**：HTML 改灯组+pe 块，C# 订阅+回调改名，JS 同步。
7. **重打 pkg.bin**（parse-time 改 HTML 必重打，坑 66）——本机 build pkg，家里机 pull 后 Unity 验收。
8. **浏览器全页走查 + Unity PlayMode 验收（家里机）**。

## 6. 待查/风险

- **LoomStage 子节点遍历 API**：亮灯机制 §3 依赖。若无 → 降级整容器变色。实现时先查。
- **`SetScrollPos` FFI 签名**：5.6 依赖。实现时查 `LoomStage.cs` 确认签名 + 取 contentSize 法子。
- **`position:absolute` 嵌套**：4.7 pe-none 叠加依赖 v1.4-b absolute 支持（已确认围栏内）。
- **重打 pkg.bin**：HTML 改了必须重打（坑 66），家里机 Unity 才能看到新 showcase。
