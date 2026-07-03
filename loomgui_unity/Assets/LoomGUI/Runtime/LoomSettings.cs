using System;
using System.Collections.Generic;
using UnityEngine;
using UnityEngine.U2D;

namespace LoomGUI
{
    /// <summary>
    /// 全局配置资产（Editor+运行时同源）。放 Resources/LoomGUI/，运行时 Resources.Load 可读。
    /// LoomSettingsWindow 编辑它；LoomStage 运行时读它建图集路由表。
    /// </summary>
    [CreateAssetMenu(menuName = "LoomGUI/Settings", fileName = "LoomSettings")]
    public sealed class LoomSettings : ScriptableObject
    {
        [Tooltip("工作区根（Unity 工程相对路径，open-design import 此目录）")]
        public string workspaceDir = "Assets/LoomUI/";

        [Tooltip("资源目录名（打包器按此前缀归一化 img path，默认 res）")]
        public string resDirName = "res";

        [Tooltip("pkg.bin 输出目录（Unity 工程相对路径）")]
        public string pkgOutputDir = "Assets/StreamingAssets/";

        [Tooltip("包列表")]
        public List<PackageEntry> packages = new();

        [Tooltip("图集配置（path 顶层子目录 → 图集 路由）")]
        public List<AtlasEntry> atlasEntries = new();

        /// Resources 内相对路径（无扩展名），Resources.Load 用。
        public const string ResourcesPath = "LoomGUI/LoomSettings";

        /// 在 Resources 找配置资产；不存在则创建（Editor 下；运行时找不到返 null 调用方容错）。
        public static LoomSettings GetOrCreateDefault()
        {
            var existing = Resources.Load<LoomSettings>(ResourcesPath);
#if UNITY_EDITOR
            if (existing == null)
            {
                existing = CreateInstance<LoomSettings>();
                const string assetPath = "Assets/Resources/LoomGUI/LoomSettings.asset";
                UnityEditor.AssetDatabase.CreateAsset(existing, assetPath);
                UnityEditor.AssetDatabase.SaveAssets();
            }
#endif
            return existing;
        }
    }

    /// 单个包配置。sourceDir 相对工作区根（如 "showcase"）。
    [Serializable]
    public sealed class PackageEntry
    {
        public string pkgName = "";
        [Tooltip("源目录（相对工作区根，含 html + 引用 res 下图片）")]
        public string sourceDir = "";
        [Tooltip("html 文件名列表（相对 sourceDir）")]
        public List<string> htmlFiles = new();

        public PackageEntry() { }
        public PackageEntry(string pkgName, string sourceDir) { this.pkgName = pkgName; this.sourceDir = sourceDir; }
    }

    /// 图集配置。folders 拖文件夹当 packables；atlas 运行时引用（图集 tab 同步时绑）。
    [Serializable]
    public sealed class AtlasEntry
    {
        public string atlasName = "";
        [Tooltip("res 根图（path 无子目录）兜底走此图集")]
        public bool isDefault;
        [Tooltip("packables 文件夹（Unity 相对路径）")]
        public List<string> folders = new();
        [Tooltip("运行时图集引用（同步时自动绑）")]
        public SpriteAtlas atlas;
    }
}
