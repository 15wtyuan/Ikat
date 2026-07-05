# Showcase 内容完善 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 重做 showcase 的 §5 滚动、§7 动效(7.3/7.5)、亮灯机制(全局)、§4.7 路由，让每个演示卡"看得见内容、有明确交互、能判断对错"。

**Architecture:** HTML + C# `LoomShowcaseDriver` + 预览 JS（`loom-preview.js`）三者同步改。每卡放可见内容、加缺失按钮、强化反馈。亮灯改"id-addressable 点亮保持+计数"（无需子节点遍历 API——每个灯编号 id，`FindNodeById`+`SetStyle` 定位）。

**Tech Stack:** showcase HTML/CSS、C#（`LoomShowcaseDriver.cs`，纯 C# 无 .dll 重编）、JS（`loom-preview.js`，浏览器预览）、Rust pkg 打包器（重打 showcase.pkg.bin）。

## Global Constraints

- **HTML 改动必须重打 pkg.bin**（坑 66：parse-time 改 HTML 只重编 .dll 不够，须 `loomgui_pkg.exe` 重打）。T5 专门做。
- **C# 改动是纯 C#，无需 .dll 重编**（`LoomShowcaseDriver` 不碰 FFI）。
- **两机工作流**：本机改 HTML+C#+JS、重打 pkg.bin、commit；家里机 pull 后 Unity PlayMode 验收。
- **三者同步**：凡加按钮/改交互的，HTML 加 id → C# `LoomShowcaseDriver` 订阅 → JS `loom-preview.js` 绑定，三处缺一不可。
- **预览与 Unity 对齐**：同 id、同触发逻辑、同反馈。浏览器不能演示的（惯性物理、合成滚动条、NativeHost）标注"Unity 限定"。
- **围栏**：只用 div/span/img/button；CSS 仅围栏内属性（`position:absolute` v1.4-b 起支持）。
- **路径基准**：worktree = `E:/workspace/LoomGUI/.claude/worktrees/showcase-browser-preview`。showcase HTML 在 `loomgui_unity/Assets/LoomUI/showcase/`，预览 JS 在 `loomgui_unity/Assets/LoomUI/preview/`，C# 在 `loomgui_unity/Assets/LoomGUI/Runtime/`。
- **commit 中文**（项目惯例）。

---

## File Structure

| 文件 | 改动 | 涉及 task |
|---|---|---|
| `loomgui_unity/Assets/LoomUI/showcase/page_scroll.html` | §5.1-5.6 内容重做（可见内容 + 5.6 三按钮） | T2 |
| `loomgui_unity/Assets/LoomUI/showcase/page_tween.html` | 7.4 灯加 id+计数；7.5 加播放按钮 | T1, T3 |
| `loomgui_unity/Assets/LoomUI/showcase/page_interact.html` | 4.1-4.6 灯加 id+计数；4.7 灯组+pe 重构 | T1, T4 |
| `loomgui_unity/Assets/LoomGUI/Runtime/LoomShowcaseDriver.cs` | `LightLamp` 重写；`SubscribeScroll`/`SubscribeTween`/`SubscribeInteract` 改 | T1, T2, T3, T4 |
| `loomgui_unity/Assets/LoomUI/preview/loom-preview.js` | `pulseLamp`→`lightNext`；scroll/tween/interact handler 改 | T1, T2, T3, T4 |
| `loomgui_unity/Assets/StreamingAssets/showcase.pkg.bin` | 重打（build 产物） | T5 |

---

### Task 1: 亮灯机制改"id-addressable 点亮保持+计数"（7.4 + 4.1-4.6，不含 4.7）

**Files:**
- Modify: `loomgui_unity/Assets/LoomUI/showcase/page_tween.html`（7.4 lamp-complete）
- Modify: `loomgui_unity/Assets/LoomUI/showcase/page_interact.html`（4.1-4.6 lamp-click/hover/drag/longpress/key）
- Modify: `loomgui_unity/Assets/LoomGUI/Runtime/LoomShowcaseDriver.cs`（`LightLamp` 重写）
- Modify: `loomgui_unity/Assets/LoomUI/preview/loom-preview.js`（`pulseLamp`→`lightNext`）

**Interfaces:**
- Produces: `LightLamp(string name, int totalCount)`（C#）——点亮 `lamp-{name}-{lit}` 第 lit 盏（id-addressable），`count-{name}` 显示 totalCount。`_lampLit` dict 跟踪当前亮数（0..N 循环重置）。
- Produces: `lightNext(name)`（JS）——DOM 同逻辑。
- 灯组数量表（C# `LampCount` + JS `LAMP_COUNT`）：`click:6, hover:4, drag:4, key:8, complete:3, longpress:3`（4.7 的 outer/inner/pe:3 在 T4 加）。

- [ ] **Step 1: page_tween.html 7.4 灯加 id + 计数 span**

Edit `page_tween.html`，替换 7.4 卡的灯容器（`<div class="lamps" id="lamp-complete">...`）：
- old:
```
            <div class="lamps" id="lamp-complete">
              <div class="lamp"></div><div class="lamp"></div><div class="lamp"></div>
            </div>
```
- new:
```
            <div class="lamps">
              <div class="lamp" id="lamp-complete-0"></div>
              <div class="lamp" id="lamp-complete-1"></div>
              <div class="lamp" id="lamp-complete-2"></div>
            </div>
            <span class="card-x">已触发 <span id="count-complete">0</span> 次</span>
```

- [ ] **Step 2: page_interact.html 4.1-4.6 灯加 id + 计数 span**

对 4.1(lamp-click,6)、4.2(lamp-hover,4)、4.4(lamp-drag,4)、4.5(lamp-longpress,3)、4.6(lamp-key,8) 各灯容器做同样改造：去掉容器 id（或保留无妨），子 `<div class="lamp">` 加编号 id `lamp-{name}-{i}`，并在灯容器后加 `<span class="card-x">已触发 <span id="count-{name}">0</span> 次</span>`。

例 4.1 lamp-click（6 盏）：
- old:
```
            <div class="lamps" id="lamp-click">
              <div class="lamp"></div><div class="lamp"></div><div class="lamp"></div><div class="lamp"></div><div class="lamp"></div><div class="lamp"></div>
            </div>
```
- new:
```
            <div class="lamps">
              <div class="lamp" id="lamp-click-0"></div><div class="lamp" id="lamp-click-1"></div><div class="lamp" id="lamp-click-2"></div>
              <div class="lamp" id="lamp-click-3"></div><div class="lamp" id="lamp-click-4"></div><div class="lamp" id="lamp-click-5"></div>
            </div>
            <span class="card-x">已触发 <span id="count-click">0</span> 次</span>
```

对 lamp-hover(4)、lamp-drag(4)、lamp-longpress(3)、lamp-key(8) 同理（编号 0..N-1）。**4.7 lamp-route 不动（T4 处理）。**

- [ ] **Step 3: C# `LightLamp` 重写**

Edit `LoomShowcaseDriver.cs`，在字段区（`_routeCount` 那行附近，约 line 49）加：
```csharp
        // lamp 组当前已亮盏数（0..N 循环重置）。key=灯组 name。
        readonly Dictionary<string, int> _lampLit = new();
        static int LampCount(string name) => name switch {
            "click" => 6, "hover" => 4, "drag" => 4, "key" => 8,
            "complete" => 3, "longpress" => 3,
            "outer" => 3, "inner" => 3, "pe" => 3,   // T4 用
            _ => 3
        };
```

替换 `LightLamp` 方法（约 line 332）：
- old:
```csharp
        // 点亮 lamp-{name} 容器：无 get_children API，改用整容器 opacity 脉冲指示触发。
        void LightLamp(string name, int count)
        {
            uint container = _stage.FindNodeById("lamp-" + name);
            if (container == uint.MaxValue) return;
            _stage.Tween(container, TweenProp.Opacity,
                new float[] { 1f, 0, 0, 0 }, new float[] { 0.3f, 0, 0, 0 },
                0.2f, Ease.QuadOut, 0f, 0);
        }
```
- new:
```csharp
        // 点亮 lamp-{name}-{lit} 第 lit 盏（id-addressable）+ count-{name} 计数。
        // 全亮后下一次触发先灭所有再重新从 0 点亮（循环）。count 显示累计触发次数。
        void LightLamp(string name, int totalCount)
        {
            int n = LampCount(name);
            if (!_lampLit.TryGetValue(name, out int lit)) lit = 0;
            if (lit >= n)
            {
                for (int i = 0; i < n; i++)
                {
                    uint lamp = _stage.FindNodeById($"lamp-{name}-{i}");
                    if (lamp != uint.MaxValue) _stage.SetStyle(lamp, "background-color:#3a3f55");
                }
                lit = 0;
            }
            uint node = _stage.FindNodeById($"lamp-{name}-{lit}");
            if (node != uint.MaxValue) _stage.SetStyle(node, "background-color:#5fb2c4");
            _lampLit[name] = lit + 1;
            uint countNode = _stage.FindNodeById("count-" + name);
            if (countNode != uint.MaxValue) _stage.SetText(countNode, totalCount.ToString());
        }
```

调用方（`OnClickHit`/`OnHoverHit` 等）不动——它们已传 `++_clickCount` 等累计值。

- [ ] **Step 4: JS `pulseLamp` → `lightNext` 重写**

Edit `loom-preview.js`，在 CONFIG 区（TEMPLATES 后）加灯组数量表：
```js
  // 灯组数量（与 C# LampCount 对齐）。
  var LAMP_COUNT = { click:6, hover:4, drag:4, key:8, complete:3, longpress:3, outer:3, inner:3, pe:3 };
  // 各灯组当前已亮盏数（0..N 循环重置）。
  var lampLit = {};
```

替换 `pulseLamp` 函数：
- old:
```js
  // 灯阵脉冲：lamp-{name} 容器 opacity 1→0.3→1（CSS transition，近似 C# LightLamp）。
  function pulseLamp(name) {
    var c = $('lamp-' + name);
    if (!c) return;
    c.style.transition = 'opacity .2s';
    c.style.opacity = '0.3';
    setTimeout(function () { c.style.opacity = '1'; }, 200);
  }
```
- new:
```js
  // 点亮 lamp-{name}-{lit}（id-addressable）+ count-{name} 计数，全亮后循环重置。
  // 与 C# LightLamp 对齐。
  function lightNext(name) {
    var n = LAMP_COUNT[name] || 3;
    var lit = lampLit[name] || 0;
    if (lit >= n) {
      for (var i = 0; i < n; i++) { var e = $('lamp-' + name + '-' + i); if (e) e.style.backgroundColor = '#3a3f55'; }
      lit = 0;
    }
    var lamp = $('lamp-' + name + '-' + lit);
    if (lamp) lamp.style.backgroundColor = '#5fb2c4';
    lampLit[name] = lit + 1;
    var counter = $('count-' + name);
    if (counter) counter.textContent = String((parseInt(counter.textContent, 10) || 0) + 1);
  }
```

替换所有 `pulseLamp(` 调用为 `lightNext(`（在 tween/interact handler 里——T3/T4 会再改 4.7，但先把非 4.7 的 pulseLamp 全改 lightNext）。用 Edit `replace_all` 或逐个：`pulseLamp('complete')`→`lightNext('complete')`、`pulseLamp('click')`→`lightNext('click')` 等。

- [ ] **Step 5: 验证 + commit**

```bash
node --check loomgui_unity/Assets/LoomUI/preview/loom-preview.js   # JS 语法 OK
git add -A && git commit -m "feat(showcase): 亮灯改 id-addressable 点亮保持+计数（7.4/4.1-4.6）"
```

浏览器打开 page_interact.html 点 hit-click → 青灯一盏盏亮 + 计数涨；page_tween 7.4 播放完 → lamp-complete 同样。（C# 侧家里机 Unity 验收。）

---

### Task 2: §5 滚动实验室重做（page_scroll.html + C# SubscribeScroll + JS）

**Files:**
- Modify: `loomgui_unity/Assets/LoomUI/showcase/page_scroll.html`（5.1-5.6 全部）
- Modify: `loomgui_unity/Assets/LoomGUI/Runtime/LoomShowcaseDriver.cs`（`SubscribeScroll` 加 3 按钮）
- Modify: `loomgui_unity/Assets/LoomUI/preview/loom-preview.js`（`page_scroll` handler 加 3 按钮）

**Interfaces:**
- Consumes: `_stage.SetScrollPos(uint node, float x, float y, bool animated=true)`、`_stage.FindNodeById`。
- Produces: `scroll-top`/`scroll-mid`/`scroll-bottom` 三按钮 id（HTML）；C#/JS 各绑 click → SetScrollPos/scrollTop。

- [ ] **Step 1: page_scroll.html §5 内容重做**

替换整个 `<div class="sec" id="sec-5">...</div>` 块（约 line 41-81）为：
```html
      <div class="sec" id="sec-5">
        <div class="sec-h">§5 滚动实验室</div>
        <div class="card">
          <div class="card-h"><span class="card-t">5.1 overflow 模式</span><span class="card-x">预期: scroll/auto 能滚看完 8 行，hidden 截断</span></div>
          <div class="card-b">
            <div class="mini" style="overflow:scroll"><div class="filler-text">第1行<br>第2行<br>第3行<br>第4行<br>第5行<br>第6行<br>第7行<br>第8行</div></div>
            <div class="mini" style="overflow:auto"><div class="filler-text">第1行<br>第2行<br>第3行<br>第4行<br>第5行<br>第6行<br>第7行<br>第8行</div></div>
            <div class="mini" style="overflow:hidden"><div class="filler-text">第1行<br>第2行<br>第3行<br>第4行<br>第5行<br>第6行<br>第7行<br>第8行</div></div>
          </div>
        </div>
        <div class="card">
          <div class="card-h"><span class="card-t">5.2 overflow-x/y</span><span class="card-x">预期: 4 item 横排，shift+滚轮/拖滚动条横滚看完</span></div>
          <div class="card-b"><div class="hscroll"><div class="hfiller">🏠 主页</div><div class="hfiller">📷 图片</div><div class="hfiller">📝 文本</div><div class="hfiller">⚙ 设置</div></div></div>
        </div>
        <div class="card">
          <div class="card-h"><span class="card-t">5.3 惯性/回弹</span><span class="card-x">Unity 限定：拖本页/滚轮体验</span></div>
          <div class="card-b"><span class="card-x">本页外层 #page-scroll 即体验载体。Unity 有松手惯性 + 触顶/触底边界回弹；浏览器原生滚动无（已知差异）。</span></div>
        </div>
        <div class="card">
          <div class="card-h"><span class="card-t">5.4 滚动条 + grip</span><span class="card-x">预期: 拖右侧滚动条定位到不同色块</span></div>
          <div class="card-b">
            <div class="mini" style="overflow:scroll"><div class="color-block" style="background-color:#c2605a;height:60px">红</div><div class="color-block" style="background-color:#e09a4a;height:60px">橙</div><div class="color-block" style="background-color:#5fb2c4;height:60px">青</div><div class="color-block" style="background-color:#6fa66c;height:60px">绿</div><div class="color-block" style="background-color:#7b6cd9;height:60px">紫</div></div>
            <span class="card-x">浏览器原生滚动条 ≠ LoomGUI 合成 thumb（Unity 限定），但都能拖定位。</span>
          </div>
        </div>
        <div class="card">
          <div class="card-h"><span class="card-t">5.5 嵌套+轴锁</span><span class="card-x">预期: 内层竖滚里嵌横滚条 + 可拖块 + 文字</span></div>
          <div class="card-b" style="flex-direction:column;gap:12px;width:100%">
            <div class="mini" id="nested-scroll" style="overflow-y:scroll;height:200px">
              <div class="hscroll"><div class="hfiller">h1</div><div class="hfiller">h2</div><div class="hfiller">h3</div><div class="hfiller">h4</div></div>
              <div class="filler" draggable="true" style="height:60px">drag me</div>
              <div class="filler-text">嵌套段 A<br>嵌套段 B<br>嵌套段 C<br>嵌套段 D</div>
            </div>
            <span class="card-x">竖向拖=外层竖滚 / 横向拖=内层横滚 / 拖 drag me=item 跟动（C# OnDragHit，预览不演）。轴锁 + scroll-vs-drag 仲裁 Unity 限定。</span>
          </div>
        </div>
        <div class="card">
          <div class="card-h"><span class="card-t">5.6 SetScrollPos</span><span class="card-x">预期: 点按钮程序跳转本页滚动定位</span></div>
          <div class="card-b">
            <button id="scroll-top">跳顶</button>
            <button id="scroll-mid">跳中</button>
            <button id="scroll-bottom">跳底</button>
          </div>
        </div>
      </div>
```

并在 `<style>` 里加（替换原 `.filler`/`.hfiller` 或新增）：
```css
.filler-text{padding:8px;color:#9aa0b4;font-size:13px;line-height:24px;}
.color-block{color:#1a1d2e;font-size:16px;font-weight:700;display:flex;align-items:center;justify-content:center;}
```
（`.filler`/`.hfiller`/`.mini`/`.hscroll` 已有，保留。`.color-block` 新增。）

- [ ] **Step 2: C# `SubscribeScroll` 加 3 按钮**

Edit `LoomShowcaseDriver.cs` `SubscribeScroll`（约 line 299）：
- old:
```csharp
        void SubscribeScroll()
        {
            SubscribeBackHome();
            Debug.Log("[Showcase] page_scroll 订阅完成（back）");
        }
```
- new:
```csharp
        void SubscribeScroll()
        {
            SubscribeBackHome();
            uint pageScroll = _stage.FindNodeById("page-scroll");
            AddPageListener(_stage.FindNodeById("scroll-top"), EventType.Click, _ => _stage.SetScrollPos(pageScroll, 0f, 0f));
            AddPageListener(_stage.FindNodeById("scroll-mid"), EventType.Click, _ => _stage.SetScrollPos(pageScroll, 0f, 600f));
            AddPageListener(_stage.FindNodeById("scroll-bottom"), EventType.Click, _ => _stage.SetScrollPos(pageScroll, 0f, 99999f));
            Debug.Log("[Showcase] page_scroll 订阅完成（back + 3 SetScrollPos 按钮）");
        }
```

- [ ] **Step 3: JS `page_scroll` handler 加 3 按钮**

Edit `loom-preview.js`，替换 `page_scroll` stub（注意 T1 已把 stub 改过——若已非 stub 用当前内容做 old）：
- old:
```js
    page_scroll:   function () { wireBackHome(); },
```
- new:
```js
    page_scroll:   function () {
      wireBackHome();
      var ps = $('page-scroll');
      bindClick('scroll-top', function () { if (ps) ps.scrollTop = 0; });
      bindClick('scroll-mid', function () { if (ps) ps.scrollTop = 600; });
      bindClick('scroll-bottom', function () { if (ps) ps.scrollTop = 99999; });
    },
```

- [ ] **Step 4: 验证 + commit**

```bash
node --check loomgui_unity/Assets/LoomUI/preview/loom-preview.js
git add -A && git commit -m "feat(showcase): §5 滚动重做——可见内容 + 5.6 SetScrollPos 三按钮（HTML+C#+JS）"
```

浏览器 page_scroll：5.1 三框能看见"第N行"滚动；5.4 色块滚动；5.6 三按钮跳顶/中/底。

---

### Task 3: §7 动效 7.3 修 + 7.5 播放按钮（page_tween.html + C# + JS）

**Files:**
- Modify: `loomgui_unity/Assets/LoomUI/showcase/page_tween.html`（7.5 加播放按钮）
- Modify: `loomgui_unity/Assets/LoomGUI/Runtime/LoomShowcaseDriver.cs`（`SubscribeTween`）
- Modify: `loomgui_unity/Assets/LoomUI/preview/loom-preview.js`（`page_tween` handler）

**Interfaces:**
- Produces: `play-kill-target` 按钮 id；C# `SubscribeTween` 订阅它 → PlayProp rotation loop；JS 同。

- [ ] **Step 1: page_tween.html 7.5 加播放按钮**

Edit 7.5 卡（约 line 86-93），在 `kill-target` 后、`kill-btn` 前插播放按钮：
- old:
```html
            <div class="demo" id="kill-target">旋转中</div>
            <div class="hit" id="kill-btn">kill（停末值）</div>
```
- new:
```html
            <div class="demo" id="kill-target">旋转中</div>
            <div class="hit" id="play-kill-target">▶ 播放</div>
            <div class="hit" id="kill-btn">kill（停末值）</div>
```
把 `kill-target` 文本从"旋转中"改"未转"（进页不自动转了）——同处改：`<div class="demo" id="kill-target">未转</div>`。

- [ ] **Step 2: C# `SubscribeTween` 改成点播**

Edit `LoomShowcaseDriver.cs` `SubscribeTween`（约 line 364-378）。删掉末尾"进页即旋转"那行，加 `play-kill-target` 订阅：
- old（SubscribeTween 末尾）:
```csharp
            // kill-target：启动即开始持续旋转（单次长 tween——loop 需 TweenComplete 重启，简化省略）。
            PlayProp("kill-target", TweenProp.Rotation, new float[] { 0f, 0, 0, 0 }, new float[] { 360f, 0, 0, 0 }, 4f, Ease.Linear, 0f, 0);
            Debug.Log("[Showcase] page_tween 订阅完成（play/ease/delay/complete/kill/clear + kill-target 旋转）");
```
- new:
```csharp
            // kill-target：点"播放"才开始持续旋转（不再进页自动转——和预览对齐）。
            SubscribeLamp("play-kill-target", EventType.Click, OnPlayKillTarget);
            Debug.Log("[Showcase] page_tween 订阅完成（play/ease/delay/complete/kill/clear + play-kill-target）");
        }
        void OnPlayKillTarget(EventContext ctx)
        {
            PlayProp("kill-target", TweenProp.Rotation, new float[] { 0f, 0, 0, 0 }, new float[] { 360f, 0, 0, 0 }, 4f, Ease.Linear, 0f, 0);
```
（注意：原 SubscribeTween 末尾的 `}` 闭合要保留——new 里把方法闭合 + 新加 `OnPlayKillTarget` 方法。实施时 Read 确认括号。）

- [ ] **Step 3: JS `page_tween` 修 7.3 + 加 7.5 播放**

Edit `loom-preview.js` `page_tween` handler。

7.3 修（delay-play 不靠 CSS 类对撞）：替换 delay-play 的 bindClick：
- old:
```js
      // delay 错峰：递增 transition-delay。
      bindClick('delay-play', function () {
        ['d-0', 'd-1', 'd-2'].forEach(function (id, i) {
          var el = $(id);
          if (el) { el.style.transitionDelay = (i * 0.2) + 's'; el.classList.toggle('play'); }
        });
      });
```
- new:
```js
      // delay 错峰：JS 直接控 opacity（不靠 CSS 类——inline style 优先级高于类选择器会卡死）。
      var dState = false;
      bindClick('delay-play', function () {
        dState = !dState;
        ['d-0', 'd-1', 'd-2'].forEach(function (id, i) {
          var el = $(id);
          if (!el) return;
          el.style.transition = 'opacity .5s ease ' + (i * 0.2) + 's';
          el.style.opacity = dState ? '1' : '0';
        });
      });
```
并删掉之前 `['d-0','d-1','d-2'].forEach(...opacity='0'...)` 那行初始化（现在 dState 初始 false=隐，点一次显）——保留也行（初始隐），但确保 delay-play 能切。实施时 Read 确认。

7.5 加播放按钮 + 改 kill-target 不自动转：删掉 `#kill-target{animation:loom-spin...}` 自动转（进页不该转），改为点播放才转。
- 在注入的 `<style>` 里把 `#kill-target{animation:loom-spin 4s linear infinite}` 改成 `#kill-target.spinning{animation:loom-spin 4s linear infinite}`（加 .spinning 类才转）。
- 加 `play-kill-target` 绑定：
```js
      bindClick('play-kill-target', function () {
        var el = $('kill-target');
        if (el) { el.classList.remove('cleared'); el.classList.add('spinning'); if (el.textContent !== null) el.textContent = '旋转中'; }
      });
```
- kill-btn / clear-btn 逻辑改：kill 加 `.paused`（animation-play-state:paused）、clear 移除 `.spinning`+`.paused` 加 `.cleared`。实施时 Read 当前 kill/clear 逻辑调整。

- [ ] **Step 4: 验证 + commit**

```bash
node --check loomgui_unity/Assets/LoomUI/preview/loom-preview.js
git add -A && git commit -m "feat(showcase): §7 动效——7.3 修 delay 错峰 JS bug + 7.5 加播放按钮（HTML+C#+JS）"
```

浏览器 page_tween：7.3 点 delay-play → 3 块依次淡入（再点淡出）；7.5 点播放 → kill-target 转起来，kill 停、clear 回不转。

---

### Task 4: §4.7 路由 + pointer-events 重构（page_interact.html + C# + JS）

**Files:**
- Modify: `loomgui_unity/Assets/LoomUI/showcase/page_interact.html`（4.7 灯组+pe 重构）
- Modify: `loomgui_unity/Assets/LoomGUI/Runtime/LoomShowcaseDriver.cs`（`SubscribeInteract` route 订阅 + 回调）
- Modify: `loomgui_unity/Assets/LoomUI/preview/loom-preview.js`（`page_interact` route 绑定）

**Interfaces:**
- Produces: `lamp-outer`/`lamp-inner`/`lamp-pe` 三组灯（各 3 盏 id-addressable）+ count span；`route-pe-under`（下层可点）+ `pe-none`（absolute 盖住）。
- C# callbacks: `OnRouteOuter`→亮 lamp-outer；`OnRouteInner`→亮 lamp-inner + StopProp；`OnRoutePeUnder`→亮 lamp-pe。

- [ ] **Step 1: page_interact.html 4.7 重构**

Edit 4.7 卡（约 line 98-112），替换整个 `<div class="card">`（4.7 的）：
- old:
```html
        <div class="card">
          <div class="card-h"><span class="card-t">4.7 路由 + pointer-events</span><span class="card-x">预期: bubble + StopProp + 属性设置</span></div>
          <div class="card-b" style="flex-direction:column;gap:8px;width:100%">
            <div class="lamps" id="lamp-route">
              <div class="lamp"></div><div class="lamp"></div><div class="lamp"></div>
            </div>
            <div class="outer-r" id="route-outer">
              <div class="inner-r" id="route-inner">内（StopPropagation 止冒泡）</div>
            </div>
            <div class="hit" id="route-pe">可点（route-pe）</div>
            <div class="pe-none">pointer-events:none 流内块（hit_test 跳过）</div>
            <span class="card-x">pointer-events:none 节点 hit_test 跳过（点击穿透到下层）。v1 无 position:absolute 叠加，穿透叠加效果不可视，本卡演属性设置 + 路由 capture/bubble/StopPropagation。</span>
          </div>
        </div>
```
- new:
```html
        <div class="card">
          <div class="card-h"><span class="card-t">4.7 路由 + pointer-events</span><span class="card-x">预期: 点 inner 只 inner 亮（StopProp）；pe-none 穿透到下层</span></div>
          <div class="card-b" style="flex-direction:column;gap:8px;width:100%">
            <div style="flex-direction:row;gap:16px;flex-wrap:wrap">
              <div style="flex-direction:column;gap:4px"><div class="lamps"><div class="lamp" id="lamp-outer-0"></div><div class="lamp" id="lamp-outer-1"></div><div class="lamp" id="lamp-outer-2"></div></div><span class="card-x">outer <span id="count-outer">0</span></span></div>
              <div style="flex-direction:column;gap:4px"><div class="lamps"><div class="lamp" id="lamp-inner-0"></div><div class="lamp" id="lamp-inner-1"></div><div class="lamp" id="lamp-inner-2"></div></div><span class="card-x">inner <span id="count-inner">0</span></span></div>
              <div style="flex-direction:column;gap:4px"><div class="lamps"><div class="lamp" id="lamp-pe-0"></div><div class="lamp" id="lamp-pe-1"></div><div class="lamp" id="lamp-pe-2"></div></div><span class="card-x">pe <span id="count-pe">0</span></span></div>
            </div>
            <div class="outer-r" id="route-outer">
              <div class="inner-r" id="route-inner">点 inner（StopPropagation，outer 不亮）</div>
            </div>
            <div style="position:relative;width:200px;height:50px">
              <div class="hit" id="route-pe-under" style="width:100%;height:100%">下层可点（route-pe-under）</div>
              <div class="pe-none" style="position:absolute;inset:0">pointer-events:none（点我穿透到下层）</div>
            </div>
            <span class="card-x">点 inner → 只 lamp-inner 亮（StopPropagation 止冒泡，outer 不亮）；点 outer → lamp-outer 亮；点 pe-none 区块 → 穿透命中 route-pe-under → lamp-pe 亮。</span>
          </div>
        </div>
```

- [ ] **Step 2: C# `SubscribeInteract` route 订阅 + 回调改名**

Edit `LoomShowcaseDriver.cs`。

`SubscribeInteract`（约 line 319-321）route 三行：
- old:
```csharp
            SubscribeLamp("route-outer", EventType.Click, OnRouteOuter);
            SubscribeLamp("route-inner", EventType.Click, OnRouteInner);
            SubscribeLamp("route-pe", EventType.Click, OnRoutePe);
```
- new:
```csharp
            SubscribeLamp("route-outer", EventType.Click, OnRouteOuter);
            SubscribeLamp("route-inner", EventType.Click, OnRouteInner);
            SubscribeLamp("route-pe-under", EventType.Click, OnRoutePeUnder);
```

回调（约 line 354-360）：
- old:
```csharp
        void OnRouteOuter(EventContext ctx) { LightLamp("route", ++_routeCount); }
        void OnRouteInner(EventContext ctx)
        {
            ctx.StopPropagation();
            LightLamp("route", ++_routeCount);
        }
        void OnRoutePe(EventContext ctx) { LightLamp("route", ++_routeCount); }
```
- new:
```csharp
        void OnRouteOuter(EventContext ctx) { LightLamp("outer", ++_routeCount); }
        void OnRouteInner(EventContext ctx)
        {
            ctx.StopPropagation();
            LightLamp("inner", ++_routeCount);
        }
        void OnRoutePeUnder(EventContext ctx) { LightLamp("pe", ++_routeCount); }
```

（`_routeCount` 复用为三组共用计数——也可拆三个，但 LightLamp 内部按 name 分组 _lampLit，count 显示用 _routeCount 累计也行。简化：保留 _routeCount 共用。）

- [ ] **Step 3: JS `page_interact` route 绑定改**

Edit `loom-preview.js` `page_interact` handler 的 route 部分：
- old:
```js
      // 路由：inner stopPropagation 止冒泡。
      bind('route-outer', 'click', function () { lightNext('route'); });
      bind('route-pe', 'click', function () { lightNext('route'); });
      bind('route-inner', 'click', function (e) { e.stopPropagation(); lightNext('route'); });
```
- new:
```js
      // 路由：inner stopPropagation 止冒泡——点 inner 只 inner 亮，outer 不亮。
      bind('route-outer', 'click', function () { lightNext('outer'); });
      bind('route-pe-under', 'click', function () { lightNext('pe'); });
      bind('route-inner', 'click', function (e) { e.stopPropagation(); lightNext('inner'); });
      // pe-none: pointer-events:none → 点击穿透到下层 route-pe-under（浏览器原生支持，无需绑）。
```

- [ ] **Step 4: 验证 + commit**

```bash
node --check loomgui_unity/Assets/LoomUI/preview/loom-preview.js
git add -A && git commit -m "feat(showcase): §4.7 路由重构——三组独立灯+StopProp 可见+pe 穿透（HTML+C#+JS）"
```

浏览器 page_interact 4.7：点 inner → 只 lamp-inner 亮；点 outer → lamp-outer 亮；点 pe-none 区 → 穿透到 route-pe-under → lamp-pe 亮。

---

### Task 5: 重打 pkg.bin + 最终验证

**Files:** 无源码改动；build 产物 + 验证。

- [ ] **Step 1: 重打 showcase.pkg.bin（坑 66——HTML 改了必须重打）**

```bash
cd "E:/workspace/LoomGUI/.claude/worktrees/showcase-browser-preview"
WT="E:/workspace/LoomGUI/.claude/worktrees/showcase-browser-preview"
EXE="$WT/loomgui_unity/Assets/LoomGUI/Editor/Tools/loomgui_pkg.exe"
SRC="$WT/loomgui_unity/Assets/LoomUI/showcase"
RES="$WT/loomgui_unity/Assets/LoomUI/res"
OUT="$WT/loomgui_unity/Assets/StreamingAssets/showcase.pkg.bin"
HTMLS="home.html,list_item.html,mail.html,page_controls.html,page_dyntree.html,page_image.html,page_interact.html,page_list.html,page_scroll.html,page_text.html,page_tween.html,tips_toast.html"
"$EXE" "$SRC" showcase --html "$HTMLS" --res-root "$RES" -o "$OUT"
echo "exit=$?"
```
Expected: `wrote .../showcase.pkg.bin (N bytes, 12 components, 4 manifest paths)`, exit 0。无围栏违规报错。

- [ ] **Step 2: pkg.bin 入库（build 产物，tracked）**

```bash
git add loomgui_unity/Assets/StreamingAssets/showcase.pkg.bin
git commit -m "build(showcase): 重打 pkg.bin——内容完善后产物（HTML 改动必重打，坑 66）"
```

- [ ] **Step 3: 浏览器全页走查（本机）**

双击 `home.html`，逐页验证：
- page_scroll：5.1 三框能看见行号滚；5.4 色块滚；5.6 三按钮跳顶/中/底。
- page_tween：7.3 delay-play 错峰淡入；7.5 播放→转、kill→停、clear→不转；7.4 complete 亮灯+计数。
- page_interact：4.1-4.6 各事件亮灯+计数；4.7 三组灯独立+pe 穿透。

- [ ] **Step 4: 交付家里机 Unity PlayMode 验收**

```bash
git log --oneline main..HEAD   # 确认本轮所有 commit 在分支上
git push                       # 若家里机通过 pull 取（或合 main 后 pull）
```
家里机 pull → Unity PlayMode → 验收同上清单（C# driver 改动 + 新 pkg.bin）。

---

## Self-Review 记录

- **Spec 覆盖**：spec §1(§5)→T2；§2(§7.3/7.5)→T3；§3(亮灯)→T1；§4(§4.7)→T4；§5 实施顺序→T1-T4；§6 待查（LoomStage 子节点 API）→resolved（用 id-addressable，不需子节点遍历，无需降级）。全覆盖。
- **占位符扫描**：无 TBD/TODO。每步有完整代码或确切命令。Step 中"实施时 Read 确认"是 Edit 前置要求（Edit 需先 Read），非占位符。
- **类型/命名一致**：`LightLamp(string name, int totalCount)`（C#）/ `lightNext(name)`（JS）跨 T1/T4 一致；`LampCount`(C#)/`LAMP_COUNT`(JS) 表一致；按钮 id（`scroll-top/mid/bottom`、`play-kill-target`、`route-pe-under`）跨 HTML/C#/JS 一致。
- **坑 66 兜底**：T5 专门重打 pkg.bin，HTML 改动不漏。
