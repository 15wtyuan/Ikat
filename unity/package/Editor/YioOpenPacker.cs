using System.Diagnostics;
using System.IO;
using UnityEditor;
using UnityEngine;

namespace Yio.Editor
{
    /// Yio 打包器 GUI 拉起入口（工作区/图集/字体配置全在独立 Tauri app 里，不再进 Unity）。
    public static class YioOpenPacker
    {
        [MenuItem("Yio/Open Packer")]
        public static void Open()
        {
            string exe = ResolveExe();
            if (string.IsNullOrEmpty(exe) || !File.Exists(exe))
            {
                UnityEngine.Debug.LogError(
                    $"[Yio] 打包器 GUI 未找到：{(string.IsNullOrEmpty(exe) ? "<com.yio.unity 包未正确安装>" : exe)}\n" +
                    "请先构建并把 exe 放进包的 Editor/Tools/：\n" +
                    "  cargo build -p yio_gui --release\n" +
                    "  拷 target/release/yio_gui.exe → yio_unity_package/Editor/Tools/");
                return;
            }
            Process.Start(new ProcessStartInfo(exe)
            {
                UseShellExecute = true,
                // 引擎无关的持久化契约：GUI 把最近工作区列表等状态写进这里。
                // Unity 侧落在 UserSettings/Yio（per-user，默认 .gitignore 已忽略）。
                // --unity-root：GUI 新建工作区时写反向配置（.yio/unity.json），让 yio CLI
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

        /// GUI 状态目录：<Project>/UserSettings/Yio。目录不存在由 GUI 侧自建。
        static string ResolveStateDir()
        {
            // Application.dataPath = <Project>/Assets，取上级即工程根。
            string projectRoot = Path.GetDirectoryName(Application.dataPath);
            return Path.Combine(projectRoot, "UserSettings", "Yio");
        }

        /// 定位 GUI 可执行文件。
        /// com.yio.unity 是 manifest.json 里的外部 file: 本地包，Unity 对它只做虚拟挂载——
        /// Packages/com.yio.unity 不是真实文件系统路径，System.IO 探不到。必须用 PackageInfo
        /// 取包的真实磁盘根 (resolvedPath)，再拼 Editor/Tools。
        static string ResolveExe()
        {
            var pkg = UnityEditor.PackageManager.PackageInfo.FindForAssetPath("Packages/com.yio.unity/package.json");
            if (pkg == null)
                return "";
            string toolsDir = Path.Combine(pkg.resolvedPath, "Editor/Tools");
#if UNITY_EDITOR_WIN
            return Path.Combine(toolsDir, "yio_gui.exe");
#elif UNITY_EDITOR_OSX
            return Path.Combine(toolsDir, "yio_gui.app/Contents/MacOS/yio_gui");
#else
            return Path.Combine(toolsDir, "yio_gui");
#endif
        }
    }
}
