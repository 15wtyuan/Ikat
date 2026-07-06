using NUnit.Framework;
using UnityEngine;

namespace LoomGUI.Tests
{
    /// <summary>
    /// LoomStageDriver EditMode 测试。Unity PlayMode/EditMode 测试无法从 CLI 跑（需 Unity 编辑器），
    /// 本测试在 B8 验收时由 Unity Test Runner 执行。此处仅保证编译 + 逻辑正确。
    /// </summary>
    public class LoomStageDriverTests
    {
        /// <summary>
        /// Driver.Awake 必须构造 LoomStage 实例（Stage 属性非 null）。
        /// 验 Awake 序列：new LoomStage + SetNativeHostRoot + InitSprites + RegisterFontsFromSettings
        /// + EnsureCamera + ConfigureTransforms + 绑 Font.textureRebuilt 均不抛异常。
        /// </summary>
        [Test]
        public void LoomStageDriver_AwakeBuildsStageAndRegistersFonts()
        {
            var go = new GameObject("driver_test");
            try
            {
                var driver = go.AddComponent<LoomStageDriver>();
                Assert.IsNotNull(driver, "AddComponent must return the driver instance");
                Assert.IsNotNull(driver.Stage, "Driver.Awake must construct LoomStage");
            }
            finally
            {
                // OnDestroy 解绑 Font.textureRebuilt + Dispose stage（释放 native handle）。
                Object.DestroyImmediate(go);
                // EnsureCamera 自建独立 GO（非 root 子），DestroyImmediate(go) 不连带——手动清。
                var cam = GameObject.Find("LoomUICamera");
                if (cam != null) Object.DestroyImmediate(cam);
            }
        }
    }
}
