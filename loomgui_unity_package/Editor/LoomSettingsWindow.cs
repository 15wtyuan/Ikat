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
    /// LoomGUI 设置面板（四 tab：工作区 / 包管理 / 图集 / 字体）。菜单 LoomGUI > Settings。
    /// 共享全局 LoomSettings 资产。改任意字段 → 自动同步 config.json（LoomConfigExporter）。
    /// 底部「发布」= 一键 sync atlas + pack pkg + publish fonts + export config → Bundles/{atlas,ui,fonts}/。
    /// </summary>
    public sealed class LoomSettingsWindow : EditorWindow
    {
        enum Tab { Workspace, Packages, Atlas, Fonts }
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

        void OnEnable() { _settings = LoomSettings.GetOrCreateDefault(); }

        void OnGUI()
        {
            if (_settings == null) _settings = LoomSettings.GetOrCreateDefault();

            _tab = (Tab)GUILayout.SelectionGrid((int)_tab, new[] { "工作区", "包管理", "图集", "字体" }, 4, EditorStyles.toolbarButton);
            EditorGUILayout.Space(8);

            EditorGUI.BeginChangeCheck();
            _scroll = EditorGUILayout.BeginScrollView(_scroll);
            switch (_tab)
            {
                case Tab.Workspace: DrawWorkspace(); break;
                case Tab.Packages: DrawPackages(); break;
                case Tab.Atlas: DrawAtlas(); break;
                case Tab.Fonts: DrawFonts(); break;
            }
            EditorGUILayout.EndScrollView();
            bool changed = EditorGUI.EndChangeCheck();

            EditorGUILayout.Space(8);
            DrawLog();
            // Publish 出现于所有 tab：单步按钮留作分步调试，发布做一键产出全部到 Bundles/。
            DrawPublishButton();

            if (changed)
            {
                EditorUtility.SetDirty(_settings);
                AssetDatabase.SaveAssetIfDirty(_settings);
                LoomConfigExporter.Export(_settings);
            }
        }

        // —— 工作区 tab ——————————————————————————————————————————————
        void DrawWorkspace()
        {
            EditorGUILayout.LabelField("工作区配置（拖目录设置）", EditorStyles.boldLabel);
            // workspaceDir/pkgOutputDir：存 Unity 工程相对路径。
            _settings.workspaceDir = DirectoryDropField("工作区根", _settings.workspaceDir, null);
            // resDirName：存相对工作区根的路径（如 "res"）。须工作区根先设。
            _settings.resDirName = DirectoryDropField("res 目录（拖到工作区根下的资源目录）", _settings.resDirName, _settings.workspaceDir);
            _settings.pkgOutputDir = DirectoryDropField("pkg.bin 输出目录", _settings.pkgOutputDir, null);

            EditorGUILayout.Space(8);
            if (GUILayout.Button("初始化工作区（注入围栏规则 + skill + config.json）", GUILayout.Height(28)))
            {
                var r = LoomWorkspaceInitializer.Initialize(_settings);
                AppendLog(r.ok ? $"[init] OK — {r.msg}" : $"[init] 失败 — {r.msg}");
                if (!r.ok) UnityEngine.Debug.LogError($"[LoomGUI] {r.msg}");
            }
        }

        // —— 包管理 tab ——————————————————————————————————————————————
        void DrawPackages()
        {
            DrawPackageDropZone();
            EditorGUILayout.Space(4);
            EditorGUILayout.LabelField("包列表（" + _settings.packages.Count + "）", EditorStyles.boldLabel);
            for (int i = 0; i < _settings.packages.Count; i++) DrawPackageEntry(i);
            if (GUILayout.Button("+ 手动添加空包", GUILayout.Width(120)))
                _settings.packages.Add(new PackageEntry("new_pkg", ""));
            EditorGUILayout.Space(8);
            if (GUILayout.Button("一键打包全部", GUILayout.Height(28))) PackAll();
        }

        void DrawPackageDropZone()
        {
            Rect drop = GUILayoutUtility.GetRect(0, 48, GUILayout.ExpandWidth(true));
            GUI.Box(drop, "拖入目录智能建包\npkgName = 目录名，htmlFiles = 顶层 *.html（不递归，排除 res）\n（目录须在工作区根下）", EditorStyles.helpBox);
            if (!drop.Contains(Event.current.mousePosition)) return;
            if (Event.current.type == UnityEngine.EventType.DragUpdated)
            {
                bool hasDir = false;
                foreach (var p in DragAndDrop.paths) if (Directory.Exists(p)) { hasDir = true; break; }
                DragAndDrop.visualMode = hasDir ? DragAndDropVisualMode.Copy : DragAndDropVisualMode.Rejected;
            }
            if (Event.current.type == UnityEngine.EventType.DragPerform)
            {
                DragAndDrop.AcceptDrag();
                foreach (var p in DragAndDrop.paths)
                    if (Directory.Exists(p)) SmartRecognizeDir(p);
                Event.current.Use();
            }
        }

        /// 拖目录 → pkgName=目录名 + sourceDir（相对工作区根）+ 顶层 .html。
        void SmartRecognizeDir(string dropped)
        {
            string abs = ToAbsAny(dropped);
            if (abs == null || !Directory.Exists(abs)) { AppendLog($"[skip] 不是目录：{dropped}"); return; }
            string dirName = Path.GetFileName(abs.TrimEnd('/', '\\'));
            if (string.IsNullOrEmpty(dirName)) { AppendLog($"[skip] 目录名空：{dropped}"); return; }
            if (string.IsNullOrEmpty(_settings.workspaceDir)) { AppendLog("[skip] 先设工作区根"); return; }
            string wsAbs = ToAbs(_settings.workspaceDir);
            string sourceDir = Path.GetRelativePath(wsAbs, abs).Replace('\\', '/');
            if (sourceDir.StartsWith(".."))
            {
                AppendLog($"[skip] 目录不在工作区根下：{dropped}（工作区={_settings.workspaceDir}）");
                return;
            }
            var entry = new PackageEntry(dirName, sourceDir)
            {
                htmlFiles = ScanTopLevelHtml(abs)
            };
            _settings.packages.Add(entry);
            EditorUtility.SetDirty(_settings);
            AssetDatabase.SaveAssetIfDirty(_settings);
            LoomConfigExporter.Export(_settings);
            AppendLog($"[+] 建包：{entry.pkgName}（sourceDir={entry.sourceDir}，html ×{entry.htmlFiles.Count}）");
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
            EditorGUILayout.BeginHorizontal();
            if (GUILayout.Button("+ 添加 html", GUILayout.Width(100))) pkg.htmlFiles.Add("");
            if (GUILayout.Button("刷新（重扫 html）", GUILayout.Width(120))) RefreshPackage(idx);
            EditorGUILayout.EndHorizontal();
            EditorGUILayout.BeginHorizontal();
            if (GUILayout.Button("打包", GUILayout.Width(80))) PackPackage(idx);
            if (GUILayout.Button("删除", GUILayout.Width(80))) { _settings.packages.RemoveAt(idx); EditorGUILayout.EndHorizontal(); EditorGUILayout.EndVertical(); GUIUtility.ExitGUI(); return; }
            EditorGUILayout.EndHorizontal();
            EditorGUILayout.EndVertical();
        }

        /// 重扫 sourceDir 顶层 .html，diff htmlFiles：新增加入，目录里没了的顶层 html 移除。
        void RefreshPackage(int idx)
        {
            var pkg = _settings.packages[idx];
            string abs = ToAbs(Path.Combine(_settings.workspaceDir, pkg.sourceDir));
            if (!Directory.Exists(abs)) { AppendLog($"[refresh] {pkg.pkgName}: sourceDir 不存在 ({pkg.sourceDir})"); return; }
            var scanned = ScanTopLevelHtml(abs);
            var scannedSet = new HashSet<string>(scanned);
            int added = 0;
            foreach (var s in scanned) if (!pkg.htmlFiles.Contains(s)) { pkg.htmlFiles.Add(s); added++; }
            int removed = 0;
            for (int j = pkg.htmlFiles.Count - 1; j >= 0; j--)
            {
                string hf = pkg.htmlFiles[j];
                bool isTopLevel = hf.IndexOfAny(new[] { '/', '\\' }) < 0;
                if (isTopLevel && !scannedSet.Contains(hf)) { pkg.htmlFiles.RemoveAt(j); removed++; }
            }
            EditorUtility.SetDirty(_settings);
            AssetDatabase.SaveAssetIfDirty(_settings);
            LoomConfigExporter.Export(_settings);
            AppendLog($"[refresh] {pkg.pkgName}: +{added} / -{removed} / 保留 {pkg.htmlFiles.Count}");
        }

        // —— 图集 tab ——————————————————————————————————————————————
        void DrawAtlas()
        {
            EditorGUILayout.LabelField("图集配置（" + _settings.atlasEntries.Count + "）——同步时自动在 {pkgOutputDir}/atlas/ 建 .spriteatlasv2", EditorStyles.boldLabel);
            for (int i = 0; i < _settings.atlasEntries.Count; i++) DrawAtlasEntry(i);
            if (GUILayout.Button("+ 添加图集", GUILayout.Width(120)))
                _settings.atlasEntries.Add(new AtlasEntry { atlasName = "NewAtlas" });
            EditorGUILayout.Space(8);
            if (GUILayout.Button("同步全部图集 packables（自动建缺失的）", GUILayout.Height(28)))
            {
                LoomAtlasSync.SyncAll(_settings);
                AppendLog("[atlas] 同步完成（详情看 Console）");
                EditorUtility.SetDirty(_settings);
                AssetDatabase.SaveAssetIfDirty(_settings);
            }
        }

        void DrawAtlasEntry(int idx)
        {
            var e = _settings.atlasEntries[idx];
            EditorGUILayout.BeginVertical(EditorStyles.helpBox);
            e.atlasName = EditorGUILayout.TextField("图集名", e.atlasName);
            // 状态按 .spriteatlasv2 文件是否存在判定（AtlasEntry 不再持有 SpriteAtlas 引用）。
            string atlasRel = Path.Combine(_settings.pkgOutputDir, "atlas", e.atlasName + ".spriteatlasv2").Replace('\\', '/');
            EditorGUILayout.LabelField("状态", File.Exists(ToAbs(atlasRel)) ? "已同步" : "未同步");
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
            EditorGUILayout.BeginHorizontal();
            if (GUILayout.Button("同步此图集", GUILayout.Width(100)))
            {
                LoomAtlasSync.EnsureAtlasAsset(e, _settings.pkgOutputDir);
                LoomAtlasSync.SyncEntry(e, _settings);
                EditorUtility.SetDirty(_settings);
                AssetDatabase.SaveAssetIfDirty(_settings);
            }
            if (GUILayout.Button("删除图集", GUILayout.Width(100)))
            {
                bool deletedFile = LoomAtlasSync.DeleteAutoAtlas(e, _settings.pkgOutputDir);
                _settings.atlasEntries.RemoveAt(idx);
                EditorUtility.SetDirty(_settings);
                AssetDatabase.SaveAssetIfDirty(_settings);
                AppendLog(deletedFile
                    ? $"[atlas] 删除 {e.atlasName}（含自动建的 .spriteatlasv2）"
                    : $"[atlas] 移除 {e.atlasName}（无自动建文件或已绑外部引用，未删文件）");
                EditorGUILayout.EndHorizontal();
                EditorGUILayout.EndVertical();
                GUIUtility.ExitGUI();
                return;
            }
            EditorGUILayout.EndHorizontal();
            EditorGUILayout.EndVertical();
        }

        void HandleFolderDrop(Rect rect, AtlasEntry e)
        {
            if (!rect.Contains(Event.current.mousePosition)) return;
            if (Event.current.type == UnityEngine.EventType.DragUpdated)
            {
                bool hasDir = false;
                foreach (string p in DragAndDrop.paths) if (Directory.Exists(p)) { hasDir = true; break; }
                DragAndDrop.visualMode = hasDir ? DragAndDropVisualMode.Copy : DragAndDropVisualMode.Rejected;
            }
            if (Event.current.type == UnityEngine.EventType.DragPerform)
            {
                DragAndDrop.AcceptDrag();
                foreach (string p in DragAndDrop.paths)
                {
                    if (!Directory.Exists(p)) continue;
                    string norm = NormalizeDroppedDir(p);
                    if (!e.folders.Contains(norm)) e.folders.Add(norm);
                }
                Event.current.Use();
            }
        }

        // —— 字体 tab ——————————————————————————————————————————————
        // FontEntry 只存 familyName + sourceFileName，不持 Font 引用——避免 Resources build 把
        // Font asset 拖入。拖入时从 asset path 拆出文件名族名即丢弃 ref。发布时按 sourceFileName
        // 在 AssetDatabase 重新定位源文件，拷一份 .bytes 到 Bundles/fonts/（核心自产 atlas，不需 Unity Font asset）。
        void DrawFonts()
        {
            EditorGUILayout.LabelField("字体列表（" + _settings.fonts.Count + "）——拖 Font asset 自动填", EditorStyles.boldLabel);
            DrawFontDropZone();
            for (int i = 0; i < _settings.fonts.Count; i++) DrawFontEntry(i);
            if (GUILayout.Button("+ 手动添加", GUILayout.Width(120)))
                _settings.fonts.Add(new FontEntry());
            EditorGUILayout.Space(8);
            // 始终保证恰好一个默认字体：发布时 driver 缺 familyName 回退到 isDefault 的那个。
            if (_settings.fonts.Count > 0 && !_settings.fonts.Exists(f => f.isDefault))
                _settings.fonts[0].isDefault = true;
        }

        void DrawFontDropZone()
        {
            Rect drop = GUILayoutUtility.GetRect(0, 48, GUILayout.ExpandWidth(true));
            GUI.Box(drop, "拖 Font asset 到此\n自动填 sourceFileName + familyName（不持引用）", EditorStyles.helpBox);
            if (!drop.Contains(Event.current.mousePosition)) return;
            if (Event.current.type == UnityEngine.EventType.DragUpdated)
            {
                bool hasFont = false;
                foreach (var o in DragAndDrop.objectReferences) if (o is Font) { hasFont = true; break; }
                DragAndDrop.visualMode = hasFont ? DragAndDropVisualMode.Copy : DragAndDropVisualMode.Rejected;
            }
            if (Event.current.type == UnityEngine.EventType.DragPerform)
            {
                DragAndDrop.AcceptDrag();
                foreach (var o in DragAndDrop.objectReferences)
                {
                    if (o is Font f)
                    {
                        string assetPath = AssetDatabase.GetAssetPath(f);
                        string fileName = Path.GetFileName(assetPath);                  // NotoSansSC.ttc
                        string family = Path.GetFileNameWithoutExtension(assetPath);   // NotoSansSC
                        _settings.fonts.Add(new FontEntry
                        {
                            familyName = family,
                            sourceFileName = fileName,
                            isDefault = _settings.fonts.Count == 0
                        });
                    }
                }
                Event.current.Use();
                SaveSettings();
            }
        }

        void DrawFontEntry(int idx)
        {
            var e = _settings.fonts[idx];
            EditorGUILayout.BeginVertical(EditorStyles.helpBox);
            e.familyName = EditorGUILayout.TextField("familyName (CSS)", e.familyName);
            EditorGUILayout.LabelField("sourceFileName", e.sourceFileName);
            e.isDefault = EditorGUILayout.Toggle("isDefault", e.isDefault);
            if (GUILayout.Button("删除", GUILayout.Width(60)))
            {
                _settings.fonts.RemoveAt(idx);
                SaveSettings();
                GUIUtility.ExitGUI();
                return;
            }
            EditorGUILayout.EndVertical();
        }

        // —— 打包（复用 exe，固定路径）——————————————————————————————
        void PackAll() { for (int i = 0; i < _settings.packages.Count; i++) PackPackage(i); }

        void PackPackage(int idx)
        {
            var pkg = _settings.packages[idx];
            string exe = LoomExePath.Resolve();
            if (!File.Exists(exe)) { AppendLog($"[pack] exe 不存在: {exe}"); return; }
            string absSrc = ToAbs(Path.Combine(_settings.workspaceDir, pkg.sourceDir));
            // pkg.bin 落 Bundles/ui/，与 atlas/、fonts/ 并列——发布根按产物类型分目录。
            string outPath = ToAbs(Path.Combine(_settings.pkgOutputDir, "ui", pkg.pkgName + ".pkg.bin"));
            Directory.CreateDirectory(Path.GetDirectoryName(outPath));
            string htmlArg = pkg.htmlFiles.Count > 0 ? string.Join(",", pkg.htmlFiles) : "";
            var sb = new StringBuilder();
            sb.Append('"').Append(absSrc).Append("\" ").Append(pkg.pkgName);
            if (pkg.htmlFiles.Count > 0) sb.Append(" --html ").Append(htmlArg);
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

        // —— 发布（一键产出全部到 Bundles/）——————————————————————————
        // 单步按钮（同步图集 / 打包）留作分步调试；发布串起全部 4 步，任何一步抛异常只记日志不中断后续刷新。
        void DrawPublishButton()
        {
            EditorGUILayout.Space(12);
            if (GUILayout.Button("发布", GUILayout.Height(36))) Publish();
        }

        void Publish()
        {
            AppendLog("[发布] 开始...");
            try
            {
                LoomAtlasSync.SyncAll(_settings);
                AppendLog("[发布] Atlas: OK");
                for (int i = 0; i < _settings.packages.Count; i++) PackPackage(i);
                AppendLog("[发布] Pkg: OK");
                PublishFonts();
                LoomConfigExporter.Export(_settings);
                AppendLog("[发布] Config: OK");
            }
            catch (Exception ex) { AppendLog($"[发布] FAIL: {ex.Message}"); }
            AssetDatabase.Refresh();
        }

        // v1.6 font-to-core：核心自绘字体，后端只需原始 ttf/otf/ttc 字节喂 Rust。
        // 只拷一份 .bytes 到 Bundles/fonts/——不再复制 Unity Font asset（核心自产 atlas，不靠引擎字体 API）。
        void PublishFonts()
        {
            string fontsDir = ToAbs(Path.Combine(_settings.pkgOutputDir, "fonts"));
            Directory.CreateDirectory(fontsDir);
            int count = 0;
            foreach (var entry in _settings.fonts)
            {
                if (string.IsNullOrEmpty(entry.sourceFileName)) continue;
                string assetPath = FindFontAssetPath(entry.sourceFileName);
                if (string.IsNullOrEmpty(assetPath))
                {
                    AppendLog($"[发布] 字体 {entry.sourceFileName} 找不到源文件，跳过");
                    continue;
                }
                string absSrc = Path.GetFullPath(assetPath);
                File.Copy(absSrc, Path.Combine(fontsDir, entry.sourceFileName + ".bytes"), overwrite: true);
                count++;
                AppendLog($"[发布] 字体 {entry.sourceFileName} → {entry.sourceFileName}.bytes");
            }
            AppendLog($"[发布] Fonts: {count} → {fontsDir}");
        }

        // 按 sourceFileName 在 AssetDatabase 查字体源文件路径。v1.6 后不限 asset 类型——
        // .ttc/.ttf 可能以 Font 或 DefaultAsset 导入（甚至未导入），按文件名精确匹配过滤同名。
        // 注意：FindAssets 搜索词是 asset 名字（不含扩展名），sourceFileName 带扩展名须先去掉再搜，
        // 拿到路径后再用 GetFileName(p) == sourceFileName 精确匹配（带扩展名）防同名不同后缀误匹配。
        string FindFontAssetPath(string sourceFileName)
        {
            string searchName = Path.GetFileNameWithoutExtension(sourceFileName);
            var guids = AssetDatabase.FindAssets(searchName);
            if (guids.Length == 0)
            {
                AppendLog($"[发布] FindAssets('{searchName}') 返回 0——文件不在 Assets/ 下或名字不符");
                return null;
            }
            foreach (var g in guids)
            {
                var p = AssetDatabase.GUIDToAssetPath(g);
                if (Path.GetFileName(p) == sourceFileName) return p;
            }
            AppendLog($"[发布] '{sourceFileName}' 有 {guids.Length} 个 GUID 但文件名无一精确匹配");
            return null;
        }

        /// 标记配置脏 + 存盘 + 同步 config.json。拖拽/删除等即时操作完成后调。
        void SaveSettings()
        {
            EditorUtility.SetDirty(_settings);
            AssetDatabase.SaveAssetIfDirty(_settings);
            LoomConfigExporter.Export(_settings);
        }

        // —— 工具 ————————————————————————————————————————————————————

        /// 目录 drop area（纯拖拽，无输入框）。未设显示「将目录拖到这里」。
        /// relativeBase 非空 → 拖入目录存相对 relativeBase 的路径（res 目录用，相对工作区根）；
        /// relativeBase 空 → 存 Unity 工程相对路径。
        string DirectoryDropField(string label, string value, string relativeBase)
        {
            EditorGUILayout.LabelField(label);
            Rect drop = GUILayoutUtility.GetRect(0, 26, GUILayout.ExpandWidth(true));
            string display = string.IsNullOrEmpty(value) ? "  将目录拖到这里" : "  " + value;
            GUI.Box(drop, display, EditorStyles.helpBox);
            // × 清除按钮（已设时）
            if (!string.IsNullOrEmpty(value) &&
                GUI.Button(new Rect(drop.xMax - 24, drop.y + 3, 20, drop.height - 6), "×"))
            {
                value = "";
                GUI.changed = true;
            }
            if (drop.Contains(Event.current.mousePosition))
            {
                if (Event.current.type == UnityEngine.EventType.DragUpdated)
                {
                    bool hasDir = false;
                    foreach (var p in DragAndDrop.paths) if (Directory.Exists(p)) { hasDir = true; break; }
                    DragAndDrop.visualMode = hasDir ? DragAndDropVisualMode.Copy : DragAndDropVisualMode.Rejected;
                }
                else if (Event.current.type == UnityEngine.EventType.DragPerform)
                {
                    DragAndDrop.AcceptDrag();
                    foreach (var p in DragAndDrop.paths)
                    {
                        if (!Directory.Exists(p)) continue;
                        if (relativeBase != null)
                        {
                            if (string.IsNullOrEmpty(relativeBase)) { AppendLog("[!] 先设工作区根"); break; }
                            string rel = RelativizeTo(p, relativeBase);
                            if (rel == null) { AppendLog($"[!] 目录不在工作区根下：{p}"); break; }
                            value = rel;
                        }
                        else value = NormalizeDroppedDir(p);
                        GUI.changed = true;
                        break;
                    }
                    Event.current.Use();
                }
            }
            return value;
        }

        /// 拖入目录 → 相对 baseUnityRel 的路径（如 "res"）。不在 base 下返 null。
        string RelativizeTo(string dropped, string baseUnityRel)
        {
            string abs = ToAbsAny(dropped);
            if (abs == null) return null;
            string rel = Path.GetRelativePath(ToAbs(baseUnityRel), abs).Replace('\\', '/');
            return rel.StartsWith("..") ? null : rel;
        }

        /// 拖入路径（Unity 相对 or 绝对）→ 绝对。无法定位返 null。
        static string ToAbsAny(string p)
        {
            if (string.IsNullOrEmpty(p)) return null;
            string norm = p.Replace('\\', '/');
            if (norm.StartsWith("Assets/", StringComparison.OrdinalIgnoreCase))
                return Path.GetFullPath(Path.Combine(ProjectRoot(), norm));
            if (Path.IsPathRooted(p)) return Path.GetFullPath(p);
            return null;
        }

        /// 拖入路径 → 归一化为 Unity 工程相对（工程内）或绝对（工程外）。
        static string NormalizeDroppedDir(string p)
        {
            if (string.IsNullOrEmpty(p)) return p;
            string norm = p.Replace('\\', '/');
            if (norm.StartsWith("Assets/", StringComparison.OrdinalIgnoreCase)) return norm;
            try
            {
                string full = Path.GetFullPath(p).Replace('\\', '/');
                string projRoot = ProjectRoot().Replace('\\', '/').TrimEnd('/') + "/";
                if (full.StartsWith(projRoot, StringComparison.OrdinalIgnoreCase))
                    return full.Substring(projRoot.Length);
            }
            catch { }
            return p;
        }

        static string ToAbs(string unityRel)
        {
            if (string.IsNullOrEmpty(unityRel)) return ProjectRoot();
            return Path.GetFullPath(Path.Combine(ProjectRoot(), unityRel));
        }

        static string ProjectRoot() => Directory.GetParent(Application.dataPath).FullName;

        /// 扫 sourceDir 顶层 .html（不递归）。
        static List<string> ScanTopLevelHtml(string absDir)
        {
            var list = new List<string>();
            if (string.IsNullOrEmpty(absDir) || !Directory.Exists(absDir)) return list;
            foreach (var f in Directory.GetFiles(absDir, "*.html", SearchOption.TopDirectoryOnly))
                list.Add(Path.GetFileName(f));
            list.Sort(StringComparer.OrdinalIgnoreCase);
            return list;
        }

        void AppendLog(string line)
        {
            _log.AppendLine(line);
            if (_log.Length > 8000) _log.Remove(0, 4000);   // 防无限涨
        }

        void DrawLog()
        {
            EditorGUILayout.LabelField("日志", EditorStyles.boldLabel);
            EditorGUILayout.TextArea(_log.ToString(), GUILayout.Height(120));
        }
    }
}
