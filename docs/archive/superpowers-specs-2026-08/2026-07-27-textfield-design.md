# 控件束 P2：TextField 全家（text/password/search + TextArea）

> 状态：设计稿（2026-07-27 brainstorming 共识）
> 范围：§4 控件束第二棒，文本输入控件 + IME（中文输入法）
> 对齐契约：`docs/design/main-design.md` §6、`docs/design/public-api.md` §7、`docs/design/projection-layer.md` §2
> 前序：P1（ProgressBar + Toggle + Slider，建立了 ControlState side table + 控件即容器 + 状态→子节点 inline 模型）

---

## 1. 背景与目标

P1 建立了控件地基（side table + 子节点注入 + 状态绑定 + set_transform 还债）。本棒做**文本输入**——input 全家的核心剩余，也是游戏 UI 高频需求（用户名、搜索、密码、聊天、表单）。

**本次做**：TextField + PasswordField + SearchField（单行三兄弟，同构）+ TextArea（多行）。三兄弟做完 TextField 即得（仅显示变换差异）；TextArea 复用同一编辑内核，加跨行导航与换行。

**IME（中文输入法）本轮就做**，架构分两层：
- core 端：composition 当 value 的"标记子串"渲染（拼进显示文本一起排版 + 画下划线）
- 后端采集：Unity 读 `Input.compositionString` + `Input.imeCompositionMode` + 光标坐标回灌 core

**对照参考**：RmlUi（`WidgetTextInput` 几何内核 + composition 标记子串架构）、FairyGUI（`InputTextField`：后端读 Unity 全局 `Input.compositionString` 极简采集 + core 拼渲染）。两个参考在 IME 上共识一致：**采集在后端，渲染在 core**，无纯后端方案（composition 串必须参与文本排版）。

## 2. 核心架构决策

### 2.1 渲染模型：TextField 当 leaf（取 RmlUi 内核，丢 DOM 外壳）

**决策**：TextField/Password/Search/TextArea 是**叶子节点**，自己渲染文本 + 光标 + 选区 + composition。**不**注入 `.loom-*` 文本子节点（偏离 P1 的"控件即容器"，但有充分理由）。

**理由**（对照 RmlUi 调研 + LoomGUI 约束）：
- RmlUi 的 `WidgetTextInput` 把文本做成 `#text` DOM 子元素，是**DOM 架构遗产**（RmlUi 是 DOM 引擎，文本必须是节点才能参与样式/事件）。但它的**几何内核**（断行、光标、选区位置）全在 Widget 自己手里（`SuppressAutoLayout` 关掉子元素自动布局 + `SetOffset` 手动定位 + `cursor_geometry`/`selection_geometry` Widget 直接画），子元素只是显示外壳。
- LoomGUI 没有开放 DOM 约束（TextNode 本来就是叶子，`text_contents → measure_text → glyph mesh`）。所以取 RmlUi 的几何内核、丢 DOM 外壳：**TextField 自己持有 TextLayout，自己渲染一切**。
- 避免时序难题：若注入动态文本子节点，光标位置要从子节点的 TextLayout 取，但 TextLayout 在子节点 render 时才 lazy 算，TextField（父）在 sync 阶段写光标位置时拿不到。leaf 模型下 TextField 在 layout 阶段自己 measure → TextLayout 全程在手，光标/选区几何 render 时直接取。

**与 P1 的关系**：P1 的 progress/slider/toggle 视觉子部分是**固定结构**（fill/track/thumb/check，骨架不变），适合注入。TextField 的"文本"是**动态内容**（随输入变化），不是固定视觉子部分——本质不同，不该套同一模式。

### 2.2 value 存储：ControlState side table（非 text_contents）

value 存 `ControlState` 的编辑态里（与光标/选区/composition 原子一起），**不**走 `scene.text_contents`。render 时从 ControlState 读 value。

理由：编辑内核自洽（value + cursor + selection + composition 是一个原子状态机，改 value 必然连带光标/选区更新）。走 text_contents 会割裂（text_contents 是普通文本节点的存储，不承载编辑态）。打包期初始 value 从 HTML `value` 属性 bake 进 `ControlInit`。

### 2.3 TextLayout：layout 阶段 measure（非 render lazy）

TextNode 现在 render 时 lazy measure（`scene.text_layouts.get().unwrap_or_else(measure_text)`）。TextField 改为 **layout/sync 阶段主动 measure 并缓存**到 `scene.text_layouts`——因为光标命中（点击定位光标）、光标几何、选区矩形都要用 TextLayout，不能等 render。

时序：`tick → process(命中用上帧 TextLayout) → solve → measure_text(写 text_layouts) → compute_world_transforms → render(画文本+光标+选区)`。光标几何 render 时从已缓存的 TextLayout 直接算。

### 2.4 IME 架构：core 标记子串 + 后端采集

照 RmlUi/FairyGUI 共识：
- **composition 不另开缓冲区**：composition 串在显示时**拼进 value**（`value[0..pos] + composition + value[pos..]`），core 文本布局正常排版（算 composition 串宽度、断行、光标位置）。composition 段额外画下划线。
- **后端采集极简**（FairyGUI 证实）：Unity 读全局 `Input.compositionString`（未确认串）+ `Input.imeCompositionMode = On/Off`（激活/关闭）+ `Input.compositionCursorPos = 光标 world 坐标`（摆候选窗）。core 不认识平台 IME。
- **FFI**：`set_composition`（未确认串 + 位置）/ `commit_composition`（提交落定）/ `get_cursor_rect`（给后端摆候选窗）。

⚠️ **唯一硬约束**：Unity IME 采集对不对，编码机验不了（`Input.compositionString` 要 Unity 运行时 + 真输入法），**必须家里机 PlayMode 验**。core 的标记子串渲染、光标、选区、拉丁字符输入都能在编码机 headless 全验。

## 3. 数据模型

### 3.1 ControlState 扩 TextField / TextArea 变体

复用 P1 的 `ControlTable`（side table，按 NodeId 索引）。新增两个变体：

```rust
pub enum ControlState {
    // P1 既有
    Progress { value: f32, max: f32, indeterminate: bool },
    Toggle { checked: bool },
    Radio { checked: bool, name: String },
    Slider { value: f32, min: f32, max: f32, step: f32, dragging: bool },
    // P2 新增
    TextField(EditState),
    TextArea(EditState),
}

/// 文本编辑内核（单行/多行共用，靠 ControlState 变体 + NodeKind 分行为）。
pub struct EditState {
    /// 已提交文本（UTF-8）。真相源——光标/选区/composition 都是对它的字节偏移。
    pub value: String,
    /// 光标字节偏移 [0, value.len()]。 UTF-8 边界（总在字符首字节）。
    pub cursor: usize,
    /// 选区锚字节偏移（shift 点击 / 拖拽扩展用）。选区 = [min(anchor,cursor), max]。
    pub anchor: usize,
    /// IME 未确认串 + 在 value 中的插入位置。None=无 composition（正常态）。
    /// 渲染显示文本 = value[0..pos] + composition + value[pos..]。
    pub composition: Option<Composition>,
    /// 最大字符数（UTF-8 字符数，非字节）。0 = 无限。
    pub max_length: usize,
    pub readonly: bool,
    /// 光标闪烁态（cursor_timer 累加达阈值翻转 cursor_visible）。
    pub cursor_visible: bool,
    pub cursor_timer: f32,
    /// 理想光标 x（上下行移动时 sticky，照 RmlUi ideal_cursor_position）。
    pub ideal_cursor_x: f32,
}

pub struct Composition {
    pub text: String,   // 未确认串（如拼音 "nihao"）
    pub pos: usize,     // 插入位置（value 字节偏移）
}
```

- 单行（TextField/Password/Search）与多行（TextArea）共用 `EditState`，靠 `ControlState` 变体 + `NodeKind` 在 control.rs 分行为（sanitize 换行、LineBreak 语义、跨行导航）。
- 所有字节偏移严格遵守 UTF-8 边界（光标移动时 `MoveCursorToCharacterBoundaries` 钳到字符首字节，照 RmlUi）。

### 3.2 行模型（光标几何用）

照 RmlUi 的 `Line` struct（光标数学的黄金数据）。LoomGUI 的 `TextLayout.lines` 已有 `Line { y, height, baseline, width, runs }`，但缺"value 内字节偏移"。本棒给光标命中加一个**派生查询**（不持久化，命中时算）：

```rust
/// 给定 TextLayout + value，算每行的 value 字节范围（供光标命中/跨行导航）。
/// 复用 layout 期的断行信息——measure_text 断行已知每行覆盖哪些字符。
fn line_byte_ranges(layout: &TextLayout, value: &str) -> Vec<(usize, usize)> { ... }
```

（实现细节：**光标命中/几何计算时基于 TextLayout 的 glyph codepoint 派生算**——遍历 lines→runs→glyphs，按 codepoint 还原字符序列与 value 字节对齐，切分每行字节范围。免改 `measure_text` 签名，零持久化。）

### 3.3 密码 / 搜索变体（显示变换）

- **PasswordField**：渲染时 value 经 `transform_value` 掩码（`'•' × char_count`，照 RmlUi `TransformValue`，不改长度）。光标/选区/composition 几何按掩码文本算（字符等宽，几何更简单）。value 本身存明文。
- **SearchField**：与 TextField 同语义表面（public-api 已拆类型为 attribute-selector `[type=search]` 精确匹配服务，运行时 API 一致）。本轮无额外行为，纯 NodeKind 分派。
- **TextField**：正常显示 value。

NodeKind 分派在 control.rs / render 的 `transform_value(kind, &value) -> String`。

## 4. 文本渲染链路

### 4.1 render 加 NodeKind::TextField/Password/Search/TextArea arm

当前 render 只有 `Image` 和 `TextNode` 两个 arm，TextField 落到 Container 默认分支只画背景框（**现状 = 空盒子**）。本棒加文本 arm：

```rust
NodeKind::TextField | NodeKind::PasswordField | NodeKind::SearchField | NodeKind::TextArea => {
    // 1. 从 ControlState 读 EditState
    // 2. 算显示文本 = transform_value(kind, value_with_composition)
    // 3. 取 layout 阶段缓存的 TextLayout（已 measure）
    // 4. 画背景/border（复用 Container 的 mesh 路径）
    // 5. 画文本 glyph mesh（复用 build_text_mesh）
    // 6. 画选区背景 mesh（若有选区）
    // 7. 画 composition 下划线 mesh（若有 composition）
    // 8. 画光标 mesh（若 focused + cursor_visible）
}
```

- value_with_composition：`value[0..comp.pos] + composition.text + value[comp.pos..]`（无 composition 则纯 value）。
- 密码掩码在拼 composition 前应用（掩码只作用于 value，composition 串不掩码——用户正在输入的拼音该可见）。

### 4.2 TextLayout layout 阶段 measure

stage tick 在 solve 之后、render 之前插入"TextField measure"步骤（遍历有 EditState 的节点，measure 显示文本写 `scene.text_layouts`）。measure 复用现有 `measure_text`（content area 宽度、font、line_height 等从节点 style 取，同 TextNode）。

TextArea 多行：`white_space_nowrap=false`（允许断行）；单行：内容超出时由后端滚动跟随光标（defer，见 §11）。

### 4.3 光标 / 选区 / composition 几何与 mesh

全部在 TextField 的 render arm 内画（纯 mesh，非子节点）：
- **光标**：1px 竖条 quad。位置 = 字符 offset 对应的 glyph x（从 TextLayout 二分查），高度 = line_height，y = 光标所在行 y。颜色 = `caret-color`（CSS，缺省 = 文本色）。
- **选区背景**：每行选区内字符的矩形 quad（按行拆，照 RmlUi `selection_composition_geometry`）。颜色 = `selection-background`（CSS，缺省 = 蓝色半透明）。
- **选中文本**：选区内 glyph 用反色画（`selection-color`，缺省 = 白）——render 时按选区范围分两段着色。
- **composition 下划线**：composition 段每行底部 2px 下划线 quad（照 RmlUi `COMPOSITION_UNDERLINE_WIDTH`）。

字形位置查询（光标命中 + 光标几何共用）：
```rust
/// 字节 offset → 光标像素 x（在 TextLayout 指定行的 glyph 里二分累加 advance）。
fn cursor_pixel_x(layout: &TextLayout, line_byte_ranges: &[(usize,usize)], offset: usize) -> (f32, usize) // (x, line_index)
/// 像素 (x, y) → 字节 offset（点击命中：先 y 定行，再 x 在行内 glyph 二分取最近邻）。
fn hit_byte_offset(layout: &TextLayout, line_byte_ranges: &[(usize,usize)], x: f32, y: f32) -> usize
```
用已有 glyph.advance 做 O(log n) 二分（优于 RmlUi 的 O(n) 重测宽）。

## 5. 字符输入通道

### 5.1 现状

已有 `KeyEvent`（key_code + modifiers + is_down）+ `set_key_input` FFI + `process_keys`（keydown/up + Tab 导航）。`key_code` 是 Unity KeyCode 透传，**只有物理键，无字符语义**——可打印字符要从 keydown 映射（拉丁可行），中文/日文必须走 IME 字符通道。

### 5.2 新增字符输入 FFI（textinput 通道）

新增"可打印字符"注入通道（与 keydown 分离，照 RmlUi `textinput` 事件模型）：

```rust
/// 注入本帧可打印字符（UTF-32 codepoint 数组）。后端把已 shift 映射好的字符传进。
/// 仅作用于 focused 的 TextField；无焦点丢弃。tick 前调，与 set_key_input 同期。
#[csbindgen]
pub unsafe extern "C" fn loomgui_stage_set_text_input(h, codepoints: *const u32, len: usize) -> i32
```

- 后端（Unity）：`Event.character`（OnGUI 模式）或 InputSystem 字符回调 → 累积 codepoints → `set_text_input` 注入。非可打印字符（控制字符）不进此通道（走 keydown）。
- core process：若 focused 节点是 TextField/TextArea 且非 readonly → 在 cursor 处插入字符（先删选区），检查 max_length，触发 ValueChanged。

### 5.3 控制键（keydown，复用现有 KeyEvent）

TextField focus 时，`process_keys` 把控制键路由给编辑内核（control.rs）：
- 方向键：左右移动光标（ctrl=词、shift=选区扩展）；上下（TextArea）跨行移动（按行盒 y + sticky ideal_x）
- Home/End：行首/行尾（ctrl=文档首/尾）
- Backspace/Delete：删字符（ctrl=删词）
- Enter：单行=Submitted 事件；TextArea=插入 `\n`
- Escape：失焦（blur）
- Ctrl+A：全选；Ctrl+C/X/V：复制/剪切/粘贴（剪贴板走后端 FFI，见 §5.5）
- Tab：不消费（照 RmlUi，留给焦点导航）

### 5.4 IME composition FFI

```rust
/// 设置 IME 未确认串 + 插入位置（后端读 Input.compositionString 回灌）。
#[csbindgen]
pub unsafe extern "C" fn loomgui_stage_set_composition(h, node: u32, text: *const u8, text_len: usize, pos: usize) -> i32
/// 提交 composition（落定为正式文本）。后端 Input.compositionString 变空时调。
#[csbindgen]
pub unsafe extern "C" fn loomgui_stage_commit_composition(h, node: u32) -> i32
/// 读光标世界矩形（给后端摆 IME 候选窗：Input.compositionCursorPos）。
#[csbindgen]
pub unsafe extern "C" fn loomgui_stage_get_cursor_rect(h, node: u32, out: *mut CursorRectRepr) -> i32
```

core 收到 set_composition → 写 EditState.composition → 下帧重 measure（composition 串参与排版）→ render 画下划线。commit → 把 composition 串落定进 value（插 cursor 处）+ 清 composition + 触发 ValueChanged。

### 5.5 剪贴板

照 RmlUi 走 `SystemInterface`——core 不直接访问系统剪贴板，经 FFI 让后端（Unity `GUIUtility.systemCopyBuffer`）读写：
```rust
/// 后端实现：把文本写系统剪贴板 / 读系统剪贴板。core 经此与剪贴板交互。
#[csbindgen]
pub unsafe extern "C" fn loomgui_clipboard_set(text: *const u8, len: usize) -> i32
pub unsafe extern "C" fn loomgui_clipboard_get(out: *mut *const u8, out_len: *mut usize) -> i32
```
（core 侧调这两个 FFI 完成 copy/cut/paste。后端实现里转 Unity `systemCopyBuffer`。本轮做基础接通，defer 富文本剪贴板。）

## 6. 编辑操作（control.rs）

照 RmlUi `WidgetTextInput` 的编辑原语，core 实现（基于 EditState + TextLayout）：

- **插入字符**（textinput / commit）：删选区 → 在 cursor 插入 → cursor 前进 → max_length 截断 → ValueChanged
- **删除**（backspace/delete/选区）：删字符/词/选区 → cursor 收缩 → ValueChanged
- **导航**：移动 cursor（Left/Right/Word/Home/End/LineUp/LineDown），shift 扩展选区（更新 anchor 或 cursor）
- **选区操作**：全选 / 选词（双击，CharacterClass 词边界）/ 删选区 / 复制 / 剪切
- **sanitize**（单行）：value/输入剥 `\r\n\t`（TextArea 保留 `\n`）
- **UTF-8 边界**：cursor 移动后钳到字符首字节（`MoveCursorToCharacterBoundaries`）

**焦点门**：所有编辑操作仅当 `scene.focused_node == Some(this)` 且 `!readonly` 时生效。

## 7. 光标闪烁

`cursor_timer` 每帧累加 dt（tick 时序里），达 0.7s 翻转 `cursor_visible`、重置 timer。
- 任何编辑操作（cursor 移动/输入）重置 timer + 强制 `cursor_visible = true`（照 RmlUi：编辑时立即显示）。
- 失焦停止闪烁（cursor_visible = false）。
- ⚠️ **并入单一动画时钟不变量**：cursor_timer 推进走 Stage tick（不另起定时器），与 TweenManager 同源。TweenManager 不驱动光标（光标不是 tween），但时钟基准一致。

## 8. FFI + 攒批

### 8.1 新增 FFI 清单

| 命令 | 用途 |
|---|---|
| `set_control_text(node, text_ptr, len)` | 写 value（C# `TextField.Value` setter）。**区别于 `set_text`（改 text_contents）**：TextField/TextArea 不用 text_contents，value 存 EditState，此 FFI 改 EditState.value |
| `get_control_text(node, out_ptr, out_len)` | 读 value |
| `set_selection(node, start, end)` / `get_selection` | TextSelection 读写 |
| `set_control_placeholder(node, text_ptr, len)` / `get_*` | placeholder |
| `set_control_readonly(node, bool)` / `set_control_maxlength` | readonly / max_length |
| `set_text_input(codepoints, len)` | 字符输入通道（§5.2） |
| `set_composition` / `commit_composition` / `get_cursor_rect` | IME（§5.4） |
| `clipboard_set` / `clipboard_get` | 剪贴板（§5.5） |

- 复用 P1 的 return-code + out-param 模式（字符串 ptr+len）。
- `set_control_text` 对 TextField 节点改写 EditState.value（非 text_contents）。

### 8.2 攒批

本棒**先即时过桥**（仿 4a/P1 的 StyleMirror）：C# setter 即时调 FFI。文本输入是用户节奏（非每帧高频动画级），即时 FFI 开销可接受。真正攒批优化标 ponytail，profile 出热点再做。projection-layer §2.1 明示"升级攒批只改 setter 调用时机，不推翻镜像结构"。

## 9. C# 投影层

填 `Public/LoomGUI.Nodes.cs` 的 NE 壳（TextField/PasswordField/SearchField/TextArea 各 Value/Placeholder/Selection/ReadOnly/Disabled + ValueChanged/Submitted）：
- getter/setter 转发 FFI（Value → `set/get_control_text`，Selection → `set/get_selection`）。
- Disabled：照 P1（disabled 控件不响应输入 + 视觉灰化 defer 到视觉束）。
- Submitted（单行回车）：接 EVT_SUBMITTED demux。
- ValueChanged：接 EVT_VALUE_CHANGED demux（已有，复用 ValueChangedEvent<string>）。
- EventType.cs 加 `Submitted`（新 EVT 常量）。

## 10. 事件

### 10.1 新增 EVT 常量

```rust
pub const EVT_SUBMITTED: u8 = 25;   // 单行回车（TextArea 不触发）
// EVT_VALUE_CHANGED (22) 复用（value 变化）
// EVT_FOCUS_IN/OUT (14/15) 复用（聚焦/失焦）
// EVT_KEY_DOWN/UP (12/13) 复用
```

### 10.2 事件产生

- ValueChanged：value 每次改变（输入/删除/IME 提交/setter）→ EventRecord
- Submitted：单行 Enter → EventRecord（TextArea 不触发）
- FocusIn/Out：focus/blur 时（复用现有 focus 机制）

## 11. 不在本次（defer）

- **光标选词双击**（CharacterClass 词边界扫描）：本轮只做基础点击定位 + 全选，选词 defer
- **undo/redo**：编辑历史栈，独立机制，defer
- **滚动跟随光标**（TextField 内容超出时水平滚动 / TextArea 垂直滚动）：复用 ScrollPane，但光标 in-view 算法（照 RmlUi `MoveToCursor`）defer——本轮 TextField 固定宽度不滚动，内容超出裁剪
- **移动端 TouchScreenKeyboard / WebGL 原生输入**：照 FairyGUI `!supportsCaret` 分支，移动端走原生输入框（framework 只接收最终文本）。本轮只做桌面 IME，移动端 defer
- **Placeholder 伪类 `:placeholder-shown`**：本轮 placeholder 渲染做（空 value 显示 placeholder 文本 + 灰色），但 `:placeholder-shown` 伪类选择器匹配 defer
- **富文本剪贴板**（带样式的 copy/paste）：本轮纯文本
- **bidi（RTL 语言）**：LoomGUI 文本栈纯 LTR，bidi 整体 defer（游戏 UI 主场景中/英/日全 LTR）
- **Disabled 灰化渲染**：视觉束
- **max_length 值域打包期校验**：本轮只认属性，值域校验后置

## 12. showcase 覆盖

showcase 多页有 input/textarea（form/settings/mail）：
- **form.html**：text/password/search/textarea 全覆盖，配标准 CSS（边框/光标色/选区色/placeholder）
- **settings.html**：search 搜索框 + text 输入
- **mail.html**：textarea（写邮件正文）
- 每页配**教学 CSS**（`caret-color` / `selection-background` / `::placeholder`），用户可参考

交互演示（headless 可验的）：拉丁字符输入、光标移动、选区拖拽、密码掩码、回车提交。中文 IME 演示标"需家里机"。

## 13. 验收标准

1. **headless 单测（core 层）**：
   - HTML value 属性 → bridge → ControlInit → instantiate → EditState 初始值正确
   - 字符插入/删除/导航/选区（拉丁字符全链路）
   - 光标命中（点击 → byte offset）+ 光标几何（byte offset → 像素 x）
   - 密码掩码显示
   - TextArea 换行 + 跨行光标移动
   - composition 标记子串渲染（显示文本拼接 + 下划线 mesh）
   - SingleLine Enter → Submitted；TextArea Enter → 插入 \n（不 Submitted）
   - max_length 截断 + UTF-8 边界
2. **PublicApi 编译门**：TextField/Password/Search/TextArea 壳填实，编译通过
3. **showcase Unity PlayMode（家里机）**：form/settings/mail 各输入控件可交互；**中文 IME 输入**（拼音候选 → 提交）真机验
4. **围栏校验**：input/textarea 无 CSS 命中 → 打包期报错 + 教学（复用 P1 的控件 CSS 命中校验，扩到文本控件）

## 14. pkg 版本

bump **v24 → v25**（ControlInit 加 TextField/TextArea 变体 + placeholder/max_length/readonly/value 字段，bincode 布局变）。一刀切升，不留后向兼容（个人项目惯例）。加 bincode 稳定性测试。

## 15. 关键决策记录

- **leaf 渲染（取 RmlUi 内核丢 DOM 外壳）**：对照 RmlUi 源码——文本子元素是 DOM 遗产，几何内核在 Widget；LoomGUI 无 DOM 约束，直接把内核做进 TextField，避免文本子节点的时序难题。
- **value 存 ControlState（非 text_contents）**：编辑内核原子自洽（value + cursor + selection + composition 一起）。
- **IME = core 标记子串 + 后端采集**：RmlUi + FairyGUI 两个参考共识。FairyGUI 证实后端采集极简（读 `Input.compositionString` 全局属性），非大工程。
- **字符输入独立通道（textinput）**：照 RmlUi/web 模型，keydown 处理控制键、textinput 处理字符。现有 KeyEvent 只有物理键，必须新建字符通道（拉丁字符 + IME 提交都走它）。
- **光标/选区/composition render 层画（非子节点）**：TextField 自持 TextLayout，几何 render 时直接取，纯 mesh 无 DOM 节点开销。
- **UTF-8 字节偏移做光标索引**：照 RmlUi，光标/选区/composition 全用字节偏移，严格遵守字符边界。
- **光标闪烁并入单一时钟**：cursor_timer 走 Stage tick，不破 TweenManager 单一时钟不变量。
- **即时过桥不攒批**：文本输入是用户节奏非动画高频，profile 出热点再攒批。
