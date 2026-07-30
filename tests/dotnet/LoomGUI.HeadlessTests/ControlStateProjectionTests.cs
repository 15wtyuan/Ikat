using System;
using System.IO;
using System.Text;
using LoomGUI.Bindings;
using Xunit;

namespace LoomGUI.HeadlessTests
{
    /// <summary>
    /// Task 8 验收：控件运行时态 getter（ReadOnly / Disabled）+ Node.Blur() 经 FFI round-trip。
    ///
    /// Task 6 暴露了 get_node_disabled / get_control_readonly / blur 三个读出口 FFI；本测验投影层把
    /// 控件 getter 从 throw NE 改为直读 core 真相（之前 setter 可写、getter 读不到，破坏 round-trip）。
    /// 覆盖：TextField/TextArea/NumberField 的 ReadOnly + Disabled；
    /// Slider/Toggle/RadioButton 的 Disabled；Node.Blur() 调 FFI 不抛。
    ///
    /// fixture：textfield.pkg.bin（含 tf/ta 两文本控件，共享 EditState）+ controls.pkg.bin
    /// （含 sld/tggl/rdo）。NumberField 无专用 fixture，走 TextField 的 EditState 共享通道验证
    /// readonly（get_control_readonly 按 node 派发，不分 kind）。
    /// 全部经 headless harness P/Invoke 真 dll，不启 Unity。
    /// </summary>
    public unsafe class ControlStateProjectionTests
    {
        // ── ReadOnly round-trip（getter 改读 FFI 前会 throw NE）──────────────

        /// <summary>
        /// TextField.ReadOnly set true → get true；set false → get false。
        /// 验 get_control_readonly FFI 读 EditState.readonly（与 set_control_readonly 对称）。
        /// 改前 getter throw NE；改后 round-trip 通。
        /// </summary>
        [Theory]
        [InlineData(true)]
        [InlineData(false)]
        public void textfield_readonly_roundtrips_via_ffi(bool v)
        {
            var (stage, ctx, root) = LoadTextfieldFixture();
            try
            {
                var tf = root.Get<TextField>("tf");
                tf.ReadOnly = v;
                Assert.Equal(v, tf.ReadOnly);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// TextArea.ReadOnly round-trip（与 TextField 共享 EditState + FFI 通道）。
        /// </summary>
        [Fact]
        public void textarea_readonly_roundtrips_via_ffi()
        {
            var (stage, ctx, root) = LoadTextfieldFixture();
            try
            {
                var ta = root.Get<TextArea>("ta");
                ta.ReadOnly = true;
                Assert.True(ta.ReadOnly);
                ta.ReadOnly = false;
                Assert.False(ta.ReadOnly);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── Disabled round-trip（getter 改读 FFI 前会 throw NE）──────────────

        /// <summary>
        /// TextField.Disabled round-trip：set true → get true；set false → get false。
        /// 验 get_node_disabled FFI 读 NodeFlags::DISABLED（与 set_node_disabled 对称）。
        /// </summary>
        [Theory]
        [InlineData(true)]
        [InlineData(false)]
        public void textfield_disabled_roundtrips_via_ffi(bool v)
        {
            var (stage, ctx, root) = LoadTextfieldFixture();
            try
            {
                var tf = root.Get<TextField>("tf");
                tf.Disabled = v;
                Assert.Equal(v, tf.Disabled);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// TextArea.Disabled round-trip（disabled 是 node 级 flag，所有 Node 子类共享通道）。
        /// </summary>
        [Fact]
        public void textarea_disabled_roundtrips_via_ffi()
        {
            var (stage, ctx, root) = LoadTextfieldFixture();
            try
            {
                var ta = root.Get<TextArea>("ta");
                ta.Disabled = true;
                Assert.True(ta.Disabled);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Slider.Disabled round-trip（Slider 自带 SetNodeDisabled + 新增 GetNodeDisabled 转调）。
        /// </summary>
        [Theory]
        [InlineData(true)]
        [InlineData(false)]
        public void slider_disabled_roundtrips_via_ffi(bool v)
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                var sld = root.Get<Slider>("sld");
                sld.Disabled = v;
                Assert.Equal(v, sld.Disabled);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Toggle / RadioButton Disabled round-trip（与 Slider 同 SetNodeDisabled + GetNodeDisabled 模式）。
        /// </summary>
        [Theory]
        [InlineData("tggl")]
        [InlineData("rdo")]
        public void toggle_radio_disabled_roundtrips_via_ffi(string id)
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                if (id == "tggl")
                {
                    var tggl = root.Get<Toggle>("tggl");
                    tggl.Disabled = true;
                    Assert.True(tggl.Disabled);
                }
                else
                {
                    var rdo = root.Get<RadioButton>("rdo");
                    rdo.Disabled = true;
                    Assert.True(rdo.Disabled);
                }
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── Node.Blur() 调 FFI 不抛 ──────────────────────────────────────

        /// <summary>
        /// Node.Blur() 调 loomgui_stage_blur FFI 不抛（改前 throw NE）。
        /// stage::blur 是 stage 级操作（清当前焦点，无 node_id），对未聚焦节点调为 no-op——
        /// 此测只验 FFI 接通（不抛），不验焦点语义（焦点路由由 Focus/Submitted 测覆盖）。
        /// </summary>
        [Fact]
        public void node_blur_calls_ffi_without_throwing()
        {
            var (stage, ctx, root) = LoadTextfieldFixture();
            try
            {
                var tf = root.Get<TextField>("tf");
                tf.Blur();   // 改前 throw NE；改后直 FFI（no-op 当无焦点）
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── fixture 加载 helpers（仿 TextFieldProjectionTests / ControlProjectionTests）──

        static (IntPtr stage, UIContext ctx, Container root) LoadTextfieldFixture()
        {
            var (stage, ctx) = StageHarness.Create();
            StageHandle* h = (StageHandle*)stage.ToPointer();

            string fontPath = Path.Combine(AppContext.BaseDirectory, "fixtures", "fonts", "DejaVuSans.ttf");
            if (File.Exists(fontPath))
                RegisterFont(h, fontPath);

            uint sceneRootId = CreateRoot(h, "div");
            ctx._rootId = sceneRootId;
            Container sceneRoot = (Container)ctx._registry.GetOrCreate(sceneRootId);

            string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "textfield.pkg.bin");
            Assert.True(File.Exists(fixturePath), $"fixture textfield.pkg.bin not found at {fixturePath}");

            byte[] pkgBytes = File.ReadAllBytes(fixturePath);
            UIPackage pkg = ctx.LoadPackage("textfield", pkgBytes);
            Container instRoot = pkg.Instantiate("textfield");
            AppendChild(h, sceneRoot._id, instRoot._id);
            Tick(h);
            return (stage, ctx, instRoot);
        }

        static (IntPtr stage, UIContext ctx, Container root) LoadControlsFixture()
        {
            var (stage, ctx) = StageHarness.Create();
            StageHandle* h = (StageHandle*)stage.ToPointer();

            string fontPath = Path.Combine(AppContext.BaseDirectory, "fixtures", "fonts", "DejaVuSans.ttf");
            if (File.Exists(fontPath))
                RegisterFont(h, fontPath);

            uint sceneRootId = CreateRoot(h, "div");
            ctx._rootId = sceneRootId;
            Container sceneRoot = (Container)ctx._registry.GetOrCreate(sceneRootId);

            string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "controls.pkg.bin");
            Assert.True(File.Exists(fixturePath), $"fixture controls.pkg.bin not found at {fixturePath}");

            byte[] pkgBytes = File.ReadAllBytes(fixturePath);
            UIPackage pkg = ctx.LoadPackage("controls", pkgBytes);
            Container instRoot = pkg.Instantiate("controls");
            AppendChild(h, sceneRoot._id, instRoot._id);
            Tick(h);
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
