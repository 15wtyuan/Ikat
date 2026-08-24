using System;
using System.Collections.Generic;
using UnityEngine;

namespace LoomGUI
{
    /// <summary>
    /// Sprite lookup result from self-drawn atlas. Texture + UV rect (in atlas) + original pixel size.
    /// uvRect uses Unity Rect convention: x=u0, y=v0, width=u1-u0, height=v1-v0.
    /// </summary>
    public struct SpriteLookup
    {
        public Texture2D tex;
        public UnityEngine.Rect uvRect;
        public int origW;
        public int origH;
        public bool found;
    }

    /// <summary>
    /// Sprite key to texture + UV rect resolver.
    /// Consumes self-drawn atlas.png + atlas.json (UV table) from the standalone packer.
    /// Drops Unity SpriteAtlas/Sprite dependency entirely — a cross-engine portability win
    /// (Godot/UE load a texture + UV table the same way).
    ///
    /// All atlases are merged into one global sprite table at Init. Page textures are
    /// lazy-loaded via the loadPage delegate. Font atlas pages are registered separately
   /// and take priority in GetSprite lookup.
   /// </summary>
    /// <summary>
    /// Font-atlas image_path construction. Must match Rust render::font_atlas_path
    /// (image_path field in blob). Changing the format here requires changing both sides.
    /// </summary>
   public static class FontAtlasPath
   {
        public static string Format(uint page) => $"loomgui://font-atlas/p{page}";
   }

   public sealed class SpriteResolver
    {
        // Merged sprite table: sprite_key → (atlasIdx, page, uvRect, origW, origH).
        Dictionary<string, (int atlasIdx, int page, UnityEngine.Rect uvRect, int origW, int origH)> _sprites;

        // Page texture cache: (atlasIdx, page) → Texture2D. Lazy-loaded via loadPage delegate.
        Dictionary<(int, int), Texture2D> _pageCache;

        // Font atlas pages: path → full-region SpriteLookup. Registered externally by SyncFontAtlas.
        Dictionary<string, SpriteLookup> _fontPages;

        // Miss dedup: warn once per missing key per session.
        HashSet<string> _warned;

        // loadPage delegate: pageFileName (e.g. "ui.png") → Texture2D.
        Func<string, Texture2D> _loadPage;

        // Atlas page filenames indexed by (atlasIdx, page).
        List<List<string>> _atlasPages;

        /// <summary>
        /// Initialize from atlas manifests. Merges ALL atlases' sprites into one global table.
        /// loadPage(pageFileName) lazily loads a page texture (e.g. "ui.png").
        /// atlases=null → empty (safe to call GetSprite — all miss).
        /// </summary>
        public void Init(List<AtlasManifest> atlases, Func<string, Texture2D> loadPage)
        {
            _sprites = new Dictionary<string, (int, int, UnityEngine.Rect, int, int)>();
            _pageCache = new Dictionary<(int, int), Texture2D>();
            _fontPages = new Dictionary<string, SpriteLookup>();
            _warned = new HashSet<string>();
            _loadPage = loadPage;
            _atlasPages = new List<List<string>>();

            if (atlases == null) return;

            for (int atlasIdx = 0; atlasIdx < atlases.Count; atlasIdx++)
            {
                var atlas = atlases[atlasIdx];
                if (atlas == null) { _atlasPages.Add(new List<string>()); continue; }
                _atlasPages.Add(atlas.pages ?? new List<string>());

                if (atlas.sprites == null) continue;
                foreach (var kv in atlas.sprites)
                {
                    var entry = kv.Value;
                    var uv = entry.uv;
                    if (uv == null || uv.Length < 4) continue;
                    if (entry.orig == null || entry.orig.Length < 2) continue;
                    // atlas.json 的 uv 是像素左上原点（v0=顶，打包器按 image crate 约定算）；
                    // Unity 纹理采样 v=0 在底，故翻转 v：y = 1 - v1（atlas 底对应 Unity 底）。
                    var uvRect = new UnityEngine.Rect(uv[0], 1f - uv[3], uv[2] - uv[0], uv[3] - uv[1]);
                    _sprites[kv.Key] = (atlasIdx, entry.page, uvRect, entry.orig[0], entry.orig[1]);
                }
            }
        }

        /// <summary>
        /// Look up a sprite by its workspace-relative key.
        /// Returns SpriteLookup with found=false on miss (caller fallback).
        /// Empty/null key returns found=false without warning.
        /// </summary>
        public SpriteLookup GetSprite(string key)
        {
            if (string.IsNullOrEmpty(key))
                return new SpriteLookup { found = false };

            // Font atlas pages take priority — check before sprite table.
            if (_fontPages != null && _fontPages.TryGetValue(key, out var fontLookup))
                return fontLookup;

            if (_sprites == null || !_sprites.TryGetValue(key, out var entry))
            {
                if (_warned != null && _warned.Add(key))
                    Debug.LogWarning($"[SpriteResolver] sprite not found: '{key}'");
                return new SpriteLookup { found = false };
            }

            Texture2D tex = GetOrLoadPage(entry.atlasIdx, entry.page);
            if (tex == null)
            {
                if (_warned != null && _warned.Add(key))
                    Debug.LogWarning($"[SpriteResolver] page tex load fail: atlas[{entry.atlasIdx}] p{entry.page}, key='{key}'");
                return new SpriteLookup { found = false };
            }

            return new SpriteLookup
            {
                tex = tex,
                uvRect = entry.uvRect,
                origW = entry.origW,
                origH = entry.origH,
                found = true
            };
        }

        /// <summary>
        /// Register a font atlas page. Text mesh image_path="loomgui://font-atlas/p{n}"
        /// hits this cache via GetSprite, returning a full-region (0,0,1,1) SpriteLookup.
        /// Re-registering the same path replaces the old entry (font atlas pages are immutable
        /// per-session; old Texture2D is GC'd).
        /// </summary>
        public void RegisterFontAtlasPage(string path, Texture2D tex)
        {
            if (tex == null) return;
            if (_fontPages == null)
                _fontPages = new Dictionary<string, SpriteLookup>();
            _fontPages[path] = new SpriteLookup
            {
                tex = tex,
                uvRect = new UnityEngine.Rect(0, 0, 1, 1),
                origW = tex.width,
                origH = tex.height,
                found = true
            };
        }

        public void Clear()
        {
            _sprites?.Clear();
            _pageCache?.Clear();
            _fontPages?.Clear();
            _warned?.Clear();
        }

        Texture2D GetOrLoadPage(int atlasIdx, int page)
        {
            var key = (atlasIdx, page);
            if (_pageCache == null) return null;
            if (_pageCache.TryGetValue(key, out var cached)) return cached;

            if (_loadPage == null) return null;
            if (_atlasPages == null || atlasIdx >= _atlasPages.Count) return null;
            var pages = _atlasPages[atlasIdx];
            if (page < 0 || page >= pages.Count) return null;

            string fileName = pages[page];
            Texture2D tex = _loadPage(fileName);
            if (tex != null) _pageCache[key] = tex;
            return tex;
        }
    }
}
