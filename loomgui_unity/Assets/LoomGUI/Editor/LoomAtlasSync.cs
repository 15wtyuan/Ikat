using System.Collections.Generic;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEngine;
using UnityEngine.U2D;

namespace LoomGUI.Editor
{
    /// <summary>
    /// 图集 packables 同步：扫 atlasEntry.folders 下 PNG，与 atlas 当前 packables diff，
    /// 增删 Sprite 引用。改用显式 Sprite 列表（修 B2：folder packable Unity 6 静默打成空）。
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

        /// 同步所有图集。Unity Editor only。
        public static void SyncAll(LoomSettings settings)
        {
            if (settings == null) return;
            foreach (var entry in settings.atlasEntries)
            {
                SyncEntry(settings, entry);
            }
            EditorUtility.SetDirty(settings);
            AssetDatabase.SaveAssetIfDirty(settings);
        }

        /// 同步单个图集：确保 atlas 资产存在 + packables = folders 下所有 Sprite。
        public static void SyncEntry(LoomSettings settings, AtlasEntry entry)
        {
            if (entry == null || string.IsNullOrEmpty(entry.atlasName)) return;

            // 确保图集资产存在（不存在则创建）。
            if (entry.atlas == null)
            {
                string atlasPath = $"Assets/LoomUI/res/{entry.atlasName}.spriteatlas";
                entry.atlas = AssetDatabase.LoadAssetAtPath<SpriteAtlas>(atlasPath);
                if (entry.atlas == null)
                {
                    entry.atlas = new SpriteAtlas();
                    AssetDatabase.CreateAsset(entry.atlas, atlasPath);
                }
            }

            // 扫 folders 下 PNG → Sprite 引用集合。
            var scannedSprites = new HashSet<string>();
            var toAdd = new List<UnityEngine.Object>();
            foreach (var folder in entry.folders)
            {
                if (string.IsNullOrEmpty(folder)) continue;
                string absFolder = ToAbs(folder);
                if (!Directory.Exists(absFolder)) continue;
                foreach (var png in Directory.GetFiles(absFolder, "*.png", SearchOption.AllDirectories))
                {
                    string assetPath = ToAssetPath(png);
                    var importer = AssetImporter.GetAtPath(assetPath) as TextureImporter;
                    if (importer != null && importer.textureType != TextureImporterType.Sprite)
                    {
                        importer.textureType = TextureImporterType.Sprite;
                        importer.SaveAndReimport();
                    }
                    var sp = AssetDatabase.LoadAssetAtPath<Sprite>(assetPath);
                    if (sp != null) { scannedSprites.Add(assetPath); toAdd.Add(sp); }
                }
            }

            // 显式设 packables（替 folder packable，修 B2）。
            entry.atlas.SetPackables(toAdd.ToArray());
            EditorUtility.SetDirty(entry.atlas);
        }

        static string ToAbs(string unityRel)
        {
            string projRoot = Directory.GetParent(Application.dataPath).FullName;
            return Path.GetFullPath(Path.Combine(projRoot, unityRel));
        }

        static string ToAssetPath(string abs)
        {
            string projRoot = Directory.GetParent(Application.dataPath).FullName.Replace('\\', '/');
            return abs.Replace('\\', '/').Replace(projRoot + "/", "");
        }
    }
}
