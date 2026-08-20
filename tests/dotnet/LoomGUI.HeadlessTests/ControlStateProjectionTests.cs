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

        // ── 公共 API FFI 批（2026-08-14）：NumberField bounds setter / ProgressBar
        //    indeterminate / RadioButton.Name / UIContext.Pick——四处原 throw NE，本轮接通。──

        /// <summary>
        /// NumberField.Min/Max/Step setter round-trip：改界后 core 侧把 value 文本
        /// parse→clamp→量化→re-format（set_number_value 同口径）。fixture nf：
        /// value=0 min=0 max=10 step=2（aria-valuenow/min/max + data-step bake）。
        /// </summary>
        [Fact]
        public void numberfield_bounds_setters_roundtrip()
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                var nf = root.Get<NumberField>("nf");
                Assert.Equal(0f, nf.Min);
                Assert.Equal(10f, nf.Max);
                Assert.Equal(2f, nf.Step);
                // Max 10→4：后续写入 clamp 进 [0,4]。
                nf.Max = 4f;
                Assert.Equal(4f, nf.Max);
                nf.Value = 9f;
                Assert.Equal(4f, nf.Value);
                // Min 0→1 + Step 2→1：量化到 1 的倍数。
                nf.Min = 1f;
                Assert.Equal(1f, nf.Min);
                nf.Step = 1f;
                Assert.Equal(1f, nf.Step);
                nf.Value = 3.6f;
                Assert.Equal(4f, nf.Value);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// ProgressBar.IsIndeterminate round-trip（原 getter+setter 双 throw NE）。
        /// 纯状态位：value/max 不受扰动（视觉切换走作者 CSS）。
        /// </summary>
        [Fact]
        public void progressbar_indeterminate_roundtrip()
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                var prog = root.Get<ProgressBar>("prog");
                Assert.False(prog.IsIndeterminate);
                Assert.Equal(40f, prog.Value);
                prog.IsIndeterminate = true;
                Assert.True(prog.IsIndeterminate);
                Assert.Equal(40f, prog.Value);
                prog.IsIndeterminate = false;
                Assert.False(prog.IsIndeterminate);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// RadioButton.Name 读分组名（原 throw NE）。fixture rdo data-name="grp" bake。
        /// 只读——分组是结构性属性（互斥语义源）。
        /// </summary>
        [Fact]
        public void radiobutton_name_reads_group()
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                var rdo = root.Get<RadioButton>("rdo");
                Assert.Equal("grp", rdo.Name);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// UIContext.Pick（原 throw NE）：命中点返回子树内节点（沿 Parent 链可回溯到
        /// fixture root），画布外返回 null。core hit::hit_test 走上帧 world_transforms
        /// （fixture 加载已 Tick 一帧，layout 就绪）。
        /// </summary>
        [Fact]
        public void uicontext_pick_hits_and_misses()
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                Node hit = ctx.Pick(new LoomVector2(100f, 50f));
                Assert.NotNull(hit);
                // 命中者必在 fixture root 子树内。
                Node n = hit;
                while (n != null && !ReferenceEquals(n, root)) n = n.Parent;
                Assert.Same(root, n);
                // 画布外 → null。
                Assert.Null(ctx.Pick(new LoomVector2(999f, 999f)));
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Node.Touchable round-trip（原 throw NE）+ Pick 联动：untouchable 节点自身
        /// 不参与命中（点落到父），恢复后命中回归。CSS pointer-events 的运行时面。
        /// </summary>
        [Fact]
        public void node_touchable_roundtrip_and_pick()
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                var sld = root.Get<Slider>("sld");
                Assert.True(sld.Touchable, "default touchable");
                Assert.True(root.Touchable, "root default touchable");
                // slider 区域中心点当前命中 sld（或其 thumb 子）——先取基线命中者。
                // 控件子节点多为 0 高空盒（fixture 无内容），(100,50) 的命中者是 root
                // 自身（子全 miss 后 fallback）。对 root 开关 touchable 验整树命中门。
                Node before = ctx.Pick(new LoomVector2(100f, 50f));
                Assert.Same(root, before);
                root.Touchable = false;
                Assert.False(root.Touchable);
                // 外层 scene root（fixture harness 造的包装 div）仍可命中——本节点被跳过。
                Assert.NotSame(root, ctx.Pick(new LoomVector2(100f, 50f)));
                root.Touchable = true;
                Assert.True(root.Touchable);
                Assert.Same(root, ctx.Pick(new LoomVector2(100f, 50f)));
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
