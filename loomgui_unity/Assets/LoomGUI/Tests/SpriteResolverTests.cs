using System.Collections.Generic;
using NUnit.Framework;
using UnityEngine;
using UnityEngine.U2D;

namespace LoomGUI.Tests
{
    public class SpriteResolverTests
    {
        // 构造一个不依赖真实 SpriteAtlas 资产的 SpriteResolver：直接注入 folder→atlas 映射。
        // Init(LoomSettings) 走 atlasEntries；测试用 InitWithMap 直接注入映射表。

        [Test]
        public void Route_ByTopLevelSubdir()
        {
            var resolver = new SpriteResolver();
            var atlasIcons = ScriptableObject.CreateInstance<SpriteAtlas>(); // 空 atlas
            resolver.InitWithMap(new Dictionary<string, SpriteAtlas> { { "icons", atlasIcons } }, atlasIcons);
            // atlasIcons 无 sprite → 返 missingSprite（null），但路由到 icons（不抛）。
            // 验证：不遍历别的 atlas（无 NRE），miss 返 null。
            Assert.IsNull(resolver.GetSprite("icons/home.png"));
        }

        [Test]
        public void Route_RootImage_FallsBackToDefault()
        {
            var resolver = new SpriteResolver();
            var defaultAtlas = ScriptableObject.CreateInstance<SpriteAtlas>();
            resolver.InitWithMap(new Dictionary<string, SpriteAtlas>(), defaultAtlas);
            // path 无子目录 → 走 default atlas。
            Assert.IsNull(resolver.GetSprite("home.png"));
        }

        [Test]
        public void Miss_NotCached()
        {
            var resolver = new SpriteResolver();
            var atlas = ScriptableObject.CreateInstance<SpriteAtlas>();
            resolver.InitWithMap(new Dictionary<string, SpriteAtlas> { { "icons", atlas } }, atlas);
            // 首次 miss。
            Assert.IsNull(resolver.GetSprite("icons/missing.png"));
            // 假装 atlas 后来 pack 好——但空 atlas.GetSprite 仍 miss。
            // 关键断言：miss 不进缓存。用内部 CacheCount 验证（miss 不增）。
            Assert.AreEqual(0, resolver.CacheCount, "miss 不应进缓存");
        }
    }
}
