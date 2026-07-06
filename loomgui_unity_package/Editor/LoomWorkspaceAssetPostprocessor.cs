using System.IO;
using UnityEditor;
using UnityEngine;

namespace LoomGUI.Editor
{
    /// <summary>
    /// 工作区 PNG 强制导入为 Sprite（进 SpriteAtlas）。
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
