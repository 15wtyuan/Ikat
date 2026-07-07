# Task 7 Report: Unity SyncFontAtlas (R8 upload + SpriteResolver registration + shader .r)

**Status:** DONE
**Commit:** `6d9f109` `feat(unity): SyncFontAtlas uploads core R8 atlas + shader samples .r`

## FFI C# Signatures Called (from `LoomGUIBindings.cs`)

```csharp
internal static extern nuint loomgui_stage_font_atlas_dirty_pages(StageHandle* h, uint* @out, nuint max);
internal static extern nuint loomgui_stage_font_atlas_page(StageHandle* h, uint page, uint* out_w, uint* out_h, byte* out_buf, nuint buf_len);
internal static extern void loomgui_stage_font_atlas_clear_dirty(StageHandle* h);
```

All three take `StageHandle*` directly (csbindgen mapped `*const StageHandle` and `*mut StageHandle` to `StageHandle*` in C#). Output params are `uint*`/`byte*` (raw pointers, not `ref`/`out`). Return type is `nuint` (C# `UIntPtr`), cast to `int` at call sites.

## Double-Call Implementation

`font_atlas_page` uses the double-call pattern per the T5 Rust contract:
1. Call with `out_buf=null`, `buf_len=0` -- returns needed byte count (no write to w/h/pixels).
2. Allocate buffer `ArrayPool<byte>.Shared.Rent(needed)`.
3. Call again with real buffer -- this time writes w, h, and pixel bytes.
4. Verify `got == needed` before proceeding.

## SyncFontAtlas Method

Flow:
1. Probe dirty pages via `stackalloc uint[8]` (no heap alloc for typical case; heap fallback if >8).
2. For each dirty page: double-call to get R8 pixels, create `Texture2D(w, h, TextureFormat.R8, false)`, `LoadRawTextureData(buf, needed)`, `Apply(false, true)`.
3. Register as `Sprite.Create(tex, fullRect, centerPivot)` under path `loomgui://font-atlas/f0/p<page>` via `_sprites.RegisterFontAtlasPage`.
4. After all pages: `loomgui_stage_font_atlas_clear_dirty(_stage)`.

Called in `Tick()` at line 159, between `new FrameBlob(_frameBuf)` and `_pool.Sync(...)`, so atlas sprites are registered before MirrorPool does GetSprite lookups for text nodes.

## RegisterFontAtlasPage

```csharp
public void RegisterFontAtlasPage(string path, Texture2D tex)
{
    if (tex == null) return;
    var sprite = Sprite.Create(tex, new Rect(0, 0, tex.width, tex.height), new Vector2(0.5f, 0.5f));
    _cache[path] = sprite;
}
```

Uses simplest 3-arg `Sprite.Create` overload (defaults pixelsPerUnit=100, extrude=0, meshType=Tight). Sprite rect/pivot don't matter for rendering -- core provides UVs -- the Sprite is purely a texture handle for GetSprite lookup. Same path re-registration replaces old entry (old Texture2D collected by GC).

## Synthetic Path Format

T3 emits: `loomgui://font-atlas/f{font_id}/p{page}` (from `render/mod.rs:388`). FontTable assigns font_id sequentially from 0. For v1.6 single-font showcase (one font registered first and marked default), font_id=0. Unity registers under `loomgui://font-atlas/f0/p{page}` matching the T3 format.

## Shader Change

`loomgui_unity_package/Shaders/LoomGUI-Unlit.shader`, ALPHA_MASK block:

Before:
```hlsl
half4 col = half4(vcol.rgb, vcol.a * tex.a);
```

After:
```hlsl
half4 col = half4(vcol.rgb, vcol.a * tex.r);
```

R8 texture swizzle in Unity: .r=coverage, .g=0, .b=0, .a=1. The old `tex.a` would be constant 1.0 (no masking). Non-ALPHA_MASK paths (image/bg-composite) are unchanged.

## Grep Self-Check Results

- `SyncFontAtlas|RegisterFontAtlasPage|font_atlas` in Runtime/: 12 matches across LoomStage.cs, SpriteResolver.cs, MirrorPool.cs -- all expected. `SyncFontAtlas` call at line 159, method definition at 186. `RegisterFontAtlasPage` at SpriteResolver.cs:109.
- `R8|LoadRawTextureData` in LoomStage.cs: 2 matches (R8 at tex creation line 222, LoadRawTextureData at line 223).
- `ALPHA_MASK` in shader: 2 matches (pragma at line 42, `#if defined` at line 99). Confirmed `tex.r` usage at line 102.

## Files Changed

- `loomgui_unity_package/Runtime/LoomStage.cs` (+58 lines: SyncFontAtlas method + Tick insertion)
- `loomgui_unity_package/Runtime/SpriteResolver.cs` (+16 lines: RegisterFontAtlasPage)
- `loomgui_unity_package/Shaders/LoomGUI-Unlit.shader` (+3/-2: tex.a->tex.r + comment)

## Self-Review Findings

- FFI parameter types match `LoomGUIBindings.cs` exactly: `StageHandle*`, `uint*`, `byte*`, `nuint`. No `ref`/`out` mismatch.
- `stackalloc` for dirty pages avoids heap alloc in typical case (8 pages); heap fallback for overflow.
- ArrayPool buffer properly returned in `finally` block.
- `TextureFormat.R8` requires Unity 2021.2+ (Unity 6.5 satisfies).
- `LoadRawTextureData(byte[], int)` overload: passes `needed` explicitly, safe even if array pool returned oversized buffer.
- Double-call logic matches T5 Rust contract: first call null buf returns needed, second call fills data.
- `_stage` is `StageHandle*` (csbindgen opaque struct), passed directly to all 3 FFI functions.
- `clear_dirty` takes `StageHandle*` (csbindgen flattened `*mut StageHandle`), not `ref StageHandle` -- verified in LoomGUIBindings.cs.

## Concerns

1. **Unity compile + PlayMode NOT verified** -- no Unity on this machine. Home machine does T9.
2. **`Sprite.Create` overload**: used 3-arg overload `(Texture2D, Rect, Vector2)`. If obsolete in Unity 6.5, the home machine compiler will flag; fallback to 5-arg `(tex, rect, pivot, pixelsPerUnit, extrude, meshType)` or 8-arg as in brief.
3. **`TextureFormat.R8`**: supported since Unity 2021.2, present in 6.5. If somehow unavailable, fallback to `TextureFormat.Alpha8` (but that maps to .a, which contradicts the shader .r change -- need coordinated change).
4. **font_id in path**: hardcoded `f0` for single-font v1.6. If showcase registers non-default fonts before the default, font_id won't be 0 and path won't match. Mitigation: the brief and CLAUDE.md prescribe registering the default font first.
5. **V-flip unvalidated until PlayMode**: T3 computes UVs with a possible V-flip. If text renders upside-down in PlayMode, the UV V direction or Sprite V convention needs adjustment. This is a data-level issue (core UVs), not a T7 code bug.
6. **`_sprites` is private**: accessed from `SyncFontAtlas` (same class), compiles fine. But if a subclass needed to call `RegisterFontAtlasPage`, visibility would need revisiting -- not needed for v1.6.
7. **Atlas Sprite lifetime**: text Sprite textures are allocated with `makeNoLongerReadable: true` (upload-only, GPU memory). Old textures are replaced in `_cache` dict and left for GC (no explicit `Destroy()`). For hot-reload scenarios, the old Texture2D's native GPU resource may leak until GC collects the managed wrapper -- acceptable for v1.6 (atlas pages are ~256KB each, dirty only on font change, not per-frame).


## Fixes (review round 1)

**Commit:** `7724235` (amend of `6d9f109`). All 3 findings fixed.

### Critical 1 -- Use-after-unpin in heap fallback (LoomStage.cs:194-204)

Removed the `heapDirty` / `fixed` / heap fallback path entirely. Bumped `MAX_DIRTY` from 8 to 16. If `n > 16`, `Debug.LogWarning` fires and the method processes only the first 16 pages (skipping extras). v1.6 single-font atlas has few pages, so this is a safe bound. The `stackalloc uint[16]` path is always stack-allocated -- no GC pinning, no UB.

### Important 2 -- LoadRawTextureData(buf, needed) overload (LoomStage.cs:223)

Replaced `tex.LoadRawTextureData(buf, needed)` (non-existent `(byte[], int)` overload) with:
```csharp
fixed (byte* p = buf) { tex.LoadRawTextureData((IntPtr)p, needed); }
```
This uses the well-established `LoadRawTextureData(IntPtr, int)` overload, reading exactly `needed` bytes from the pinned ArrayPool buffer. The `fixed` block keeps the buffer pinned during the call, and the surrounding `fixed (byte* pBuf = buf)` block (for FFI fill) is already closed before this point, so no nested-fixed-to-same-variable issue.

### Important 3 -- Sprite.Create meshType default (SpriteResolver.cs:114)

Changed from 3-arg `Sprite.Create(tex, rect, pivot)` (defaults `Tight`) to explicit:
```csharp
Sprite.Create(tex, new Rect(0, 0, tex.width, tex.height), new Vector2(0.5f, 0.5f), 100f, 0, SpriteMeshType.FullRect)
```
Now uses `SpriteMeshType.FullRect` (simple quad) as the brief specified.

### Verification

- `heapDirty|dirtyStack`: 0 occurrences in `LoomStage.cs` (old heap fallback fully removed).
- `LoadRawTextureData\(buf`: 0 occurrences (old non-existent overload fully removed).
- `LoadRawTextureData\(\(IntPtr\)`: 1 occurrence, correct pattern on line 216.
- `SpriteMeshType.FullRect`: 1 occurrence in `SpriteResolver.cs:114`.
- `Sprite.Create` with `Tight` defaulting: 0 occurrences in `SpriteResolver.cs`.
- All files syntactically sound (manual review). No Unity compiler on this machine -- home machine does T9 compile check.
