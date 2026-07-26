using System;
using System.IO;
using System.Text;
using LoomGUI.Bindings;
using Xunit;

namespace LoomGUI.HeadlessTests
{
    /// <summary>
    /// E2 fixture pkg.bin smoke tests: covers E3 acceptance gate's 9 criteria via
    /// LoadPackage + Instantiate on a pre-built fixture workspace.
    ///
    /// The fixture (test.workspace/test.html) contains:
    ///   div#root.container.highlight > div#child.spaced > [span#text, button#btn.spaced, img#img]
    ///   CSS rules: .container{display:flex;flex-direction:column} .highlight{color:red}
    ///   #root{width:200px;height:100px} .spaced{margin:4px}
    ///
    /// Note on span: the fence parser maps &lt;span&gt; → SemanticKind::TextElement → NodeKind::TextElement (2).
    /// This differs from kind_from_tag("span") → NodeKind::TextNode (1) used by the dynamic create_node path.
    /// Both are valid projection types — TextElement : Container (inline text container) while TextNode : Node
    /// (leaf text run). The fixture tests use Get&lt;TextElement&gt; to match the pkg-built path.
    ///
    /// E3 9 criteria covered:
    ///   1. Type fidelity: Instantiate yields Container root; Get&lt;T&gt;("id") returns typed nodes
    ///   2. Scope lookup: Get&lt;T&gt;("child"/"text"/"btn"/"img") resolve within subtree
    ///   3. Write/Read Geometry: #root width:200px → tick → LayoutRect reflects dimensions
    ///   4. Unset: Set child width→300, Unset→reverts to base (auto)
    ///   5. Class affects computed: .highlight{color:red} on root → computed_style.color=[1,0,0,1]
    ///   6. Tree structure: Parent chain root→child→text/btn/img correct
    ///   7. Lifecycle: Dispose root → children disposed (IsDisposed==true)
    ///   8. Events: button.Clicked delegate can be subscribed
    ///   9. Inline inheritance: parent color:red → child computed color also red
    /// </summary>
    public unsafe class FixtureSmokeTests
    {
        private const uint RootSentinel = 0xFFFF_FFFFu;

        /// <summary>
        /// Central smoke test: LoadPackage(fixture) → Instantiate → verify all 9 E3 criteria.
        /// This cashes E1's deferred LoadPackage/Instantiate test.
        /// </summary>
        [Fact]
        public void FixtureLoadsAndInstantiatesAll9Criteria()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                // ── Setup: create scene root + register font + LoadPackage + Instantiate ──
                StageHandle* h = (StageHandle*)stage.ToPointer();
                RegisterDefaultFont(h);
                uint sceneRootId = CreateRoot(h, "div");
                ctx._rootId = sceneRootId;
                Container sceneRoot = (Container)ctx._registry.GetOrCreate(sceneRootId);

                string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "test.pkg.bin");
                Assert.True(File.Exists(fixturePath),
                    $"fixture pkg.bin not found at {fixturePath}. " +
                    "Ensure csproj <None CopyToOutputDirectory> is configured.");

                byte[] pkgBytes = File.ReadAllBytes(fixturePath);
                Assert.True(pkgBytes.Length > 0, "fixture pkg.bin is empty");

                UIPackage pkg = ctx.LoadPackage("test", pkgBytes);
                Assert.NotNull(pkg);
                Assert.Equal("test", pkg.Name);

                Container instRoot = pkg.Instantiate("test");
                Assert.NotNull(instRoot);
                Assert.IsType<Container>(instRoot);

                // Append instantiated tree into scene so layout works.
                AppendChild(h, sceneRoot._id, instRoot._id);

                // Tick to run cascade + solve (base_style + CSS rules → computed_style + layout).
                Tick(h);

                // ── Criterion 1: Type fidelity ───────────────────────────
                // div#root → Container (kind=0)
                byte rootKind = 0xFF;
                Assert.Equal(0, Native.loomgui_stage_get_node_kind(h, instRoot._id, &rootKind));
                Assert.Equal(0, rootKind);

                // div#child → Container (kind=0)
                Container child = instRoot.Get<Container>("child");
                Assert.NotNull(child);
                Assert.IsType<Container>(child);
                byte childKind = 0xFF;
                Assert.Equal(0, Native.loomgui_stage_get_node_kind(h, child._id, &childKind));
                Assert.Equal(0, childKind);

                // span#text → TextElement (kind=2 — fence parse path; not TextNode=1 from kind_from_tag)
                TextElement text = child.Get<TextElement>("text");
                Assert.NotNull(text);
                Assert.IsType<TextElement>(text);
                byte textKind = 0xFF;
                Assert.Equal(0, Native.loomgui_stage_get_node_kind(h, text._id, &textKind));
                Assert.Equal(2, textKind);

                // button#btn → Button (kind=6)
                Button btn = child.Get<Button>("btn");
                Assert.NotNull(btn);
                Assert.IsType<Button>(btn);
                byte btnKind = 0xFF;
                Assert.Equal(0, Native.loomgui_stage_get_node_kind(h, btn._id, &btnKind));
                Assert.Equal(3, btnKind);   // Button

                // img#img → Image (kind=8)
                Image img = child.Get<Image>("img");
                Assert.NotNull(img);
                Assert.IsType<Image>(img);
                byte imgKind = 0xFF;
                Assert.Equal(0, Native.loomgui_stage_get_node_kind(h, img._id, &imgKind));
                Assert.Equal(4, imgKind);   // Image

                // ── Criterion 2: Scope lookup via Get<T>("id") ────────────
                // Positive paths already covered above. Verify TryGet also works.
                Assert.True(instRoot.TryGet<Container>("child", out var childTry));
                Assert.NotNull(childTry);
                Assert.Same(child, childTry);

                // Verify scope boundary: Get on child for nonexistent id throws.
                Assert.Throws<UIContractException>(() => child.Get<Container>("not-exists"));

                // ── Criterion 3: Write/Read Geometry ─────────────────────
                // #root { width: 200px; height: 100px } → after tick, LayoutRect reflects.
                Rect lr = instRoot.Geometry.LayoutRect;
                Assert.InRange(lr.Width, 195, 205);
                Assert.InRange(lr.Height, 95, 105);

                // ── Criterion 4: Unset ───────────────────────────────────
                // child's width is auto (no explicit width in HTML).
                // Set 300px, tick, verify; then Unset → reverts to auto (content-dependent).
                {
                    child.Style.Width = Length.Px(300);
                    Tick(h);
                    float wBefore = child.Geometry.LayoutRect.Width;
                    Assert.InRange(wBefore, 295, 305);
                }

                {
                    child.Style.Width = Length.Unset();
                    Tick(h);
                    float wAfter = child.Geometry.LayoutRect.Width;
                    Assert.True(wAfter < 295,
                        $"unset width should revert below 300; got {wAfter}");
                }

                // ── Criterion 5: Class affects computed style ────────────
                // instRoot has class "highlight" → CSS .highlight{color:red} → computed color red.
                {
                    ComputedNodeStyleRepr cs = GetComputedStyle(h, instRoot._id);
                    Assert.True(cs.color[0] >= 0.99f, $"color.R should be ~1.0; got {cs.color[0]}");
                    Assert.True(cs.color[1] <= 0.01f, $"color.G should be ~0.0; got {cs.color[1]}");
                    Assert.True(cs.color[2] <= 0.01f, $"color.B should be ~0.0; got {cs.color[2]}");
                    Assert.True(cs.color[3] >= 0.99f, $"color.A should be ~1.0; got {cs.color[3]}");
                }

                // ── Criterion 6: Tree structure ──────────────────────────
                Assert.Same(sceneRoot, instRoot.Parent);
                Assert.Same(instRoot, child.Parent);
                Assert.Same(child, text.Parent);
                Assert.Same(child, btn.Parent);
                Assert.Same(child, img.Parent);

                // ── Criterion 7: Lifecycle ───────────────────────────────
                // Dispose instRoot → children cascade-disposed.
                {
                    uint childIdLc = child._id;
                    uint textIdLc = text._id;
                    uint btnIdLc = btn._id;
                    uint imgIdLc = img._id;

                    Assert.False(instRoot.IsDisposed);
                    Assert.False(child.IsDisposed);
                    Assert.False(text.IsDisposed);
                    Assert.False(btn.IsDisposed);
                    Assert.False(img.IsDisposed);

                    instRoot.Dispose();

                    Assert.True(instRoot.IsDisposed);
                    Assert.True(child.IsDisposed);
                    Assert.True(text.IsDisposed);
                    Assert.True(btn.IsDisposed);
                    Assert.True(img.IsDisposed);

                    // Rust side: removed nodes produce non-zero rc from get_node_kind.
                    byte deadKind = 0xFF;
                    Assert.NotEqual(0, Native.loomgui_stage_get_node_kind(h, childIdLc, &deadKind));
                }

                // ── Criterion 8: Events ──────────────────────────────────
                // Re-instantiate for event test (old tree was disposed).
                {
                    Container inst2 = pkg.Instantiate("test");
                    Assert.NotNull(inst2);
                    AppendChild(h, sceneRoot._id, inst2._id);
                    Tick(h);

                    Button btn2 = inst2.Get<Button>("btn");
                    Assert.NotNull(btn2);

                    // Verify Clicked delegate is subscribable.
                    bool eventFired = false;
                    btn2.Clicked += () => eventFired = true;

                    // Fire via demux.
                    using (var buf = new NativeEventBuffer())
                    {
                        buf.AddClick(btn2._id);
                        ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                    }
                    Assert.True(eventFired, "Button.Clicked should fire on ClickEvent");

                    inst2.RemoveFromParent();
                    inst2.Dispose();
                }

                // ── Criterion 9: Inline inheritance ──────────────────────
                // Root has class "highlight" (color:red). Child inherits via cascade.
                {
                    Container inst3 = pkg.Instantiate("test");
                    Assert.NotNull(inst3);
                    AppendChild(h, sceneRoot._id, inst3._id);
                    Tick(h);

                    Container child3 = inst3.Get<Container>("child");
                    Assert.NotNull(child3);

                    ComputedNodeStyleRepr childCs = GetComputedStyle(h, child3._id);
                    Assert.True(childCs.color[0] >= 0.99f,
                        $"child color.R should inherit red (~1.0); got {childCs.color[0]}");
                    Assert.True(childCs.color[1] <= 0.01f,
                        $"child color.G should be ~0.0; got {childCs.color[1]}");
                    Assert.True(childCs.color[2] <= 0.01f,
                        $"child color.B should be ~0.0; got {childCs.color[2]}");
                    Assert.True(childCs.color[3] >= 0.99f,
                        $"child color.A should be ~1.0; got {childCs.color[3]}");

                    inst3.RemoveFromParent();
                    inst3.Dispose();
                }
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── helpers ──────────────────────────────────────────────────────

        private static void RegisterDefaultFont(StageHandle* h)
        {
            string fontPath = Path.Combine(AppContext.BaseDirectory, "fixtures", "fonts", "DejaVuSans.ttf");
            byte[] fontBytes = File.ReadAllBytes(fontPath);
            byte[] family = Encoding.UTF8.GetBytes("DejaVuSans");
            fixed (byte* fp = family)
            fixed (byte* bp = fontBytes)
            {
                int rc = Native.loomgui_stage_register_font(
                    h, fp, (nuint)family.Length, bp, (nuint)fontBytes.Length, is_default: 1);
                if (rc != 0)
                    throw new InvalidOperationException(
                        $"register_font failed rc={rc}; font path={fontPath}");
            }
        }

        private static uint CreateRoot(StageHandle* h, string kind)
        {
            byte[] k = Encoding.UTF8.GetBytes(kind);
            fixed (byte* kp = k)
                return Native.loomgui_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        private static void AppendChild(StageHandle* h, uint parent, uint child)
        {
            int rc = Native.loomgui_stage_append_child(h, parent, child);
            if (rc != 0)
                throw new InvalidOperationException(
                    $"append_child(parent={parent}, child={child}) failed rc={rc}");
        }

        private static void Tick(StageHandle* h) =>
            Native.loomgui_stage_tick(h, 0.016f);

        private static ComputedNodeStyleRepr GetComputedStyle(StageHandle* h, uint id)
        {
            ComputedNodeStyleRepr repr;
            int rc = Native.loomgui_stage_get_node_computed_style(h, id, &repr);
            if (rc != 0)
                throw new InvalidOperationException(
                    $"get_node_computed_style(id={id}) failed rc={rc}");
            return repr;
        }
    }
}
