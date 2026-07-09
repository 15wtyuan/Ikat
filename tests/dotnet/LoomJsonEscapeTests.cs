using LoomGUI.Editor;
using Xunit;

namespace LoomGUI.Tests
{
    public class LoomJsonEscapeTests
    {
        [Fact]
        public void EscapesDoubleQuote()
        {
            // "a"b" → a\"b
            Assert.Equal("a\\\"b", LoomJsonEscape.Escape("a\"b"));
        }

        [Fact]
        public void EscapesBackslash()
        {
            // a\b → a\\b
            Assert.Equal("a\\\\b", LoomJsonEscape.Escape("a\\b"));
        }

        [Fact]
        public void EscapesNewlineAndTab()
        {
            Assert.Equal("a\\nb\\tc", LoomJsonEscape.Escape("a\nb\tc"));
        }

        [Fact]
        public void LeavesPlainPathUnchanged()
        {
            // 正斜杠路径无需转义（JSON 不要求转 /）。
            Assert.Equal("Assets/LoomUI/home.html", LoomJsonEscape.Escape("Assets/LoomUI/home.html"));
        }
    }
}
