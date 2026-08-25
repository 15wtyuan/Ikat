using System;
using System.IO;
using System.Text;
using LoomGUI.Bindings;
using Xunit;

namespace LoomGUI.HeadlessTests
{
    /// <summary>
    /// Task 9 验收：TabList/Tab 投影层（NodeFactory 派发 + SelectedIndex FFI round-trip +
    /// SelectionChanged 事件 demux）。
    ///
    /// fixture：tablist.pkg.bin（div role=tablist#tl 含 2 tab：t-a(selected)/t-b + 2 tabpanel），
    /// 经 LoadPackage+Instantiate 拿到带 ControlState::TabList 的 tablist 节点（selected_index 烘焙于
    /// 打包期 aria-selected="true"）。事件测试（SelectionChanged）复用 Dropdown 同源 demux 路径——
    /// NativeEventBuffer 造 raw EVT_SELECTION_CHANGED EventRecord 直驱 EventBus。
    ///
    /// 全部经 headless harness P/Invoke 真 dll，不启 Unity。
    /// </summary>
    public unsafe class TabListProjectionTests
    {
        // ── SelectedIndex FFI round-trip ────────────────────────────────

        /// <summary>
        /// TabList.SelectedIndex round-trip：fixture t-a selected（index=0）→ set 1 → get 1。
        /// 验 FFI get/set_tablist_selected_index 全链通（Task 8 FFI 经 C# 投影）。
        /// </summary>
        [Fact]
        public void tablist_selected_index_roundtrips()
        {
            var (stage, ctx, root) = LoadTabListFixture();
            try
            {
                var tl = root.Get<TabList>("tl");
                Assert.Equal(0, tl.SelectedIndex);   // 打包期：t-a selected

                tl.SelectedIndex = 1;
                Assert.Equal(1, tl.SelectedIndex);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── NodeFactory 派发到专用子类 ────────────────────────────────

        /// <summary>
        /// role=tablist（div）→ TabList 实例；role=tab（button）→ Tab 实例。
        /// 改前若 NodeFactory 未加 arm 会回落 Container（Get&lt;TabList&gt; 抛 not found）。
        /// Assert.IsType 验真实类型非裸 Container。
        /// </summary>
        [Fact]
        public void tablist_factory_dispatches_typed()
        {
            var (stage, ctx, root) = LoadTabListFixture();
            try
            {
                var tl = root.Get<TabList>("tl");
                Assert.IsType<TabList>(tl);

                var tab = root.Get<Tab>("t-a");
                Assert.IsType<Tab>(tab);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── SelectionChanged 事件 demux ────────────────────────────────

        /// <summary>
        /// TabList.SelectionChanged 经 demux 触发：NativeEventBuffer 喂
        /// EVT_SELECTION_CHANGED(touch_id=1) → demux → ControlSelectionChangedEvent → 翻译为公共
        /// SelectionChangedEvent，handler 收到 NewIndex=1。
        /// 验 TabList 复用 Dropdown 同源 backing-dict 模式（同 ControlSelectionChangedEvent）全链通。
        ///
        /// payload 契约：core 把新 selected_index 装进 EventRecord.touch_id（i32，同 Dropdown）——
        /// 故 NativeEventBuffer.Add 传 touchId。
        /// </summary>
        [Fact]
        public void tablist_selection_changed_via_demux()
        {
            var (stage, ctx, root) = LoadTabListFixture();
            try
            {
                var tl = root.Get<TabList>("tl");
                SelectionChangedEvent received = default;
                tl.SelectionChanged += e => received = e;

                using (var buf = new NativeEventBuffer())
                {
                    // touch_id=1 → 新 selected_index（EVT_SELECTION_CHANGED = 26）
                    buf.Add(tl._id, (byte)EventType.SelectionChanged, touchId: 1);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.Equal(1, received.NewIndex);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── fixture 加载 helper ──────────────────────────────────────────

        static (IntPtr stage, UIContext ctx, Container root) LoadTabListFixture()
        {
            var (stage, ctx) = StageHarness.Create();
            StageHandle* h = (StageHandle*)stage.ToPointer();

            string fontPath = Path.Combine(AppContext.BaseDirectory, "fixtures", "fonts", "DejaVuSans.ttf");
            if (File.Exists(fontPath))
                RegisterFont(h, fontPath);

            ulong sceneRootId = CreateRoot(h, "div");
            ctx._rootId = sceneRootId;
            Container sceneRoot = (Container)ctx._registry.GetOrCreate(sceneRootId);

            string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "tablist.pkg.bin");
            Assert.True(File.Exists(fixturePath), $"fixture tablist.pkg.bin not found at {fixturePath}");

            byte[] pkgBytes = File.ReadAllBytes(fixturePath);
            UIPackage pkg = ctx.LoadPackage("tablist", pkgBytes);
            Container instRoot = pkg.Instantiate("tablist");
            AppendChild(h, sceneRoot._id, instRoot._id);
            Tick(h);   // cascade + solve
            return (stage, ctx, instRoot);
        }

        static void RegisterFont(StageHandle* h, string fontPath)
        {
            byte[] fontBytes = File.ReadAllBytes(fontPath);
            byte[] family = Encoding.UTF8.GetBytes("DejaVuSans");
            fixed (byte* fp = family)
            fixed (byte* bp = fontBytes)
            {
                Native.loomgui_stage_register_font(
                    h, fp, (nuint)family.Length, bp, (nuint)fontBytes.Length, is_default: 1);
            }
        }

        static ulong CreateRoot(StageHandle* h, string kind)
        {
            byte[] k = Encoding.UTF8.GetBytes(kind);
            fixed (byte* kp = k)
                return Native.loomgui_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        static void AppendChild(StageHandle* h, ulong parent, ulong child)
        {
            int rc = Native.loomgui_stage_append_child(h, parent, child);
            if (rc != 0)
                throw new InvalidOperationException($"append_child(parent={parent}, child={child}) failed rc={rc}");
        }

        static void Tick(StageHandle* h) => Native.loomgui_stage_tick(h, 0.016f);
    }
}
