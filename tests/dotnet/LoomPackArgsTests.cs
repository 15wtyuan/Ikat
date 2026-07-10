using System.Collections.Generic;
using System.Text;
using LoomGUI.Editor;
using Xunit;

namespace LoomGUI.Tests
{
    public class LoomPackArgsTests
    {
        // 命令行参数转义：文件名/包名含空格必须加引号，否则 Windows 解析会断参、exe 调崩。
        string BuildWith(string src, string pkg, List<string> html, string res, string outPath) =>
            LoomPackArgs.Build(src, pkg, html, res, outPath);

        [Fact]
        public void HtmlArgWithSpaceIsQuoted()
        {
            var s = BuildWith("C:/src", "pkg", new List<string> { "a b.html", "c.html" }, "C:/res", "C:/o/x.pkg.bin");
            Assert.Contains("--html \"a b.html,c.html\"", s);
        }

        [Fact]
        public void EmptyHtmlListOmitsFlag()
        {
            var s = BuildWith("C:/src", "pkg", new List<string>(), "C:/res", "C:/o/x.pkg.bin");
            Assert.DoesNotContain("--html", s);
        }

        [Fact]
        public void PkgNameWithSpaceIsQuoted()
        {
            var s = BuildWith("C:/src", "my pkg", new List<string>(), "C:/res", "C:/o/x.pkg.bin");
            Assert.Contains("\"my pkg\"", s);
        }

        [Fact]
        public void SourceResRootOutPathQuoted()
        {
            var s = BuildWith("C:/my src", "pkg", new List<string>(), "C:/my res", "C:/my out/x.pkg.bin");
            Assert.Contains("\"C:/my src\"", s);
            Assert.Contains("--res-root \"C:/my res\"", s);
            Assert.Contains("-o \"C:/my out/x.pkg.bin\"", s);
        }

        [Fact]
        public void InternalDoubleQuoteEscaped()
        {
            // 文件名含 " 时须转义为 \"，否则引号配对错乱。
            var s = BuildWith("C:/src", "a\"b", new List<string>(), "C:/res", "C:/o/x.pkg.bin");
            Assert.Contains("\"a\\\"b\"", s);
        }
    }
}
