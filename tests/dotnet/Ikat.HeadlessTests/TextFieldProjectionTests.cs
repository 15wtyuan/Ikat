using System;
using System.IO;
using System.Text;
using Ikat.Bindings;
using Xunit;

namespace Ikat.HeadlessTests
{
    /// <summary>
    /// P2 TextField 投影层验收：TextField/TextArea 经 FFI 填实的属性 +
    /// 文本控件事件 demux（EVT_VALUE_CHANGED / EVT_SUBMITTED）。
    ///
    /// 文本控件的 EditState 是打包期产物（pkg.bin 经 create_node_from_template + ControlInit::TextField
    /// 注入 scene.controls side table），运行时无 control_init setter FFI——故用预构建的
    /// textfield.pkg.bin fixture（含 tf/ta 两节点）经 LoadPackage+Instantiate 拿到带 EditState 的
    /// 文本控件节点，而非 create_root。
    ///
    /// 输入流测试（Submitted/textinput）走核心真实管线：request_focus → set_key_input/set_text_input →
    /// tick → borrow_events → demuxer.Pump。core 把 textinput 插进聚焦 TextField + 产 EVT_VALUE_CHANGED；
    /// 单行框 Enter 产 EVT_SUBMITTED（TextArea 不发——Enter 插换行）。
    ///
    /// 全部经 headless harness P/Invoke 真 dll，不启 Unity。
    /// </summary>
    public unsafe class TextFieldProjectionTests
    {
        private const uint KeyCodeEnter = 13;

        // ── 属性 FFI round-trip ──────────────────────────────────────────

        /// <summary>
        /// TextField.Value set "hello" → get "hello"（FFI set/get_control_text round-trip；
        /// UTF-8 ptr+len 通道）。验投影层 Value setter（set_control_text）+ getter（get_control_text
        /// return-code + out-param 双调法）全链通。
        /// </summary>
        [Fact]
        public void textfield_value_roundtrips_via_ffi()
        {
            var (stage, ctx, root) = LoadTextfieldFixture();
            try
            {
                var tf = root.Get<TextField>("tf");
                Assert.Equal("", tf.Value);   // 打包期无 value 属性 → 空

                tf.Value = "hello";
                Assert.Equal("hello", tf.Value);

                // UTF-8 多字节字符（验字节偏移通道正确，非 surrogate/ASCII-only）。
                tf.Value = "你好";
                Assert.Equal("你好", tf.Value);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// TextField.Placeholder round-trip：fixture tf 的 placeholder="name"（打包期），set 覆盖 → get 回读。
        /// 验 FFI set/get_control_placeholder 全链通（与 get_control_text 同双调法）。
        /// </summary>
        [Fact]
        public void textfield_placeholder_roundtrips_via_ffi()
        {
            var (stage, ctx, root) = LoadTextfieldFixture();
            try
            {
                var tf = root.Get<TextField>("tf");
                Assert.Equal("name", tf.Placeholder);   // 打包期 placeholder 属性

                tf.Placeholder = "enter your name";
                Assert.Equal("enter your name", tf.Placeholder);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// TextField.Selection round-trip：set (2,5) → get (2,5)（字节偏移归一）。
        /// 验 FFI set/get_selection 全链通（nuint* 双 out-param）。注意 get 归一为 [start,end]。
        /// </summary>
        [Fact]
        public void textfield_selection_roundtrips_via_ffi()
        {
            var (stage, ctx, root) = LoadTextfieldFixture();
            try
            {
                var tf = root.Get<TextField>("tf");
                tf.Value = "abcdef";       // 需有 value 才能选（clamp 到 value.len）

                tf.Selection = new TextSelection(2, 5);
                var sel = tf.Selection;
                Assert.Equal(2, sel.Start);
                Assert.Equal(5, sel.End);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// TextField.ReadOnly set true → 行为生效（readonly 拦用户编辑，不拦编程 setter）。
        /// core 无 readonly getter FFI——投影层 ReadOnly getter 暂留 throw（同 Slider.Disabled 模式）。
        /// 此测只验 setter 不抛（编程可写路径）+ Value 编程 setter 仍可改（readonly 不拦编程写）。
        /// </summary>
        [Fact]
        public void textfield_readonly_setter_does_not_throw()
        {
            var (stage, ctx, root) = LoadTextfieldFixture();
            try
            {
                var tf = root.Get<TextField>("tf");
                tf.ReadOnly = true;
                tf.Value = "programmatic";    // readonly 不拦编程 setter（HTML JS 语义）
                Assert.Equal("programmatic", tf.Value);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// TextArea.Value round-trip（含换行——sanitize_str 单行框才剥换行，TextArea 保留）。
        /// 验 TextArea 共享 FFI 通道。
        /// </summary>
        [Fact]
        public void textarea_value_roundtrips_with_newline()
        {
            var (stage, ctx, root) = LoadTextfieldFixture();
            try
            {
                var ta = root.Get<TextArea>("ta");
                ta.Value = "Multi\nline";
                Assert.Equal("Multi\nline", ta.Value);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// TextArea 无 Submitted 事件（Enter 插换行而非提交）——投影层 TextArea 不暴露 Submitted。
        /// 此测验证 TextArea 经 textinput 通道输入换行（set_text_input('\n') 经 line_break 多行分支
        /// insert_text）：聚焦 ta → set_text_input('\n') → tick → value 含 '\n'。
        /// 注：本测走 textinput 通道（非 KEY Enter 下键路由）——KEY Enter→line_break 的键路由分支
        /// 由 core（input/tests.rs）覆盖；此处仅验 C# 投影的 textinput 路径通。
        /// </summary>
        [Fact]
        public void textarea_textinput_newline_works()
        {
            var (stage, ctx, root) = LoadTextfieldFixture();
            try
            {
                var ta = root.Get<TextArea>("ta");
                ta.Focus();
                Tick((StageHandle*)stage.ToPointer());   // 消费 pending_focus_request

                StageHandle* h = (StageHandle*)stage.ToPointer();
                uint[] codepoints = { '\n' };
                SetTextInput(h, codepoints);
                Tick(h);

                Assert.Contains("\n", ta.Value);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── 控件事件 demux（走核心真实管线）──────────────────────────────

        /// <summary>
        /// TextField.Submitted 经核心管线触发：request_focus(tf) → tick → set_key_input(Enter down)
        /// → tick → borrow_events → demuxer.Pump → EVT_SUBMITTED(25) → ControlSubmittedEvent →
        /// TextField.Submitted handler 收到当前 value。
        /// 验焦点路由（无焦点 Enter 不产 Submitted）+ demux 分支 25 + Submitted backing-dict 全链通。
        /// </summary>
        [Fact]
        public void textfield_submitted_fires_on_enter()
        {
            var (stage, ctx, root) = LoadTextfieldFixture();
            try
            {
                var tf = root.Get<TextField>("tf");
                tf.Value = "query";
                tf.Focus();
                Tick((StageHandle*)stage.ToPointer());   // 消费 pending_focus_request（focus 下 tick 才生效）

                string submitted = null;
                tf.Submitted += v => submitted = v;

                StageHandle* h = (StageHandle*)stage.ToPointer();
                SetKeyDown(h, KeyCodeEnter);
                Tick(h);

                PumpEvents(h, ctx);

                Assert.Equal("query", submitted);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// TextField.ValueChanged 经核心管线触发：request_focus(tf) → tick → set_text_input("AB")
        /// → tick → borrow_events → demuxer.Pump → EVT_VALUE_CHANGED(22) → ControlValueChangedEvent →
        /// TextField.ValueChanged handler 收到 NewValue="AB"。
        /// 文本框的 EVT_VALUE_CHANGED 不携值（x=0）——demux dispatch ControlValueChangedEvent，
        /// 投影层 handler 在触发时回读当前 value（get_control_text）填 ValueChangedEvent&lt;string&gt;。
        /// </summary>
        [Fact]
        public void textfield_textinput_appends_chars_and_raises_valuechanged()
        {
            var (stage, ctx, root) = LoadTextfieldFixture();
            try
            {
                var tf = root.Get<TextField>("tf");
                tf.Value = "X";
                tf.Focus();
                Tick((StageHandle*)stage.ToPointer());

                ValueChangedEvent<string> received = default;
                tf.ValueChanged += e => received = e;

                StageHandle* h = (StageHandle*)stage.ToPointer();
                SetTextInput(h, new uint[] { 'A', 'B' });
                Tick(h);

                PumpEvents(h, ctx);

                Assert.Equal("XAB", tf.Value);             // 追加到光标末尾
                Assert.Equal("XAB", received.NewValue);    // ValueChanged 携回读的当前值
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── fixture 加载 helper（仿 ControlProjectionTests.LoadControlsFixture）─────────

        static (IntPtr stage, UIContext ctx, Container root) LoadTextfieldFixture()
        {
            var (stage, ctx) = StageHarness.Create();
            StageHandle* h = (StageHandle*)stage.ToPointer();

            string fontPath = Path.Combine(AppContext.BaseDirectory, "fixtures", "fonts", "DejaVuSans.ttf");
            if (File.Exists(fontPath))
                RegisterFont(h, fontPath);

            ulong sceneRootId = CreateRoot(h, "div");
            ctx._rootId = sceneRootId;
            Container sceneRoot = (Container)ctx._registry.GetOrCreate(sceneRootId);

            string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "textfield.pkg.bin");
            Assert.True(File.Exists(fixturePath),
                $"fixture textfield.pkg.bin not found at {fixturePath}");

            byte[] pkgBytes = File.ReadAllBytes(fixturePath);
            UIPackage pkg = ctx.LoadPackage("textfield", pkgBytes);
            Container instRoot = pkg.Instantiate("textfield");
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

        // set_key_input 单条 KeyEvent(Enter down)。照 IkatKeyEvent.cs 字段序：
        // key_code(u32) + modifiers(u8) + is_down(bool→u8) + pad[2]。
        static void SetKeyDown(StageHandle* h, uint keyCode, byte modifiers = 0)
        {
            KeyEvent ev = new KeyEvent
            {
                key_code = keyCode,
                modifiers = modifiers,
                is_down = true,
            };
            Native.ikat_stage_set_key_input(h, &ev, 1);
        }

        // set_text_input 注入 UTF-32 codepoints（已 shift-mapped 的可打印字符）。
        static void SetTextInput(StageHandle* h, uint[] codepoints)
        {
            fixed (uint* cp = codepoints)
                Native.ikat_stage_set_text_input(h, cp, (nuint)codepoints.Length);
        }

        // borrow_events → demuxer.Pump（复刻 IkatHost.Step 的事件段）。
        static void PumpEvents(StageHandle* h, UIContext ctx)
        {
            nuint len = 0;
            byte* ptr = Native.ikat_stage_borrow_events(h, &len);
            ctx._eventDemuxer.Pump((IntPtr)ptr, (int)len);
        }
    }
}
