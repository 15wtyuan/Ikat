using NUnit.Framework;
using System.Collections.Generic;
using Ikat;

namespace Ikat.Tests
{
    /// <summary>
    /// EditMode tests for RuntimeManifest + AtlasManifest JSON parsing.
    /// Feeds known JSON strings (matching what the Rust packer produces) and asserts
    /// the parsed models match.
    ///
    /// Run in Unity Test Runner (EditMode). These tests do NOT require PlayMode.
    /// </summary>
    public class IkatManifestsTests
    {
        [Test]
        public void ParseRuntime_Basic()
        {
            var json = @"
{
    ""version"": 1,
    ""packages"": [""showcase""],
    ""atlases"": [""ui""],
    ""fonts"": [
        {
            ""family"": ""TestFont"",
            ""file"": ""fonts/TestFont.ttf.bytes"",
            ""default"": true,
            ""fallback"": false
        }
    ]
}";
            var m = RuntimeManifest.ParseRuntime(json);

            Assert.AreEqual(1, m.version, "version");
            Assert.AreEqual(1, m.packages.Count, "packages count");
            Assert.AreEqual("showcase", m.packages[0], "packages[0]");
            Assert.AreEqual(1, m.atlases.Count, "atlases count");
            Assert.AreEqual("ui", m.atlases[0], "atlases[0]");
            Assert.AreEqual(1, m.fonts.Count, "fonts count");

            var f = m.fonts[0];
            Assert.AreEqual("TestFont", f.family, "font family");
            Assert.AreEqual("fonts/TestFont.ttf.bytes", f.file, "font file");
            Assert.IsTrue(f.@default, "font default");
            Assert.IsFalse(f.fallback, "font fallback");
        }

        [Test]
        public void ParseRuntime_MultiplePackagesAndAtlases()
        {
            var json = @"
{
    ""version"": 2,
    ""packages"": [""pkg_a"", ""pkg_b""],
    ""atlases"": [""atlas_ui"", ""atlas_fx""],
    ""fonts"": []
}";
            var m = RuntimeManifest.ParseRuntime(json);

            Assert.AreEqual(2, m.version);
            Assert.AreEqual(2, m.packages.Count);
            Assert.AreEqual("pkg_a", m.packages[0]);
            Assert.AreEqual("pkg_b", m.packages[1]);
            Assert.AreEqual(2, m.atlases.Count);
            Assert.AreEqual("atlas_ui", m.atlases[0]);
            Assert.AreEqual("atlas_fx", m.atlases[1]);
            Assert.AreEqual(0, m.fonts.Count);
        }

        [Test]
        public void ParseRuntime_MultipleFonts()
        {
            var json = @"
{
    ""version"": 1,
    ""packages"": [],
    ""atlases"": [],
    ""fonts"": [
        { ""family"": ""Primary"", ""file"": ""p.ttf.bytes"", ""default"": true, ""fallback"": false },
        { ""family"": ""Fallback"", ""file"": ""f.ttf.bytes"", ""default"": false, ""fallback"": true },
        { ""family"": ""Icon"",     ""file"": ""i.ttf.bytes"", ""default"": false, ""fallback"": false }
    ]
}";
            var m = RuntimeManifest.ParseRuntime(json);

            Assert.AreEqual(3, m.fonts.Count);
            Assert.AreEqual("Primary", m.fonts[0].family);
            Assert.IsTrue(m.fonts[0].@default);
            Assert.IsFalse(m.fonts[0].fallback);

            Assert.AreEqual("Fallback", m.fonts[1].family);
            Assert.IsFalse(m.fonts[1].@default);
            Assert.IsTrue(m.fonts[1].fallback);

            Assert.AreEqual("Icon", m.fonts[2].family);
            Assert.IsFalse(m.fonts[2].@default);
            Assert.IsFalse(m.fonts[2].fallback);
        }

        [Test]
        public void ParseRuntime_EmptyManifest()
        {
            var json = @"{""version"":0,""packages"":[],""atlases"":[],""fonts"":[]}";
            var m = RuntimeManifest.ParseRuntime(json);

            Assert.AreEqual(0, m.version);
            Assert.AreEqual(0, m.packages.Count);
            Assert.AreEqual(0, m.atlases.Count);
            Assert.AreEqual(0, m.fonts.Count);
        }

        [Test]
        public void ParseAtlas_SingleSprite()
        {
            var json = @"
{
    ""pages"": [""ui.png""],
    ""sprites"": {
        ""assets/icons/home.png"": {
            ""page"": 0,
            ""uv"": [0.012, 0.048, 0.137, 0.170],
            ""orig"": [64, 64]
        }
    }
}";
            var m = AtlasManifest.ParseAtlas(json);

            Assert.AreEqual(1, m.pages.Count, "pages count");
            Assert.AreEqual("ui.png", m.pages[0], "pages[0]");
            Assert.AreEqual(1, m.sprites.Count, "sprites count");

            Assert.IsTrue(m.sprites.ContainsKey("assets/icons/home.png"), "contains key");
            var e = m.sprites["assets/icons/home.png"];
            Assert.AreEqual(0, e.page, "page");
            Assert.AreEqual(4, e.uv.Length, "uv length");
            Assert.AreEqual(0.012f, e.uv[0], 0.001f, "uv[0]");
            Assert.AreEqual(0.048f, e.uv[1], 0.001f, "uv[1]");
            Assert.AreEqual(0.137f, e.uv[2], 0.001f, "uv[2]");
            Assert.AreEqual(0.170f, e.uv[3], 0.001f, "uv[3]");
            Assert.AreEqual(2, e.orig.Length, "orig length");
            Assert.AreEqual(64, e.orig[0], "orig w");
            Assert.AreEqual(64, e.orig[1], "orig h");
        }

        [Test]
        public void ParseAtlas_MultipleSprites()
        {
            var json = @"
{
    ""pages"": [""ui.png"", ""ui.1.png""],
    ""sprites"": {
        ""a.png"": { ""page"": 0, ""uv"": [0.0, 0.0, 0.5, 0.5], ""orig"": [128, 128] },
        ""b.png"": { ""page"": 0, ""uv"": [0.5, 0.0, 1.0, 0.5], ""orig"": [128, 128] },
        ""c.png"": { ""page"": 1, ""uv"": [0.0, 0.0, 1.0, 1.0], ""orig"": [256, 256] }
    }
}";
            var m = AtlasManifest.ParseAtlas(json);

            Assert.AreEqual(2, m.pages.Count);
            Assert.AreEqual("ui.png", m.pages[0]);
            Assert.AreEqual("ui.1.png", m.pages[1]);
            Assert.AreEqual(3, m.sprites.Count);

            var a = m.sprites["a.png"];
            Assert.AreEqual(0, a.page);
            Assert.AreEqual(128, a.orig[0]);

            var b = m.sprites["b.png"];
            Assert.AreEqual(0, b.page);
            Assert.AreEqual(0.5f, b.uv[0], 0.001f);

            var c = m.sprites["c.png"];
            Assert.AreEqual(1, c.page);
            Assert.AreEqual(256, c.orig[1]);
        }

        [Test]
        public void ParseAtlas_EmptySprites()
        {
            var json = @"{""pages"":[""ui.png""],""sprites"":{}}";
            var m = AtlasManifest.ParseAtlas(json);

            Assert.AreEqual(1, m.pages.Count);
            Assert.AreEqual(0, m.sprites.Count);
        }

        [Test]
        public void ParseAtlas_SpritesKeyWithNestedPath()
        {
            var json = @"
{
    ""pages"": [""ui.png""],
    ""sprites"": {
        ""assets/ui/panel/header_bg.png"": {
            ""page"": 0,
            ""uv"": [0.1, 0.2, 0.3, 0.4],
            ""orig"": [100, 50]
        }
    }
}";
            var m = AtlasManifest.ParseAtlas(json);

            Assert.IsTrue(m.sprites.ContainsKey("assets/ui/panel/header_bg.png"),
                "nested path sprite key");
            var e = m.sprites["assets/ui/panel/header_bg.png"];
            Assert.AreEqual(0, e.page);
            Assert.AreEqual(100, e.orig[0]);
            Assert.AreEqual(50, e.orig[1]);
        }

        [Test]
        public void ParseRuntime_EmptyJson_Throws()
        {
            Assert.Throws<System.FormatException>(() =>
                RuntimeManifest.ParseRuntime(""));
        }

        [Test]
        public void ParseRuntime_NullJson_Throws()
        {
            Assert.Throws<System.ArgumentNullException>(() =>
                RuntimeManifest.ParseRuntime(null));
        }

        [Test]
        public void ParseAtlas_MalformedJson_Throws()
        {
            Assert.Throws<System.FormatException>(() =>
                AtlasManifest.ParseAtlas("{bad json}"));
        }
    }
}
