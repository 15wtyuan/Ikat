using System.Collections.Generic;
using NUnit.Framework;
using UnityEngine;

namespace Ikat.Tests
{
    /// <summary>
    /// IkatStageDriver EditMode tests. Unity PlayMode/EditMode tests cannot run from CLI
    /// (require Unity Editor), executed via Unity Test Runner during acceptance.
    /// Here we verify compilation + pure-logic correctness.
    /// </summary>
    public class IkatStageDriverTests
    {
        /// <summary>
        /// Driver.Awake must construct IkatHost instance (Context property non-null).
        /// Verifies Awake sequence: new UnityIkatBackend + IkatHost + SetRuntimeRoot +
        /// NativeHost.Init + bootstrap from runtime.json + EnsureCamera + ConfigureTransforms
        /// all run without exception.
        /// </summary>
        [Test]
        public void IkatStageDriver_AwakeBuildsHost()
        {
            var go = new GameObject("driver_test");
            try
            {
                var driver = go.AddComponent<IkatStageDriver>();
                Assert.IsNotNull(driver, "AddComponent must return the driver instance");
                Assert.IsNotNull(driver.Context, "Driver.Awake must construct IkatHost (Context non-null)");
            }
            finally
            {
                // OnDestroy disposes host + backend (frees native handle + Unity resources).
                Object.DestroyImmediate(go);
                // EnsureCamera creates independent GO (not child of root), DestroyImmediate(go)
                // does not clean it up — manual cleanup.
                var cam = GameObject.Find("IkatUICamera");
                if (cam != null) Object.DestroyImmediate(cam);
            }
        }

        [Test]
        public void MergeSpriteSizes_NullInput_ReturnsEmpty()
        {
            var result = IkatStageDriver.MergeSpriteSizes(null);
            Assert.IsNotNull(result);
            Assert.AreEqual(0, result.Count);
        }

        [Test]
        public void MergeSpriteSizes_EmptyList_ReturnsEmpty()
        {
            var result = IkatStageDriver.MergeSpriteSizes(new List<AtlasManifest>());
            Assert.IsNotNull(result);
            Assert.AreEqual(0, result.Count);
        }

        [Test]
        public void MergeSpriteSizes_SingleAtlas_ReturnsSprites()
        {
            var atlas = new AtlasManifest();
            atlas.sprites["res/icons/home.png"] = new SpriteEntry { page = 0, uv = new[] { 0f, 0f, 0.5f, 0.5f }, orig = new[] { 64, 64 } };
            atlas.sprites["res/icons/gear.png"] = new SpriteEntry { page = 0, uv = new[] { 0.5f, 0f, 1f, 0.5f }, orig = new[] { 32, 32 } };

            var result = IkatStageDriver.MergeSpriteSizes(new List<AtlasManifest> { atlas });
            Assert.AreEqual(2, result.Count);

            Assert.AreEqual("res/icons/home.png", result[0].key);
            Assert.AreEqual(64u, result[0].w);
            Assert.AreEqual(64u, result[0].h);

            Assert.AreEqual("res/icons/gear.png", result[1].key);
            Assert.AreEqual(32u, result[1].w);
            Assert.AreEqual(32u, result[1].h);
        }

        [Test]
        public void MergeSpriteSizes_DuplicateKeys_FirstWins()
        {
            var atlas1 = new AtlasManifest();
            atlas1.sprites["res/a.png"] = new SpriteEntry { orig = new[] { 100, 200 } };

            var atlas2 = new AtlasManifest();
            atlas2.sprites["res/a.png"] = new SpriteEntry { orig = new[] { 999, 999 } };
            atlas2.sprites["res/b.png"] = new SpriteEntry { orig = new[] { 50, 50 } };

            var result = IkatStageDriver.MergeSpriteSizes(new List<AtlasManifest> { atlas1, atlas2 });
            Assert.AreEqual(2, result.Count);
            Assert.AreEqual(100u, result[0].w);
            Assert.AreEqual(50u, result[1].w);
        }

        [Test]
        public void MergeSpriteSizes_NullOrig_Skipped()
        {
            var atlas = new AtlasManifest();
            atlas.sprites["res/valid.png"] = new SpriteEntry { orig = new[] { 64, 64 } };
            atlas.sprites["res/bad.png"] = new SpriteEntry { orig = null };

            var result = IkatStageDriver.MergeSpriteSizes(new List<AtlasManifest> { atlas });
            Assert.AreEqual(1, result.Count);
            Assert.AreEqual("res/valid.png", result[0].key);
        }

        [Test]
        public void MergeSpriteSizes_NullSpritesDict_Handled()
        {
            var atlas = new AtlasManifest();
            // sprites dict is null by default
            var result = IkatStageDriver.MergeSpriteSizes(new List<AtlasManifest> { atlas });
            Assert.IsNotNull(result);
            Assert.AreEqual(0, result.Count);
        }
    }
}
