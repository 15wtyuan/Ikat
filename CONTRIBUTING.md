# 贡献指南

感谢关注 LoomGUI。贡献前请先读 [README](README.md) 的「为什么」与「快速上手」，了解项目定位与构建命令。

## 开发环境

- **Rust**（stable，edition 2021）：核心 / 打包器 / FFI。
  ```bash
  cargo test --workspace          # 全量测试
  cargo test -p loomgui_core fence_contract   # 围栏契约门
  cargo build -p loomgui_ffi_c --release      # 编 .dll
  ```
- **Unity 6.5（URP）**：打开 `unity/showcase-unity/`，PlayMode 从 `StreamingAssets/` 加载 `.pkg.bin`。

详见 [CLAUDE.md](CLAUDE.md) 的「构建/测试命令」「Rust → Unity .dll 闭环」。

## 两机工作流

- 编码机（Windows）：Rust 改动后重编 `.dll` + `LoomGUIBindings.cs` + commit + push。
- 验收机：pull 后做 Unity PlayMode 验收。
- **任何 Rust 改动后必须重编并提交 `.dll`**，否则验收机测不了。

## Commit 风格

Conventional Commits，描述用中文：

```
feat(scope): 新功能
fix(scope): 修 bug
docs:        文档
refactor:    重构（不改行为）
chore:       杂项 / 工具链
test:        测试
```

`scope` 如 `core` / `unity` / `ffi` / `pkg` / `spec`。

## PR 自查清单

- [ ] `cargo test --workspace` 全过
- [ ] 改了 Rust FFI/ABI → 重编 `.dll` + 重生 `LoomGUIBindings.cs` 并提交
- [ ] 改了 parse-time 逻辑（cascade/mapping）→ 重打 `.pkg.bin`
- [ ] 改了围栏（CSS 属性/标签）→ 更新 `fence_contract.rs` 测试 + `fence.md`
- [ ] 代码注释自包含、说 WHY，不引用内部编号或暗语
- [ ] Unity 侧改动 → PlayMode 无红

## 设计文档

- 设计契约：[`docs/design/main-design.md`](docs/design/main-design.md)
- 围栏权威：[`docs/design/fence.md`](docs/design/fence.md)（不一致时 [`fence_contract.rs`](crates/core/tests/fence_contract.rs) 测试赢）
- 踩坑规则手册：[`docs/pitfalls.md`](docs/pitfalls.md)（依赖适配/闭环规则/Unity 平台，开工前查）
- 路线图：[`docs/roadmap/roadmap.md`](docs/roadmap/roadmap.md)

新功能先在 `docs/superpowers/specs/` 写 spec，讨论定稿后再实现。
