using System;
using System.Text;
using LoomGUI.Bindings;
using Xunit;

namespace LoomGUI.HeadlessTests
{
    /// <summary>
    /// E1: UIContext / UIPackage / UITemplate method bodies (TDD).
    ///
    /// Cover Create&lt;T&gt; whitelist (Container ok, Button/Slider throw UIContractException),
    /// Root (create_root + _rootId), FocusedNode (FFI round-trip), IsPointerOnUI (FFI).
    ///
    /// LoadPackage / Instantiate end-to-end tests deferred to E2/E3 (fixture pkg.bin dependency).
    /// UnloadPackage / Pick / CallLater / CallNextFrame deferred (no FFI).
    /// </summary>
    public unsafe class UiContextCreationTests
    {
        const uint RootSentinel = 0xFFFF_FFFFu;

        // ── helpers ────────────────────────────────────────────────────

        /// <summary>
        /// 调 create_root FFI 建根节点 + 注册到 UIContext._rootId。
        /// 返回 typed Container（registry 缓存 + _rootId 已设）。
        /// </summary>
        static Container InitRoot(UIContext ctx, string kind = "div", string css = "")
        {
            IntPtr stage = ctx._stage;
            StageHandle* h = (StageHandle*)stage.ToPointer();
            byte[] k = Encoding.UTF8.GetBytes(kind);
            byte[] c = Encoding.UTF8.GetBytes(css);
            uint id;
            fixed (byte* kp = k, cp = c)
                id = Native.loomgui_stage_create_root(h, kp, (nuint)k.Length, cp, (nuint)c.Length);
            if (id == RootSentinel)
                throw new InvalidOperationException($"create_root(\"{kind}\") failed");
            ctx._rootId = id;
            return (Container)ctx._registry.GetOrCreate(id);
        }

        /// <summary>
        /// 调 create_root FFI（low-level：返回 raw NodeId，不设 _rootId）。
        /// FocusedNodeAfterRequestFocus 等需要 tick 的测试用——先建 root 建 scene，再 focus。
        /// </summary>
        static uint CreateRootFFI(StageHandle* h, string kind, string css)
        {
            byte[] k = Encoding.UTF8.GetBytes(kind ?? "");
            byte[] c = Encoding.UTF8.GetBytes(css ?? "");
            fixed (byte* kp = k, cp = c)
                return Native.loomgui_stage_create_root(h, kp, (nuint)k.Length, cp, (nuint)c.Length);
        }

        // ── Create<T> ──────────────────────────────────────────────────

        [Fact]
        public void CreateContainerWhitelist()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                var c = ctx.Create<Container>();
                Assert.IsType<Container>(c);
                Assert.NotEqual(RootSentinel, c._id);
                Assert.False(c._disposed);
                // NodeId 必须是活的——get_node_kind 返 Container(0) 而非 0xFF。
                StageHandle* h = (StageHandle*)stage.ToPointer();
                byte kind = 0xFF;
                int rc = Native.loomgui_stage_get_node_kind(h, c._id, &kind);
                Assert.Equal(0, rc);
                Assert.Equal(0, kind);   // NodeKind::Container = 0
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void CreateAbsolutePanelWhitelist()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                var p = ctx.Create<AbsolutePanel>();
                Assert.IsType<AbsolutePanel>(p);
                Assert.NotEqual(RootSentinel, p._id);
                // AbsolutePanel kind = Container (同 div)
                StageHandle* h = (StageHandle*)stage.ToPointer();
                byte kind = 0xFF;
                int rc = Native.loomgui_stage_get_node_kind(h, p._id, &kind);
                Assert.Equal(0, rc);
                Assert.Equal(0, kind);   // Container
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void CreateTextNodeWhitelist()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                var tn = ctx.Create<TextNode>();
                Assert.IsType<TextNode>(tn);
                Assert.NotEqual(RootSentinel, tn._id);
                // TextNode kind = 1
                StageHandle* h = (StageHandle*)stage.ToPointer();
                byte kind = 0xFF;
                int rc = Native.loomgui_stage_get_node_kind(h, tn._id, &kind);
                Assert.Equal(0, rc);
                Assert.Equal(1, kind);   // TextNode
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void CreateImageWhitelist()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                var img = ctx.Create<Image>();
                Assert.IsType<Image>(img);
                Assert.NotEqual(RootSentinel, img._id);
                // Image kind = 8（Rust NodeKind 枚举序：Container=0, TextNode=1, ..., Image=8）
                StageHandle* h = (StageHandle*)stage.ToPointer();
                byte kind = 0xFF;
                int rc = Native.loomgui_stage_get_node_kind(h, img._id, &kind);
                Assert.Equal(0, rc);
                Assert.Equal(8, kind);   // Image
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void CreateRejectsButton()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                var ex = Assert.Throws<UIContractException>(() => ctx.Create<Button>());
                Assert.Contains("Button", ex.Message);
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void CreateRejectsSlider()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                var ex = Assert.Throws<UIContractException>(() => ctx.Create<Slider>());
                Assert.Contains("Slider", ex.Message);
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void CreateRejectsToggle()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Assert.Throws<UIContractException>(() => ctx.Create<Toggle>());
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void CreateRejectsListView()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Assert.Throws<UIContractException>(() => ctx.Create<ListView>());
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void CreateRejectsDropdown()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Assert.Throws<UIContractException>(() => ctx.Create<Dropdown>());
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void CreateRejectsProgressBar()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Assert.Throws<UIContractException>(() => ctx.Create<ProgressBar>());
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── Root ───────────────────────────────────────────────────────

        [Fact]
        public void RootBeforeCreateRootThrows()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                // _rootId still RootSentinel (no create_root called yet)
                Assert.Throws<InvalidOperationException>(() => _ = ctx.Root);
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void RootAfterCreateRootReturnsContainer()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container r = InitRoot(ctx);
                Container root = ctx.Root;
                Assert.Same(r, root);   // identity stable: same wrapper instance
                Assert.IsType<Container>(root);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── FocusedNode ────────────────────────────────────────────────

        [Fact]
        public void FocusedNodeInitiallyNull()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                _ = InitRoot(ctx);
                Assert.Null(ctx.FocusedNode);   // no focus requested yet
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void FocusedNodeAfterRequestFocus()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                // request_focus writes pending_focus_request；tick 消费它 → scene.focused_node。
                // focused_node FFI 读 scene.focused_node，不是 pending——故需先 tick。
                StageHandle* h = (StageHandle*)stage.ToPointer();
                uint rootId = CreateRootFFI(h, "div", "");
                ctx._rootId = rootId;
                var c = ctx.Create<Container>();

                Native.loomgui_stage_request_focus(h, c._id);
                Native.loomgui_stage_tick(h, 0.016f);   // consume pending_focus_request

                Node f = ctx.FocusedNode;
                Assert.NotNull(f);
                Assert.Equal(c._id, f._id);
                Assert.Same(c, f);   // identity stable
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── IsPointerOnUI ─────────────────────────────────────────────

        [Fact]
        public void IsPointerOnUiInitiallyFalse()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                _ = InitRoot(ctx);
                // No pointer input fed yet → false
                Assert.False(ctx.IsPointerOnUI);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── StyleSheet ─────────────────────────────────────────────────

        [Fact]
        public void StyleSheetReturnsSameInstance()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                var s1 = ctx.StyleSheet;
                var s2 = ctx.StyleSheet;
                Assert.Same(s1, s2);   // lazy, single instance
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void StyleSheetAddThrowsNe()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                var ss = ctx.StyleSheet;
                Assert.Throws<NotImplementedException>(() => ss.Add("div { color: red; }"));
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void StyleSheetClearThrowsNe()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                var ss = ctx.StyleSheet;
                Assert.Throws<NotImplementedException>(() => ss.Clear());
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── LoadPackage (duplicate) ────────────────────────────────────

        [Fact]
        public void LoadPackageNullNameThrows()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                _ = InitRoot(ctx);
                Assert.Throws<ArgumentNullException>(() => ctx.LoadPackage(null, new byte[] { 1 }));
                Assert.Throws<ArgumentNullException>(() => ctx.LoadPackage("", new byte[] { 1 }));
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void LoadPackageNullBytesThrows()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                _ = InitRoot(ctx);
                Assert.Throws<ArgumentNullException>(() => ctx.LoadPackage("test", null));
                Assert.Throws<ArgumentNullException>(() => ctx.LoadPackage("test", Array.Empty<byte>()));
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── UIPackage / UITemplate basics ──────────────────────────────

        [Fact]
        public void UIPackageNameRoundTrip()
        {
            // UIPackage is internal-ctor only, but we can test Name via LoadPackage mock.
            // Since LoadPackage end-to-end needs fixture, test the ctor path directly:
            // UIPackage's ctor is internal but we're in HeadlessTests (different assembly).
            // Ponytail: test Name/GetTemplate via the getter pattern — we need a UIPackage instance
            // but ctor is internal. Skip for now — E2 fixture will cover this path.
        }

        // ── Deferred methods ───────────────────────────────────────────

        [Fact]
        public void UnloadPackageThrowsNe()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Assert.Throws<NotImplementedException>(() => ctx.UnloadPackage("foo"));
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void PickThrowsNe()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Assert.Throws<NotImplementedException>(() => ctx.Pick(new Vector2(100, 100)));
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void CallLaterThrowsNe()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Assert.Throws<NotImplementedException>(() => ctx.CallLater(1f, () => { }));
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void CallNextFrameThrowsNe()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Assert.Throws<NotImplementedException>(() => ctx.CallNextFrame(() => { }));
            }
            finally { StageHarness.Destroy(stage); }
        }
    }
}
