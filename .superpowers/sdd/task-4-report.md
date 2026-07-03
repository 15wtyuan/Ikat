# Task 4 Report: LoomStage 自动取配置

## Status: done

## Commit

`986082a` feat(unity): LoomStage 自动取 LoomSettings（砍 _spriteAtlases Inspector）

## Files modified

- `loomgui_unity/Assets/LoomGUI/Runtime/LoomStage.cs` (1 file, +6 -11)

## Changes

1. **砍 `_spriteAtlases` 字段**（原 L45-48）：
   - 删 `[SerializeField] List<SpriteAtlas> _spriteAtlases = new();` 及三段注释。
   - 替换为两行注释说明图集路由从 `LoomSettings` 自动取。

2. **改 Awake 初始化**（原 L349-353）：
   - 删 `if (_spriteAtlases != null) _sprites.RegisterAtlases(_spriteAtlases);`
   - 替换为 `_sprites.Init(LoomSettings.GetOrCreateDefault());`

3. **删 `using UnityEngine.U2D;`**：`SpriteAtlas` 类型不再被此文件引用，using 已冗余。

4. **更新 OnDestroy 注释**（原 L553-554）：`List<SpriteAtlas>` → `folder→atlas 映射来自 LoomSettings`。

## Syntax verification

- `grep "RegisterAtlases\|_spriteAtlases"` across `loomgui_unity/Assets/LoomGUI/` : **zero matches** (clean).
- `grep "MissingSprite"` in LoomStage.cs: **zero matches** -- 不读 `MissingSprite`，与 Task 3 的 set-only 变更兼容。
- `_pool.Sync(blob, transform, _mm, _sprites, ...)` 调用点不变 -- `SpriteResolver.GetSprite` 接口不变。

## Self-review

- 改动严格按 brief 执行，无多余修改。
- `LoomSettings.GetOrCreateDefault()` 在当前 namespace `LoomGUI` 内，无需 using。
- `Font.textureRebuilt` 注册/解绑、`_frameBuf` 归还等无关逻辑未触碰。
- 无 Unity 环境，C# 编译未跑。语法层面（命名空间、类型、方法签名）已核对。

## Tests

未跑（本机无 Unity）。Unity Test Runner 需在家机验证。

## Concerns

- 无。
