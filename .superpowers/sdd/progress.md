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
- T5: complete (commits 986082a..b699da5, review clean after fix) — LoomSettingsWindow 三 tab + LoomExePath + 3 桩注释. Fix: --res→--res-root 绝对路径 + Process stdout/stderr 死锁修. ⚠️待家里机: Unity 编译/PlayMode
- T6: complete (commits b699da5..a6dcf52, review clean after fix) — LoomAtlasSync 同步 packables(修B2) + 2测试 + 取消T5桩. Fix: new SpriteAtlas→CreateInstance/ToAssetPath StartsWith/删scannedSprites/删SyncEntry settings参. ⚠️待家里机: Unity编译/SetPackables行为

---

## NativeHost FFI query (2026-07-05)
Plan: docs/superpowers/plans/2026-07-05-nativehost-ffi-query.md
Spec: docs/superpowers/specs/2026-07-05-nativehost-ffi-query-design.md
Worktree: .claude/worktrees/nativehost-ffi-query (branch worktree-nativehost-ffi-query)
BASE=9f4c162

### Tasks
- T1: complete (commits 9f4c162..b16b12c, review APPROVED) — Scene.node_sort_keys + assign_sort_keys DFS 填 + build_render_nodes 返回 + tick_and_render 存。2 新测试。515 tests pass
- T2: complete (commits b16b12c..f60df88, review APPROVE) — Stage 3 getter (world_matrix/sort_key/visible) + 5 测。Display 路径 = taffy::Display::None via n.style.taffy_style.display（brief 假设错，implementer 修正）。520 tests pass
- T3: complete (commits f60df88..216741c, review APPROVED) — FFI 3 extern + dll 重编 + nm/md5 验证 + Bindings.cs 重生成（路径 …/Bindings/）。3 文件同 commit。Affine2 列主序 + null/无效写默认。Concerns 给 T4：P/Invoke raw 指针 + Affine2→Unity Matrix4x4 列主序 + visible=0 skip
- T4: complete (commits 216741c..de6f9c2, review APPROVE) — NativeHostManager.Sync 改 FFI 查询（遍历 _bindings，visible/world_matrix/sort_key）+ LoomStage 调用点。csbindgen raw 指针（非 ref，brief 假设错）+ Affine2 列主序未转置。Minor F1: FFI 在 _wrappers 检查前（Bind/Unbind 保证等价，不触发）
- T5: complete (commit a7351d3) — dump_nativehost_slot example：nh-stage NOT IN frame.nodes（merge 吞）+ world_transforms tx=240 ty=123（slot 落 nh-stage 框）+ node_sort_keys=9（DFS 序）。直查 scene 验 FFI 通道独立于 merge。fence_contract 15/15 回归通过。Handoff 给用户明早 PlayMode 验收
