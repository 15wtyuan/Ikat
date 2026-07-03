using System;
using System.Collections.Generic;
using UnityEngine;
using UnityEngine.U2D;

namespace LoomGUI
{
    /// <summary>
    /// path → Sprite 显式路由。核心不知图集，path 是归一化后的相对路径（如 "icons/home.png"）。
    ///
    /// 路由：path 顶层子目录 → folder→atlas 映射表 → atlas.GetSprite(文件名去扩展)。
    /// res 根图（无子目录）或子目录不在表 → 走 isDefault atlas。
    /// miss 不缓存（修坑 104）——atlas 启动全加载，重查成本可控。
    /// </summary>
    public sealed class SpriteResolver
    {
        readonly Dictionary<string, SpriteAtlas> _folderToAtlas = new();
        readonly Dictionary<string, Sprite> _cache = new();
        SpriteAtlas _defaultAtlas;
        Sprite _missingSprite;

        public Sprite MissingSprite { set => _missingSprite = value; }
        public int AtlasCount => _folderToAtlas.Count;
        /// 测试用：缓存条目数（miss 不增）。
        public int CacheCount => _cache.Count;

        /// 从 LoomSettings.atlasEntries 建 folder→atlas 映射表。
        public void Init(LoomSettings settings)
        {
            _folderToAtlas.Clear();
            _cache.Clear();
            _defaultAtlas = null;
            if (settings == null) return;
            foreach (var entry in settings.atlasEntries)
            {
                if (entry == null || entry.atlas == null) continue;
                if (entry.isDefault && _defaultAtlas == null) _defaultAtlas = entry.atlas;
                foreach (var folder in entry.folders)
                {
                    if (string.IsNullOrEmpty(folder)) continue;
                    // folder 是 Unity 路径如 Assets/LoomUI/res/icons → 子目录 key = 最末段 "icons"。
                    string key = folder.TrimEnd('/', '\\');
                    int sep = key.LastIndexOfAny(new[] { '/', '\\' });
                    if (sep >= 0) key = key.Substring(sep + 1);
                    if (!string.IsNullOrEmpty(key))
                        _folderToAtlas[key] = entry.atlas;
                }
            }
        }

        /// 测试用：直接注入映射表。
        public void InitWithMap(Dictionary<string, SpriteAtlas> map, SpriteAtlas defaultAtlas)
        {
            _folderToAtlas.Clear();
            _cache.Clear();
            foreach (var kv in map) _folderToAtlas[kv.Key] = kv.Value;
            _defaultAtlas = defaultAtlas;
        }

        public Sprite GetSprite(string path)
        {
            if (string.IsNullOrEmpty(path)) return null;
            if (_cache.TryGetValue(path, out var cached)) return cached;

            string spriteName = System.IO.Path.GetFileNameWithoutExtension(path);
            SpriteAtlas atlas = ResolveAtlas(path);
            Sprite found = atlas != null ? atlas.GetSprite(spriteName) : null;

            if (found != null)
            {
                _cache[path] = found;   // 只缓存命中
                return found;
            }
            // miss 不缓存（修坑 104）。
            return _missingSprite;
        }

        /// path → atlas。顶层子目录查表；无子目录或 miss → default。
        SpriteAtlas ResolveAtlas(string path)
        {
            string p = path.Replace('\\', '/');
            int slash = p.IndexOf('/');
            if (slash <= 0)
            {
                // 无子目录 → default
                return _defaultAtlas ?? FirstAtlas();
            }
            string topDir = p.Substring(0, slash);
            if (_folderToAtlas.TryGetValue(topDir, out var atlas)) return atlas;
            return _defaultAtlas ?? FirstAtlas();
        }

        SpriteAtlas FirstAtlas()
        {
            foreach (var kv in _folderToAtlas) return kv.Value;
            return null;
        }

        public void Clear()
        {
            _folderToAtlas.Clear();
            _cache.Clear();
            _defaultAtlas = null;
            _missingSprite = null;
        }
    }
}
