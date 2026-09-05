using System;
using System.IO;
using System.Text;
using Yio.Bindings;
using Xunit;

namespace Yio.HeadlessTests
{
    /// <summary>
    /// Task 8 验收：NodeFactory 把 OptionItem/CustomElement NodeKind 派发到专用 C# 子类。
    /// （Slot 派发测试已删：组件系统打包期展开后产物不再有 NodeKind::Slot 节点，slot 是编译期糖。）
    /// （替代之前的 Container 回落）。
    ///
    /// 改前 NodeFactory 对这三 kind 回落 Container（NodeFactory.cs:65-67）——业务 Get&lt;OptionItem&gt;()
    /// 永远 miss（实例是 Container，is OptionItem false）。改后派发到专用子类，Get&lt;T&gt; 命中 +
    /// Assert.IsType 验真实类型。
    ///
    /// fixture：dropdown.pkg.bin（select 含 3 option + slot + custom-element &lt;my-widget&gt;），
    /// 经 LoadPackage+Instantiate 拿到带正确 NodeKind 的节点树。
    /// 全部经 headless harness P/Invoke 真 dll，不启 Unity。
    /// </summary>
    public unsafe class OptionItemDispatchTests
    {
        // ── NodeFactory 派发到专用子类（vs Container 回落）──────────────────

        /// <summary>
        /// &lt;option&gt; 节点经 NodeFactory 派发到 OptionItem 实例（非 Container）。
        /// Get&lt;OptionItem&gt; 命中（改前抛 UIContractException "not found"——实例是 Container，类型不符）。
        /// Assert.IsType 进一步验真实类型（非 Container 子类伪装）。
        /// </summary>
        [Fact]
        public void option_node_dispatches_to_optionitem_class()
        {
            var (stage, ctx, root) = LoadDropdownFixture();
            try
            {
                var opt = root.Get<OptionItem>("opt-a");
                Assert.IsType<OptionItem>(opt);
                Assert.IsNotType<Container>(opt);   // 严格：不是裸 Container（OptionItem : Container，IsNotType 验运行时类型非 Container 本身）
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// <summary>
        /// 自定义标签 &lt;my-widget&gt;（含连字符）派发到 CustomElement 实例（非 Container）。
        /// 含连字符的标签须在 components/ 注册（围栏），实例根投影为 CustomElement。
        /// </summary>
        [Fact]
        public void custom_element_dispatches_to_customelement_class()
        {
            var (stage, ctx, root) = LoadDropdownFixture();
            try
            {
                var cw = root.Get<CustomElement>("cw");
                Assert.IsType<CustomElement>(cw);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// OptionItem 经 Dropdown 父节点也能 Get 到（验 OptionItem 在 Container 子树内可查）。
        /// select &gt; option 结构：dropdown.Get&lt;OptionItem&gt; 命中子树内的 option。
        /// </summary>
        [Fact]
        public void optionitem_accessible_via_dropdown_subtree()
        {
            var (stage, ctx, root) = LoadDropdownFixture();
            try
            {
                var sel = root.Get<Dropdown>("sel");
                var opt = sel.Get<OptionItem>("opt-b");
                Assert.IsType<OptionItem>(opt);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// OptionItem.Disabled round-trip（Disabled 读 NodeFlags::DISABLED，与 Slider 等一致；
        /// Value/Selected 暂无 FFI 故未测——core 无 per-option value/selected getter）。
        /// </summary>
        [Theory]
        [InlineData("opt-a", true)]
        [InlineData("opt-a", false)]
        public void optionitem_disabled_roundtrips_via_ffi(string id, bool v)
        {
            var (stage, ctx, root) = LoadDropdownFixture();
            try
            {
                var opt = root.Get<OptionItem>(id);
                opt.Disabled = v;
                Assert.Equal(v, opt.Disabled);
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

            ulong sceneRootId = CreateRoot(h, "div");
            ctx._rootId = sceneRootId;
            Container sceneRoot = (Container)ctx._registry.GetOrCreate(sceneRootId);

            string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "dropdown.pkg.bin");
            Assert.True(File.Exists(fixturePath), $"fixture dropdown.pkg.bin not found at {fixturePath}");

            byte[] pkgBytes = File.ReadAllBytes(fixturePath);
            UIPackage pkg = ctx.LoadPackage("dropdown", pkgBytes);
            Container instRoot = pkg.Instantiate("dropdown");
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
                Native.yio_stage_register_font(
                    h, fp, (nuint)family.Length, bp, (nuint)fontBytes.Length, is_default: 1);
            }
        }

        static ulong CreateRoot(StageHandle* h, string kind)
        {
            byte[] k = Encoding.UTF8.GetBytes(kind);
            fixed (byte* kp = k)
                return Native.yio_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        static void AppendChild(StageHandle* h, ulong parent, ulong child)
        {
            int rc = Native.yio_stage_append_child(h, parent, child);
            if (rc != 0)
                throw new InvalidOperationException($"append_child(parent={parent}, child={child}) failed rc={rc}");
        }

        static void Tick(StageHandle* h) => Native.yio_stage_tick(h, 0.016f);
    }
}
