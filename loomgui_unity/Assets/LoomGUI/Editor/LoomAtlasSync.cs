using System.Collections.Generic;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEngine;
using UnityEngine.U2D;
using UnityEditor.U2D;

namespace LoomGUI.Editor
{
    /// <summary>
    /// 图集 packables 同步：扫 atlasEntry.folders 下 PNG（递归）→ Sprite 列表，
    /// 用 Unity 6 Sprite Atlas V2 API（SpriteAtlasAsset.Load + Add/Remove + Save）替换 atlas packables。
    ///
    /// Unity 6 V2 真相（Agent 调研 + Venkify 官方 discussions.unity.com/t/949154, /938750）：
    ///   - V2 文件扩展名 .spriteatlasv2（V1 是 .spriteatlas）。SpriteAtlasAsset.Load 只认 V2。
    ///   - V2 创建：new SpriteAtlasAsset()（不是 new SpriteAtlas）+ Save(.spriteatlasv2) + Refresh。
    ///   - pack 须 Project Settings > Editor > Sprite Packer > Mode = Always Enabled。Disabled 时 packed.spriteCount=0，
    ///     运行时 GetSprite 全 miss（SaveAndReimport 触发 pack，但 Packer 关了不 pack）。
    ///   - m_Guid 全零是 V1 字段问题，V2 没这字段（用 .meta guid）。
    ///   - 改 packables：SpriteAtlasAsset.Load → Add/Remove → Save（用 Refresh，不用 SetDirty+SaveAssets）。
    /// </summary>
    public static class LoomAtlasSync
    {
        /// 纯逻辑：算 packables 增删（可单测，不碰 AssetDatabase）。
        public static (HashSet<string> toAdd, HashSet<string> toRemove) DiffPackables(
            HashSet<string> current, HashSet<string> scanned)
        {
            var toAdd = new HashSet<string>(scanned);
            toAdd.ExceptWith(current);
            var toRemove = new HashSet<string>(current);
            toRemove.ExceptWith(scanned);
            return (toAdd, toRemove);
        }

        /// 同步所有图集。Unity Editor only。自动建缺失的 V2 .spriteatlasv2。
        public static void SyncAll(LoomSettings settings)
        {
            if (settings == null) return;
            int okCount = 0, failCount = 0;
            foreach (var entry in settings.atlasEntries)
            {
                string atlasRel = EnsureAtlasAsset(entry, settings.workspaceDir);
                if (atlasRel == null) { failCount++; continue; }
                SyncEntry(entry);
                okCount++;
            }
            EditorUtility.SetDirty(settings);
            AssetDatabase.SaveAssetIfDirty(settings);
            if (failCount > 0)
                Debug.LogWarning($"[LoomAtlasSync] {failCount} 个图集创建/定位失败（已跳过），{okCount} 个同步。");
        }

        /// 确保 entry 的 V2 .spriteatlasv2 存在。返 Unity 相对路径或 null。
        public static string EnsureAtlasAsset(AtlasEntry entry, string workspaceDir)
        {
            if (entry == null || string.IsNullOrEmpty(entry.atlasName)) return null;
            string rel = ResolveAtlasPath(entry);
            if (rel != null && File.Exists(ToAbs(rel)))
            {
                entry.atlas = AssetDatabase.LoadAssetAtPath<SpriteAtlas>(rel);   // 强制绑 V2（覆盖旧 V1 引用）
                return rel;
            }
            if (string.IsNullOrEmpty(workspaceDir)) return null;

            string dir = (Path.Combine(workspaceDir, "atlas")).Replace('\\', '/');
            Directory.CreateDirectory(ToAbs(dir));
            rel = dir + "/" + entry.atlasName + ".spriteatlasv2";

            // new SpriteAtlasAsset() + Save 建 V2（Venkify 法）；Refresh 触发 import 生成 SpriteAtlasImporter。
            var saa = new SpriteAtlasAsset();
            SpriteAtlasAsset.Save(saa, rel);
            AssetDatabase.Refresh();

            entry.atlas = AssetDatabase.LoadAssetAtPath<SpriteAtlas>(rel);
            Debug.Log($"[LoomAtlasSync] 自动创建 V2 图集：{rel}");
            return rel;
        }

        /// 删除 entry 的自动生成图集（workspaceDir/atlas/ 下，.spriteatlasv2 优先，.spriteatlas 兼容）。
        public static bool DeleteAutoAtlas(AtlasEntry entry, string workspaceDir)
        {
            if (entry == null || string.IsNullOrEmpty(entry.atlasName) || string.IsNullOrEmpty(workspaceDir)) return false;
            foreach (var ext in new[] { ".spriteatlasv2", ".spriteatlas" })
            {
                string rel = (Path.Combine(workspaceDir, "atlas", entry.atlasName + ext)).Replace('\\', '/');
                if (File.Exists(ToAbs(rel))) return AssetDatabase.DeleteAsset(rel);
            }
            return false;
        }

        /// 同步单个图集：扫 folders 下 PNG → Sprite → 替换 atlas packables（V2 API）。
        public static void SyncEntry(AtlasEntry entry)
        {
            if (entry == null || string.IsNullOrEmpty(entry.atlasName)) return;

            string atlasRel = ResolveAtlasPath(entry);
            if (atlasRel == null || !File.Exists(ToAbs(atlasRel)))
            {
                Debug.LogError($"[LoomAtlasSync] V2 .spriteatlasv2 不存在：{entry.atlasName}");
                return;
            }

            // 扫 folders 下 PNG（递归）→ Sprite 引用。
            var sprites = new List<Object>();
            var missing = new List<string>();
            foreach (var folder in entry.folders)
            {
                foreach (var pngAbs in EnumeratePngs(folder))
                {
                    string pngRel = ToAssetPath(pngAbs);
                    var sp = AssetDatabase.LoadAssetAtPath<Sprite>(pngRel);
                    if (sp != null) sprites.Add(sp);
                    else missing.Add(pngRel);
                }
            }
            if (missing.Count > 0)
                Debug.LogWarning($"[LoomAtlasSync] {entry.atlasName}：{missing.Count} 个 PNG 未导成 Sprite（跳过）：\n  " +
                                 string.Join("\n  ", missing.Take(8)) + (missing.Count > 8 ? "\n  ..." : ""));

            var atlas = AssetDatabase.LoadAssetAtPath<SpriteAtlas>(atlasRel);
            if (atlas == null) { Debug.LogError($"[LoomAtlasSync] {atlasRel} 不是 SpriteAtlas 资产"); return; }

            var oldPackables = atlas.GetPackables();
            var atlasAsset = SpriteAtlasAsset.Load(atlasRel);
            if (atlasAsset == null)
            {
                Debug.LogError($"[LoomAtlasSync] SpriteAtlasAsset.Load 失败：{atlasRel}（不是 V2 .spriteatlasv2？删旧 .spriteatlas 重同步）");
                return;
            }
            if (oldPackables != null && oldPackables.Length > 0)
                atlasAsset.Remove(oldPackables);
            if (sprites.Count > 0)
                atlasAsset.Add(sprites.ToArray());
            SpriteAtlasAsset.Save(atlasAsset, atlasRel);

            // SaveAndReimport 触发 SpriteAtlasImporter pack（须 Project Settings > Sprite Packer = Always Enabled）。
            var importer = (SpriteAtlasImporter)AssetImporter.GetAtPath(atlasRel);
            if (importer == null) { Debug.LogError($"[LoomAtlasSync] SpriteAtlasImporter 获取失败：{atlasRel}"); return; }
            importer.SaveAndReimport();

            entry.atlas = AssetDatabase.LoadAssetAtPath<SpriteAtlas>(atlasRel);
            Debug.Log($"[LoomAtlasSync] {entry.atlasName}：{sprites.Count} Sprite 同步 → {atlasRel}");
        }

        /// atlasEntry → V2 .spriteatlasv2 路径。优先 entry.atlas 引用（须 .spriteatlasv2），否则按名搜 V2。
        static string ResolveAtlasPath(AtlasEntry entry)
        {
            if (entry.atlas != null)
            {
                string p = AssetDatabase.GetAssetPath(entry.atlas);
                if (!string.IsNullOrEmpty(p) && p.EndsWith(".spriteatlasv2", System.StringComparison.OrdinalIgnoreCase))
                    return p;
            }
            string[] guids = AssetDatabase.FindAssets(entry.atlasName + " t:SpriteAtlas");
            foreach (var g in guids)
            {
                string p = AssetDatabase.GUIDToAssetPath(g);
                if (Path.GetFileName(p) == entry.atlasName + ".spriteatlasv2") return p;
            }
            return null;
        }

        static IEnumerable<string> EnumeratePngs(string folderUnityRel)
        {
            string abs = ToAbs(folderUnityRel);
            if (!Directory.Exists(abs)) yield break;
            foreach (var f in Directory.GetFiles(abs, "*.png", SearchOption.AllDirectories))
                yield return f;
        }

        static string ToAbs(string unityRel)
        {
            string projRoot = Directory.GetParent(Application.dataPath).FullName;
            return Path.GetFullPath(Path.Combine(projRoot, unityRel));
        }

        static string ToAssetPath(string abs)
        {
            string projRoot = Directory.GetParent(Application.dataPath).FullName.Replace('\\', '/');
            string normalized = abs.Replace('\\', '/');
            string prefix = projRoot.EndsWith("/") ? projRoot : projRoot + "/";
            if (normalized.StartsWith(prefix, System.StringComparison.OrdinalIgnoreCase))
                return normalized.Substring(prefix.Length);
            return normalized;
        }
    }
}
