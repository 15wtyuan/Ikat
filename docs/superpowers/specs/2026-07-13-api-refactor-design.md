# LoomGUI 运行时 API 重构设计

> 日期：2026-07-13
> 状态：设计已与用户逐条确认，待 spec 审阅
> 前置：`docs/design/api-surface.md`（当前 API 盘点 + 摩擦点 A–M）
> 参考实现：`temp/FairyGUI-unity/`、`temp/RmlUi/`（只读）+ Unity UI Toolkit（web 文档）

---

## 1. 背景与动机

当前对外 API（见 `api-surface.md`）有几个根本问题：

- **web 味漏到运行时**：用户拿到的是 `Stage` 全局 + `FindNodeById`（平铺全局查找）+ `NodeId` 整数句柄 + `stage.SetX(nodeId,...)` + `SetStyle(node,"color:red")` CSS 串。是"HTML/CSS 框架"的实现细节漏给了用户。
- **没有对象身份**：操作主语是 `Stage`，不是节点对象；事件 `EventHandler.AddListener(nodeId,...)` 是 nodeId-keyed。
- **摩擦 A–M**（哨兵满天飞、隐式时序、Instantiate 脱挂静默、CSS 串无诊断、虚拟列表画猫、NativeHost 三步手动仪式等）。

**范式转换**：把运行时 API 从"HTML/CSS 文件 + stage 全局操作"重做成"**UI 树 + Node 对象为主语**"，对标 fgui/UITK/RmlUi 的游戏 UI 框架手感。HTML/CSS 保留为**设计期 DSL**（AI 编辑用，核心价值不动）。

## 2. 设计哲学

**LoomGUI = flexbox 游戏 UI 框架。** 三层分离：

```
设计期（AI / 设计师）       打包期                运行时（程序员 = 游戏开发者）
HTML/CSS DSL  ──打包器──▶  pkg.bin 里的   ──▶   Node 对象树
（AI 强先验，              component               （fgui GObject / UITK VisualElement / RmlUi Element 手感）
 不变）                    （已有）
```

- **HTML/CSS 是给 AI/设计师的设计期 DSL**（不动）。
- **打包器编译成 component**（已有）。
- **运行时 API 是给程序员的 Node 对象树**——这是本次重构的新表面。

**对标**：对象模型参考 **fgui**（typed 子类、GetChild、事件挂对象、包即工厂）；**layout 参考 UITK + RmlUi**（flex 流式），fgui 的绝对定位模型**不抄**。flex 流式是 LoomGUI 的差异化超能力，也是 AI 强先验。

## 3. 运行时 API：Node 对象树

### 3.1 Node 模型（UITK 式）

`Node` 既是基类又是容器——任何 Node 都能 `GetChild`/`AddChild`（同 UITK `VisualElement`、RmlUi `Element`），不强行分 Container/Leaf 层。围栏保证叶子标签（img/span）无子节点，运行时 `AddChild` 到叶子报错（围栏强制）。

```
Node                         // 基类+容器：GetChild/AddChild/Children/GetController/事件/属性
├── <div>   → 裸 Node        // 通用容器（同 UITK VisualElement）
├── <button> → Button : Node // 容器 + .Text/.Disabled/.Clicked，能 GetChild 拿内部 img/span
├── <span>/文本 → Text : Node // 叶子，.Text
│   └── RichText             // .Markup
└── <img>  → Image : Node    // 叶子，.Src
```

- 类型转换：`.AsButton()`/`.AsText()`/`.AsImage()`/`.AsRichText()`（fgui `.asButton` 风）。`<div>` 不转换，就是 Node。
- `Button : Node`（不是 Text 子类）——`<button>` 能含 img+span，是容器，更像 fgui GButton。

### 3.2 对象身份

用户持有 `Node` 引用（不是整数 NodeId），方法/属性调在对象上。内部仍持 `(stageRef, nodeId)` 调 FFI——Node 是 facade，core/FFI 不动。

### 3.3 创建

**主路径：实例化**（绝大多数 UI 来自设计期 authoring，同 fgui `CreateObject` / UITK `Instantiate` / RmlUi `LoadDocument`）：

```csharp
Package pkg = ui.LoadPackage("showcase", bytes);   // 载入一次
Node home   = pkg.Instantiate("Home");              // 克隆 authored 组件树 → Node
ui.Root.AddChild(home);                             // 入流（挂根）
```

**逃生舱：动态建节点（少用）**——节点活在 Rust Stage 里（需上下文，不能裸 `new`），走 `ui` 句柄工厂。默认 detached（同 Instantiate），`AddChild` 挂载：

```csharp
Node  panel = ui.Create<Node>();      // = <div>
Button btn  = ui.Create<Button>();
panel.AddChild(btn);
```

## 4. 子节点寻址

| 方法 | 作用域 | 用途 |
|---|---|---|
| `node.GetChild(name)` | **直系子节点**（1 层） | 主寻址，fgui `GetChild` 式逐层下钻 |
| `node.GetChildAt(i)` / `Children()` / `ChildCount` | 直系 | 位置/结构/迭代 |
| `node.Query<T>()` / `node.Query("button")` | **整棵子树**（递归 descendants） | "这个 panel 下所有 Button"（UITK `Query<T>` 式） |
| ~~`node.GetChildByPath("a.b.c")`~~ | — | **不做**（YAGNI，用户定） |

- **`name` 属性**（新增，fgui/UXML 式）：per-parent 寻址句柄。**同父重复 = 打包器编译期报错**（围栏哲学：结构性违反编译期挡，不降级；也服务 AI 可预测——AI 读 `GetChild("x")` 确信 x 是直系子节点）。
- 跨父同名：正常允许（per-parent 作用域，两个面板各有 "title" 是想要的）。
- 运行时动态建节点重名：first-match + dev log warning 兜底（罕见）。
- 惯用法（三家共识）：**name 你要寻址的，iterate 你不寻址的**。无 name 子节点要么是 List 处理的重复项，要么是纯展示 code 不碰——index 单独寻址是 code smell。

**非泛型为主 + 泛型可选 C# 糖**（脚本友好——逻辑层常是 Lua，泛型方法 Lua 绑定难）：

| 操作 | 非泛型（主） | 泛型（C# 可选糖） |
|---|---|---|
| 创建 | `ui.Create("button")` → Node，再 `.AsButton()` | `ui.Create<Button>()` |
| 按类型查 | `node.Query("button")` → Node[] | `node.Query<Button>()` |
| 类型转换 | `.AsButton()` 等属性 | — |

typed 子类（`Button` 等）是**具体类**（Lua 按类型注册即可），只有**泛型方法**才是脚本毒。所以：类型化子类（多）+ 非泛型方法（主）+ 泛型方法（可选）= 两边都顺。

## 5. 布局（layout）

**CSS-first（RmlUi 端），不搞 typed IStyle 对象。**

| 干什么 | 怎么干 |
|---|---|
| 布局（flex） | **authored CSS**（设计期，主语言） |
| 状态改动（含布局状态 compact/vertical/hidden） | **class 切换** `node.AddClass/RemoveClass`（声明式，UITK+RmlUi 共识惯用法） |
| 值改动（罕见，如拖动位置/服务器下发尺寸） | **`node.SetStyle("width:200px")`**（CSS 串，与 authoring 一致） |
| 内容/状态常用 | **typed 属性**（见 §6） |
| transform（render 层偏移/动画） | **typed**，见下 |

**为什么 CSS-first 不抄 UITK 的 typed IStyle**：
1. LoomGUI 是 authoring-heavy（布局主要设计期 authored），运行时改布局是少数 → typed layout surface（40+ 属性）= **YAGNI**。
2. 一致性——authoring 是 CSS 串，运行时 mutation 也走 CSS，一套心智模型（typed IStyle 是 Unity-ism 另一套）。
3. 最小 API 面 + 脚本友好（SetStyle 一个方法）。
4. AI 可预测——AI 预测渲染靠 HTML/CSS，运行时 mutation 走同款 CSS 串，连贯。

**围栏细节**（研究验证 + 本设计钉死）：

- **box-sizing 默认 `border-box`**（UITK 强制同）——所见即所声，AI 写 `width:100px` 渲 100px（content-box 会渲 124px 预测错）。
- **`gap` 暴露**——taffy 支持、RmlUi 有、CSS 标准、AI 懂（UITK 没有是它的缺）。
- **`overflow:scroll` 保留**——LoomGUI 用 div 属性（vs UITK 的 ScrollView 元素），更贴 HTML-as-DSL；`List`（§7）建在上面。
- **围栏是生产级子集**（UITK 验证）：div 恒 flex + column 默认 + position 仅 absolute/relative——UITK 也这么砍，不是乱限制。
- **position 包含块**：taffy 用最近定位祖先（CSS 式）vs UITK 用父——实现期验证 taffy 实际行为，在 `fence.md`/测试钉。

### transform（与 layout 分清）

LoomGUI 的 transform（x/y/scale/rotation）架构上是**渲染层叠加**（不进 taffy、不触发 solve、廉价），是**动画通道，不是 layout 定位**。所以：

- typed 暴露，归到 **`node.Transform.X/Y/Scale/Rotation`** 子对象（明确"叠加/偏移"语义，不与 layout 位置混）。
- 动画主走 `node.Tween(Prop.X, 0, 100, 0.3f, Ease.CubicOut)`。
- **不当主定位**——主定位是 flex 流式。撤回 ~~`node.X/Y` 作为主 API~~（fgui 绝对思维）。

## 6. 类型化属性（内容/状态，code 主战场）

```csharp
text.Text = "hi"; img.Src = "icon"; node.Visible = false; node.Alpha = 0.5f;
btn.Disabled = true; text.Color = Color.Red;
node.AddClass("active"); node.RemoveClass("active");
```

颜色 tween 归一化在 facade 内部消化（解摩擦 G：传 `Color`，内部转归一化，不再要用户手动 [0,1]）。

## 7. 富 widget（`data-*` 属性 → typed Node 子类）

富 widget = `<div>`/`<button>` 挂 `data-*` 属性 → 运行时 surfaced 成 typed `XxxNode : Node`。**不新增围栏标签**（仍 4 标签 div/span/img/button，AI 友好）。

### List（本轮做，解摩擦 I 虚拟列表画猫）

**设计期**：`<div data-list item="ItemComp" name="maillist">`（item 模板引用 pkg 组件名，fgui `defaultItem` 模式——设计师做 item 布局，代码只填数据）。

**运行时**：`.AsList()` 拿 `List : Node`，框架包揽 pooling / reuse_key / scroll / content_size（现在业务侧 ~200 行全消失）：

```csharp
List mail = home.GetChild("maillist").AsList();
mail.NumItems = data.Count;
mail.ItemRenderer = (i, item) => {            // fgui 单回调：item 是回收的 Node
    item.GetChild("icon").AsImage().Src  = data[i].icon;
    item.GetChild("title").AsText().Text = data[i].title;
};
mail.ItemClicked += item => { /* ... */ };
```

API 风格选 fgui 单回调（`ItemRenderer(i, item)` + `NumItems`），不抄 UITK 的 makeItem/bindItem 拆分——更简单、更贴 fgui 倾向。

### 范围裁定

- **List**：本轮必做。
- **Window**：**出局**——是逻辑层（Show/Hide/模态/生命周期），与 UI 框架无关，业务层自理（用户定）。
- **TextInput**：后续有安排，本轮不做（IME/光标/剪贴板成本高）。
- **Slider/ProgressBar/ComboBox**：按需，div+`data-*`+typed 包装可组合出来，不急。
- **Tree/MovieClip/Graph/Group**：砍/远期（YAGNI，CSS 形状/class 可代）。

## 8. 事件系统

- **事件挂对象**：`btn.Clicked += handler` / `node.On(EventType.Down, cb)`（fgui `onClick.Add` / UITK `RegisterCallback` 风）。
- **清理靠脱离**：`node.RemoveFromParent()` / `parent.RemoveChild(node)`，监听器随子树死（fgui 模式，不做监听器记账——解摩擦：监听器生命周期手动且顺序敏感）。
- 事件路由本身仍在业务侧（C#），核心只做命中 + hover/active diff + 伪类 rematch（CLAUDE.md 不变量不变）。

## 9. 北极星（before / after）

**现状（flat Stage API）**：
```csharp
stage = driver.Stage;
uint root = stage.CreateRoot("div", "width:1080px;height:1920px;");
stage.LoadPackage("showcase", driver.LoadPackageBytes("showcase"));
uint page = stage.Instantiate("showcase", "HomePage");
stage.AppendChild(root, page);                       // 忘了 → 静默不显示
uint btn = stage.FindNodeById("btn_start");           // 全局查找
if (btn == uint.MaxValue) return;                     // 哨兵，忘查 → 静默
stage.EventHandler.AddListener(btn, EventType.Click, OnStart);
stage.SetText(stage.FindNodeById("lbl_score"), "0");
// 销毁：手动 RemoveListener 每个 + RemoveNode
```

**新（Node 树 API）**：
```csharp
Package pkg = ui.LoadPackage("showcase", bytes);
Node home = pkg.Instantiate("Home");                  // 布局 authored 在 HTML/CSS
ui.Root.AddChild(home);                               // 入流

home.GetChild("start").AsButton().Clicked += OnStart; // 事件挂对象
home.GetChild("score").AsText().Text = "0";           // 属性挂对象
home.GetController("tab").SelectedIndex = 1;
home.AddClass("compact");                             // 改布局 = 切 class

List mail = home.GetChild("maillist").AsList();
mail.NumItems = data.Count;
mail.ItemRenderer = (i, item) => item.GetChild("title").AsText().Text = data[i].title;

home.RemoveFromParent();                              // 脱离即清理
```

**全程没有 x/y 定位、没有整数句柄、没有哨兵、没有全局 find**——位置由 flex 流式算出。这才是 LoomGUI 的味道。

## 10. 重构范围（layer by layer）

| 层 | 改动 |
|---|---|
| **表面 1 Rust 核心** | **不动** |
| **表面 2 FFI C ABI** | **不动**（57 个导出照旧） |
| **表面 3 Unity C#（旧）** | `LoomStage`/`EventHandler.AddListener`/`FindNodeById`/`SetX(nodeId,...)` 等**旧用户面退役**（降 internal 保留过渡期，便于迁移；最终删） |
| **表面 3 Unity C#（新）** | **新增 Node facade**：`Node`/`Button`/`Text`/`Image`/`RichText`/`List` 包住 FFI |
| **pkg.bin** | 节点块加 `name` 字段（v16→v17）+ 打包器同父 name 唯一性校验 + `data-list`/`item` 属性解析 |
| **HTML 围栏** | 加 `name` 属性 + `data-list`/`item` 属性；标签仍 4 个；box-sizing 默认 border-box |

新 facade 直接调 FFI（持 `(stageRef, nodeId)`），不经旧 `LoomStage`。`LoomStage` 的 FFI-持有角色被新 `ui`/`Stage` 句柄接管。其它后端（Godot 等）镜像这层 facade。

**对应解的摩擦**：A（返回值五制→对象方法/属性）、B（哨兵→对象 null 检查一次）、C（CSS 串→typed 属性 + class）、D（隐式时序→对象封装有效性）、E（Instantiate 脱挂→typed 对象 + 可选 parent 重载）、G（颜色归一化→facade 内消化）、I（虚拟列表→内置 List）。F（NativeHost 三步仪式）、H（out_len 字节/个数——FFI 不动保留）、J/K/L/M 部分缓解或不在本轮。

## 11. 开放 / 待定（spec 标记，下轮或实现期钉）

- `GetChild(name)` 找不到：返回 null 还是抛？（建议 null + 可选 `GetChildOrThrow`）
- 顶层句柄命名：`ui` / `Stage` / `Loom`？（spec 暂用 `ui`）
- `pkg.Instantiate("Home", parent)` 重载是否提供（缓解摩擦 E）？
- 旧 API 是直接删还是降 internal 过渡期？（建议降 internal 过渡）
- `Query<T>` vs `GetDescendants<T>` 命名？（暂用 `Query<T>`，UITK 习惯）
- taffy position 包含块行为验证（写测试钉）。
- color tween 归一化在 facade 的具体映射。

## 12. 不变量

- **AI 可预测性是首要判据**（CLAUDE.md）：每条 API 决策服务"AI 读代码/HTML 能预测渲染"。border-box、name 直系作用域、CSS-first mutation、围栏生产级子集——都服务此点。
- **围栏哲学**：结构性违反编译期报错（同父 name 重复），不降级。
- **core/FFI 不变量全保留**（CLAUDE.md §架构不变量）：代际 NodeId、tick 时序、transform 不进 taffy、单一动画时钟、虚拟列表层 B'、NativeHost 分层——facade 只是包一层，不动这些。

## 13. 已锁决策清单

1. 类型模型 = fgui 式类型子类（`Button`/`Text`/`Image`/`RichText : Node`）
2. 命名 = 新增 `name` 属性，per-parent，同父重复编译期报错
3. 术语 `Node`（不用 Component）；Node 基类+容器（UITK 式）；`<div>` = 裸 Node
4. child-access：`GetChild(name)` 直系 + `GetChildAt`/`Children` 直系 + `Query<T>()` 递归；无 CSS 选择器串 query；无 `GetChildByPath`
5. 创建 = 实例化为主（`pkg.Instantiate`）+ `ui.Create` 工厂为辅
6. List = `data-list` + item 组件名 + fgui 单回调 `ItemRenderer`/`NumItems`
7. Window 出局（逻辑层）；TextInput 后续
8. 运行时 CSS 串（`SetStyle`）保留，不标注
9. 非泛型为主 + 泛型可选 C# 糖（脚本友好）
10. 重构范围：core/FFI 不动，旧 C# 用户面退役（降 internal 过渡），新 Node facade 上位，pkg.bin 加 `name` 字段（v16→v17）
11. layout = CSS-first（无 typed IStyle），authored + class 切换 + SetStyle；typed 仅内容/状态 + transform
12. transform = `node.Transform` 子对象（render 层偏移/动画），非主定位
13. box-sizing 默认 border-box
