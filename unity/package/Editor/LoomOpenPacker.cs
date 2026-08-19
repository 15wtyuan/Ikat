using System.Diagnostics;
using System.IO;
using UnityEditor;
using UnityEngine;

namespace LoomGUI.Editor
{
    /// LoomGUI 打包器 GUI 拉起入口（工作区/图集/字体配置全在独立 Tauri app 里，不再进 Unity）。
    public static class LoomOpenPacker
    {
        [MenuItem("LoomGUI/Open Packer")]
        public static void Open()
        {
            string exe = ResolveExe();
            if (string.IsNullOrEmpty(exe) || !File.Exists(exe))
            {
                UnityEngine.Debug.LogError(
                    $"[LoomGUI] 打包器 GUI 未找到：{(string.IsNullOrEmpty(exe) ? "<com.loomgui.unity 包未正确安装>" : exe)}\n" +
                    "请先构建并把 exe 放进包的 Editor/Tools/：\n" +
                    "  cargo build -p loomgui_gui --release\n" +
                    "  拷 target/release/loomgui_gui.exe → loomgui_unity_package/Editor/Tools/");
                return;
            }
            Process.Start(new ProcessStartInfo(exe)
            {
                UseShellExecute = true,
                // 引擎无关的持久化契约：GUI 把最近工作区列表等状态写进这里。
                // Unity 侧落在 UserSettings/LoomGUI（per-user，默认 .gitignore 已忽略）。
                // --unity-root：GUI 新建工作区时写反向配置（.loom/unity.json），让 loom CLI
                // 的 build 产物直落本工程的 Assets（output_dir 相对工程根解析）。
                Arguments = "--state-dir \"" + ResolveStateDir() + "\""
                    + " --unity-root \"" + ResolveProjectRoot() + "\"",
            });
        }

        /// Unity 工程根（反向配置的基座）：<Project>/Assets 的上级。
        static string ResolveProjectRoot()
        {
            return Path.GetDirectoryName(Application.dataPath);
        }

        /// GUI 状态目录：<Project>/UserSettings/LoomGUI。目录不存在由 GUI 侧自建。
        static string ResolveStateDir()
        {
            // Application.dataPath = <Project>/Assets，取上级即工程根。
            string projectRoot = Path.GetDirectoryName(Application.dataPath);
            return Path.Combine(projectRoot, "UserSettings", "LoomGUI");
        }

        /// 定位 GUI 可执行文件。
        /// com.loomgui.unity 是 manifest.json 里的外部 file: 本地包，Unity 对它只做虚拟挂载——
        /// Packages/com.loomgui.unity 不是真实文件系统路径，System.IO 探不到。必须用 PackageInfo
        /// 取包的真实磁盘根 (resolvedPath)，再拼 Editor/Tools。
        static string ResolveExe()
        {
            var pkg = UnityEditor.PackageManager.PackageInfo.FindForAssetPath("Packages/com.loomgui.unity/package.json");
            if (pkg == null)
                return "";
            string toolsDir = Path.Combine(pkg.resolvedPath, "Editor/Tools");
#if UNITY_EDITOR_WIN
            return Path.Combine(toolsDir, "loomgui_gui.exe");
#elif UNITY_EDITOR_OSX
            return Path.Combine(toolsDir, "loomgui_gui.app/Contents/MacOS/loomgui_gui");
#else
            return Path.Combine(toolsDir, "loomgui_gui");
#endif
        }
    }
}
