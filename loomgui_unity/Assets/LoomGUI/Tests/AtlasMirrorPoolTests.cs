using NUnit.Framework;
using UnityEngine;

namespace LoomGUI.Tests
{
    /// SpriteResolver path→Sprite 查询冒烟测试（不依赖 blob，纯 path→Sprite 逻辑）。
    ///
    /// 图集路由：path 顶层子目录 → folder→atlas 映射（LoomSettings 配）→ atlas.GetSprite(文件名去扩展)。
    /// 同 atlas 多 sprite 共享 texture 由 SpriteAtlas 保证；子区 UV 由 MirrorPool.RemapMeshUvToSprite 算。
    public class AtlasMirrorPoolPathTests
    {
        /// 无 atlas 注册 → GetSprite 返 null（fallback 路径，调用方走 Texture2D.whiteTexture，不崩）。
        [Test]
        public void SpriteResolver_NoAtlas_ReturnsNull()
        {
            var resolver = new SpriteResolver();
            var sp = resolver.GetSprite("icons/skin.png");
            Assert.IsNull(sp, "无 atlas 注册 → GetSprite 返 null");
        }

        /// SpriteResolver Init(null) 不崩（防御）。
        [Test]
        public void SpriteResolver_InitNull_DoesNotCrash()
        {
            var resolver = new SpriteResolver();
            resolver.Init(null);
            Assert.AreEqual(0, resolver.AtlasCount, "null settings → AtlasCount=0");
        }
    }
}
