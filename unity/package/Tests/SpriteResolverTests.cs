using System.Collections.Generic;
using NUnit.Framework;
using UnityEngine;

namespace Ikat.Tests
{
    /// <summary>
    /// SpriteResolver test for self-drawn atlas (atlas.png + atlas.json) lookup.
    /// Pure logic EditMode tests: construct AtlasManifests + a fake loadPage → Init → GetSprite asserts.
    /// No Unity SpriteAtlas/Sprite dependency.
    /// </summary>
    public class SpriteResolverTests
    {
        /// <summary>
        /// Construct two AtlasManifests with known sprite entries → Init → GetSprite verifies
        /// found flag, uvRect, origW, origH. Also verifies that loadPage is called lazily and
        /// cached (second GetSprite for same atlas page does not re-invoke loadPage).
        /// </summary>
        [Test]
        public void Init_WithTwoAtlases_GetSpriteReturnsCorrectLookup()
        {
            var resolver = new SpriteResolver();

            // Atlas 0: one page "ui.png", one sprite
            var atlas0 = new AtlasManifest();
            atlas0.pages.Add("ui.png");
            atlas0.sprites["icons/home"] = new SpriteEntry
            {
                page = 0,
                uv   = new float[] { 0.1f, 0.2f, 0.5f, 0.6f },
                orig = new int[] { 128, 128 }
            };

            // Atlas 1: one page "icons.png", two sprites
            var atlas1 = new AtlasManifest();
            atlas1.pages.Add("icons.png");
            atlas1.sprites["icons/star"] = new SpriteEntry
            {
                page = 0,
                uv   = new float[] { 0.0f, 0.0f, 0.25f, 0.25f },
                orig = new int[] { 64, 64 }
            };
            atlas1.sprites["icons/gear"] = new SpriteEntry
            {
                page = 0,
                uv   = new float[] { 0.5f, 0.5f, 1.0f, 1.0f },
                orig = new int[] { 32, 48 }
            };

            int loadCount = 0;
            string loadedFile = null;
            var fakeTex = new Texture2D(512, 512);

            resolver.Init(
                new List<AtlasManifest> { atlas0, atlas1 },
                name => { loadCount++; loadedFile = name; return fakeTex; });

            // First lookup: triggers lazy load
            var r0 = resolver.GetSprite("icons/home");
            Assert.IsTrue(r0.found, "icons/home should be found");
            Assert.AreEqual(fakeTex, r0.tex, "texture should be the fake page tex");
            Assert.AreEqual(0.1f, r0.uvRect.x, "u0");
            Assert.AreEqual(0.2f, r0.uvRect.y, "v0");
            Assert.AreEqual(0.4f, r0.uvRect.width, "u1-u0");
            Assert.AreEqual(0.4f, r0.uvRect.height, "v1-v0");
            Assert.AreEqual(128, r0.origW);
            Assert.AreEqual(128, r0.origH);
            Assert.AreEqual(1, loadCount, "first lookup triggers lazy load");
            Assert.AreEqual("ui.png", loadedFile);

            // Second lookup in same atlas: page cached, loadPage not re-invoked
            loadedFile = null;
            var r1 = resolver.GetSprite("icons/home");
            Assert.IsTrue(r1.found);
            Assert.AreEqual(0, loadCount - 1, "same page should hit cache, not re-load");

            // Different atlas
            var r2 = resolver.GetSprite("icons/star");
            Assert.IsTrue(r2.found);
            Assert.AreEqual(fakeTex, r2.tex);
            Assert.AreEqual(0.0f, r2.uvRect.x);
            Assert.AreEqual(0.0f, r2.uvRect.y);
            Assert.AreEqual(0.25f, r2.uvRect.width);
            Assert.AreEqual(0.25f, r2.uvRect.height);
            Assert.AreEqual(64, r2.origW);
            Assert.AreEqual(64, r2.origH);

            // Same atlas (atlas 1), different sprite — page already loaded, no new load call
            int countBefore = loadCount;
            var r3 = resolver.GetSprite("icons/gear");
            Assert.IsTrue(r3.found);
            Assert.AreEqual(0.5f, r3.uvRect.x);
            Assert.AreEqual(0.5f, r3.uvRect.y);
            Assert.AreEqual(0.5f, r3.uvRect.width);
            Assert.AreEqual(0.5f, r3.uvRect.height);
            Assert.AreEqual(32, r3.origW);
            Assert.AreEqual(48, r3.origH);
            Assert.AreEqual(countBefore, loadCount, "same atlas page hit cache");

            Object.DestroyImmediate(fakeTex);
        }

        [Test]
        public void GetSprite_MissingKey_ReturnsNotFound()
        {
            var resolver = new SpriteResolver();
            var atlas = new AtlasManifest();
            atlas.pages.Add("ui.png");
            atlas.sprites["icons/home"] = new SpriteEntry
            {
                page = 0,
                uv   = new float[] { 0, 0, 1, 1 },
                orig = new int[] { 64, 64 }
            };
            resolver.Init(new List<AtlasManifest> { atlas }, _ => Texture2D.whiteTexture);

            var r = resolver.GetSprite("icons/missing");
            Assert.IsFalse(r.found, "missing key should return found=false");
        }

        [Test]
        public void GetSprite_NullOrEmptyKey_ReturnsNotFound()
        {
            var resolver = new SpriteResolver();
            resolver.Init(null, null);

            var r0 = resolver.GetSprite(null);
            Assert.IsFalse(r0.found, "null key → found=false");

            var r1 = resolver.GetSprite("");
            Assert.IsFalse(r1.found, "empty key → found=false");
        }

        [Test]
        public void Init_NullAtlases_DoesNotCrash()
        {
            var resolver = new SpriteResolver();
            resolver.Init(null, null);
            // no exception = pass
            var r = resolver.GetSprite("anything");
            Assert.IsFalse(r.found, "no atlases → all miss");
        }

        [Test]
        public void RegisterFontAtlasPage_GetSpriteHitsBeforeSpriteTable()
        {
            var resolver = new SpriteResolver();
            // Register a sprite table entry AND a font atlas page with the same key.
            var atlas = new AtlasManifest();
            atlas.pages.Add("ui.png");
            atlas.sprites["ikat://font-atlas/p0"] = new SpriteEntry
            {
                page = 0,
                uv   = new float[] { 0, 0, 1, 1 },
                orig = new int[] { 64, 64 }
            };

            var pageTex = new Texture2D(512, 512);
            int loadCount = 0;
            resolver.Init(new List<AtlasManifest> { atlas }, _ => { loadCount++; return pageTex; });

            // Register font atlas page with same key
            var fontTex = new Texture2D(256, 256);
            resolver.RegisterFontAtlasPage("ikat://font-atlas/p0", fontTex);

            // GetSprite should return the font atlas entry (priority), not the sprite table entry
            var r = resolver.GetSprite("ikat://font-atlas/p0");
            Assert.IsTrue(r.found, "font atlas page should be found");
            Assert.AreEqual(fontTex, r.tex, "font atlas tex takes priority over sprite table");
            Assert.AreEqual(0, r.uvRect.x, "font atlas is full-region");
            Assert.AreEqual(0, r.uvRect.y);
            Assert.AreEqual(1, r.uvRect.width);
            Assert.AreEqual(1, r.uvRect.height);
            Assert.AreEqual(256, r.origW);
            Assert.AreEqual(256, r.origH);

            Object.DestroyImmediate(pageTex);
            Object.DestroyImmediate(fontTex);
        }

        [Test]
        public void RegisterFontAtlasPage_ReRegisterSamePath_ReplacesEntry()
        {
            var resolver = new SpriteResolver();
            resolver.Init(null, null);

            var tex1 = new Texture2D(256, 256);
            var tex2 = new Texture2D(512, 512);
            resolver.RegisterFontAtlasPage("ikat://font-atlas/p0", tex1);
            resolver.RegisterFontAtlasPage("ikat://font-atlas/p0", tex2);

            var r = resolver.GetSprite("ikat://font-atlas/p0");
            Assert.IsTrue(r.found);
            Assert.AreEqual(tex2, r.tex, "re-register replaces old entry");
            Assert.AreEqual(512, r.origW);
            Assert.AreEqual(512, r.origH);

            Object.DestroyImmediate(tex1);
            Object.DestroyImmediate(tex2);
        }

        [Test]
        public void RegisterFontAtlasPage_NullTexture_DoesNotCrash()
        {
            var resolver = new SpriteResolver();
            resolver.Init(null, null);
            resolver.RegisterFontAtlasPage("ikat://font-atlas/p0", null);
            // no exception = pass
            var r = resolver.GetSprite("ikat://font-atlas/p0");
            Assert.IsFalse(r.found, "null tex not registered");
        }
    }
}
