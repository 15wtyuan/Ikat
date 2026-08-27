using System;
using System.IO;
using System.Text;
using Ikat.Bindings;
using Xunit;

namespace Ikat.HeadlessTests
{
    /// <summary>
    /// P1 控件投影层验收：ProgressBar/Toggle/Slider 经 FFI 填实的属性 + 控件事件 demux。
    ///
    /// 控件节点的 ControlState（value/max/checked/...）是打包期产物（pkg.bin 经 create_node_from_template
    /// + ControlInit 注入 scene.controls side table），运行时无 control_init setter FFI——故用预构建的
    /// controls.pkg.bin fixture（含 progress/slider/toggle/radio）经 LoadPackage+Instantiate 拿到带
    /// ControlState 的控件节点，而非 create_root（create_root 不产 ControlState）。
    ///
    /// 事件测试（ValueChanged/CheckedChanged）不需要 ControlState——demux 经 NativeEventBuffer 造 raw
    /// EVT_* EventRecord 直驱 EventBus，与 control side table 无关。
    ///
    /// 全部经 headless harness P/Invoke 真 dll，不启 Unity。
    /// </summary>
    public unsafe class ControlProjectionTests
    {
        private const ulong RootSentinel = ulong.MaxValue;

        // ── 属性 FFI round-trip ──────────────────────────────────────────

        /// <summary>
        /// ProgressBar.Value set 90 → get 90（FFI set/get_control_value round-trip；90&lt;max=100 不 clamp）。
        /// fixture 的 prog 节点 value=40/max=100（打包期 ControlInit::Progress），验 setter 覆盖成功。
        /// </summary>
        [Fact]
        public void progress_value_roundtrips_via_ffi()
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                var prog = root.Get<ProgressBar>("prog");
                Assert.Equal(40f, prog.Value);   // 打包期初值（value="40"）

                prog.Value = 90f;
                Assert.Equal(90f, prog.Value);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// ProgressBar.Max round-trip + Value clamp：set Max=50 后原 Value(90) clamp 到 50。
        /// 验 FFI set_control_max 改区间 + get_control_value 反映 clamp 后值（core clamp 语义）。
        /// </summary>
        [Fact]
        public void progress_max_clamps_value()
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                var prog = root.Get<ProgressBar>("prog");
                prog.Value = 90f;
                Assert.Equal(90f, prog.Value);

                prog.Max = 50f;          // 缩小区间 → core 重 clamp value 到 [0,50]
                Assert.Equal(50f, prog.Max);
                Assert.Equal(50f, prog.Value);   // 90 被 clamp 到新 max
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Slider.Value/Min/Max/Step round-trip + step 量化：fixture step=5，set 83 → 量化 85。
        /// 验 FFI set/get_control_value·min·max·step 全链通 + core 量化语义（round 到最近 step）。
        /// </summary>
        [Fact]
        public void slider_value_roundtrips_and_quantizes()
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                var sld = root.Get<Slider>("sld");
                Assert.Equal(0f, sld.Min);
                Assert.Equal(100f, sld.Max);
                Assert.Equal(5f, sld.Step);

                sld.Value = 83f;                 // step=5 → 量化到 85
                Assert.Equal(85f, sld.Value, 1);  // 容差 1（浮点量化误差）
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// NumberField.Value clamp+量化：fixture min=0 max=10 step=2。set 15 → clamp 到 max=10；
        /// set 3 → 量化到 2（round 到最近 step）。验 FFI set/get_number_value round-trip（core 侧 clamp+量化）。
        /// </summary>
        [Fact]
        public void numberfield_value_clamps_and_quantizes()
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                var nf = root.Get<NumberField>("nf");

                // clamp 到 max=10（fixture max=10）。
                nf.Value = 15f;
                Assert.Equal(10f, nf.Value, 0.01f);

                // 量化到最近 step=2（3 → round((3-0)/2)*2+0 = 2*2 = 4 → 取偶数 step 端：round(1.5)=2 → 4）。
                nf.Value = 3f;
                Assert.Equal(4f, nf.Value, 0.01f);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// NumberField.Min/Max/Step 读回打包期烘焙的约束值：fixture nf min=0 max=10 step=2。
        /// 验 FFI get_control_min/max/step 已扩到 NumberField（c55389d，原只 match Slider 返 -1）。
        /// 三者打包期冻结、运行时不可变——C# 只读 getter，与 Slider 同 get+set 形状但 setter throw NE。
        /// </summary>
        [Fact]
        public void numberfield_min_max_step_read_baked_values()
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                var nf = root.Get<NumberField>("nf");

                Assert.Equal(0f, nf.Min);
                Assert.Equal(10f, nf.Max);
                Assert.Equal(2f, nf.Step);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// NumberField.ValueChanged 经 demux 触发：NativeEventBuffer 喂 EVT_VALUE_CHANGED(x=7)
        /// → demux → ControlValueChangedEvent → 翻译为 ValueChangedEvent&lt;float&gt;，handler 收到 NewValue≈7。
        /// 与 Slider.ValueChanged 同 demux 分支（22），backing-dict 模式相同。
        /// </summary>
        [Fact]
        public void numberfield_value_changed_raises_via_demux()
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                var nf = root.Get<NumberField>("nf");
                ValueChangedEvent<float> received = default;
                nf.ValueChanged += e => received = e;

                using (var buf = new NativeEventBuffer())
                {
                    // x=7 → 新 float 值（EVT_VALUE_CHANGED = 22）。
                    buf.Add(nf._id, (byte)EventType.ValueChanged, x: 7f);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.Equal(7f, received.NewValue, 2);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Toggle.IsChecked round-trip：set true → get true；set false → get false。
        /// 验 FFI set/get_control_checked 全链通（bool* out 经 local + &local）。
        /// </summary>
        [Fact]
        public void toggle_ischecked_roundtrips_via_ffi()
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                var tggl = root.Get<Toggle>("tggl");
                Assert.False(tggl.IsChecked);   // 无 checked 属性 → 打包期 false

                tggl.IsChecked = true;
                Assert.True(tggl.IsChecked);
                tggl.IsChecked = false;
                Assert.False(tggl.IsChecked);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── 控件事件 demux ───────────────────────────────────────────────

        /// <summary>
        /// Toggle.CheckedChanged 经 demux 触发：NativeEventBuffer 喂 EVT_CHECKED_CHANGED(pad[0]=1)
        /// → demux → ControlCheckedChangedEvent → 翻译为 ValueChangedEvent<bool>，handler 收到 NewValue=true。
        /// 验 demux 分支 23 + 控件事件 backing-dict（同 Button.Clicked 模式）全链通。
        /// </summary>
        [Fact]
        public void toggle_check_changed_raises_via_demux()
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                var tggl = root.Get<Toggle>("tggl");
                ValueChangedEvent<bool> received = default;
                tggl.CheckedChanged += e => received = e;

                using (var buf = new NativeEventBuffer())
                {
                    // pad[0]=1 → checked=true（EVT_CHECKED_CHANGED = 23）
                    buf.Add(tggl._id, (byte)EventType.CheckedChanged, pad: 1);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.True(received.NewValue, "CheckedChanged handler 应收到 NewValue=true");
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Slider.ValueChanged 经 demux 触发：NativeEventBuffer 喂 EVT_VALUE_CHANGED(x=42)
        /// → demux → ControlValueChangedEvent → 翻译为 ValueChangedEvent<float>，handler 收到 NewValue≈42。
        /// 验 demux 分支 22 + 控件事件 backing-dict 全链通。
        /// </summary>
        [Fact]
        public void slider_value_changed_raises_via_demux()
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                var sld = root.Get<Slider>("sld");
                ValueChangedEvent<float> received = default;
                sld.ValueChanged += e => received = e;

                using (var buf = new NativeEventBuffer())
                {
                    // x=42 → 新 float 值（EVT_VALUE_CHANGED = 22）
                    buf.Add(sld._id, (byte)EventType.ValueChanged, x: 42f);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.Equal(42f, received.NewValue, 2);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Slider.ChangeCommitted 经 demux 触发：NativeEventBuffer 喂 EVT_CHANGE_COMMITTED(x=77)
        /// → demux → ControlChangeCommittedEvent → Action<float> handler 收到 77。
        /// 验 demux 分支 24 + ChangeCommitted backing-dict（Action<float> 直给终值）全链通。
        /// </summary>
        [Fact]
        public void slider_change_committed_raises_via_demux()
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                var sld = root.Get<Slider>("sld");
                float committed = -1f;
                sld.ChangeCommitted += v => committed = v;

                using (var buf = new NativeEventBuffer())
                {
                    // x=77 → 终值（EVT_CHANGE_COMMITTED = 24）
                    buf.Add(sld._id, (byte)EventType.ChangeCommitted, x: 77f);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.Equal(77f, committed, 2);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// 控件事件退订：-= handler 后 demux 不再触发。验 backing-dict remove 路径（同 Button.Clicked）。
        /// </summary>
        [Fact]
        public void slider_value_changed_remove_unsubscribes()
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                var sld = root.Get<Slider>("sld");
                int count = 0;
                Action<ValueChangedEvent<float>> handler = _ => count++;
                sld.ValueChanged += handler;
                sld.ValueChanged -= handler;

                using (var buf = new NativeEventBuffer())
                {
                    buf.Add(sld._id, (byte)EventType.ValueChanged, x: 1f);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.Equal(0, count);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── fixture 加载 helper ──────────────────────────────────────────

        /// <summary>
        /// 加载 controls.pkg.bin → Instantiate → AppendChild 到 scene root → tick（cascade+solve）。
        /// 返回 (stage, ctx, instRoot)——调用方 finally Destroy(stage)。
        /// </summary>
        static (IntPtr stage, UIContext ctx, Container root) LoadControlsFixture()
        {
            var (stage, ctx) = StageHarness.Create();
            StageHandle* h = (StageHandle*)stage.ToPointer();

            string fontPath = Path.Combine(AppContext.BaseDirectory, "fixtures", "fonts", "DejaVuSans.ttf");
            if (File.Exists(fontPath))
                RegisterFont(h, fontPath);

            ulong sceneRootId = CreateRoot(h, "div");
            ctx._rootId = sceneRootId;
            Container sceneRoot = (Container)ctx._registry.GetOrCreate(sceneRootId);

            string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "controls.pkg.bin");
            Assert.True(File.Exists(fixturePath),
                $"fixture controls.pkg.bin not found at {fixturePath}");

            byte[] pkgBytes = File.ReadAllBytes(fixturePath);
            UIPackage pkg = ctx.LoadPackage("controls", pkgBytes);
            Container instRoot = pkg.Instantiate("controls");
            AppendChild(h, sceneRoot._id, instRoot._id);
            Tick(h);   // cascade + solve（控件 inline style display:block + 尺寸）
            return (stage, ctx, instRoot);
        }

        static void RegisterFont(StageHandle* h, string fontPath)
        {
            byte[] fontBytes = File.ReadAllBytes(fontPath);
            byte[] family = Encoding.UTF8.GetBytes("DejaVuSans");
            fixed (byte* fp = family)
            fixed (byte* bp = fontBytes)
            {
                Native.ikat_stage_register_font(
                    h, fp, (nuint)family.Length, bp, (nuint)fontBytes.Length, is_default: 1);
            }
        }

        static ulong CreateRoot(StageHandle* h, string kind)
        {
            byte[] k = Encoding.UTF8.GetBytes(kind);
            fixed (byte* kp = k)
                return Native.ikat_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        static void AppendChild(StageHandle* h, ulong parent, ulong child)
        {
            int rc = Native.ikat_stage_append_child(h, parent, child);
            if (rc != 0)
                throw new InvalidOperationException($"append_child(parent={parent}, child={child}) failed rc={rc}");
        }

        static void Tick(StageHandle* h) => Native.ikat_stage_tick(h, 0.016f);
    }
}
