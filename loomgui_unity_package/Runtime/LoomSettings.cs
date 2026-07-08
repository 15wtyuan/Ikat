using System;
using System.Collections.Generic;
using UnityEngine;


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
        public string pkgOutputDir = "Assets/LoomGUI/Bundles/";

        [Tooltip("字体列表（familyName=CSS font-family；sourceFileName=driver 拼 .bytes 路径）")]
        public List<FontEntry> fonts = new();

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

        /// 只加载不建——import 流水线里 CreateAsset 被禁，用这个。找不到返 null，调用方容错跳过。
        public static LoomSettings GetDefault() => Resources.Load<LoomSettings>(ResourcesPath);
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

    /// 图集配置。folders 拖文件夹当 packables；atlas 运行时按 atlasName 加载（图集 tab 同步到 Bundles/atlas/，不持资产引用）。
    [Serializable]
    public sealed class AtlasEntry
    {
        public string atlasName = "";
        [Tooltip("res 根图（path 无子目录）兜底走此图集")]
        public bool isDefault;
        [Tooltip("packables 文件夹（Unity 相对路径）")]
        public List<string> folders = new();
        // REMOVED: public SpriteAtlas atlas — would drag asset into Resources build.
        // SpriteAtlas is now resolved by atlasName in the build pipeline, not stored as a serialized reference.
    }

    /// 字体配置。不持有 Font asset 引用（避免 Resources build 拖入资产）。
    /// familyName 对应 CSS font-family；sourceFileName 让 driver 拼 .bytes 路径。
    [Serializable]
    public class FontEntry {
        [Tooltip("CSS font-family 值。拖入时默认=源文件名去扩展，可手改")]
        public string familyName;
        [Tooltip("源文件名（如 NotoSansSC.ttc）。拖 asset 时自动填，driver 拼 .bytes 路径用")]
        public string sourceFileName;
        [Tooltip("默认回退字体（measure 时 primary font-family 无匹配/为空用此）。全局唯一")]
        public bool isDefault;
        [Tooltip("是否纳入全局回退链：主字体缺字时按序 probe 这些字体补。多个 isFallback 按列表序")]
        public bool isFallback;
    }
}
