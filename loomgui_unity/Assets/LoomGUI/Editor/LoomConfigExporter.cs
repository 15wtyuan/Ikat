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
            // exe_path：工作区根 → Assets/LoomGUI/Editor/Tools/。两者都在 Assets/ 下。
            // workspaceDir = Assets/LoomUI/ → 深度 2（Assets/, LoomUI/），回到 Assets/ 需 ../，再进 LoomGUI/...
            string exeRel = RelativeFromWorkspace(s.workspaceDir, "Assets/LoomGUI/Editor/Tools/loomgui_pkg.exe");
            string outRel = RelativeFromWorkspace(s.workspaceDir, s.pkgOutputDir);

            var sb = new StringBuilder();
            sb.Append("{\n");
            sb.Append($"  \"exe_path\": \"{exeRel}\",\n");
            sb.Append($"  \"res_dir\": \"{s.resDirName}\",\n");
            sb.Append($"  \"output_dir\": \"{outRel}\",\n");
            sb.Append("  \"packages\": [");
            for (int i = 0; i < s.packages.Count; i++)
            {
                var p = s.packages[i];
                if (i > 0) sb.Append(",");
                sb.Append("\n    {");
                sb.Append($"\"name\": \"{p.pkgName}\", ");
                sb.Append($"\"source\": \"{p.sourceDir}\", ");
                sb.Append("\"html\": [");
                for (int j = 0; j < p.htmlFiles.Count; j++)
                {
                    if (j > 0) sb.Append(", ");
                    sb.Append($"\"{p.htmlFiles[j]}\"");
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
            // 简化：工作区根 = Assets/LoomUI/（深度2）。targetDir 在 Assets/ 下。
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
