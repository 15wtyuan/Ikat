using System;
using System.Collections.Generic;
using UnityEngine;
using UnityEngine.U2D;

namespace LoomGUI
{
    /// <summary>
    /// path → Sprite 显式路由。path 是归一化后的相对路径（如 "icons/home.png"）。
    ///
    /// 名字解耦：本类不持有任何序列化的 SpriteAtlas 引用（那会把图集资产拖进 Resources/AB 构建）。
    /// LoomSettings 只配 folder → atlasName（纯字符串）。运行时按需通过注入的 loadAtlas 委托懒加载
    /// SpriteAtlas（Driver 的 LoadSpriteAtlas 钩子，按构建后端走 Resources/AB/Addressables），加载结果
    /// 缓存在 _atlasCache，避免每次 GetSprite 都回调。
    ///
    /// 路由：path 顶层子目录 → folder→atlasName 表 → loadAtlas(atlasName) → SpriteAtlas（缓存）→
    /// atlas.GetSprite(文件名去扩展)。res 根图（无子目录）或子目录不在表 → 走 default atlasName。
    /// miss 不缓存 Sprite——但 atlas 缓存（atlas 加载远重于 hash 查，且同 atlas 多 sprite 共享）。
    /// miss 时 Debug.LogWarning 一次（_warned 去重，避免每帧刷屏）。
    /// </summary>
    public sealed class SpriteResolver
    {
        readonly Dictionary<string, string> _folderToAtlasName = new();  // folder key → atlas name
        readonly Dictionary<string, Sprite> _cache = new();              // path → Sprite（仅命中缓存）
        readonly HashSet<string> _warned = new();                        // miss 路径去重 warn
        readonly Dictionary<string, SpriteAtlas> _atlasCache = new();    // atlasName → SpriteAtlas（懒加载一次）
        Func<string, SpriteAtlas> _loadAtlas;
        string _defaultAtlasName;

        /// folder→atlasName 映射条目数（null settings / 未 Init → 0）。
        public int AtlasCount => _folderToAtlasName.Count;
        /// 测试用：Sprite 缓存条目数（miss 不增）。
        public int CacheCount => _cache.Count;

        /// 从 LoomSettings.atlasEntries 建 folder→atlasName 映射，注入 atlas 懒加载委托。
        /// settings=null → 清空 + 不崩（防御）。loadAtlas=null → ResolveAtlas 永远返 null（GetSprite 全 miss，调用方 fallback）。
        public void Init(LoomSettings settings, Func<string, SpriteAtlas> loadAtlas)
        {
            _folderToAtlasName.Clear();
            _cache.Clear();
            _warned.Clear();
            _atlasCache.Clear();
            _loadAtlas = loadAtlas;
            _defaultAtlasName = null;
            if (settings == null) return;

            // 每个 AtlasEntry 的 folder 可能是 Unity 完整路径（"Assets/LoomUI/res/icons"），
            // 但 path 路由用顶层子目录（"icons"）匹配，所以取 folder 的末段作 key。
            foreach (var entry in settings.atlasEntries)
            {
                if (entry == null || string.IsNullOrEmpty(entry.atlasName)) continue;
                if (entry.isDefault) _defaultAtlasName = entry.atlasName;
                foreach (var folder in entry.folders)
                {
                    if (string.IsNullOrEmpty(folder)) continue;
                    string key = LastSegment(folder);
                    if (!string.IsNullOrEmpty(key)) _folderToAtlasName[key] = entry.atlasName;
                }
            }
        }

        /// 测试用：直接注入 folder→atlasName 名字映射 + 加载委托 + default 名字。
        /// 绕开 LoomSettings 的 ScriptableObject 构造，纯逻辑验证路由 + 缓存。
        public void InitWithMap(Dictionary<string, string> folderToAtlasName,
            Func<string, SpriteAtlas> loadAtlas, string defaultAtlasName)
        {
            _folderToAtlasName.Clear();
            _cache.Clear();
            _warned.Clear();
            _atlasCache.Clear();
            if (folderToAtlasName != null)
                foreach (var kv in folderToAtlasName)
                    _folderToAtlasName[kv.Key] = kv.Value;
            _loadAtlas = loadAtlas;
            _defaultAtlasName = defaultAtlasName;
        }

        /// path → Sprite 查询。
        /// null/空 path → null（纯色无图）。查不到 → null（调用方 fallback，不崩）+ Debug.LogWarning 一次。
        public Sprite GetSprite(string path)
        {
            if (string.IsNullOrEmpty(path)) return null;
            if (_cache.TryGetValue(path, out var cached)) return cached;

            string atlasName = ResolveAtlasName(path);
            SpriteAtlas atlas = ResolveAtlas(atlasName);
            string spriteName = System.IO.Path.GetFileNameWithoutExtension(path);
            Sprite found = atlas != null ? atlas.GetSprite(spriteName) : null;

            if (found != null)
            {
                _cache[path] = found;       // 只缓存命中
                _warned.Remove(path);       // 命中清 warn（下次再 miss 会重 warn）
                return found;
            }
            // miss 不缓存 Sprite。warn 一次（去重，避免每帧刷屏）。
            if (_warned.Add(path))
                Debug.LogWarning(WarnMessage(path, atlasName, atlas, spriteName));
            return null;
        }

        public void Clear()
        {
            _folderToAtlasName.Clear();
            _cache.Clear();
            _warned.Clear();
            _atlasCache.Clear();
        }

        // ----- impl -----

        /// atlasName → SpriteAtlas。缓存命中直接返；miss 调 loadAtlas 委托（Driver 钩子），非 null 结果回填缓存。
        /// atlasName=null（无映射且无 default）或 loadAtlas=null → 返 null。
        SpriteAtlas ResolveAtlas(string atlasName)
        {
            if (atlasName == null) return null;
            if (_atlasCache.TryGetValue(atlasName, out var cached)) return cached;
            SpriteAtlas atlas = _loadAtlas != null ? _loadAtlas(atlasName) : null;
            if (atlas != null) _atlasCache[atlasName] = atlas;
            return atlas;
        }

        /// path → atlasName。顶层子目录查 folder→atlasName 表；无子目录或 miss → default atlasName（可能为 null）。
        string ResolveAtlasName(string path)
        {
            string topDir = TopDir(path);
            if (topDir == null) return _defaultAtlasName;                  // res 根图 → default
            if (_folderToAtlasName.TryGetValue(topDir, out var name)) return name;
            return _defaultAtlasName;                                      // 未知子目录 → default
        }

        static string WarnMessage(string path, string atlasName, SpriteAtlas atlas, string spriteName)
        {
            if (atlasName == null)
                return $"[SpriteResolver] 图不存在：path={path}（顶层子目录无映射且未配 default 图集）";
            if (atlas == null)
                return $"[SpriteResolver] 图集加载失败：atlasName='{atlasName}', path={path}";
            return $"[SpriteResolver] 图不存在：图集 '{atlasName}' 无 sprite '{spriteName}'（path={path}）";
        }

        /// 取 folder 末段作路由 key："Assets/LoomUI/res/icons" → "icons"。
        static string LastSegment(string folder)
        {
            string key = folder.TrimEnd('/', '\\');
            int sep = key.LastIndexOfAny(new[] { '/', '\\' });
            return sep >= 0 ? key.Substring(sep + 1) : key;
        }

        /// path 顶层子目录："icons/home.png" → "icons"；"home.png"（res 根图）→ null。
        static string TopDir(string path)
        {
            string p = path.Replace('\\', '/');
            int slash = p.IndexOf('/');
            return slash <= 0 ? null : p.Substring(0, slash);
        }
    }
}
