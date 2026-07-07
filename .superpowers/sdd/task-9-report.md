# Task 9 报告：dump example 验证 + 家里机 PlayMode 全验收手交

**Status**: DONE
**Commit**: `03e463e`（branch: `feat/v1.6-font-to-core`）

## 1. dump_text 扩展

### 扩展内容

在现有 `dump_text.rs`（原本验证文本换行诊断）基础上增加 v1.6 GlyphAtlas 独立验证段。验证段在当前文本 dump 前运行，**不依赖 pkg.bin**——直接用测试字体文件（`wqy-microhei.ttc`）构造 Font + GlyphAtlas，独立 exercise atlas API。

验证项目：
- **ensure 'H' @32px**：返回 page/UV/px_w/px_h
- **dirty_pages**：ensure 后非空
- **page_bytes(0)**：4096x4096 R8 字节 = 16,777,216
- **.notdef (gid 0)**：tofu 路径确保返回非零尺寸
- **多 size**：'H' @48px vs @32px 走不同槽位（不同 UV）
- **CJK 字形**：'中' @32px 可分配
- **page_bytes OOB**：大 page 号返空切片不 panic

### 实际运行输出

```
─── v1.6 GlyphAtlas 验证 ───
ensure 'H' @32px  page=0  uv=(0.0000,0.0000)-(0.0046,0.0061)  px=19x25
dirty_pages: [0]  (len=1)
page0: 4096x4096  16777216 bytes (R8, expected 16777216)
.notdef(gid0) @32px  page=0  uv=(0.0046,0.0000)-(0.0085,0.0061)  px=16x25
dirty_pages after .notdef: [0]
page_bytes(999) OOB: (0, 0, 0)  -- OK
ensure 'H' @48px  page=0  uv=(0.0000,0.0078)-(0.0066,0.0168)  px=27x37
ensure '中' @32px  page=0  uv=(0.0085,0.0000)-(0.0154,0.0078)  px=28x32
─── GlyphAtlas 验证通过 ───
```

所有 assertion 通过。UV 在 [0..1] 范围内，page_bytes 非空，dirty_pages 正确标记，多 size 分片正确，OOB 安全。

### 文本 dump 部分

pkg.bin 加载后 `tick_and_render` 返回 scene=None。这是预期行为：v1.6 的 blob v10 格式变化（NodePayload 删 Text 变体）导致旧 pkg.bin 不再兼容，需重建 pkg。这不影响 atlas 验证——atlas 段完全独立于 pkg。

## 2. fence_contract 围栏门

```
running 25 tests
test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured
```

全部通过。

## 3. 全回归

```
543 + 25 + 2 + 3 + 5 + 2 + 62 + 6 + 10 = 658 passed, 0 failed
```

全部通过。

## 4. 文件变更 + Commit

- **修改**：`loomgui_core/examples/dump_text.rs`（+128/-6 lines）

```
commit 03e463e
diag(example): dump_text verifies GlyphAtlas ensure/UV/dirty-pages
```

## 5. PlayMode 手交文档

路径：`F:/WorkSpace/projects/LoomGUI/.superpowers/sdd/v1.6-playmode-handoff.md`

覆盖内容：
- 前置操作（pull + Unity focus 编译 + stale .dll 诊断）
- 9 项 PlayMode 验收（ASCII/CJK/合批/tofu/多页/全功能回归/坑113/shader .r/V-flip）
- 已知未验证项 + 症状->修复位置速查表

## 6. 自审

- [x] Spec 覆盖：dump_text 覆盖 atlas ensure/UV/dirty/page_bytes/.notdef 所有关键 API 路径
- [x] fence_contract：25/25 通过
- [x] 全回归：658/658 通过
- [x] 代码注释上线品质：自包含，无坑号暗语
- [x] PlayMode 手交：完整清单，症状->修复位置映射清晰
- [x] commits 仅含 `dump_text.rs` 变更

## 7. 顾虑

1. **pkg.bin 需重建**：dump_text 的文本 dump 段因 blob v10 格式变化无法运行，需 `cargo run -p loomgui_pkg` 重建 showcase pkg.bin。atlas 验证段不受影响已全部通过。
2. **V-flip 方向**：build_text_mesh UV 沿用 Image quad 惯例，但 text 的 y-up->y-down 翻转路径独立，PlayMode 需目视确认文字方向。
3. **shader .r**：R8 TextureFormat 在 Unity 6.5 已支持，但 `tex2D` 采样 .r 返回值需 PlayMode 确认（非实心方块）。
4. **多页 atlas**：公司机单页 4096^2 未触发溢出，多页路径由 `overflow_allocates_second_page` 单元测试覆盖，但 PlayMode 真实多页渲染未验证——需用大量 CJK 字形触发。
5. **C# IL2CPP**：家里机 Editor 是 Mono（非 IL2CPP），`Span<byte>` / `BinaryPrimitives` 路径已单测覆盖，但真实 IL2CPP build 未验证（通常等出包阶段才测）。
