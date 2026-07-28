using System;
using System.IO;
using System.Text;
using LoomGUI.Bindings;
using Xunit;

namespace LoomGUI.HeadlessTests
{
    /// <summary>
    /// Task 14 验收：Dropdown 投影层填实（SelectedIndex FFI round-trip + SelectionChanged 事件 demux）。
    ///
    /// fixture：dropdown.pkg.bin（select#sel 含 3 option：Alpha/Beta-selected/Gamma-disabled），
    /// 经 LoadPackage+Instantiate 拿到带 ControlState::Dropdown 的 select 节点（selected_index 烘焙于
    /// 打包期 ControlInit::Dropdown）。事件测试（SelectionChanged）不需 ControlState——demux 经
    /// NativeEventBuffer 造 raw EVT_SELECTION_CHANGED EventRecord 直驱 EventBus。
    ///
    /// 全部经 headless harness P/Invoke 真 dll，不启 Unity。
    /// </summary>
    public unsafe class DropdownProjectionTests
    {
        // ── SelectedIndex FFI round-trip ────────────────────────────────

        /// <summary>
        /// Dropdown.SelectedIndex round-trip：fixture opt-b selected（index=1）→ set 2 → get 2。
        /// 验 FFI get/set_dropdown_selected_index 全链通（uint* out 经 local + &local）。
        /// </summary>
        [Fact]
        public void dropdown_selected_index_roundtrips_via_ffi()
        {
            var (stage, ctx, root) = LoadDropdownFixture();
            try
            {
                var sel = root.Get<Dropdown>("sel");
                Assert.Equal(1, sel.SelectedIndex);   // 打包期：opt-b selected

                sel.SelectedIndex = 2;
                Assert.Equal(2, sel.SelectedIndex);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Dropdown.Disabled round-trip：set true → get true；set false → get false。
        /// 验 FFI get/set_node_disabled（Dropdown 复用通用 node flag 通道，与 Slider 等一致）。
        /// </summary>
        [Theory]
        [InlineData(true)]
        [InlineData(false)]
        public void dropdown_disabled_roundtrips_via_ffi(bool v)
        {
            var (stage, ctx, root) = LoadDropdownFixture();
            try
            {
                var sel = root.Get<Dropdown>("sel");
                sel.Disabled = v;
                Assert.Equal(v, sel.Disabled);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── SelectionChanged 事件 demux ────────────────────────────────

        /// <summary>
        /// Dropdown.SelectionChanged 经 demux 触发：NativeEventBuffer 喂
        /// EVT_SELECTION_CHANGED(touch_id=2) → demux → ControlSelectionChangedEvent → 翻译为公共
        /// SelectionChangedEvent，handler 收到 NewIndex=2。
        /// 验 demux 分支 26 + 控件事件 backing-dict（同 Slider.ValueChanged）全链通。
        ///
        /// payload 契约：core commit_dropdown_selection（control.rs:422）把新 selected_index 装进
        /// EventRecord.touch_id（i32），NOT x（float）——故 NativeEventBuffer.Add 传 touchId。
        /// </summary>
        [Fact]
        public void dropdown_selection_changed_raises_via_demux()
        {
            var (stage, ctx, root) = LoadDropdownFixture();
            try
            {
                var sel = root.Get<Dropdown>("sel");
                SelectionChangedEvent received = default;
                sel.SelectionChanged += e => received = e;

                using (var buf = new NativeEventBuffer())
                {
                    // touch_id=2 → 新 selected_index（EVT_SELECTION_CHANGED = 26）
                    buf.Add(sel._id, (byte)EventType.SelectionChanged, touchId: 2);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.Equal(2, received.NewIndex);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Dropdown.SelectionChanged 退订：-= handler 后 demux 不再触发。验 backing-dict remove 路径
        /// （同 Slider.ValueChanged / Button.Clicked）。
        /// </summary>
        [Fact]
        public void dropdown_selection_changed_remove_unsubscribes()
        {
            var (stage, ctx, root) = LoadDropdownFixture();
            try
            {
                var sel = root.Get<Dropdown>("sel");
                int count = 0;
                Action<SelectionChangedEvent> handler = _ => count++;
                sel.SelectionChanged += handler;
                sel.SelectionChanged -= handler;

                using (var buf = new NativeEventBuffer())
                {
                    buf.Add(sel._id, (byte)EventType.SelectionChanged, touchId: 2);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.Equal(0, count);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── fixture 加载 helper ──────────────────────────────────────────

        static (IntPtr stage, UIContext ctx, Container root) LoadDropdownFixture()
        {
            var (stage, ctx) = StageHarness.Create();
            StageHandle* h = (StageHandle*)stage.ToPointer();

            string fontPath = Path.Combine(AppContext.BaseDirectory, "fixtures", "fonts", "DejaVuSans.ttf");
            if (File.Exists(fontPath))
                RegisterFont(h, fontPath);

            uint sceneRootId = CreateRoot(h, "div");
            ctx._rootId = sceneRootId;
            Container sceneRoot = (Container)ctx._registry.GetOrCreate(sceneRootId);

            string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "dropdown.pkg.bin");
            Assert.True(File.Exists(fixturePath), $"fixture dropdown.pkg.bin not found at {fixturePath}");

            byte[] pkgBytes = File.ReadAllBytes(fixturePath);
            UIPackage pkg = ctx.LoadPackage("dropdown", pkgBytes);
            Container instRoot = pkg.Instantiate("dropdown");
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

        static uint CreateRoot(StageHandle* h, string kind)
        {
            byte[] k = Encoding.UTF8.GetBytes(kind);
            fixed (byte* kp = k)
                return Native.loomgui_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        static void AppendChild(StageHandle* h, uint parent, uint child)
        {
            int rc = Native.loomgui_stage_append_child(h, parent, child);
            if (rc != 0)
                throw new InvalidOperationException($"append_child(parent={parent}, child={child}) failed rc={rc}");
        }

        static void Tick(StageHandle* h) => Native.loomgui_stage_tick(h, 0.016f);
    }
}
