using System.Collections.Generic;
using NUnit.Framework;
using UnityEngine;

namespace LoomGUI.Tests
{
    /// <summary>
    /// SpriteResolver basic lookup smoke tests (self-drawn atlas, no Unity SpriteAtlas).
    /// Pure path → SpriteLookup logic; no FrameBlob dependency.
    /// </summary>
    public class AtlasMirrorPoolPathTests
    {
        /// <summary>
        /// No atlases registered → GetSprite returns found=false (caller fallback to whiteTexture, no crash).
        /// </summary>
        [Test]
        public void SpriteResolver_NoAtlas_ReturnsNotFound()
        {
            var resolver = new SpriteResolver();
            resolver.Init(null, null);
            var look = resolver.GetSprite("icons/skin.png");
            Assert.IsFalse(look.found, "no atlases → found=false");
        }

        [Test]
        public void SpriteResolver_InitNull_DoesNotCrash()
        {
            var resolver = new SpriteResolver();
            resolver.Init(null, null);
            var look = resolver.GetSprite("icons/skin.png");
            Assert.IsFalse(look.found);
        }
    }
}
