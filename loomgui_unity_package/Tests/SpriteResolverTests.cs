using System.Collections.Generic;
using NUnit.Framework;
using UnityEngine;
using UnityEngine.U2D;

namespace LoomGUI.Tests
{
    /// <summary>
    /// SpriteResolver 名字路由 + 懒加载委托 + atlas 缓存 测试。
    ///
    /// 纯逻辑 EditMode 测：SpriteAtlas.GetSprite 需要真实导入的打包图集（EditMode 造不出可读 sprite），
    /// 所以这里只验"路由 + 缓存"——folder→atlasName 解析、loadAtlas 委托按 atlasName 调一次后缓存、
    /// default 回退。Sprite 实际命中由 PlayMode 验收（真实图集）。
    /// 哨兵 SpriteAtlas 用 new SpriteAtlas()（空图集，GetSprite 返 null，但对象非空 → 会进 _atlasCache，
    /// 用来证明 atlas 缓存生效）。
    /// </summary>
    public class SpriteResolverTests
    {
        [Test]
        public void Init_BuildsFolderToAtlasNameMapping_FromSettings()
        {
            var resolver = new SpriteResolver();
            var settings = ScriptableObject.CreateInstance<LoomSettings>();
            settings.atlasEntries.Add(new AtlasEntry
            {
                atlasName = "ui_atlas",
                folders = new List<string> { "Assets/LoomUI/res/ui" }   // 末段 "ui" 作 key
            });

            int loadCount = 0;
            string receivedName = null;
            resolver.Init(settings, name => { loadCount++; receivedName = name; return null; });

            // path 顶层 "ui" → atlasName "ui_atlas" → 委托被调一次。
            resolver.GetSprite("ui/btn.png");
            Assert.AreEqual(1, loadCount, "folder→atlasName 命中应调 loadAtlas");
            Assert.AreEqual("ui_atlas", receivedName);

            ScriptableObject.DestroyImmediate(settings);
        }

        [Test]
        public void GetSprite_CachesAtlas_LoaderRunsOncePerAtlasName()
        {
            var resolver = new SpriteResolver();
            int loadCount = 0;
            // 映射两个 folder 到同一 atlasName，验证同 atlasName 只加载一次。
            // 非空哨兵 SpriteAtlas → 进 _atlasCache（null 不进缓存，测不出缓存命中）。
            resolver.InitWithMap(
                new Dictionary<string, string> { { "icons", "icons_atlas" }, { "flags", "icons_atlas" } },
                name => { loadCount++; return new SpriteAtlas(); },
                null);

            // 同一 atlasName 下查多个 sprite + 多个 folder：atlas 应只加载一次。
            resolver.GetSprite("icons/home.png");
            resolver.GetSprite("icons/back.png");
            resolver.GetSprite("flags/cn.png");
            Assert.AreEqual(1, loadCount, "同 atlasName 多次 GetSprite 应命中 atlas 缓存，loader 只调一次");
        }

        [Test]
        public void GetSprite_ResRootPath_FallsBackToDefaultAtlasName()
        {
            var resolver = new SpriteResolver();
            string receivedName = "untouched";
            resolver.InitWithMap(
                new Dictionary<string, string> { { "icons", "icons_atlas" } },
                name => { receivedName = name; return null; },
                "default_atlas");

            // path 无子目录 → TopDir=null → default atlasName。
            resolver.GetSprite("logo.png");
            Assert.AreEqual("default_atlas", receivedName, "res 根图应走 default atlasName");
        }

        [Test]
        public void GetSprite_UnknownSubdir_FallsBackToDefaultAtlasName()
        {
            var resolver = new SpriteResolver();
            string receivedName = "untouched";
            resolver.InitWithMap(
                new Dictionary<string, string> { { "icons", "icons_atlas" } },
                name => { receivedName = name; return null; },
                "default_atlas");

            // 顶层子目录不在表 → default。
            resolver.GetSprite("unknown_thing/x.png");
            Assert.AreEqual("default_atlas", receivedName);
        }

        [Test]
        public void GetSprite_NoMappingNoDefault_LoaderNeverCalled()
        {
            var resolver = new SpriteResolver();
            int loadCount = 0;
            resolver.InitWithMap(
                new Dictionary<string, string>(),
                name => { loadCount++; return null; },
                null);   // 无 default

            // 无映射 + 无 default → atlasName=null → ResolveAtlas 早返 null，loadAtlas 不应被调。
            Assert.IsNull(resolver.GetSprite("icons/home.png"));
            Assert.AreEqual(0, loadCount, "atlasName=null 时不应回调 loader");
        }

        [Test]
        public void GetSprite_NullOrEmptyPath_ReturnsNull()
        {
            var resolver = new SpriteResolver();
            resolver.InitWithMap(new Dictionary<string, string> { { "icons", "a" } }, _ => null, null);
            Assert.IsNull(resolver.GetSprite(null));
            Assert.IsNull(resolver.GetSprite(""));
        }

        [Test]
        public void Init_NullSettings_DoesNotCrash()
        {
            var resolver = new SpriteResolver();
            resolver.Init(null, null);
            Assert.AreEqual(0, resolver.AtlasCount);
            // null settings + null loader → GetSprite 安全返 null。
            Assert.IsNull(resolver.GetSprite("icons/x.png"));
        }

        [Test]
        public void Miss_NotCachedInSpriteCache()
        {
            var resolver = new SpriteResolver();
            resolver.InitWithMap(
                new Dictionary<string, string> { { "icons", "icons_atlas" } },
                _ => null,   // atlas=null → ResolveAtlas 返 null → GetSprite miss
                null);

            Assert.IsNull(resolver.GetSprite("icons/missing.png"));
            // miss 不进 Sprite 缓存（与 atlas 缓存区分）。
            Assert.AreEqual(0, resolver.CacheCount, "Sprite miss 不应进 _cache");
        }
    }
}
