using System;
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
            s.pkgOutputDir = "Assets/LoomGUI/Bundles/";
            s.packages.Add(new PackageEntry("showcase", "showcase"));
            s.packages[0].htmlFiles.Add("home.html");

            string json = LoomGUI.Editor.LoomConfigExporter.BuildJson(s);
            // exe_path 相对工作区根：Assets/LoomUI/ → Packages/com.loomgui.unity/Editor/Tools/ = ../../Packages/com.loomgui.unity/Editor/Tools/loomgui_pkg.exe
            StringAssert.Contains("\"exe_path\": \"../../Packages/com.loomgui.unity/Editor/Tools/loomgui_pkg.exe\"", json);
            // output_dir 相对工作区根：Assets/LoomUI/ → Assets/LoomGUI/Bundles/ = ../LoomGUI/Bundles/
            StringAssert.Contains("\"output_dir\": \"../LoomGUI/Bundles/\"", json);
            StringAssert.Contains("\"res_dir\": \"res\"", json);
            StringAssert.Contains("\"name\": \"showcase\"", json);
            StringAssert.Contains("\"source\": \"showcase\"", json);
            StringAssert.Contains("\"home.html\"", json);
        }

        [Test]
        public void FontEntry_HasNoFontAssetField()
        {
            var fields = typeof(FontEntry).GetFields();
            Assert.IsFalse(Array.Exists(fields, f => f.FieldType == typeof(UnityEngine.Font)),
                "FontEntry must NOT hold a Font asset reference (would drag asset into Resources build)");
            Assert.IsTrue(Array.Exists(fields, f => f.Name == "sourceFileName"),
                "FontEntry must have sourceFileName for driver .bytes path");
        }
    }
}
