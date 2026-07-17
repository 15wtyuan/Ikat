using System;
using System.Text;
using LoomGUI.Bindings;
using Xunit;

namespace LoomGUI.HeadlessTests
{
    /// <summary>
    /// C3 投影层核心机制验收：NodeStyle 稀疏镜像 + FlushInline seam（即时过桥版）。
    ///
    /// 每条 Fact 验一条不变量：
    /// - getter 读镜像：写过的属性即时读回；未写过返 Unset 哨兵（Length/Color/enum）。
    /// - FlushInline seam：setter 触发 set_inline_override FFI → core rematch → solve，
    ///   下帧 layout_rect / computed_style 反映新值（projection §3.2 即时过桥）。
    /// - Unset 哨兵 setter → unset_inline_override FFI → core bit 清 → 回落 base。
    /// - Color 8-hex round-trip（#rrggbbaa）：验 parse_color fix（commit 4aa8b3c）+ seam 全链通。
    /// - Style 同一 Node 多次访问返同一实例（projection §2.5 稳定单一实例）。
    ///
    /// 全部经 headless harness P/Invoke 真 dll，不启 Unity。
    /// </summary>
    public unsafe class NodeStyleMirrorTests
    {
        // lib.rs create_root 失败哨兵（与 parent 哨兵同值）。
        private const uint InvalidNodeId = 0xFFFF_FFFFu;

        // ── 镜像读回（不依赖 tick）──────────────────────────────────────

        /// <summary>
        /// setter 写 Width=100px → getter 立即读回 100px（mirror 即时反映，不等 tick）。
        /// 验 Set 路径 + Get 路径 + Length 的 typed 往返。
        /// </summary>
        [Fact]
        public void StyleWriteReadsBackFromMirror()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                n.Style.Width = Length.Px(100);

                Assert.Equal(Length.Px(100), n.Style.Width);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// 未写过的 Length 属性 getter 返 Length.Unset()（frozen 契约：getter 只反映写过的）。
        /// </summary>
        [Fact]
        public void StyleUnwrittenLengthReturnsUnset()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Assert.Equal(LengthUnit.Unset, n.Style.Width.Unit);
                Assert.Equal(LengthUnit.Unset, n.Style.Height.Unit);
                Assert.Equal(LengthUnit.Unset, n.Style.MinWidth.Unit);
                Assert.Equal(LengthUnit.Unset, n.Style.Gap.Unit);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// 未写过的 Color / enum 属性 getter 返 Unset 哨兵。
        /// </summary>
        [Fact]
        public void StyleUnwrittenColorAndEnumReturnUnset()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Assert.True(n.Style.BackgroundColor.IsUnset);
                Assert.True(n.Style.Color.IsUnset);
                Assert.Equal(DisplayMode.Unset, n.Style.Display);
                Assert.Equal(FlexDirection.Unset, n.Style.FlexDirection);
                Assert.Equal(Overflow.Unset, n.Style.OverflowX);
                Assert.Equal(PositionMode.Unset, n.Style.Position);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// enum setter 写入后 getter 读回同值（FlexDirection.Column 往返）。
        /// </summary>
        [Fact]
        public void StyleEnumRoundTrips()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                n.Style.FlexDirection = FlexDirection.RowReverse;
                Assert.Equal(FlexDirection.RowReverse, n.Style.FlexDirection);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── FlushInline seam：下帧 layout/style 反映 ─────────────────────

        /// <summary>
        /// setter 触发 FlushInline → set_inline_override FFI → core rematch → solve，
        /// 下帧 layout_rect.w 反映 Width=100px（seam 全链通）。挂在 root 的子 div 上：
        /// root 占满 viewport（1280x720）不受 inline 改（root layout 强制 viewport）；子 div 的
        /// inline width:100px 经 solve 生效 → layout_rect.w == 100。
        /// </summary>
        [Fact]
        public void StyleFlushesAndLayoutReflects()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node child = AppendChildDiv(stage, ctx);
                child.Style.Width = Length.Px(100);
                child.Style.Height = Length.Px(50);

                Tick(stage);
                var (_, _, w, h) = GetLayoutRect(stage, child._id);
                Assert.InRange(w, 99, 101);
                Assert.InRange(h, 49, 51);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Unset 哨兵 setter → unset_inline_override FFI 清 bit → 下帧 layout 回落 base。
        /// 验 StyleMirror.Set 的 Unset 哨兵分支（Length.Unset() → Unset(prop)）+ core bit 清路径。
        /// 子 div 设 width:100px 后 unset → 回落 auto（默认 flex column 下 auto 收缩到内容 0）。
        /// </summary>
        [Fact]
        public void StyleUnsetFallsBack()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node child = AppendChildDiv(stage, ctx);
                child.Style.Width = Length.Px(100);
                Tick(stage);
                var (_, _, wSet, _) = GetLayoutRect(stage, child._id);
                Assert.InRange(wSet, 99, 101);

                child.Style.Width = Length.Unset();   // Unset 哨兵 → 走 unset_inline_override
                Tick(stage);
                var (_, _, wUnset, _) = GetLayoutRect(stage, child._id);
                // auto width 在无内容时回落 0（不再是 100）。
                Assert.True(wUnset < 99 || wUnset > 101, $"unset 后 w={wUnset} 该不再为 100");
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Color 8-hex round-trip：BackgroundColor=red(1,0,0,1) → CssValueConvert 出 #ff0000ff →
        /// core parse_color（8-hex L4 fix）接受 → core rematch 应用 → get_node_computed_style 读回
        /// bg_present=1 且 background_color==[1,0,0,1]。验 parse_color fix（C3 前置 commit 4aa8b3c）
        /// + FlushInline seam 全链通。
        /// </summary>
        [Fact]
        public void StyleColorRoundTrips8Hex()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                n.Style.BackgroundColor = new Color(1f, 0f, 0f, 1f);   // → #ff0000ff
                Tick(stage);

                var cs = GetComputedStyle(stage, n._id);
                Assert.Equal(1, cs.bg_present);
                Assert.Equal(1f, cs.background_color[0], 3);
                Assert.Equal(0f, cs.background_color[1], 3);
                Assert.Equal(0f, cs.background_color[2], 3);
                Assert.Equal(1f, cs.background_color[3], 3);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Color 半透明 round-trip：BackgroundColor=(1,1,1,0.5) → #ffffff80 → 解析回 (1,1,1,~0.5)。
        /// 验 alpha 通道经 8-hex 不丢（6-hex 会强 opaque）。
        /// </summary>
        [Fact]
        public void StyleColorRoundTripsAlpha()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                n.Style.BackgroundColor = new Color(1f, 1f, 1f, 0.5f);
                Tick(stage);

                var cs = GetComputedStyle(stage, n._id);
                Assert.Equal(1, cs.bg_present);
                Assert.Equal(1f, cs.background_color[0], 3);
                Assert.Equal(1f, cs.background_color[1], 3);
                Assert.Equal(1f, cs.background_color[2], 3);
                // 0.5 * 255 = 127.5 → MidpointRounding.AwayFromZero → 128 → 128/255 ≈ 0.502
                Assert.InRange(cs.background_color[3], 0.49f, 0.51f);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Color Unset 哨兵 setter → unset → 下帧 computed_style.bg_present=0（回落 base 无 bg）。
        /// 验 Color 的 Unset 哨兵分支（Color.IsUnset → Unset(prop)）。
        /// </summary>
        [Fact]
        public void StyleColorUnsetClearsBackground()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                n.Style.BackgroundColor = new Color(1f, 0f, 0f, 1f);
                Tick(stage);
                Assert.Equal(1, GetComputedStyle(stage, n._id).bg_present);

                n.Style.BackgroundColor = Color.Unset;   // IsUnset=true → Unset(prop)
                Tick(stage);
                Assert.Equal(0, GetComputedStyle(stage, n._id).bg_present);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Node.Style 多次访问返同一实例（projection §2.5：node.Style.Width=X 与 .Height=Y 改同一 mirror）。
        /// 若每次返新实例则 mirror 状态丢失。
        /// </summary>
        [Fact]
        public void StyleReturnsSameInstance()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Assert.Same(n.Style, n.Style);

                // 两次写不同属性经同一 mirror：都能读回（证明同一 StyleMirror 实例）。
                n.Style.Width = Length.Px(100);
                n.Style.Height = Length.Px(200);
                Assert.Equal(Length.Px(100), n.Style.Width);
                Assert.Equal(Length.Px(200), n.Style.Height);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Style 在 Node Dispose 后访问抛 ObjectDisposedException（C1 ThrowIfDisposed 套到 Style 入口）。
        /// </summary>
        [Fact]
        public void StylePostDisposeThrowsObjectDisposed()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                n.Dispose();
                Assert.Throws<ObjectDisposedException>(() => { var _ = n.Style; });
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Thickness setter 写入后 getter 读回同值（Padding 四边往返）。Thickness 无 Unset 哨兵——
        /// 未写过返 default（全 0），不阻碍写真实值。
        /// </summary>
        [Fact]
        public void StyleThicknessRoundTrips()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                var pad = new Thickness(left: 10, top: 20, right: 30, bottom: 40);
                n.Style.Padding = pad;
                Assert.Equal(pad, n.Style.Padding);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// 多属性并存：Width + BackgroundColor + Opacity 同节点写入，互不干扰。验镜像多 key 共存。
        /// </summary>
        [Fact]
        public void StyleMultiplePropsCoexist()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                n.Style.Width = Length.Px(100);
                n.Style.BackgroundColor = new Color(0f, 1f, 0f, 1f);
                n.Style.Opacity = 0.5f;

                Assert.Equal(Length.Px(100), n.Style.Width);
                Assert.True(n.Style.BackgroundColor.G > 0.99f);
                Assert.Equal(0.5f, n.Style.Opacity);

                Tick(stage);
                var cs = GetComputedStyle(stage, n._id);
                Assert.Equal(0f, cs.background_color[0], 3);
                Assert.Equal(1f, cs.background_color[1], 3);
                Assert.Equal(0f, cs.background_color[2], 3);
                Assert.Equal(0.5f, cs.opacity);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── ponytail defer：未实现的 prop throw NE（不静默丢）──────────────

        /// <summary>
        /// ZIndex/Visibility/SetVar/RemoveVar 在 core apply_decl 未实现（不在 inline_bit 表），
        /// 调用必须抛 NotImplementedException（ponytail defer 显式失败，不静默丢）。
        /// </summary>
        [Theory]
        [InlineData(0)]
        [InlineData(1)]
        public void StyleDeferredPropsThrow(int which)
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                switch (which)
                {
                    case 0: Assert.Throws<NotImplementedException>(() => n.Style.ZIndex = 5); break;
                    case 1: Assert.Throws<NotImplementedException>(() => n.Style.Visibility = Visibility.Hidden); break;
                }
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── helpers ──────────────────────────────────────────────────────

        private static uint CreateRoot(IntPtr stage, string kind)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            byte[] k = Encoding.UTF8.GetBytes(kind);
            fixed (byte* kp = k)
                return Native.loomgui_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        /// <summary>
        /// 建 root div + 子 div（append），返子节点的 typed wrapper。子 div 用来测 inline override
        /// 真实影响 layout（root 的 layout_rect 强制 viewport，inline 改 root 宽无效）。
        /// </summary>
        private static Node AppendChildDiv(IntPtr stage, UIContext ctx)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            uint root = CreateRoot(stage, "div");

            byte[] k = Encoding.UTF8.GetBytes("div");
            uint child;
            fixed (byte* kp = k)
                child = Native.loomgui_stage_create_node(h, kp, (nuint)k.Length, null, 0);
            if (child == InvalidNodeId)
                throw new InvalidOperationException("create_node(div) failed");

            int rc = Native.loomgui_stage_append_child(h, root, child);
            if (rc != 0)
                throw new InvalidOperationException($"append_child(parent={root}, child={child}) failed rc={rc}");

            return ctx._registry.GetOrCreate(child);
        }

        private static void Tick(IntPtr stage) =>
            Native.loomgui_stage_tick((StageHandle*)stage.ToPointer(), 0.016f);

        private static (float x, float y, float w, float h) GetLayoutRect(IntPtr stage, uint id)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            float x = 0, y = 0, w = 0, hh = 0;
            Native.loomgui_stage_get_node_layout_rect(h, id, &x, &y, &w, &hh);
            return (x, y, w, hh);
        }

        private static ComputedNodeStyleRepr GetComputedStyle(IntPtr stage, uint id)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            ComputedNodeStyleRepr repr;
            int rc = Native.loomgui_stage_get_node_computed_style(h, id, &repr);
            if (rc != 0)
                throw new InvalidOperationException($"get_node_computed_style(id={id}) failed rc={rc}");
            return repr;
        }
    }
}
