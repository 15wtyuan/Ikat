using NUnit.Framework;
using UnityEngine;

namespace Yio.Tests
{
    /// <summary>
    /// A4 多 Stage 隔离的 EditMode 冒烟：共享相机（引用计数 + 按名认领）、
    /// sortingOrder 基址分配、层序排序。输入路由的 Pick 探测依赖 PlayMode 输入态，
    /// 验收在 PlayMode 清单（票面）；此处覆盖可静态验证的面。
    /// </summary>
    public class YioStageHubTests
    {
        [SetUp]
        public void ResetHub()
        {
            YioStageHub.ResetStatics();
        }

        [TearDown]
        public void CleanupCamera()
        {
            // 兜底：任何残留共享相机清掉（测试失败路径也可能留下）。
            var cam = GameObject.Find("YioUICamera");
            if (cam != null) Object.DestroyImmediate(cam);
        }

        /// <summary>两 Driver 同场景：共享一台相机；层序 0/1 → 基址 0/8192。</summary>
        [Test]
        public void TwoDrivers_ShareOneCamera_AndDistinctSortBases()
        {
            var go1 = new GameObject("hub_driver_1");
            var go2 = new GameObject("hub_driver_2");
            try
            {
                var d1 = go1.AddComponent<YioStageDriver>();
                var d2 = go2.AddComponent<YioStageDriver>();
                Assert.AreEqual(2, YioStageHub.DriverCount, "两个 Driver 注册在 hub");
                // 基址：层序 0 → 0；层序 1 → 8192（跨 stage sort_key 从 0 起编，基址隔离穿插）。
                Assert.AreEqual(0, YioStageHub.SortBaseOf(d1), "层序 0 基址 0");
                Assert.AreEqual(YioStageHub.SortStride, YioStageHub.SortBaseOf(d2), "层序 1 基址 = 档宽");

                // 共享相机：场景里恰一台 YioUICamera。
                Assert.AreEqual(1, CountYioUICameras(), "两 Driver 并存只建一台共享 UI 相机（layer 互画消灭）");


            }
            finally
            {
                Object.DestroyImmediate(go2);
                Object.DestroyImmediate(go1);
            }
            Assert.AreEqual(0, YioStageHub.DriverCount, "全部销毁后 hub 清空");
            // 最后一个引用释放时相机销毁。
            Assert.IsNull(GameObject.Find("YioUICamera"), "最后一个 Driver 销毁 → 共享相机释放");
        }

        /// <summary>一个 Driver 销毁、另一个仍在：共享相机保留（引用计数语义）。</summary>
        [Test]
        public void DestroyOneDriver_CameraSurvivesForTheOther()
        {
            var go1 = new GameObject("hub_driver_keep");
            var go2 = new GameObject("hub_driver_die");
            try
            {
                go1.AddComponent<YioStageDriver>();
                go2.AddComponent<YioStageDriver>();
                Object.DestroyImmediate(go2); // OnDestroy → Release（refs 2→1，相机保留）
                Assert.IsNotNull(GameObject.Find("YioUICamera"), "仍有使用者的相机不销毁");
            }
            finally
            {
                Object.DestroyImmediate(go1);
            }
            Assert.IsNull(GameObject.Find("YioUICamera"), "最后一个使用者销毁 → 相机销毁");
        }

        /// <summary>按名认领：场景里已有的存量 YioUICamera（重编译幸存者）被复用而非重建。</summary>
        [Test]
        public void ExistingCameraByMain_IsAdopted_NotDuplicated()
        {
            var pre = new GameObject("YioUICamera");
            pre.AddComponent<Camera>();
            var go = new GameObject("hub_driver_adopt");
            try
            {
                go.AddComponent<YioStageDriver>();
                Assert.AreEqual(1, CountYioUICameras(), "存量相机被认领，不建第二台");
            }
            finally
            {
                Object.DestroyImmediate(go);
                // 认领的相机会被 hub 释放（最后一个引用者）；兜底再清。
                var cam = GameObject.Find("YioUICamera");
                if (cam != null) Object.DestroyImmediate(cam);
            }
        }

        /// 场景内 YioUICamera 计数。FindObjectsInactive 重载（2023.1+ 存在、新 Unity
        /// 不在废弃名单——SortMode 版 6000.5+ obsolete，FindObjectsOfType 2023.1+ 警告源）；
        /// 2021 走旧 API。
        static int CountYioUICameras()
        {
            int count = 0;
#if UNITY_2023_1_OR_NEWER
            foreach (var c in Object.FindObjectsByType<Camera>(FindObjectsInactive.Exclude))
#else
            foreach (var c in Object.FindObjectsOfType<Camera>())
#endif
                if (c.gameObject.name == "YioUICamera") count++;
            return count;
        }
    }
}
