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
    /// res 根图（无子目录）或子目录不在表 → 走 isDefault atlas，未配 isDefault 走 FirstAtlas。
    /// miss 不缓存——atlas 启动全加载，重查成本可控。
    /// miss 时 Debug.LogWarning 一次（去重 _warned）：顶层子目录无映射 / 图集无此 sprite。
    /// </summary>
    public sealed class SpriteResolver
    {
        readonly Dictionary<string, SpriteAtlas> _folderToAtlas = new();
        readonly Dictionary<string, Sprite> _cache = new();
        readonly HashSet<string> _warned = new();   // miss 路径去重 warn（miss 不缓存，避免每帧刷屏）
        SpriteAtlas _defaultAtlas;

        public int AtlasCount => _folderToAtlas.Count;
        /// 测试用：缓存条目数（miss 不增）。
        public int CacheCount => _cache.Count;

        /// 从 LoomSettings.atlasEntries 建 folder→atlas 映射表。
        public void Init(LoomSettings settings)
        {
            _folderToAtlas.Clear();
            _cache.Clear();
            _warned.Clear();
            _defaultAtlas = null;
            if (settings == null)
            {
                Debug.LogWarning("[SpriteResolver] Init: settings 为 null（LoomSettings 未建 / 未进 Resources）");
                return;
            }
            int entriesSkipped = 0;
            foreach (var entry in settings.atlasEntries)
            {
                if (entry == null || entry.atlas == null) { entriesSkipped++; continue; }
                // isDefault 不暴露（UI 隐藏）——res 根图/未命中子目录统一走 FirstAtlas 兜底（spec §4.3 边界）。
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
            Debug.Log($"[SpriteResolver] Init: {_folderToAtlas.Count} folder→atlas 映射，{settings.atlasEntries.Count} entries" +
                      (entriesSkipped > 0
                          ? $"（{entriesSkipped} entry.atlas 为 null 跳过——Editor 未同步或运行时引用丢失）"
                          : ""));
        }

        /// 测试用：直接注入映射表。
        public void InitWithMap(Dictionary<string, SpriteAtlas> map, SpriteAtlas defaultAtlas)
        {
            _folderToAtlas.Clear();
            _cache.Clear();
            _warned.Clear();
            foreach (var kv in map) _folderToAtlas[kv.Key] = kv.Value;
            _defaultAtlas = defaultAtlas;
        }

        /// path → Sprite 查询。
        /// null/空 path → null（纯色无图）。查不到 → null（调用方 fallback，不崩）+ Debug.LogWarning 一次。
        public Sprite GetSprite(string path)
        {
            if (string.IsNullOrEmpty(path)) return null;
            if (_cache.TryGetValue(path, out var cached)) return cached;

            string spriteName = System.IO.Path.GetFileNameWithoutExtension(path);
            SpriteAtlas atlas = ResolveAtlas(path);
            Sprite found = atlas != null ? atlas.GetSprite(spriteName) : null;

            if (found != null)
            {
                _cache[path] = found;       // 只缓存命中
                _warned.Remove(path);       // 命中清 warn（下次再 miss 会重 warn）
                return found;
            }
            // miss 不缓存。warn 一次（去重，避免每帧刷屏）。
            if (_warned.Add(path))
            {
                if (atlas == null)
                    Debug.LogWarning($"[SpriteResolver] 图不存在：path={path}（顶层子目录 '{TopDir(path)}' 无图集映射）");
                else
                    Debug.LogWarning($"[SpriteResolver] 图不存在：图集 '{atlas.name}' 无 sprite '{spriteName}'（path={path}，atlas.spriteCount={atlas.spriteCount}）");
            }
            return null;
        }

        static string TopDir(string path)
        {
            string p = path.Replace('\\', '/');
            int slash = p.IndexOf('/');
            return slash <= 0 ? "(res 根)" : p.Substring(0, slash);
        }

        /// path → atlas。顶层子目录查表；无子目录或 miss → default → FirstAtlas。
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
            _warned.Clear();
            _defaultAtlas = null;
        }
    }
}
