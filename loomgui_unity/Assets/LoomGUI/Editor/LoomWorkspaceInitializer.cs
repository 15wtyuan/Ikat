using System.IO;
using System.Text;
using UnityEditor;
using UnityEngine;

namespace LoomGUI.Editor
{
    /// <summary>
    /// 工作区初始化（迁自 editor/init.mjs）：注围栏规则 + 分发 skill + 写 config.json。
    /// 围栏规则/skill 内容从插件 Editor Resources 读出注入工作区。
    /// </summary>
    public static class LoomWorkspaceInitializer
    {
        const string BEGIN = "<!-- loomgui-editor-begin -->";
        const string END = "<!-- loomgui-editor-end -->";

        public static void Initialize(LoomSettings s)
        {
            if (s == null || string.IsNullOrEmpty(s.workspaceDir)) return;
            string projRoot = Directory.GetParent(Application.dataPath).FullName;
            string ws = Path.GetFullPath(Path.Combine(projRoot, s.workspaceDir));
            Directory.CreateDirectory(ws);

            InjectFenceRules(ws);
            DistributeSkill(ws);
            LoomConfigExporter.Export(s);  // Task 7
            AssetDatabase.Refresh();
        }

        /// 注围栏规则到工作区 CLAUDE.md（标签段增量合并）。
        static void InjectFenceRules(string ws)
        {
            var tmpl = AssetDatabase.LoadAssetAtPath<TextAsset>(
                "Assets/LoomGUI/Editor/Resources/LoomGUI/fence-rules.md");
            if (tmpl == null) { Debug.LogError("[LoomGUI] fence-rules.md not found in Editor Resources"); return; }
            string content = tmpl.text;
            string tagged = content.Contains(BEGIN) ? content : $"{BEGIN}\n{content}\n{END}\n";
            string target = Path.Combine(ws, "CLAUDE.md");
            if (!File.Exists(target)) { File.WriteAllText(target, tagged, Encoding.UTF8); return; }
            string existing = File.ReadAllText(target);
            if (!existing.Contains(BEGIN))
            {
                File.WriteAllText(target, existing.TrimEnd('\n') + "\n\n" + tagged, Encoding.UTF8);
                return;
            }
            string pattern = System.Text.RegularExpressions.Regex.Escape(BEGIN) +
                @"[\s\S]*?" + System.Text.RegularExpressions.Regex.Escape(END);
            string updated = System.Text.RegularExpressions.Regex.Replace(
                existing, pattern, tagged.TrimEnd());
            File.WriteAllText(target, updated, Encoding.UTF8);
        }

        /// 分发 skill 到工作区 .claude/skills/loomgui-editor/。
        static void DistributeSkill(string ws)
        {
            string dest = Path.Combine(ws, ".claude/skills/loomgui-editor");
            Directory.CreateDirectory(dest);
            string basePath = "Assets/LoomGUI/Editor/Resources/LoomGUI/skill";
            CopyResource(Path.Combine(basePath, "SKILL.md"), Path.Combine(dest, "SKILL.md"));
            string refs = Path.Combine(dest, "references");
            Directory.CreateDirectory(refs);
            CopyResource(Path.Combine(basePath, "references/fence.md"), Path.Combine(refs, "fence.md"));
            CopyResource(Path.Combine(basePath, "references/preview-polyfill.html"), Path.Combine(refs, "preview-polyfill.html"));
            CopyResource(Path.Combine(basePath, "references/preview-trust.md"), Path.Combine(refs, "preview-trust.md"));
        }

        static void CopyResource(string assetPath, string destFile)
        {
            var ta = AssetDatabase.LoadAssetAtPath<TextAsset>(assetPath);
            if (ta == null) { Debug.LogWarning($"[LoomGUI] resource not found: {assetPath}"); return; }
            File.WriteAllText(destFile, ta.text, Encoding.UTF8);
        }
    }
}
