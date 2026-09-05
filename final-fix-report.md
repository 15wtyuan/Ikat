# Final whole-branch review fixes

## Fix 1: Remove "Task N" references from code comments (AGENTS.md constraint)

Scope expanded beyond the 9 listed: a repo-wide `grep "Task [0-9]"` surfaced
**20** internal task-number references across `crates/` (the 9 from the
control-bundle plan + 11 pre-existing from other plans: nativehost-slot
"Task 1-4", v1.8 style "Task 8/9/10/11", schema "Task 4/5", css_resolve
"Task 6/7 (E1/E2)", IR-bridge "Task 2/3", build "Task 10b"). Per AGENTS.md's
hard rule ("代码注释不引用内部编号或暗语") ALL were reworded to state the WHY
without the "Task N" prefix. Comment-only; no logic touched.

Files touched (15): core/examples/dump_nativehost_slot.rs,
core/src/scene/{control,dynamic}.rs, core/src/scene/node/tests.rs,
core/src/stage.rs, core/src/style/{computed,mapping/tests}.rs,
fence/src/css_resolve.rs, fence/src/schema/tag.rs, fence/tests/control_css.rs,
packer/pkg/src/{bridge,build}.rs, packer/pkg/tests/{build,control_init_bridge,
smoke_ir_bridge}.rs.

Post-fix: `grep -rn "Task [0-9]" crates/` → **0 matches**.

## Fix 2: Repack stale v21 dotnet fixtures → v24

**Build mechanism (clear):** each fixture has a sibling `.workspace/` dir with
`yio.workspace.json` (output_dir `../<name>-ws-out`). Built via
`cargo run -p yio_pkg -- build <fixture>.workspace` → writes
`<name>-ws-out/ui/<name>.pkg.bin`, then copied to `fixtures/<name>.pkg.bin`.

Three were v21 (`15 00 00 00`), now v24 (`18 00 00 00`):
- p1-block, p2-visual: repacked cleanly.
- **test.workspace/test.html needed a source fix**: the branch's own fence rule
  `FenceInlineElementInBlockContext` (commit 9b4ed72) rejects `<button>`/`<img>`
  as inline children of the block container `#child`. Added `style="display:block"`
  to both (the fence's recommended fix; Yio already renders them block-level,
  so zero behavior change). All 9 AcceptanceGate criteria preserved.

`controls.pkg.bin` was already v24 — untouched.

## Verification
- `cargo test` (whole workspace): **GREEN** (698 + suites, 0 failed).
- `cargo fmt --all -- --check`: clean. `cargo clippy --all-targets -D warnings`: clean.
- `grep "Task [0-9]" crates/`: 0 matches.
- Rebuilt + committed `yio_ffi_c.dll` (cargo recompiled core; FFI behavior
  unchanged — comment-only source edits).
- dotnet filtered run (BlockLayout|VisualDecoration|AcceptanceGate|FixtureSmoke|
  UiContextCreation): **35 passed / 2 failed** — all 3 fixtures now LOAD past
  TooOld (Fix 2 goal met).

## Concern (pre-existing, NOT caused by these fixes)

2 dotnet tests still fail on **content** assertions (after LoadPackage succeeds).
Root cause: the branch's NodeKind renumbering commit `a7a86e5` only updated
`NodeKindTests.cs`; these files kept stale kind numbers:

- `UiContextCreationTests.CreateImageWhitelist`: `ctx.Create<Image>()` expects
  kind 8, actual 4. (Runtime create path — touches NO fixture.)
- `FixtureSmokeTests.FixtureLoadsAndInstantiatesAll9Criteria`: span#text expects
  kind 3 (TextElement), actual 2. Packer emits the correct *new* number.

Current enum (0-indexed): Container=0, TextNode=1, TextElement=2, Button=3,
Image=4, … (old: TextElement=3, Button=6, Image=8). `NodeLifecycleTests.cs`
also has stale kind numbers in comments/assertions. Recommend a separate
"sync remaining dotnet tests to renumbered NodeKind" task (mechanical: update
expected kind numbers + comments in FixtureSmokeTests.cs,
UiContextCreationTests.cs, NodeLifecycleTests.cs).
