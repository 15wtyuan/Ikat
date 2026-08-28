using System;
using System.Collections.Generic;
using System.IO;
using System.Text;
using UnityEditor;
using UnityEngine;

namespace Ikat.Editor
{
    /// <summary>
    /// `ikat verify` 的 Unity 侧入口（batchmode 导入冒烟）。
    /// CLI 拉起形如：
    ///   Unity -batchmode -quit -nographics -projectPath &lt;root&gt;
    ///     -executeMethod Ikat.Editor.IkatVerifySmoke.Run -logFile &lt;tmp&gt;
    ///     -ikatVerifyDir=Assets/&lt;output_dir&gt; -ikatVerifyReport=&lt;tmp&gt;
    /// 语义：Refresh 全量导入后对产物目录逐文件**正向加载**（png → Texture2D
    /// 解码证明；其余 → 非空 Object 导入证明），报告写 -ikatVerifyReport 文件
    /// （每行 `OK &lt;asset path&gt;` / `FAIL &lt;asset path&gt;: &lt;reason&gt;`），
    /// 退出码 0=全过 / 1=有导入失败 / 2=参数缺失。
    /// 正向检查比扫 console error 更跨版本稳：LogEntries 内部 API 在 2021/6000
    /// 间形状不同，而导入失败的资产加载必空（null 即铁证）。
    /// </summary>
    public static class IkatVerifySmoke
    {
        public static void Run()
        {
            string dir = FindArg("-ikatVerifyDir=");
            string reportPath = FindArg("-ikatVerifyReport=");
            if (string.IsNullOrEmpty(dir) || string.IsNullOrEmpty(reportPath))
            {
                UnityEngine.Debug.LogError(
                    "[Ikat][verify] missing -ikatVerifyDir= / -ikatVerifyReport= args");
                EditorApplication.Exit(2);
                return;
            }

            AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);

            var lines = new List<string>();
            int failed = 0;
            string absRoot = Path.GetFullPath(Path.Combine(Application.dataPath, "..", dir));
            if (!Directory.Exists(absRoot))
            {
                lines.Add("FAIL " + dir + ": output directory not found under the Unity project");
                failed++;
            }
            else
            {
                foreach (string rel in EnumerateRel(absRoot, dir))
                {
                    UnityEngine.Object asset;
                    if (rel.EndsWith(".png", StringComparison.OrdinalIgnoreCase))
                        asset = AssetDatabase.LoadAssetAtPath<Texture2D>(rel);
                    else
                        asset = AssetDatabase.LoadAssetAtPath<UnityEngine.Object>(rel);
                    if (asset == null)
                    {
                        lines.Add("FAIL " + rel + ": import produced no loadable asset");
                        failed++;
                    }
                    else
                    {
                        lines.Add("OK " + rel);
                    }
                }
                if (lines.Count == 0)
                {
                    lines.Add("FAIL " + dir + ": no build outputs found");
                    failed++;
                }
            }
            File.WriteAllText(reportPath, string.Join("\n", lines) + "\n", new UTF8Encoding(false));
            EditorApplication.Exit(failed > 0 ? 1 : 0);
        }

        static string FindArg(string prefix)
        {
            foreach (string a in Environment.GetCommandLineArgs())
                if (a.StartsWith(prefix, StringComparison.Ordinal))
                    return a.Substring(prefix.Length);
            return null;
        }

        /// 目录内全部文件（.meta 除外）转 "Assets/..." 相对路径（正斜杠）。
        static IEnumerable<string> EnumerateRel(string absRoot, string relDir)
        {
            foreach (string file in Directory.GetFiles(absRoot, "*", SearchOption.AllDirectories))
            {
                if (file.EndsWith(".meta", StringComparison.OrdinalIgnoreCase)) continue;
                yield return (relDir + file.Substring(absRoot.Length)).Replace('\\', '/');
            }
        }
    }
}
