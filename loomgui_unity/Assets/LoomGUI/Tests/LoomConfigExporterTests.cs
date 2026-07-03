using NUnit.Framework;
using UnityEngine;

namespace LoomGUI.Tests
{
    public class LoomConfigExporterTests
    {
        [Test]
        public void Export_PathsRelativeToWorkspace()
        {
            var s = ScriptableObject.CreateInstance<LoomSettings>();
            s.workspaceDir = "Assets/LoomUI/";
            s.resDirName = "res";
            s.pkgOutputDir = "Assets/StreamingAssets/";
            s.packages.Add(new PackageEntry("showcase", "showcase"));
            s.packages[0].htmlFiles.Add("home.html");

            string json = LoomGUI.Editor.LoomConfigExporter.BuildJson(s);
            // exe_path 相对工作区根：Assets/LoomUI/ → Assets/LoomGUI/Editor/Tools/ = ../LoomGUI/Editor/Tools/loomgui_pkg.exe
            StringAssert.Contains("\"exe_path\": \"../LoomGUI/Editor/Tools/loomgui_pkg.exe\"", json);
            // output_dir 相对工作区根：Assets/LoomUI/ → Assets/StreamingAssets/ = ../../StreamingAssets/
            StringAssert.Contains("\"output_dir\": \"../../StreamingAssets/\"", json);
            StringAssert.Contains("\"res_dir\": \"res\"", json);
            StringAssert.Contains("\"name\": \"showcase\"", json);
            StringAssert.Contains("\"source\": \"showcase\"", json);
            StringAssert.Contains("\"home.html\"", json);
        }
    }
}
