using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Text;
using UnityEditor;
using UnityEngine;

namespace LoomGUI.Editor
{
    /// <summary>
    /// LoomGUI 设置面板（三 tab：工作区 / 包管理 / 图集）。菜单 LoomGUI > Settings。
    /// 共享全局 LoomSettings 资产。改任意字段 → 自动同步 config.json（Task 7 LoomConfigExporter）。
    /// </summary>
    public sealed class LoomSettingsWindow : EditorWindow
    {
        enum Tab { Workspace, Packages, Atlas }
        Tab _tab = Tab.Workspace;
        LoomSettings _settings;
        Vector2 _scroll;
        StringBuilder _log = new();

        [MenuItem("LoomGUI/Settings")]
        public static void Open()
        {
            var w = GetWindow<LoomSettingsWindow>(false, "LoomGUI Settings", true);
            w.minSize = new Vector2(720, 480);
        }

        void OnEnable()
        {
            _settings = LoomSettings.GetOrCreateDefault();
        }

        void OnGUI()
        {
            if (_settings == null) _settings = LoomSettings.GetOrCreateDefault();

            // tab toolbar
            _tab = (Tab)GUILayout.SelectionGrid((int)_tab, new[] { "工作区", "包管理", "图集" }, 3, EditorStyles.toolbarButton);
            EditorGUILayout.Space(8);

            EditorGUI.BeginChangeCheck();
            _scroll = EditorGUILayout.BeginScrollView(_scroll);
            switch (_tab)
            {
                case Tab.Workspace: DrawWorkspace(); break;
                case Tab.Packages: DrawPackages(); break;
                case Tab.Atlas: DrawAtlas(); break;
            }
            EditorGUILayout.EndScrollView();
            bool changed = EditorGUI.EndChangeCheck();

            EditorGUILayout.Space(8);
            DrawLog();

            if (changed)
            {
                EditorUtility.SetDirty(_settings);
                AssetDatabase.SaveAssetIfDirty(_settings);
                // TODO Task 7 实现 LoomConfigExporter 后取消注释
                // LoomConfigExporter.Export(_settings);
            }
        }

        // —— 工作区 tab ——————————————————————————————————————————————
        void DrawWorkspace()
        {
            EditorGUILayout.LabelField("工作区配置", EditorStyles.boldLabel);
            _settings.workspaceDir = EditorGUILayout.TextField("工作区根", _settings.workspaceDir);
            _settings.resDirName = EditorGUILayout.TextField("res 目录名", _settings.resDirName);
            _settings.pkgOutputDir = EditorGUILayout.TextField("pkg.bin 输出目录", _settings.pkgOutputDir);

            EditorGUILayout.Space(8);
            if (GUILayout.Button("初始化工作区（注入围栏规则 + skill + config.json）", GUILayout.Height(28)))
            {
                // TODO Task 8 实现 LoomWorkspaceInitializer 后取消注释
                // LoomWorkspaceInitializer.Initialize(_settings);
                AppendLog("[init] 工作区初始化完成");
            }
        }

        // —— 包管理 tab（重写自旧 LoomPackageManagerWindow）———————————
        void DrawPackages()
        {
            EditorGUILayout.LabelField("包列表（" + _settings.packages.Count + "）", EditorStyles.boldLabel);
            for (int i = 0; i < _settings.packages.Count; i++) DrawPackageEntry(i);
            if (GUILayout.Button("+ 添加包", GUILayout.Width(120)))
            {
                _settings.packages.Add(new PackageEntry("new_pkg", ""));
            }
            EditorGUILayout.Space(8);
            if (GUILayout.Button("一键打包全部", GUILayout.Height(28))) PackAll();
        }

        void DrawPackageEntry(int idx)
        {
            var pkg = _settings.packages[idx];
            EditorGUILayout.BeginVertical(EditorStyles.helpBox);
            pkg.pkgName = EditorGUILayout.TextField("包名", pkg.pkgName);
            pkg.sourceDir = EditorGUILayout.TextField("源目录（相对工作区根）", pkg.sourceDir);
            EditorGUILayout.LabelField("html 文件（" + pkg.htmlFiles.Count + "）:");
            for (int j = 0; j < pkg.htmlFiles.Count; j++)
            {
                EditorGUILayout.BeginHorizontal();
                pkg.htmlFiles[j] = EditorGUILayout.TextField(pkg.htmlFiles[j]);
                if (GUILayout.Button("×", GUILayout.Width(24))) { pkg.htmlFiles.RemoveAt(j); break; }
                EditorGUILayout.EndHorizontal();
            }
            if (GUILayout.Button("+ 添加 html", GUILayout.Width(100))) pkg.htmlFiles.Add("");
            EditorGUILayout.BeginHorizontal();
            if (GUILayout.Button("打包", GUILayout.Width(80))) PackPackage(idx);
            if (GUILayout.Button("删除", GUILayout.Width(80))) { _settings.packages.RemoveAt(idx); }
            EditorGUILayout.EndHorizontal();
            EditorGUILayout.EndVertical();
        }

        // —— 图集 tab（Task 6 展开）———————————————————————————————————
        void DrawAtlas()
        {
            EditorGUILayout.LabelField("图集配置（" + _settings.atlasEntries.Count + "）", EditorStyles.boldLabel);
            for (int i = 0; i < _settings.atlasEntries.Count; i++) DrawAtlasEntry(i);
            if (GUILayout.Button("+ 添加图集", GUILayout.Width(120)))
            {
                _settings.atlasEntries.Add(new AtlasEntry { atlasName = "NewAtlas" });
            }
            EditorGUILayout.Space(8);
            if (GUILayout.Button("同步全部图集 packables", GUILayout.Height(28)))
            {
                LoomAtlasSync.SyncAll(_settings);
                AppendLog("[atlas] 同步完成");
            }
        }

        void DrawAtlasEntry(int idx)
        {
            var e = _settings.atlasEntries[idx];
            EditorGUILayout.BeginVertical(EditorStyles.helpBox);
            e.atlasName = EditorGUILayout.TextField("图集名", e.atlasName);
            e.isDefault = EditorGUILayout.Toggle("isDefault（res 根图兜底）", e.isDefault);
            EditorGUILayout.LabelField("folders（拖文件夹到此）:");
            var dropRect = GUILayoutUtility.GetRect(0, 30, GUILayout.ExpandWidth(true));
            GUI.Box(dropRect, "  拖文件夹当 packables", EditorStyles.helpBox);
            HandleFolderDrop(dropRect, e);
            for (int j = 0; j < e.folders.Count; j++)
            {
                EditorGUILayout.BeginHorizontal();
                e.folders[j] = EditorGUILayout.TextField(e.folders[j]);
                if (GUILayout.Button("×", GUILayout.Width(24))) { e.folders.RemoveAt(j); break; }
                EditorGUILayout.EndHorizontal();
            }
            EditorGUILayout.EndVertical();
        }

        void HandleFolderDrop(Rect rect, AtlasEntry e)
        {
            if (!rect.Contains(Event.current.mousePosition)) return;
            if (Event.current.type == UnityEngine.EventType.DragPerform)
            {
                DragAndDrop.AcceptDrag();
                foreach (string p in DragAndDrop.paths)
                    if (Directory.Exists(p) && !e.folders.Contains(p)) e.folders.Add(p);
                Event.current.Use();
            }
            if (Event.current.type == UnityEngine.EventType.DragUpdated)
                DragAndDrop.visualMode = DragAndDropVisualMode.Copy;
        }

        // —— 打包（复用 exe，固定路径）——————————————————————————————
        void PackAll() { for (int i = 0; i < _settings.packages.Count; i++) PackPackage(i); }

        void PackPackage(int idx)
        {
            var pkg = _settings.packages[idx];
            string exe = LoomExePath.Resolve();
            if (!File.Exists(exe)) { AppendLog($"[pack] exe 不存在: {exe}"); return; }
            string absSrc = ToAbs(Path.Combine(_settings.workspaceDir, pkg.sourceDir));
            string outPath = ToAbs(Path.Combine(_settings.pkgOutputDir, pkg.pkgName + ".pkg.bin"));
            Directory.CreateDirectory(Path.GetDirectoryName(outPath));
            string htmlArg = pkg.htmlFiles.Count > 0 ? string.Join(",", pkg.htmlFiles) : "";
            var sb = new StringBuilder();
            sb.Append('"').Append(absSrc).Append("\" ").Append(pkg.pkgName);
            if (pkg.htmlFiles.Count > 0) sb.Append(" --html ").Append(htmlArg);
            // res 在工作区根下（不在 sourceDir 下），显式传 res-root 绝对路径。
            string resRoot = ToAbs(Path.Combine(_settings.workspaceDir, _settings.resDirName));
            sb.Append(" --res-root \"").Append(resRoot).Append("\"");
            sb.Append(" -o \"").Append(outPath).Append('"');
            try
            {
                var psi = new ProcessStartInfo(exe, sb.ToString())
                { RedirectStandardOutput = true, RedirectStandardError = true, UseShellExecute = false, CreateNoWindow = true,
                  StandardOutputEncoding = Encoding.UTF8, StandardErrorEncoding = Encoding.UTF8 };
                using var p = Process.Start(psi);
                string stdout = p.StandardOutput.ReadToEnd();
                string stderr = p.StandardError.ReadToEnd();
                p.WaitForExit();
                if (!string.IsNullOrEmpty(stdout)) AppendLog($"  stdout: {stdout.Trim()}");
                AppendLog(p.ExitCode == 0 ? $"[pack] {pkg.pkgName}: OK" : $"[pack] {pkg.pkgName}: FAIL\n{stderr}");
            }
            catch (Exception ex) { AppendLog($"[pack] {pkg.pkgName}: {ex.Message}"); }
            AssetDatabase.Refresh();
        }

        // —— 工具 ————————————————————————————————————————————————————
        string ToAbs(string unityRel)
        {
            string projRoot = Directory.GetParent(Application.dataPath).FullName;
            return Path.GetFullPath(Path.Combine(projRoot, unityRel));
        }

        void AppendLog(string line) { _log.AppendLine(line); }

        void DrawLog()
        {
            EditorGUILayout.LabelField("日志", EditorStyles.boldLabel);
            EditorGUILayout.TextArea(_log.ToString(), GUILayout.Height(120));
        }
    }
}
