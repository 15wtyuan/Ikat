using System.IO;

namespace LoomGUI.Editor
{
    /// <summary>
    /// loomgui_pkg.exe 固定路径（随插件发布，不暴露给用户）。
    /// </summary>
    public static class LoomExePath
    {
        public static string Resolve()
        {
            string projRoot = Directory.GetParent(UnityEngine.Application.dataPath).FullName;
            return Path.GetFullPath(Path.Combine(projRoot, "Packages/com.loomgui.unity/Editor/Tools/loomgui_pkg.exe"));
        }
    }
}
