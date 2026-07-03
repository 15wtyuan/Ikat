using System.IO;
using UnityEditor;
using UnityEngine;

namespace LoomGUI.Editor
{
    /// <summary>
    /// 拦工作区下非资源文件（.html/.css/.claude/CLAUDE.md/design-systems/.od-skills）不让 Unity 导入。
    /// 这些是给 AI/open-design 用的纯文本，导入会生成多余 .meta + 尝试导入 .css。
    /// PNG 正常导入为 Sprite（进 SpriteAtlas）。
    /// </summary>
    public sealed class LoomWorkspaceAssetPostprocessor : AssetPostprocessor
    {
        static bool ShouldSkip(string assetPath)
        {
            // 工作区根从 LoomSettings 拿（运行时配置资产）。
            var s = LoomSettings.GetOrCreateDefault();
            if (s == null || string.IsNullOrEmpty(s.workspaceDir)) return false;
            string ws = s.workspaceDir.Replace('\\', '/').TrimEnd('/') + "/";
            string p = assetPath.Replace('\\', '/');
            if (!p.StartsWith(ws)) return false;

            string name = Path.GetFileName(p);
            if (name == "CLAUDE.md") return true;
            if (p.Contains("/.claude/")) return true;
            if (p.Contains("/.od-skills/")) return true;
            if (p.Contains("/design-systems/")) return true;
            if (p.EndsWith(".html") || p.EndsWith(".css")) return true;
            return false;
        }

        void OnPreprocessAsset()
        {
            if (ShouldSkip(assetPath))
            {
                // 跳过导入：让 Unity 不生成 importer / 不尝试解析。
                var importer = assetImporter as AssetImporter;
                if (importer != null) importer.SetNonAsset();  // Unity 6：标记为非资产不入库
            }
            // PNG 强制 Sprite 导入（进 SpriteAtlas）。
            if (assetPath.EndsWith(".png"))
            {
                var ti = assetImporter as TextureImporter;
                if (ti != null && ti.textureType != TextureImporterType.Sprite)
                {
                    ti.textureType = TextureImporterType.Sprite;
                }
            }
        }
    }
}
