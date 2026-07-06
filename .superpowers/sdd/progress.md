# Subagent-Driven Development Progress Ledger

Branch: worktree-stage-refactor-font-resource
Plan: docs/superpowers/plans/2026-07-06-stage-refactor-font-resource.md

Baseline (worktree HEAD before Task A1): 4c854b2
Baseline tests: 564 passed, fmt clean, clippy clean.

## Tasks
A1_BASE=4c854b2a9c589d58c9bb880b860a94ebbb4a105f
- [x] Task A1: complete (commits 4c854b2..15a6e97, review clean — spec ✅, quality Approved)
  - Minor findings (defer to final review): pub(crate) fields (brief test design), "Compute once" comment misleading (test helper)
A2_BASE=15a6e97
- [x] Task A2: complete (commits 15a6e97..4691986, review clean — spec ✅, quality Approved, 0 findings)
  - Cross-task: stage.rs temp FontTable + TODO A3 markers (A3 replaces)
A3_BASE=4691986
- [x] Task A3: complete (commits 4691986..5d8eeeb, review clean — spec ✅, quality Approved, 0 findings)
  - Cross-task: FFI loomgui_stage_new adapted internally (sig unchanged, reads font_path→register_font "DejaVu"); A4 replaces with real (w,h)+register_font port
A4_BASE=5d8eeeb
- [x] Task A4: complete (commits 5d8eeeb..76c1c34, review — spec ✅ w/ 1 minor gap, quality Approved)
  - Minor findings (defer to final review): abi test delegates to helper not direct two-step call + "measures" name misleading; pre-existing CString::new("").unwrap() in FFI path (from brief, not introduced)
  - dll rebuilt + committed (size 1924096→1930752), symbol loomgui_stage_register_font verified in binary
=== Half A (Rust/FFI) COMPLETE ===
B1_BASE=76c1c34
- [x] Task B1: complete (commits 76c1c34..9675c94, review — spec ✅, quality Approved)
  - Critical invariant CONFIRMED: zero UnityEngine.Object refs in LoomSettings.asset
  - Minor findings (defer to final review): stale AtlasEntry doc comment (LoomSettings.cs:70), stale Window inline comment (B6 cleans)
  - Stubs: SpriteResolver.Init (B4), LoomAtlasSync atlas writes (B5), Window atlas UI (B6) — TODO markers present
  - Unity tests not run (no headless Unity) — B8 acceptance
B2_BASE=9675c94
- [x] Task B2: complete (commits 9675c94..7b086bb, review — spec ✅, quality Approved after fix)
  - Fix: Tick(dt) reads _renderRoot (was dead field) + removed B3/B4 codename comments
  - All 31 public APIs + v1.5 Controller dispatch carried verbatim
  - Known gap (controller-owned): blob has no font_family → MirrorPool uses defaultFont; B2b fixes
  - Tick signature note for B3: stage.Tick(dt) no transform param; Driver.Awake calls SetNativeHostRoot(transform)
B2b_BASE=7b086bb
- [x] Task B2b: complete (commits 7b086bb..91dd7f5, review — spec ✅, quality Approved)
  - Blob text_arena now carries per-node font_family (len-prefixed UTF-8); MirrorPool selects Font asset per node
  - payload_hash hashes family (family change → Full, closes Header-stale-font hole)
  - dll rebuilt+committed (md5 verified); 650 Rust tests, fmt/clippy clean
  - Multi-font now fully works (measure A1-A3 + raster B2b)
  - Defer to B8: Unity PlayMode runtime validation
B3_BASE=91dd7f5
- [x] Task B3: complete (commits 91dd7f5..4b0f77c, review — spec ✅, quality Approved after fix)
  - Fix: removed B8 codename + MonoBehaviour-时期 history ref + added WHY on UseSafeArea dual path
  - 3 load hooks public virtual; RegisterFontsFromSettings protected virtual; ResetStatics re-added
  - Tick(dt) no transform (B2 fix honored); SetNativeHostRoot in Awake before tick
B4_BASE=4b0f77c
- [x] Task B4: complete (commits 4b0f77c..f2291e6, review — spec ✅, quality Approved)
  - SpriteResolver folder→atlasName + injected loader + _atlasCache; no serialized atlas refs
  - Cross-task: LoomStage.InitSprites wired to forward loader (B2 left 1-arg, B4 made 2-arg)
  - Minor (defer): null-atlas not cached (retry-on-miss); atlas-cache null-skip untested
B5_BASE=f2291e6
- [x] Task B5: complete (atlas sync → Bundles/atlas/, drop atlas ref writes)
-  - LoomAtlasSync: EnsureAtlasAsset/SyncEntry/ResolveAtlasPath/DeleteAutoAtlas rethreaded to pkgOutputDir; atlas lands in {pkgOutputDir}/atlas/
-  - ResolveAtlasPath now deterministic by-name (File.Exists), dropped AssetDatabase.FindAssets scan (avoids same-name false matches + full-asset-db sweep)
-  - All entry.atlas = writes + TODO B5 markers removed (field gone since B1)
-  - LoomSettingsWindow.DrawAtlasEntry: status by File.Exists at {pkgOutputDir}/atlas/{name}.spriteatlasv2; 3 call sites updated
-  - 2 new tests: EnsureAtlasAsset_WritesToBundlesAtlas + AtlasEntry_HasNoSpriteAtlasField (Unity EditMode; not run from CLI → B8)
-  - Codename/history-ref scrubbed from LoomAtlasSync header comment
