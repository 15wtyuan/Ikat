using System.Collections;
using NUnit.Framework;
using UnityEngine;
using UnityEngine.TestTools;

namespace LoomGUI.Tests
{
    /// LoomStageDriver 生命周期测试（PlayMode）。
    /// 需 UNITY_LICENSE 配好 + CI PlayMode job 启用（删 unity-smoke.yml 里 if: false）。
    public class LoomStageDriverPlayTests
    {
        [UnityTest]
        public IEnumerator Driver_Awake_CreatesHost()
        {
            var go = new GameObject("TestDriver");
            var driver = go.AddComponent<LoomStageDriver>();
            yield return null;  // 等一帧 Awake 执行

            Assert.IsNotNull(driver.Context, "Awake 应构造 LoomHost 实例（Context 非空）");

            Object.Destroy(go);
        }
    }
}
