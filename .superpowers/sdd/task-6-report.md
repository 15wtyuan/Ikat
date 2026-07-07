# Task 6 Report: Unity FrameBlob v10 mirror + delete text rasterization path

## Status: DONE

## Commit
- **SHA**: `63237bd`
- **Subject**: `refactor(unity): drop backend text rasterizer (v1.6 font-to-core)`
- **Files**: 9 changed (127 insertions, 765 deletions, 4 deletions)

## Blob v10 Column Layout (mirrored from `loomgui_ffi_c/src/blob.rs`)

20 columns (was 22 in v9 -- text_off/text_len deleted), header 116B (was 132B).

| Idx | Column | Size | Notes |
|-----|--------|------|-------|
| 0 | node_id | 4B | |
| 1 | parent_id | 4B | |
| 2 | visible | 1B | |
| 3 | alpha | 4B | |
| 4 | sort_key | 4B | |
| 5 | mask_context | 4B | |
| 6 | m_a | 4B | world matrix |
| 7 | m_b | 4B | |
| 8 | m_c | 4B | |
| 9 | m_d | 4B | |
| 10 | m_tx | 4B | |
| 11 | m_ty | 4B | |
| 12 | payload_kind | 1B | v10: only 1=Mesh |
| 13 | mesh_off | 4B | |
| 14 | mesh_len | 4B | |
| 15 | path_idx | 4B | was col 17 in v9 |
| 16 | program | 1B | was col 18 in v9 |
| 17 | color_matrix | 80B | was col 19 in v9 |
| 18 | change_level | 1B | was col 20 in v9 |
| 19 | reuse_key | 4B | was col 21 in v9 |

Arena headers (after 20 col_offsets): mesh_arena (off @ 92, len @ 96), clip_table (off @ 100, len @ 104), path_table (off @ 108, len @ 112).

## FrameBlob.cs Changes
- `ExpectedVersion` 9 -> 10
- Header comment updated: 132B -> 116B, 22 cols -> 20
- Col count: 22 -> 20 in comment block
- `ColOff` range: `12 .. 12+22*4` -> `12 .. 12+20*4`
- Arena offset properties: deleted `TextArenaOff`/`TextArenaLen`; `ClipTableOff`/`PathTableOff` recalculated (all 8B earlier)
- `TextOff(i)` / `TextLen(i)` -- deleted
- `ReadText` + `GlyphData` struct -- deleted
- `ChangeLevel(i)`: `ColOff(20)` -> `ColOff(18)`
- `ReuseKey(i)`: `ColOff(21)` -> `ColOff(19)`
- `PathIdx(i)`: `ColOff(17)` -> `ColOff(15)`
- `Program(i)`: `ColOff(18)` -> `ColOff(16)`
- `ColorMatrix(i)`: `ColOff(19)` -> `ColOff(17)`

## TextRasterizer.cs Deletion
- `git rm`'d `loomgui_unity_package/Runtime/TextRasterizer.cs` + `.meta`
- `git rm`'d `loomgui_unity_package/Tests/TextRasterizerTests.cs` + `.meta`

## MirrorPool.cs Deletions
- **RenderObj fields removed**: `IsText`, `LastGlyphs`, `LastFontSize`, `LastTextColor`, `LastFont`
- **`_lastFontVersion` field** removed
- **fontDirty logic** removed (~15 lines: fontVersionAtStart/fontDirty calc, force-All-on-dirty, mid-rebuild race guard)
- **Text font selection branch** removed (~14 lines: ReadText call + ResolveFont cache)
- **`IsText` assignment** removed (`ro.IsText = kind == 2`)
- **`UploadMeshOrText` text path** removed (~15 lines: kind==2 branch with TextRasterizer.BuildMesh)
- **`ResolveFont` static method** removed entirely
- **`UpdateHeader` text material branch** removed (~7 lines: kind==2 material selection, font param)
- **Sync signature**: 8 params -> 5 params (dropped unityFonts, defaultFont, fontVersion)
- **`UpdateHeader` signature**: `Font font` param removed
- **`UploadMeshOrText` signature**: `Font font` param removed; simplified to single mesh path
- **`kind != 1 && kind != 2` guard** simplified to `kind != 1`

## LoomStage.cs Deletions
- `_unityFonts` dict + `_defaultUnityFont` field -- deleted
- `_fontVersion` field + `FontVersion` property -- deleted
- `OnFontRebuilt(Font)` method -- deleted
- `RegisterFont`: `(string, byte[], Font, bool)` -> `(string, byte[], bool)`
- `Tick` -> `_pool.Sync(blob, _renderRoot, _mm, _sprites, Texture2D.whiteTexture)` (no font args)

## LoomStageDriver.cs Deletions
- `Font.textureRebuilt +=/-=` subscription/unsubscription -- deleted
- `LoadFont(FontEntry)` returning `(byte[], Font)` -> `LoadFontBytes(FontEntry)` returning `byte[]`
- UnityEditor AssetDatabase.LoadAssetAtPath<Font> block -- deleted
- `RegisterFontsFromSettings`: updated to use new method signatures

## MirrorPoolTests.cs Changes
- `OneNodeBlobV9` -> `OneNodeBlobV10`: 22 cols -> 20, header 132B -> 116B, no text_arena, column indices updated
- Sync calls: old 8-arg -> new 5-arg signature
- **MirrorPoolTextTests class entirely removed** (OneTextBlobV9 helper + 2 text-specific tests obsolete)

## Grep Self-Check Result
Comprehensive grep for `TextRasterizer|OnFontRebuilt|_fontVersion|fontVersion|LastGlyphs|LastFont\b|IsText|_lastFontVersion|fontDirty|ReadText|GlyphData|text_arena|TextArena|kind == 2|unityFont|_unityFonts|_defaultUnityFont`:
- 5 matches, all in doc comments documenting the v10 change -- **no dangling code references**.

Additional verification:
- `\.LoadFont\(|\.RegisterFont\(.*Font` -> **empty** (no old signature callers)

## Concerns
1. **Unity compile + PlayMode NOT verified on this machine** -- home machine Task 9 gate. Established two-machine workflow.
2. **`LoomStage.RegisterFont` signature change**: 4 params to 3 (dropped `Font unityFont`). External callers will get compile errors -- intentional.
3. **`LoomStageDriver.LoadFont` -> `LoadFontBytes`**: renamed method. Subclass overrides will break -- intentional.
4. **MirrorPool.Sync signature change**: 8 params to 5. Task 7 will extend this further for font atlas texture upload.

## Files Changed
- `loomgui_unity_package/Runtime/FrameBlob.cs` -- v10 mirror
- `loomgui_unity_package/Runtime/MirrorPool.cs` -- text path deletions
- `loomgui_unity_package/Runtime/LoomStage.cs` -- fontVersion/unityFont deletions
- `loomgui_unity_package/Runtime/LoomStageDriver.cs` -- textureRebuilt/LoadFont deletions
- `loomgui_unity_package/Tests/MirrorPoolTests.cs` -- v10 alignment + text test removal
- `loomgui_unity_package/Runtime/TextRasterizer.cs` -- **deleted**
- `loomgui_unity_package/Runtime/TextRasterizer.cs.meta` -- **deleted**
- `loomgui_unity_package/Tests/TextRasterizerTests.cs` -- **deleted**
- `loomgui_unity_package/Tests/TextRasterizerTests.cs.meta` -- **deleted**
