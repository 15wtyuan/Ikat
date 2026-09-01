using System.Collections.Generic;
using UnityEngine;

namespace Ikat
{
    /// <summary>
    /// 多 Stage 合成器（A4 隔离）：共享 UI 相机（per-Scene 引用计数 + 按名认领）、
    /// stage 序号 → sortingOrder 基址、输入深度序独占路由。
    ///
    /// **共享相机**：相机配置完全由屏幕尺寸推导（正交 size=屏高/2、cullingMask=UI 层、
    /// 深度窗 ±9990/10000），与 stage 无关——N 个 Driver 共享一台即消灭 layer 5 互画
    /// （每台相机的 cullingMask 都含 UI 层，多台 = 每台都画全部 UI）。
    /// 按名认领（"IkatUICamera"）先于新建：编辑器脚本重编译不跑 OnDestroy、自建相机
    /// 带 DontSaveInEditor 存活，不认领会积累重复相机（历史实锤）。
    ///
    /// **sortingOrder 基址**：跨 stage 的 sort_key 都从 0 起编（每 stage 独立计数），
    /// 同相机下直接比较会互相穿插。基址 = stage 序号 × <see cref="SortStride"/>
    /// （Unity sortingOrder 16 位上限 → 8192 一档最多 4 个 stage，超出告警降级共用末档）。
    ///
    /// **输入路由**：多 Driver 并存时，指针事件按层序（高序优先）逐 stage 探测
    /// （<c>Context.Pick</c> 命中即可交互），首个命中者独占本帧输入——渲染次序即输入
    /// 次序。单 Driver 零开销直通。键盘/滚轮/文本随指针所有者路由。帧级仲裁（无
    /// down 序列锁定）：拖拽跨界时所有权随指针位置切换（focus-follows-pointer 语义，
    /// 文档写明；重叠拖拽属边缘场景）。
    /// </summary>
    public static class IkatStageHub
    {
        /// <summary>sortingOrder 每档宽度。16 位上限内 4 档（含 NativeHost +1000 lift）。</summary>
        public const int SortStride = 8192;
        /// <summary>支持的 sortingOrder 档数上限（超出告警并降级共用末档）。</summary>
        public const int MaxOrdinals = 4;
        const string CameraName = "IkatUICamera";

        static readonly List<(IkatStageDriver driver, int order, int seq)> s_drivers = new();
        // 键 = Scene 本体（IEquatable，按内部 handle 等值比较 + GetHashCode）——不取
        // .handle 转 int：新 Unity（6.2+）Scene.handle 返 SceneHandle 且隐式转 int 是
        // error 级 obsolete（CS0619），其替代 GetRawData() 旧版（2021 工程）又不存在；
        // Scene 作键全版本通用且语义同源。
        static readonly Dictionary<UnityEngine.SceneManagement.Scene, SharedCamera> s_cameras = new();
        static int s_nextSeq; // 注册序号：order 同值时的稳定 tiebreaker（List.Sort 不稳定）

        /// <summary>本会话已注册的 Driver 数。</summary>
        public static int DriverCount => s_drivers.Count;

        /// <summary>
        /// Driver Awake 注册：排入层序列表（order 升序、同序按注册先后——seq tiebreaker
        /// 保稳定），返回 sortingOrder 基址。inputEnabled=false 的 Driver（如 world-space
        /// 舞台）参与渲染排序但不参与输入路由。中途注册/注销会改变各 Driver 的档位——
        /// Driver 侧按 DriverCount 变化重取基址，勿一次性缓存。
        /// </summary>
        public static int Register(IkatStageDriver driver, int order)
        {
            if (s_drivers.FindIndex(e => e.driver == driver) < 0)
                s_drivers.Add((driver, order, s_nextSeq++));
            s_drivers.Sort((a, b) => a.order != b.order
                ? a.order.CompareTo(b.order)
                : a.seq.CompareTo(b.seq));
            return SortBaseOf(driver);
        }

        /// <summary>Driver OnDestroy 注销（相机引用随之释放）。</summary>
        public static void Unregister(IkatStageDriver driver)
        {
            int i = s_drivers.FindIndex(e => e.driver == driver);
            if (i >= 0) s_drivers.RemoveAt(i);
        }

        /// <summary>层序列表中的序号（0 = 最底层）→ sortingOrder 基址（超出档数告警降级末档）。</summary>
        public static int SortBaseOf(IkatStageDriver driver)
        {
            int idx = s_drivers.FindIndex(e => e.driver == driver);
            if (idx < 0) idx = s_drivers.Count;
            if (idx >= MaxOrdinals)
            {
                Debug.LogWarning(
                    $"[IkatStageHub] {idx + 1} stages exceed sortingOrder budget ({MaxOrdinals} x {SortStride}); stage '{driver.name}' shares the top band with stage {MaxOrdinals - 1}");
                idx = MaxOrdinals - 1;
            }
            return idx * SortStride;
        }

        /// <summary>
        /// 取本场景的共享 UI 相机（引用计数 +1）。优先级：用户指派（Driver 自持，不经此）
        /// &gt; 按名认领存量 &gt; 新建。须与 <see cref="ReleaseCamera"/> 成对。
        /// 认领/新建后跑孤儿清扫（<see cref="SweepOrphanedCameras"/>）。
        /// </summary>
        public static Camera AcquireCamera(Component caller)
        {
            var scene = caller.gameObject.scene;
            if (!s_cameras.TryGetValue(scene, out var shared) || shared.Camera == null)
            {
                Camera cam = FindExisting(caller.gameObject.scene);
                if (cam == null) cam = CreateCamera();
                shared = new SharedCamera { Camera = cam };
                s_cameras[scene] = shared;
            }
            shared.Refs++;
            SweepOrphanedCameras(shared.Camera);
            return shared.Camera;
        }

        /// <summary>
        /// 孤儿共享相机清扫。Driver 是 [ExecuteAlways]：编辑态 Awake 也建相机
        /// （DontSaveInEditor），domain reload 不跑 OnDestroy——幸存相机被 Unity 挪进
        /// 无效场景（scene 无效，不属任何已加载场景），<see cref="FindExisting"/> 按
        /// caller 场景根扫描永远认领不到。孤儿以 Base 型 + depth 0 压在宿主 3D 相机
        /// （depth 更小、先渲染）之上每帧重渲染：clear 走 Depth 抹掉宿主 3D 输出
        /// （「世界锚点页看不到 3D 场景」），目标未初始化时整屏垃圾色（「相机一片黄」）。
        /// 合法 IkatUICamera 只有两种形态：本次共享相机本体（跳过）、其它已加载场景里
        /// 的（scene 有效，跳过——多场景各自持有一台）。其余 = 遗留垃圾，即刻销毁。
        /// 用户手建的（无 DontSaveInEditor 标记）尊重不动。
        /// </summary>
        static void SweepOrphanedCameras(Camera keep)
        {
            foreach (var c in Resources.FindObjectsOfTypeAll<Camera>())
            {
                if (c == keep || c.name != CameraName) continue;
                if (c.gameObject.scene.IsValid()) continue;
                if ((c.hideFlags & HideFlags.DontSaveInEditor) == 0) continue;
                if (Application.isPlaying) Object.Destroy(c.gameObject);
                else Object.DestroyImmediate(c.gameObject);
            }
        }

        /// <summary>释放共享相机引用；本场景最后一个引用释放时销毁相机。</summary>
        public static void ReleaseCamera(Component caller)
        {
            var scene = caller.gameObject.scene;
            if (!s_cameras.TryGetValue(scene, out var shared)) return;
            shared.Refs--;
            if (shared.Refs <= 0)
            {
                if (shared.Camera != null)
                {
                    if (Application.isPlaying) Object.Destroy(shared.Camera.gameObject);
                    else Object.DestroyImmediate(shared.Camera.gameObject);
                }
                s_cameras.Remove(scene);
            }
        }

        /// <summary>按名认领存量共享相机（编辑器重编译幸存者）。只搜本场景根对象。</summary>
        static Camera FindExisting(UnityEngine.SceneManagement.Scene scene)
        {
            // 场景未完全加载（isLoaded=false，如 Driver Awake 早于场景加载完成）时
            // GetRootGameObjects 抛 ArgumentException。认领是机会主义扫描，
            // 查不了就当无可认领——走新建。
            if (!scene.IsValid() || !scene.isLoaded) return null;
            foreach (var root in scene.GetRootGameObjects())
            {
                if (root.name != CameraName) continue;
                var cam = root.GetComponent<Camera>();
                if (cam != null) return cam;
            }
            return null;
        }

        static Camera CreateCamera()
        {
            var go = new GameObject(CameraName)
            {
                hideFlags = HideFlags.DontSaveInEditor,
            };
            var cam = go.AddComponent<Camera>();
            // URP 相机数据反射附加（管线存在才需要；Built-in 无此组件）。
            var urpType = System.Type.GetType(
                "UnityEngine.Rendering.Universal.UniversalAdditionalCameraData, Unity.RenderPipelines.Universal.Runtime");
            if (urpType != null && go.GetComponent(urpType) == null) go.AddComponent(urpType);
            return cam;
        }

        /// <summary>
        /// 本帧输入仲裁：多 Driver 时按层序（高→低）探测首个 Pick 命中者；
        /// 全未命中 → 最顶层的 inputEnabled Driver（悬停语义）；单 Driver 直通。
        /// 返回 null = 无可路由 Driver（如全部 inputEnabled=false）。
        /// </summary>
        public static IkatStageDriver RouteInput(IkatStageDriver self)
        {
            if (s_drivers.Count <= 1) return self;
            // 顶→底探测。PointerHitProbe = 该 Driver 把当前屏幕指针映射到自己的 design 系做 Pick。
            for (int i = s_drivers.Count - 1; i >= 0; i--)
            {
                var d = s_drivers[i].driver;
                if (d == null || !d.InputEnabled) continue;
                if (d.PointerHitProbe()) return d;
            }
            // 全未命中：最顶层可输入 Driver 收悬停/键盘。
            for (int i = s_drivers.Count - 1; i >= 0; i--)
            {
                var d = s_drivers[i].driver;
                if (d != null && d.InputEnabled) return d;
            }
            return null;
        }

        /// <summary>domain reload 清（Driver.ResetStatics 调；同 ikat_shutdown 钩）。</summary>
        public static void ResetStatics()
        {
            s_drivers.Clear();
            s_cameras.Clear();
        }

        class SharedCamera
        {
            public Camera Camera;
            public int Refs;
        }
    }
}
