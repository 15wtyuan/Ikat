using System;
using System.Runtime.InteropServices;
using System.Text;
using LoomGUI.Bindings;
using Xunit;

namespace LoomGUI.HeadlessTests
{
    /// <summary>
    /// B3 harness smoke 测试：证明编码机能 P/Invoke 真 <c>loomgui_ffi_c.dll</c> 驱动 Stage，
    /// 不启动 Unity。这是 Phase B 收尾门 + Phase C/D/E 的基础设施验收。
    ///
    /// 两层证据：
    /// 1. <see cref="StageCreatesAndTicks"/> — 完整 P/Invoke 链路通（new → create_root → tick → free）；
    ///    若 dll 没拷到输出目录 / DllImport 名错 / dll 位数错 → <c>DllNotFoundException</c>。
    /// 2. <see cref="GetNodeKindRoundTrip"/> — 兑现 B2 defer：实际 FFI round-trip（建 div →
    ///    <c>get_node_kind</c> byte → NodeKind.Container），验证 C# enum 与 live Rust 一致。
    /// </summary>
    public unsafe class HarnessSmokeTests
    {
        // create_root 失败哨兵（lib.rs:1115）。
        private const uint InvalidNodeId = 0xFFFF_FFFFu;

        /// <summary>
        /// 完整生命周期 P/Invoke 链路通：stage new → create_root(div) → tick(16ms) → free。
        /// 不断言 panic = pass（FFI 是 C ABI，Rust panic 跨 FFI = UB；测的是「无 DllNotFoundException
        /// 且返码非失败哨兵」）。这是 harness 的存在意义：真 dll 通。
        /// </summary>
        [Fact]
        public void StageCreatesAndTicks()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Assert.Equal(stage, ctx._stage);   // UIContext 持的就是 Harness 建的那个 stage handle

                uint root = CreateRoot(stage, "div", "");
                Assert.NotEqual(InvalidNodeId, root);

                Native.loomgui_stage_tick((StageHandle*)stage.ToPointer(), 0.016f);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// 兑现 B2 defer：FFI round-trip（live Rust → byte → NodeKind）。
        /// 建 <c>div</c> 根节点 → <c>get_node_kind</c> 取判别值 → 断言 == <see cref="NodeKind.Container"/>。
        /// 若 C# enum 与 Rust <c>#[repr(u8)] NodeKind</c> 判别值漂移 → 此测红（B2 自抄写测试盲区）。
        /// </summary>
        [Fact]
        public void GetNodeKindRoundTrip()
        {
            var (stage, _) = StageHarness.Create();
            try
            {
                uint root = CreateRoot(stage, "div", "");
                Assert.NotEqual(InvalidNodeId, root);

                // Spec-3 ③ return-code + out-param：返 0=ok 且 *out=u8 判别值；非 0=节点不存在或 out null。
                byte kind = 0xFF;
                int rc = Native.loomgui_stage_get_node_kind(
                    (StageHandle*)stage.ToPointer(), root, &kind);

                Assert.Equal(0, rc);                          // FFI 返码 0 = ok
                Assert.Equal(NodeKind.Container, (NodeKind)kind);   // div → Container
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── helpers ──────────────────────────────────────────────────────

        /// <summary>
        /// 调 <c>loomgui_stage_create_root</c>。kind/css 用 UTF-8 字节（fixed 钉住 + ptr+len，
        /// 对齐 <c>LoomStage.cs</c> 风格）。返 NodeId；0xFFFF_FFFF = 失败。
        /// </summary>
        private static uint CreateRoot(IntPtr stage, string kind, string css)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            byte[] k = Encoding.UTF8.GetBytes(kind ?? "");
            byte[] c = Encoding.UTF8.GetBytes(css ?? "");
            fixed (byte* kp = k, cp = c)
                return Native.loomgui_stage_create_root(h, kp, (nuint)k.Length, cp, (nuint)c.Length);
        }
    }
}
