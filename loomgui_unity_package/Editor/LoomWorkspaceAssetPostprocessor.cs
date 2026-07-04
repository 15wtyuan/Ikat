using System.IO;
using UnityEditor;
using UnityEngine;

namespace LoomGUI.Editor
{
    /// <summary>
    /// 工作区 PNG 强制导入为 Sprite（进 SpriteAtlas）。
    ///
    /// 非资源文件（CLAUDE.md / .html / .css）挡不住导入——这是 Unity 硬规则：
    ///   - Unity 只忽略**点开头**的文件/目录（.claude / .git 等，不生成 .meta）；
    ///   - 非点开头文件（CLAUDE.md / *.html / *.css）必然 import 成 DefaultAsset + .meta，
    ///     无公开 API 能阻止（OnPreprocessAsset 无法 abort；.unityignore 是社区未实现请求）。
    ///   - 原 spec §3.2「跳过导入」理想化；AssetImporter.SetNonAsset() 是幻觉 API。
    /// 决策（2026-07-04）：接受现状——DefaultAsset 不进 build，打包器 File.ReadAllText 直读源文件，
    /// 不影响运行时/打包/AI 工作流。Project 窗口多几个资产图标是 Unity 固有代价。
    /// </summary>
    public sealed class LoomWorkspaceAssetPostprocessor : AssetPostprocessor
    {
        void OnPreprocessAsset()
        {
            // 只管工作区下 PNG（避免改工程其他 PNG——3D 纹理/背景/插件贴图——的导入设置）。
            // GetDefault 只加载不建：OnPreprocessAsset 在 import 期跑，import 期禁 CreateAsset
            // （GetOrCreateDefault 找不到会建→UnityException）。settings 没加载好就跳过。
            string ws = LoomSettings.GetDefault()?.workspaceDir;
            if (string.IsNullOrEmpty(ws)) return;
            string norm = assetPath.Replace('\\', '/');
            if (!norm.StartsWith(ws.Replace('\\', '/'), System.StringComparison.OrdinalIgnoreCase)) return;

            // PNG 强制 Sprite + 未压缩（pack 到 SpriteAtlas 不丢像素精度，避免压缩 source 警告）。
            if (norm.EndsWith(".png"))
            {
                var ti = assetImporter as TextureImporter;
                if (ti != null)
                {
                    if (ti.textureType != TextureImporterType.Sprite)
                        ti.textureType = TextureImporterType.Sprite;
                    if (ti.textureCompression != TextureImporterCompression.Uncompressed)
                        ti.textureCompression = TextureImporterCompression.Uncompressed;
                }
            }
        }
    }
}
