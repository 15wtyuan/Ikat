## Task 7 Report: set_rich_text 动态 API（core + FFI）

**状态**：完成并提交。commit `e848892`。

### Step 1-3: 代码修改

三文件各加一个函数，均在 `set_text` 后插入，照模板（mirror）：

| 文件 | 新增函数 | 行附近 |
|---|---|---|
| `loomgui_core/src/scene/dynamic.rs` | `pub fn set_rich_text(scene, node, markup) -> Result<(), String>` | ~228 |
| `loomgui_core/src/stage.rs` | `pub fn set_rich_text(&mut self, node, markup) -> Result<(), String>` | ~508 |
| `loomgui_ffi_c/src/lib.rs` | `pub extern "C" fn loomgui_stage_set_rich_text(...) -> i32` | ~1122 |

**core 逻辑**：从 `node.base_style` 取 color/font_size 构造 `RichBaseStyle`（weight/style/deco 用默认），调 `parse_rich_markup`（font_id=0 占位），非 RichText 节点返回 Err，成功设 `dirty_text = true`。

**FFI 安全**：null 句柄早返 -1；`from_utf8(..).unwrap_or("")` 非 UTF-8 降级；失效 NodeId → core Err → map 回 -1。无反身 panic。常驻（不 `#[cfg(feature = "parse")]` gate）。

### Step 4: 编译 + 符号验证 + 拷贝

```
cargo build -p loomgui_ffi_c --release  → 成功（21.59s）
```

**符号查找（PowerShell byte-scan）**：
```
FOUND: loomgui_stage_set_rich_text at offset 1999644
```

**MD5 核对**：
```
src: 121A7F52CC6FC1CEBD7B0BB369C40889
dst: 121A7F52CC6FC1CEBD7B0BB369C40889
MD5 MATCH
```

### Step 5: C# 绑定确认

```
grep -c loomgui_stage_set_rich_text Bindings/LoomGUIBindings.cs → 2
```

csbindgen build.rs 自动重新生成了正确的 P/Invoke：
```csharp
[DllImport(__DllName, EntryPoint = "loomgui_stage_set_rich_text", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
internal static extern int loomgui_stage_set_rich_text(StageHandle* h, uint node, byte* markup_ptr, nuint markup_len);
```

### Step 6: 回归测试 + fmt + clippy

- **cargo test**：全部通过（core/pkg/ffi 全 workspace）。
- **cargo fmt --all**：clean（auto-format 一行调整）。
- **cargo clippy --all-targets -- -D warnings**：clean。

### 文件变更

| 文件 | 操作 |
|---|---|
| `loomgui_core/src/scene/dynamic.rs` | modify（+26 行） |
| `loomgui_core/src/stage.rs` | modify（+5 行） |
| `loomgui_ffi_c/src/lib.rs` | modify（+22 行） |
| `loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll` | modify（新 .dll） |
| `loomgui_unity_package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs` | modify（csbindgen 重新生成，+2 处新绑定） |

### 后续验收（PlayMode）

push 后家里机验证：
- `set_rich_text` 运行时换内容正确重排（chat 动态场景）
- 多色多字号文本换行正确、字距对
