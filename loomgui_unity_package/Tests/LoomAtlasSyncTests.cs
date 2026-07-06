using System.Collections.Generic;
using System.Linq;
using System.Reflection;
using LoomGUI.Editor;
using NUnit.Framework;
using UnityEditor;
using UnityEngine;

namespace LoomGUI.Tests
{
    public class LoomAtlasSyncTests
    {
        // 纯逻辑测试：DiffPackables（算增删）。
        // 不碰真实 AssetDatabase（那需要 Unity 编辑器环境）。

        [Test]
        public void DiffPackables_AddsMissingRemovesExtra()
        {
            // 现有 atlas packables = {a, b}，扫描出 {b, c} → 应加 c 删 a。
            var current = new HashSet<string> { "a.png", "b.png" };
            var scanned = new HashSet<string> { "b.png", "c.png" };
            var (toAdd, toRemove) = LoomAtlasSync.DiffPackables(current, scanned);
            Assert.That(toAdd, Is.EquivalentTo(new[] { "c.png" }));
            Assert.That(toRemove, Is.EquivalentTo(new[] { "a.png" }));
        }

        [Test]
        public void DiffPackables_NoChange()
        {
            var current = new HashSet<string> { "a.png" };
            var scanned = new HashSet<string> { "a.png" };
            var (toAdd, toRemove) = LoomAtlasSync.DiffPackables(current, scanned);
            Assert.IsEmpty(toAdd);
            Assert.IsEmpty(toRemove);
        }

        // EnsureAtlasAsset 必须把 .spriteatlasv2 写到 {pkgOutputDir}/atlas/（= Bundles/atlas/），
        // 不能再写到工作区根下的 atlas/。运行时 Resources/AB 加载路径绑定 Bundles/atlas/，错位会全 miss。
        // 此用例在 Unity EditMode 跑（SpriteAtlasAsset.Save + AssetDatabase 需编辑器环境）。
        [Test]
        public void EnsureAtlasAsset_WritesToBundlesAtlas()
        {
            var settings = ScriptableObject.CreateInstance<LoomSettings>();
            settings.workspaceDir = "Assets/LoomUI/";
            settings.pkgOutputDir = "Assets/LoomGUI/Bundles/";
            var entry = new AtlasEntry { atlasName = "testatlas_b5", folders = new List<string>() };
            try
            {
                string rel = LoomAtlasSync.EnsureAtlasAsset(entry, settings.pkgOutputDir);
                Assert.IsNotNull(rel, "EnsureAtlasAsset 应返回新建路径");
                Assert.IsTrue(rel.StartsWith("Assets/LoomGUI/Bundles/atlas/"),
                    $"atlas 必须落在 Bundles/atlas/，实落 {rel}");
            }
            finally
            {
                // 清理测试创建的文件 + .meta，避免污染工程。
                if (LoomAtlasSync.DeleteAutoAtlas(entry, settings.pkgOutputDir)) { /* deleted */ }
                AssetDatabase.Refresh();
                Object.DestroyImmediate(settings);
            }
        }

        // AtlasEntry 不得再持有 SpriteAtlas 引用字段（会把图集资产拖进 Resources/AB 构建）。
        [Test]
        public void AtlasEntry_HasNoSpriteAtlasField()
        {
            FieldInfo fi = typeof(AtlasEntry).GetField("atlas", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
            Assert.IsNull(fi, "AtlasEntry must NOT have an 'atlas' field (resolved by atlasName at runtime)");
        }
    }
}
