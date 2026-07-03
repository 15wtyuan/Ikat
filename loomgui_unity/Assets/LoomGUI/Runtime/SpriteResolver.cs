using System.Collections.Generic;
using UnityEngine;
using UnityEngine.U2D;

namespace LoomGUI
{
    /// <summary>
    /// path → Sprite 查询（核心不知图集，图集归 Unity）。
    ///
    /// 核心只记图片相对 path（res 目录前缀归一化后的相对路径，如 "icons/skin.png"）；
    /// 图集完全归 Unity（开发者建 SpriteAtlas asset，把 res/ 下的 Sprite 划进去）。
    /// 本类维护 path→Sprite 缓存 + 已注册 SpriteAtlas 列表，懒查 SpriteAtlas.GetSprite。
    ///
    /// 查询策略：
    ///   1. 缓存命中 → 直接返回。
    ///   2. 缓存 miss → 遍历 registeredAtlases 调 GetSprite(name)。name 取 path 的
    ///      文件名去扩展（"icons/skin.png" → "skin"）——Unity Sprite 命名规则：SpriteAtlas
    ///      按 Sprite 资产名（不含目录/扩展）索引。命中则缓存 + 返回。
    ///   3. 全 miss → 返回 null（调用方 MirrorPool fallback 到 Texture2D.whiteTexture，不崩）。
    ///
    /// bg-image 同走此路径（background-image: url(icons/bg.png) 同样归一化 path）。
    /// path 查不到 Sprite：Unity fallback，不崩；核心不报错（它不知图集存在）。
    ///
    /// SpriteAtlas 注入：LoomStage Inspector 配 List<SpriteAtlas>（开发者建 SpriteAtlas asset，
    /// Inspector 拖入）。多图集：path 路由到对应 atlas 是 Unity 内部事（核心不感知）。
    /// </summary>
    public sealed class SpriteResolver
    {
        readonly Dictionary<string, Sprite> _cache = new();
        readonly List<SpriteAtlas> _atlases = new();

        /// 注册 SpriteAtlas（可多次调，追加进列表）。path 查询时遍历此列表。
        public void RegisterAtlas(SpriteAtlas atlas)
        {
            if (atlas != null && !_atlases.Contains(atlas))
                _atlases.Add(atlas);
        }

        /// 批量注册。
        public void RegisterAtlases(IEnumerable<SpriteAtlas> atlases)
        {
            if (atlases == null) return;
            foreach (var a in atlases) RegisterAtlas(a);
        }

        /// 已注册 SpriteAtlas 数（调试/Inspector 显示用）。
        public int AtlasCount => _atlases.Count;

        /// 清缓存 + 清已注册 atlas（切场景/重载时调）。
        public void Clear()
        {
            _cache.Clear();
            _atlases.Clear();
        }

        /// 仅清 path→Sprite 缓存（保留 atlas 注册）。SpriteAtlas 重建后调。
        public void ClearCache() => _cache.Clear();

        /// path → Sprite 查询。
        /// null/空 path → null（纯色无图）。查不到 → null（调用方 fallback，不崩）。
        public Sprite GetSprite(string path)
        {
            if (string.IsNullOrEmpty(path)) return null;
            if (_cache.TryGetValue(path, out var cached)) return cached;

            // Sprite 命名规则：取文件名去扩展（"icons/skin.png" → "skin"）。
            // Unity SpriteAtlas.GetSprite 按 Sprite 资产名（不含目录/扩展）索引。
            string spriteName = System.IO.Path.GetFileNameWithoutExtension(path);
            Sprite found = null;
            foreach (var atlas in _atlases)
            {
                if (atlas == null) continue;
                var sp = atlas.GetSprite(spriteName);
                if (sp != null) { found = sp; break; }
            }

            // 缓存（含 miss——避免每帧重复遍历 atlas 查同一条 miss path）。
            _cache[path] = found;
            return found;
        }
    }
}
