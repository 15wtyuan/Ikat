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
            if (!File.Exists(exe))
            {
                UnityEngine.Debug.LogError($"[LoomGUI] 打包器 GUI 未找到：{exe}。请先构建 loomgui_gui 或在设置里配置路径。");
                return;
            }
            Process.Start(new ProcessStartInfo(exe) { UseShellExecute = true });
        }

        /// 按平台定位 GUI 可执行文件。约定放插件包 Editor/Tools/ 下。
        static string ResolveExe()
        {
            string toolsDir = Path.Combine(
                Path.GetDirectoryName(Application.dataPath) ?? ".",
                "Packages/com.loomgui.unity/Editor/Tools");
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
