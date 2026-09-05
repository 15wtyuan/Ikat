using System;
using Yio.Bindings;

namespace Yio.HeadlessTests
{
    /// <summary>
    /// Headless 测试 harness：P/Invoke 真 <c>yio_ffi_c.dll</c> 驱动 Stage，
    /// 不启动 Unity。Phase C/D/E 测试基础设施——核心范式在本机 headless 验，
    /// 不每次 commit-dll-push 去家里机 PlayMode（破两台机串行瓶颈）。
    ///
    /// 工厂 <see cref="Create"/> 返 raw stage handle + 包好的 <see cref="UIContext"/>；
    /// 调用方 <c>try/finally</c> 确保配对调 <see cref="Destroy"/>（Rust 侧 per-handle 拥有内存，
    /// 漏调 = 内存泄漏，不 GC 回收）。StageHandle 在 FFI 是 opaque 空 struct，跨 FFI 当 <c>void*</c>。
    /// </summary>
    internal static unsafe class StageHarness
    {
        /// <summary>
        /// 建 Stage + 包 UIContext。失败（dll 没加载 / Stage::new 内部 Err）抛，调用方不必判 null。
        /// 默认 1280×720 与 roadmap 双终点线 headless rect demo 对齐。
        /// </summary>
        public static (IntPtr stage, UIContext ctx) Create(float w = 1280, float h = 720)
        {
            StageHandle* s = Native.yio_stage_new(w, h);
            if (s == null)
                throw new InvalidOperationException(
                    $"yio_stage_new({w}, {h}) returned null handle (dll load failed or Stage::new errored)");
            return ((IntPtr)s, new UIContext((IntPtr)s));
        }

        /// <summary>
        /// null-safe 释放 Stage。配对 <see cref="Create"/>；调后句柄不可再用。
        /// </summary>
        public static void Destroy(IntPtr stage) => Native.yio_stage_free((StageHandle*)stage.ToPointer());
    }
}
