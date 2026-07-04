using System.IO;
using System.Text;
using UnityEditor;
using UnityEngine;

namespace LoomGUI.Editor
{
    /// <summary>
    /// 工作区初始化（迁自 editor/init.mjs）：注围栏规则 + 分发 skill + 写 config.json。
    /// 围栏规则/skill 内容用 File.ReadAllText 直读磁盘（不走 AssetDatabase）——
    /// Unity 把 .md/.html 当 DefaultAsset 不是 TextAsset，LoadAssetAtPath&lt;TextAsset&gt; 返 null
    /// （这是之前「初始化按钮静默失败」的根因）。
    /// </summary>
    public static class LoomWorkspaceInitializer
    {
        const string BEGIN = "<!-- loomgui-editor-begin -->";
        const string END = "<!-- loomgui-editor-end -->";

        public struct Result { public bool ok; public string msg; }

        public static Result Initialize(LoomSettings s)
        {
            if (s == null) return new Result { ok = false, msg = "settings 为 null" };
            if (string.IsNullOrEmpty(s.workspaceDir))
                return new Result { ok = false, msg = "workspaceDir 为空（工作区 tab 没填）" };

            string projRoot = Directory.GetParent(Application.dataPath).FullName;
            string ws = Path.GetFullPath(Path.Combine(projRoot, s.workspaceDir));
            Directory.CreateDirectory(ws);

            string err = InjectFenceRules(projRoot, ws);
            if (err != null) return new Result { ok = false, msg = err };
            err = DistributeSkill(projRoot, ws);
            if (err != null) return new Result { ok = false, msg = err };
            LoomConfigExporter.Export(s);
            AssetDatabase.Refresh();
            return new Result { ok = true, msg = $"写入 {ws}（CLAUDE.md + .claude/skills/ + config.json）" };
        }

        /// 注围栏规则到工作区 CLAUDE.md（标签段增量合并）。返 null=成功，非 null=失败原因。
        static string InjectFenceRules(string projRoot, string ws)
        {
            string tmplPath = Path.Combine(projRoot, "Packages/com.loomgui.unity/Editor/Resources/LoomGUI/fence-rules.md");
            if (!File.Exists(tmplPath)) return $"围栏规则模板不存在：{tmplPath}";
            string content = File.ReadAllText(tmplPath);
            string tagged = content.Contains(BEGIN) ? content : $"{BEGIN}\n{content}\n{END}\n";
            string target = Path.Combine(ws, "CLAUDE.md");
            if (!File.Exists(target)) { File.WriteAllText(target, tagged, Encoding.UTF8); return null; }
            string existing = File.ReadAllText(target);
            if (!existing.Contains(BEGIN))
            {
                File.WriteAllText(target, existing.TrimEnd('\n') + "\n\n" + tagged, Encoding.UTF8);
                return null;
            }
            string pattern = System.Text.RegularExpressions.Regex.Escape(BEGIN) +
                @"[\s\S]*?" + System.Text.RegularExpressions.Regex.Escape(END);
            string updated = System.Text.RegularExpressions.Regex.Replace(
                existing, pattern, tagged.TrimEnd());
            File.WriteAllText(target, updated, Encoding.UTF8);
            return null;
        }

        /// 分发 skill 到工作区 .claude/skills/loomgui-editor/。返 null=成功，非 null=失败原因。
        static string DistributeSkill(string projRoot, string ws)
        {
            string dest = Path.Combine(ws, ".claude/skills/loomgui-editor");
            Directory.CreateDirectory(dest);
            Directory.CreateDirectory(Path.Combine(dest, "references"));
            string basePath = Path.Combine(projRoot, "Packages/com.loomgui.unity/Editor/Resources/LoomGUI/skill");
            string[] rels = { "SKILL.md", "references/fence.md",
                              "references/preview-polyfill.html", "references/preview-trust.md" };
            var missing = new System.Collections.Generic.List<string>();
            foreach (var rel in rels)
            {
                string src = Path.Combine(basePath, rel);
                if (!File.Exists(src)) { missing.Add(rel); continue; }
                File.WriteAllText(Path.Combine(dest, rel), File.ReadAllText(src), Encoding.UTF8);
            }
            return missing.Count > 0 ? $"skill 资源缺失：{string.Join(", ", missing)}" : null;
        }
    }
}
