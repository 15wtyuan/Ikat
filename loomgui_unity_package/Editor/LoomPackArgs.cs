using System.Collections.Generic;
using System.Text;

namespace LoomGUI.Editor
{
    /// <summary>
    /// loomgui_pkg.exe 命令行拼接（纯逻辑，可单测，不依赖 UnityEngine）。
    /// 文件名/包名含空格或引号必须转义，否则 Windows 参数解析会断参、exe 调崩。
    /// Rust CLI 的 --html 值按逗号切分，故多文件逗号拼成一个引号包起来的 token。
    /// </summary>
    public static class LoomPackArgs
    {
        public static string Build(string absSrc, string pkgName, List<string> htmlFiles, string resRoot, string outPath)
        {
            var sb = new StringBuilder();
            sb.Append(Quote(absSrc)).Append(' ').Append(Quote(pkgName));
            if (htmlFiles.Count > 0)
                sb.Append(" --html ").Append(Quote(string.Join(",", htmlFiles)));
            sb.Append(" --res-root ").Append(Quote(resRoot));
            sb.Append(" -o ").Append(Quote(outPath));
            return sb.ToString();
        }

        /// Windows 引号转义：包在 "..." 内，内部 " 转 \"。
        static string Quote(string s) => "\"" + s.Replace("\"", "\\\"") + "\"";
    }
}
