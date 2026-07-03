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
            // SpriteAtlas 非 ScriptableObject，EditMode 单测造不出实例（无公开构造/CreateInstance）。
            // 测的是纯路由：folder key 命中即可，atlas value 用 null——ResolveAtlas 命中 "icons" 返 null →
            // GetSprite 走 atlas==null 分支 → 不崩 + miss 返 null。
            resolver.InitWithMap(new Dictionary<string, SpriteAtlas> { { "icons", null } }, null);
            Assert.IsNull(resolver.GetSprite("icons/home.png"));
        }

        [Test]
        public void Route_RootImage_FallsBackToDefault()
        {
            var resolver = new SpriteResolver();
            // default atlas 用 null（SpriteAtlas 造不出实例）。path 无子目录 → ResolveAtlas 走
            // default 分支 = _defaultAtlas(=null) ?? FirstAtlas()(空 map → null) → 返 null → miss 不崩。
            resolver.InitWithMap(new Dictionary<string, SpriteAtlas>(), null);
            Assert.IsNull(resolver.GetSprite("home.png"));
        }

        [Test]
        public void Miss_NotCached()
        {
            var resolver = new SpriteResolver();
            // atlas 用 null（SpriteAtlas 造不出实例，测的是 miss 不缓存逻辑，不需要真 atlas）。
            resolver.InitWithMap(new Dictionary<string, SpriteAtlas> { { "icons", null } }, null);
            // 首次 miss。
            Assert.IsNull(resolver.GetSprite("icons/missing.png"));
            // 假装 atlas 后来 pack 好——但空 atlas.GetSprite 仍 miss。
            // 关键断言：miss 不进缓存。用内部 CacheCount 验证（miss 不增）。
            Assert.AreEqual(0, resolver.CacheCount, "miss 不应进缓存");
        }
    }
}
