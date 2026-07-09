using System.IO;
using System.Text;
using UnityEngine;

namespace LoomGUI.Editor
{
    /// <summary>
    /// LoomSettings → config.json 导出（AI 在 open-design 里读此调 exe 验证+打包）。
    /// 全相对工作区根（可移植）。LoomSettingsWindow 改配置自动调 Export。
    /// </summary>
    public static class LoomConfigExporter
    {
        /// 纯逻辑：构建 config.json 字符串（可单测，不碰磁盘）。
        public static string BuildJson(LoomSettings s)
        {
            // exe_path：工作区根 → Packages/com.loomgui.unity/Editor/Tools/（插件包内，Unity 虚拟化路径）。
            // workspaceDir = Assets/LoomUI/（深度 2）；exe 在 Packages/ 下，需先回项目根（../../）再进包。
            string exeRel = RelativeFromWorkspace(s.workspaceDir, "Packages/com.loomgui.unity/Editor/Tools/loomgui_pkg.exe");
            // Trailing slash so MakeRelativeUri treats the target as a directory; pkg.bin
            // artifacts now live under {pkgOutputDir}/ui/ (Bundles/ui/) — one publish root per
            // output kind: atlas/, ui/, fonts/ side-by-side under Bundles/.
            string outRel = RelativeFromWorkspace(s.workspaceDir, s.pkgOutputDir + "ui/");

            var sb = new StringBuilder();
            sb.Append("{\n");
            sb.Append($"  \"exe_path\": \"{LoomJsonEscape.Escape(exeRel)}\",\n");
            sb.Append($"  \"res_dir\": \"{LoomJsonEscape.Escape(s.resDirName)}\",\n");
            sb.Append($"  \"output_dir\": \"{LoomJsonEscape.Escape(outRel)}\",\n");
            sb.Append("  \"packages\": [");
            for (int i = 0; i < s.packages.Count; i++)
            {
                var p = s.packages[i];
                if (i > 0) sb.Append(",");
                sb.Append("\n    {");
                sb.Append($"\"name\": \"{LoomJsonEscape.Escape(p.pkgName)}\", ");
                sb.Append($"\"source\": \"{LoomJsonEscape.Escape(p.sourceDir)}\", ");
                sb.Append("\"html\": [");
                for (int j = 0; j < p.htmlFiles.Count; j++)
                {
                    if (j > 0) sb.Append(", ");
                    sb.Append($"\"{LoomJsonEscape.Escape(p.htmlFiles[j])}\"");
                }
                sb.Append("]}");
            }
            sb.Append(s.packages.Count > 0 ? "\n  ]\n" : "]\n");
            sb.Append("}\n");
            return sb.ToString();
        }

        /// 写 config.json 到工作区 .claude/skills/loomgui-editor/config.json。
        public static void Export(LoomSettings s)
        {
            if (s == null || string.IsNullOrEmpty(s.workspaceDir)) return;
            string projRoot = Directory.GetParent(Application.dataPath).FullName;
            string cfgPath = Path.GetFullPath(Path.Combine(projRoot, s.workspaceDir, ".claude/skills/loomgui-editor/config.json"));
            Directory.CreateDirectory(Path.GetDirectoryName(cfgPath));
            File.WriteAllText(cfgPath, BuildJson(s), Encoding.UTF8);
        }

        /// 算 from（工作区根）→ to 的相对路径。两者都是 Unity 工程相对（Assets/...）。
        static string RelativeFromWorkspace(string workspaceDir, string targetDir)
        {
            // 简化：工作区根 = Assets/LoomUI/（深度2）。targetDir 可能是 Assets/ 下或 Packages/ 下（插件包内）。
            // 用 Uri.MakeRelativeUri 算相对路径。
            string projRoot = Directory.GetParent(Application.dataPath).FullName.Replace('\\', '/');
            string from = Path.GetFullPath(Path.Combine(projRoot, workspaceDir)).Replace('\\', '/');
            // Path.GetFullPath 会剥 trailing slash。若原 targetDir 以 / 或 \ 结尾
            // 说明 targetDir 是目录，须补回 trailing slash 让 MakeRelativeUri 语义正确。
            bool toIsDir = targetDir.EndsWith("/") || targetDir.EndsWith("\\");
            string to = Path.GetFullPath(Path.Combine(projRoot, targetDir)).Replace('\\', '/');
            if (!from.EndsWith("/")) from += "/";
            if (toIsDir && !to.EndsWith("/")) to += "/";
            var uriFrom = new System.Uri(from);
            var uriTo = new System.Uri(to);
            return System.Uri.UnescapeDataString(uriFrom.MakeRelativeUri(uriTo).ToString());
        }
    }
}
