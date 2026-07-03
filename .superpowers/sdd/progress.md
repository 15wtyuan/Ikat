# v1.4-b SDD Progress Ledger

Plan: docs/superpowers/plans/2026-07-03-v1.4b-absolute-virtual-list.md
Spec: docs/superpowers/specs/2026-07-03-v1.4b-absolute-virtual-list-design.md
Worktree: .claude/worktrees/v1.4b-absolute-virtual-list (branch worktree-v1.4b-absolute-virtual-list)
BASE=f223ad5

## Tasks
- T1: complete (commits f223ad5..64aaf29, review clean — Spec ✅ + Approved, 1 Minor 测试注释措辞) — absolute 围栏
- T2: complete (commits 64aaf29..d0070fa, review clean — Spec ✅ + Approved, 1 Minor brief 措辞) — 文档 + tips overlay
- T3: complete (commits d0070fa..a329c26, review clean — Spec ✅ + Approved, 1 Minor 测试模块位置) — reuse_key 字段
- T4: complete (commits a329c26..e3921bd, review clean after fix — clear_content_size_override 补齐) — scroll 3 FFI 口子
- T5: complete (commits e3921bd..57ce6ed, review clean — 5 FFI + blob v9 + .dll 入库导出确认) — FFI 入口 + blob v9
- T6: in_progress — FrameBlob + MirrorPool 双 dict
- T7: pending — LoomStage driver API
- T8: pending — driver 列表 demo
- T9: pending — 文档校对

## Notes
- 本机做 T1-T7（Rust + C# 语法核对 + 重编 .dll）；T8 driver demo 实机验收 + T8/T9 部分需家里机。
- 改 parse-time（T1 mapping.rs）必重打 pkg（坑 66）——T8 重打。
