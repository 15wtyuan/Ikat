# 工作流+图集重做 SDD Progress Ledger

Plan: docs/superpowers/plans/2026-07-03-workflow-atlas-rework.md
Spec: docs/superpowers/specs/2026-07-03-workflow-atlas-rework-design.md
Worktree: .claude/worktrees/workflow-atlas-rework (branch worktree-workflow-atlas-rework)
BASE=be94766

## Tasks
- T1: complete (commits be94766..334cef4, review clean) — LoomSettings 配置类 + 删旧 LoomPackageSettings/LoomPackageManagerWindow + driver 日志修
- T2: complete (commits 334cef4..9fbda05, review clean) — pack 加 res_root 参数 + CLI --res-root + 8 测试改 + res 迁 LoomUI 根. Minor 未修：--res-root 丢空值 filter（功能等价）/ res_dir 推导重复 lib+main（小 CLI 不值得提 helper）
- T3: complete (commits 9fbda05..c8afb5a, review clean) — SpriteResolver 显式路由+miss不缓存+删DBG-IMG+3测试. Minor: 循环内 char[] 分配. ⚠️待家里机: 空 atlas.GetSprite 行为/MissingSprite set-only 无读方/ClearCache 移除. LoomStage 编译断点留 T4 修
- T4: complete (commits c8afb5a..986082a, review clean) — LoomStage 砍 _spriteAtlases + using U2D, 改 Init(LoomSettings.GetOrCreateDefault()). 修 T3 编译断点. ⚠️待家里机: Unity 编译/PlayMode
