using NUnit.Framework;
using UnityEngine;

namespace Yio.Tests
{
    /// <summary>
    /// YioHost.DumpSceneJson EditMode tests——验证 yio_stage_dump_scene 的 byte*→UTF-8 marshal
    /// （含 NUL 终止符剥离）+ null/disposed 守卫。host 经 YioStageDriver.Awake 构造（同
    /// YioStageDriverTests 先例），不直接 new YioHost（构造要 backend）。PlayMode 全量由 acceptance 跑。
    /// </summary>
    public class YioHostDumpTests
    {
        /// <summary>未加载场景（或已加载）的 dump 必须是合法 JSON 数组——证明 marshal 路径通。</summary>
        [Test]
        public void DumpSceneJson_ReturnsJsonArray()
        {
            var go = new GameObject("dump_test");
            try
            {
                var driver = go.AddComponent<YioStageDriver>();
                Assert.IsNotNull(driver.Host, "Awake must build host");
                string json = driver.Host.DumpSceneJson();
                Assert.IsNotNull(json);
                Assert.IsTrue(json.StartsWith("["),
                    "dump must be a JSON array, got: " +
                    json.Substring(0, System.Math.Min(40, json.Length)));
            }
            finally
            {
                Object.DestroyImmediate(go);
                // EnsureCamera 建独立 GO（非 root 子），DestroyImmediate(go) 不清——手动。
                var cam = GameObject.Find("YioUICamera");
                if (cam != null) Object.DestroyImmediate(cam);
            }
        }

        /// <summary>Dispose 后 _stage=null，DumpSceneJson 必须早返 "[]"，不 deref 已释放指针。</summary>
        [Test]
        public void DumpSceneJson_AfterDispose_ReturnsEmptyArray()
        {
            var go = new GameObject("dump_test2");
            var driver = go.AddComponent<YioStageDriver>();
            var host = driver.Host;
            Assert.IsNotNull(host);
            // DestroyImmediate 触发 OnDestroy → host.Dispose → _stage=null。
            Object.DestroyImmediate(go);
            var cam = GameObject.Find("YioUICamera");
            if (cam != null) Object.DestroyImmediate(cam);
            // 用 Dispose 前捕获的 host 引用：C# 对象在 GC 前仍有效，_stage 已 null。
            Assert.AreEqual("[]", host.DumpSceneJson());
        }
    }
}
