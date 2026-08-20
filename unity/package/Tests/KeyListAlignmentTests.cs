using NUnit.Framework;
using LoomGUI;
#if ENABLE_INPUT_SYSTEM
using UnityEngine.InputSystem;
#endif

namespace LoomGUI.Tests
{
    /// CollectKeys 新旧路径的平行键表对齐校验：KeyList（LoomKeyCode）与 NewKeyList
    /// （InputSystem.Key）按同下标一一配对，CollectKeys 新路径轮询 NewKeyList、发码取
    /// KeyList 数值（core key_code 契约）。两表错位 = 静默发错键码（按 A 出 Tab），
    /// 编译器不查数组内容，必须有此运行时校验兜底。
    public class KeyListAlignmentTests
    {
#if ENABLE_INPUT_SYSTEM
        /// LoomKeyCode 名 → Key 名的仅有差异：Alpha0-9↔Digit0-9、Return↔Enter。
        /// 其余白名单键两枚举同名（A..Z、Space、Escape、方向、Home/End、Backspace、
        /// Delete、Tab）。出现其它名字差异时本测试失败，逼着同步两张表。
        static string ExpectedKeyName(string keyCodeName)
        {
            if (keyCodeName == "Return") return "Enter";
            if (keyCodeName.Length == 6 && keyCodeName.StartsWith("Alpha"))
                return "Digit" + keyCodeName.Substring(5);
            return keyCodeName;
        }

        [Test]
        public void ParallelKeyTablesAligned()
        {
            Assert.AreEqual(
                LoomInputCollector.KeyList.Length, LoomInputCollector.NewKeyList.Length,
                "KeyList 与 NewKeyList 长度不一致——两表必须同下标一一配对");
            for (int i = 0; i < LoomInputCollector.KeyList.Length; i++)
            {
                string expected = ExpectedKeyName(LoomInputCollector.KeyList[i].ToString());
                Assert.AreEqual(
                    expected, LoomInputCollector.NewKeyList[i].ToString(),
                    $"下标 {i}：LoomKeyCode.{LoomInputCollector.KeyList[i]} 应配 Key.{expected}");
            }
        }
#endif
    }
}
