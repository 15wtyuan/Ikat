# 渐变补齐：radial + linear 多 stop / 任意角度（里程碑 1 · 任务 3）

- 日期：2026-08-14
- 状态：**已实施**（代码侧全绿：cargo workspace 1533 + dotnet 410；pkg v34 / blob v13 / dll / GUI exe / showcase pkg / Chrome 基准均已同步入库。Unity 视觉验收留家里机，随里程碑 1 任务 4 逐页过）
- 范围决策：radial + linear 多 stop + 任意角度做；**conic / repeating-\* 显式 defer**（判据：第一个真需要的 UI）
- 参考对照：`temp/FairyGUI-unity/` 已查——fgui 渐变纯顶点色（`TextFormat.gradientColor` → 四角 vertexColors），无 per-fragment radial 可抄；radial shader 方案为本仓自定。

## 1. 动机与验收载体

- 里程碑 1 门点名「home radial 光晕可见 + 多 stop 渐变对齐浏览器」。home `.root` 现有被静默丢弃的 `radial-gradient(1100px 560px at 82% -12%, rgba(95,180,212,0.10), transparent 60%)` —— 恰好是完整语法压力测试（椭圆双半径 + 负百分比定位 + 带位置 stop + `transparent` 关键字）。
- 测试载体：showcase lab 页新增「12 · 渐变」专区（多 stop / 任意角度 / radial 变体全覆盖，纯 workspace 编辑 + 重打包，零 Unity 接线）；home 光晕自动点亮作第二测试点。
- 编码机验收：TDD 单测（fence/core/FFI）+ lab 页 Chrome headless 截图入库作视觉基准 + `dump_page --json` 输出渐变参数（Unity 侧不对时可定位 core vs shader）。

## 2. 语法终态（围栏子集）

### linear-gradient

```
linear-gradient( [ <angle> | to right | to left | to top | to bottom ] , <stop> [, <stop>]* )
<angle>   := <number>deg          （CSS 语义：0deg = to top，顺时针）
<stop>    := <color> [ <percentage> ]?    （色：hex 3/4/6/8、rgb()/rgba()、transparent；位置省略 → 默认）
```

- 方向关键字归一化为角度：`to top`=0 / `to right`=90 / `to bottom`=180 / `to left`=270。
- stop 数 1..=8（首 stop 位置默认 0%，末默认 100%，中间默认相邻两已定位置的中点——CSS 规范算法）。
- **defer**：`to top right` 等角点关键字、`repeating-linear-gradient`、命名色（除 `transparent`）、>8 stops。围栏外 → inline style 打包期 `FenceBadCssValue`；`<style>` 规则运行时静默忽略（与现行为一致）。

### radial-gradient

```
radial-gradient( [ <shape> || <size> ]? [ at <position> ]? , <stop> [, <stop>]* )
<shape>   := circle | ellipse     （默认 ellipse）
<size>    := closest-side | farthest-side | closest-corner | farthest-corner | <length>{1,2}
<position> := <percentage> | <length>   （cx, cy 各一；默认 50% 50%）
```

- 显式双长度 = 椭圆 rx,ry；单长度 = 圆半径（rx=ry）。尺寸/圆心在 **core 渲染期**按当帧 box 解析成像素（box 尺寸 solve 后才知）。
- stop 语义与 linear 相同；t = 椭圆归一化距离 `sqrt((dx/rx)² + (dy/ry)²)`。
- **defer**：`repeating-radial-gradient`、`at` 里多 token 位置（`center top` 等双关键字写法）。

## 3. 数据模型（core，替换 `Gradient2`）

`crates/core/src/style/resolved.rs`：

```rust
pub struct GradientStop { pub color: [f32; 4], pub pos: Option<f32> } // None = 默认位置
pub enum RadialSize { ClosestSide, FarthestSide, ClosestCorner, FarthestCorner,
                      Explicit(Option<f32>, Option<f32>) } // 单/双长度
pub enum Gradient {
    Linear  { angle_deg: f32, stops: Vec<GradientStop> },
    Radial  { size: RadialSize, center: (Option<f32>, Option<f32>), /* pct 或 px，None=50% */ stops: Vec<GradientStop> },
}
// ResolvedStyle.background_gradient: Option<Gradient2> → Option<Gradient>
```

- `GradientDir` enum（4 正向）退役：方向归一化进 `angle_deg`。`gradient_corner_colors` / `Gradient2` bincode roundtrip / `gradient_dir_is_one_byte` 测试随之改写。
- 序列化仍走 `ResolvedStyle` 整体 bincode → 布局变化 → **PKG_FORMAT_VERSION 33 → 34**（MIN=MAX=34），showcase 重打。
- 解析挂载：`mapping.rs` 新 `parse_gradient`（linear/radial 双前缀分派），落在 `parse_linear_gradient_2` 原位置；`background-image` / `background` 两个 arm 改调它。颜色复用 `parse_color`（**`transparent` → `[0,0,0,0]` 加进 `parse_color`**，其余命名色仍拒收）；deg 解析照 `func_to_matrix` 的 `trim_end_matches("deg")` 先例。
- fence：schema `CssValueParser::Gradient2` 更名 `Gradient` 并真正挂到 `background-image` 上，`css_rules` 声明值增加围栏探针（调 core 解析函数，失败产 diagnostic）——`<style>` 里的坏渐变从运行时静默变打包期报错。

## 4. 渲染模型：统一 program=6 per-fragment

**所有背景渐变（linear 含旧 2 色 4 向）统一走新 program=6**，退役顶点色渐变路径（`quad_gradient` + `gradient_corner_colors` 不再用于背景）：

- mesh：普通直角 quad（`mesh::quad`），**uv 通道改载 box 局部像素坐标**（左上原点；照 SHADOW_BLUR 把几何编码进 uv 的 raw-uv 直通先例）。
- 帧末由 box 解析出渐变几何常量（linear：投影 t0/inv_span；radial：cx/cy/rx/ry 像素），随 blob v13 新列下发。
- shader 按常量算 t（linear：`(dot(p,dir) − t0) × inv_span`，t0/inv_span 由 4 角在梯度轴上投影的 min/max 归一——严格 CSS 语义；radial：椭圆归一化距离），再对 stops 分段 lerp，clamp t ∈ [0,1]（非 repeating）。
- **premultiplied alpha 插值**：stops 先预乘再 lerp、输出前反预乘（CSS 渐变语义，`rgba→transparent` 无灰边；Chrome 基准按此对齐）。
- 文本渐变（`background-clip:text`）：CPU 采样泛化——`sample_gradient(g, geom, x, y)` 共享同一套 t 数学（Rust 侧），每字形 4 角采样替代现 4 向 lerp（`gradient_glyph_colors` 改写）。字形内 stop 边界横跨时为逐角近似（装饰性文本可接受）。
- **背景色垫底**：渐变与 `background-color` 并存时（home 案例），渐变 quad（program=6）下面垫一层纯色 quad（program=0）——照 box-shadow 多节点合成先例（sort_key 相邻）。现状是渐变吃掉底色，属 bug 一并修。
- **维持互斥**：渐变 × 圆角 / 九宫格 / 背景图仍互斥（现 `use_gradient` 门不变）。渐变 × 圆角 defer 登记（判据：第一个圆角渐变 UI）。

## 5. FFI blob v13（VERSION 12 → 13，22 → 23 列）

新列 `grad_params[208B]`（定长内联，照 effect_block 128B 先例；不做 arena/intern——实现简单性优先，每帧 ~百节点 × 208B 拷贝可忽略）：

| offset | 类型 | 字段 | 说明 |
|---|---|---|---|
| 0 | u32 | kind | 0=linear, 1=radial |
| 4 | f32 | angle_deg | linear（调试/dump 用） |
| 8 | f32×2 | dir_x, dir_y | linear 梯度轴单位向量（屏幕 y 向下） |
| 16 | f32×2 | t0, inv_span | linear 4 角投影归一化常数 |
| 24 | f32×2 | cx, cy | radial 圆心（box 局部 px） |
| 32 | f32×2 | rx, ry | radial 半径 px（>0，下限 epsilon） |
| 40 | u32 | stop_count | 1..=8 |
| 44 | u32 | reserved | 0 |
| 48 | f32×40 | stops[8] | 每stop {r,g,b,a,pos} ×5 f32，未用置 0 |

- 双端同步：`blob.rs` VERSION=13 + 列写入/断言；`FrameBlob.cs` ExpectedVersion=13 + `GradParams(i)` 读取器；ABI size_of 断言进 ffi tests。
- `dump_page --json` 扩展：有渐变的节点附带解析后参数（kind/angle/stops/几何常量），供 Unity 侧视觉不对时定位 core vs shader。

## 6. Unity 侧（本轮全写；C# 过 `tests/dotnet/LoomGUI.PublicApi` 编译门，shader 纸面保证）

1. **`LoomGUI-Unlit.shader`**：加 `#pragma multi_compile _ GRADIENT` 变体（单变体，kind 走 uniform 分支——uniform 分支 per-draw 一致，无代价）。uniform：`_GradKind/_GradGeom(dir,t0,span)/_GradGeom2(cx,cy,rx,ry)/_GradStopCount` 进 `UnityPerMaterial` CBUFFER（SRP batcher 要求）；`_GradStopColors[8]`(float4) + `_GradStopPos[8]`(float) HLSL 数组，MPB `SetVectorArray`/`SetFloatArray`（照 effect_block 的 MPB 先例；Properties 声明坑见 MaterialManager 注释）。frag：GRADIENT 分支算 t → 分段 lerp（premultiplied）→ 输出；uv 为 raw 局部坐标不 TRANSFORM_TEX。
2. **`MaterialManager`**：program=6 → `EnableKeyword(GRADIENT)`（key 已含 program，无新增 Material 维度）。
3. **`MirrorPool`**：`Program(i)==6` → 读 `GradParams(i)` 写 MPB（照 program=5 SHADOW_BLUR 模式）。
4. **`FrameBlob`**：v13 列偏移 + 读取器（22 列注释表同步）。

## 7. showcase lab 页「12 · 渐变」专区（覆盖矩阵）

照 section 11 box-shadow 矩阵写法（类前缀 `grad-`，每格 `<span>` 说明标签）：

- linear：2 色基线（回归）｜多 stop 3/5｜中间 stop 省位置（默认中点）｜0/45/90/137/270deg｜`to top`（关键字归一）。
- radial：默认（ellipse farthest-corner）｜circle｜closest-side / farthest-side｜双长度椭圆（home 同款 1100×560 at 82% -12%）｜圆心 px 定位｜`rgba→transparent`（验 premultiplied 无灰边）｜background-color 垫底共存。
- 渐变字（background-clip:text）多 stop 变体一格。

## 8. TDD 测试清单（先写测试）

- `mapping/tests.rs`：多 stop / 45deg 解析✓（**翻转现 `multi_stop_rejected` / `diagonal_angle_rejected`**）；stop 默认位置算法（首 0/末 100/中间中点）；radial 各形（circle+关键字 / 双长度 / at %/px / 负百分比）；`transparent` stop；conic / repeating / 9 stops / 角点方向 / 坏语法拒收。
- `resolved.rs`：`Gradient` bincode roundtrip（linear/radial 全形）。
- `render/tests.rs`：program=6 mesh（uv=局部坐标）；grad_params 常量正确性（45deg t0/inv_span、radial cx/cy/rx/ry 解析）；premultiplied 采样（rgba→transparent 中点=半透明纯色）；bg-color 垫底层存在；文本渐变多 stop 逐角采样。
- `asset`：pkg v34 roundtrip + v33 拒读（TooOld）。
- `ffi`：blob v13 列 offset/size 断言 + grad_params roundtrip。
- fence：`doc_schema_sync` 通过（fence.md 渐变语法段更新 + shipped 副本字节同步）；`<style>` 坏渐变打包期报错。

## 9. 收尾清单（防漂移）

- `docs/design/fence.md` 渐变语法段重写（§5.2 值表 `Gradient2` 行更新）+ 字节级 cp 到 `unity/package/Editor/Resources/LoomGUI/skill/references/fence.md`。
- fence schema 改动 → **重出 GUI exe**（`tauri build --no-bundle` + 拷 `unity/package/Editor/Tools/`）。
- pkg bump → `.dll` 重编 + `cargo run -p xtask -- sync-bindings` + 重打 showcase pkg。
- Chrome headless 截图基准（lab 页 + home 页）入库。
- `docs/roadmap/roadmap.md` 任务 3 状态同步（代码侧 done、Unity 视觉验收留家里机）；延期项表「渐变补全」条目核销（conic/repeating 转入 defer 判据）。

## 10. 风险

- shader 无法本机编译验证：语法错误留 Unity 机（改几行的成本）；方案性风险（SRP batcher/CBUFFER/MPB 数组）已按仓内先例规避，MaterialManager 注释里的 Properties 声明坑实施时逐一对照。
- Chrome 基准 vs Unity 渲染的色空间差（坑 197 同源）：基准只作人眼/像素参照，不做逐字节门。
- 每 stop 5×f32 ×8 的 208B 列使帧 blob 变大 ~60%（effect_block 128B 先例内）——若 profiling 实证热点，后续可转 % 级 CSS 参数 + arena intern（本 spec 显式不做）。
