using System.Collections.Generic;
using System.Linq;
using LoomGUI.Editor;
using NUnit.Framework;

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
    }
}
