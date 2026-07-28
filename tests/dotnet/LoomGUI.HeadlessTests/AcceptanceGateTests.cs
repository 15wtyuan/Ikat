using System;
using System.Collections.Generic;
using System.IO;
using System.Text;
using LoomGUI.Bindings;
using Xunit;

namespace LoomGUI.HeadlessTests
{
    /// <summary>
    /// Spec-4a E3 acceptance gate: 9 independent [Fact] tests, one per spec §4 criterion.
    /// Each test re-instantiates a fresh fixture tree to avoid Dispose side-effect cross-contamination
    /// (same pattern as E2 FixtureSmokeTests).
    ///
    /// Fixture (test.workspace/test.html) structure:
    ///   div#root.container.highlight > div#child.spaced > [span#text, button#btn.spaced, img#img]
    ///   CSS: .highlight{color:red} .container{display:flex;flex-direction:column} .spaced{margin:4px}
    ///        #root{width:200px;height:100px}
    ///
    /// All 9 green = Spec-4a done (headless acceptance gate passed on this machine).
    /// </summary>
    public unsafe class AcceptanceGateTests
    {
        private const uint RootSentinel = 0xFFFF_FFFFu;

        // ═════════════════════════════════════════════════════════════════
        // Criterion 1: Type fidelity — Instantiate returns typed nodes
        // div→Container, span→TextElement, button→Button, img→Image.
        // ═════════════════════════════════════════════════════════════════

        [Fact]
        public void Criterion1_TypeFidelity()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                RegisterDefaultFont(h);
                Container instRoot = InstantiateFixture(h, ctx);

                // div#root → Container
                Assert.IsType<Container>(instRoot);

                // div#child → Container
                Container child = instRoot.Get<Container>("child");
                Assert.NotNull(child);
                Assert.IsType<Container>(child);

                // span#text → TextElement (fence parse path: kind=3)
                TextElement text = child.Get<TextElement>("text");
                Assert.NotNull(text);
                Assert.IsType<TextElement>(text);

                // button#btn → Button (kind=6)
                Button btn = child.Get<Button>("btn");
                Assert.NotNull(btn);
                Assert.IsType<Button>(btn);

                // img#img → Image (kind=8)
                Image img = child.Get<Image>("img");
                Assert.NotNull(img);
                Assert.IsType<Image>(img);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ═════════════════════════════════════════════════════════════════
        // Criterion 2: Scope lookup — Get&lt;T&gt;("id") hit; TryGet miss.
        // ═════════════════════════════════════════════════════════════════

        [Fact]
        public void Criterion2_ScopeLookup()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                RegisterDefaultFont(h);
                Container instRoot = InstantiateFixture(h, ctx);

                // Positive: Get<Container>("child") resolves within the instantiated subtree.
                Container child = instRoot.Get<Container>("child");
                Assert.NotNull(child);

                // TryGet positive path returns true + non-null result.
                Assert.True(instRoot.TryGet<Container>("child", out var childOut));
                Assert.NotNull(childOut);
                Assert.Same(child, childOut);

                // Negative: TryGet for non-existent id returns false.
                Assert.False(instRoot.TryGet<Container>("nope", out var nopeOut));
                Assert.Null(nopeOut);

                // Get for non-existent id throws UIContractException (scope boundary).
                Assert.Throws<UIContractException>(() => child.Get<Container>("not-exists"));
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ═════════════════════════════════════════════════════════════════
        // Criterion 3: Write → Read Geometry
        // Style.Width = Px(100) → Tick → LayoutRect.Width ≈ 100.
        // ═════════════════════════════════════════════════════════════════

        [Fact]
        public void Criterion3_WriteReadGeometry()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                RegisterDefaultFont(h);
                Container instRoot = InstantiateFixture(h, ctx);

                // instRoot #root has {width:200px;height:100px} → verify after tick.
                Tick(h, ctx);
                Rect lr = instRoot.Geometry.LayoutRect;
                Assert.InRange(lr.Width, 195, 205);
                Assert.InRange(lr.Height, 95, 105);

                // Write child width to 100px; tick; read back.
                Container child = instRoot.Get<Container>("child");
                child.Style.Width = Length.Px(100);
                Tick(h, ctx);
                float w = child.Geometry.LayoutRect.Width;
                Assert.InRange(w, 95, 105);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ═════════════════════════════════════════════════════════════════
        // Criterion 4: Unset fallback
        // Set width→100, tick; Unset→Tick → width reverts (not ~100).
        // ═════════════════════════════════════════════════════════════════

        [Fact]
        public void Criterion4_UnsetFallback()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                RegisterDefaultFont(h);
                Container instRoot = InstantiateFixture(h, ctx);
                Container child = instRoot.Get<Container>("child");

                // Set explicit width → verify it takes effect.
                child.Style.Width = Length.Px(300);
                Tick(h, ctx);
                float wSet = child.Geometry.LayoutRect.Width;
                Assert.InRange(wSet, 295, 305);

                // Unset → reverts to auto (content-dependent, far below 300).
                child.Style.Width = Length.Unset();
                Tick(h, ctx);
                float wUnset = child.Geometry.LayoutRect.Width;
                Assert.True(wUnset < 295,
                    $"unset width should revert below 300; got {wUnset}");
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ═════════════════════════════════════════════════════════════════
        // Criterion 5: Class affects computed style
        // Classes.Add("highlight") → Tick → get_node_computed_style color = red.
        // ═════════════════════════════════════════════════════════════════

        [Fact]
        public void Criterion5_ClassAffectsComputed()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                RegisterDefaultFont(h);
                Container instRoot = InstantiateFixture(h, ctx);
                Container child = instRoot.Get<Container>("child");

                // root has class="highlight" → .highlight{color:#ff0000}.
                // color is inherited, so child initially inherits red (R=1,G=0,B=0).
                // To prove Classes.Add actually changes computed style, we add
                // .blue{color:#0000ff} → child's color must change red→blue.
                Tick(h, ctx);
                ComputedNodeStyleRepr csBefore = GetComputedStyle(h, child._id);
                Assert.True(csBefore.color[0] >= 0.99f,
                    "pre-Add: child inherits red from root's .highlight");
                Assert.True(csBefore.color[2] <= 0.01f,
                    "pre-Add: child blue channel near 0 (not yet .blue)");

                // Add "blue" class → cascade re-runs on next tick.
                child.Classes.Add("blue");
                Tick(h, ctx);

                ComputedNodeStyleRepr cs = GetComputedStyle(h, child._id);
                Assert.True(cs.color[0] <= 0.01f,
                    $"post-Add: color.R should be ~0.0 (blue, not red); got {cs.color[0]}");
                Assert.True(cs.color[2] >= 0.99f,
                    $"post-Add: color.B should be ~1.0 (blue from .blue{{color:#0000ff}}); got {cs.color[2]}");
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ═════════════════════════════════════════════════════════════════
        // Criterion 6: Tree structure
        // ChildCount / Children types / GetChildAt matches HTML.
        // ═════════════════════════════════════════════════════════════════

        [Fact]
        public void Criterion6_TreeStructure()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                RegisterDefaultFont(h);
                Container instRoot = InstantiateFixture(h, ctx);

                // instRoot wraps the template root; its exact ChildCount depends on
                // whether the parser includes head/body wrapper elements.
                // Verify tree structure via the known #child subtree instead.
                Container childContainer = instRoot.Get<Container>("child");
                Assert.NotNull(childContainer);
                Assert.IsType<Container>(childContainer);

                // div#child has children (exact count varies: parser may create
                // text nodes for whitespace between elements). Verify known ids exist
                // with correct types, and GetChildAt matches Children entries.
                IReadOnlyList<Node> kids = childContainer.Children;
                Assert.NotEmpty(kids);
                for (int i = 0; i < kids.Count; i++)
                    Assert.Same(kids[i], childContainer.GetChildAt(i));

                // Verify known typed children exist within div#child.
                TextElement text = childContainer.Get<TextElement>("text");
                Assert.NotNull(text);
                Assert.IsType<TextElement>(text);

                Button btn = childContainer.Get<Button>("btn");
                Assert.NotNull(btn);
                Assert.IsType<Button>(btn);

                Image img = childContainer.Get<Image>("img");
                Assert.NotNull(img);
                Assert.IsType<Image>(img);

                // Parent chain: text, btn, img are children of childContainer;
                // childContainer is nested within instRoot.
                Assert.Same(childContainer, text.Parent);
                Assert.Same(childContainer, btn.Parent);
                Assert.Same(childContainer, img.Parent);
                Assert.NotNull(childContainer.Parent);
                Assert.Same(instRoot, childContainer.Parent);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ═════════════════════════════════════════════════════════════════
        // Criterion 7: Lifecycle — Dispose → IsDisposed → throws.
        // ═════════════════════════════════════════════════════════════════

        [Fact]
        public void Criterion7_Lifecycle()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                RegisterDefaultFont(h);
                Container instRoot = InstantiateFixture(h, ctx);

                Container child = instRoot.Get<Container>("child");
                TextElement text = child.Get<TextElement>("text");

                Assert.False(instRoot.IsDisposed);
                Assert.False(child.IsDisposed);
                Assert.False(text.IsDisposed);

                // Dispose instRoot → cascade to all descendants.
                instRoot.Dispose();

                Assert.True(instRoot.IsDisposed);
                Assert.True(child.IsDisposed);
                Assert.True(text.IsDisposed);

                // Post-dispose public access throws ObjectDisposedException.
                Assert.Throws<ObjectDisposedException>(() => { var _ = instRoot.Context; });
                Assert.Throws<ObjectDisposedException>(() => { var _ = child.Parent; });
                Assert.Throws<ObjectDisposedException>(() => { var _ = text.Classes; });
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ═════════════════════════════════════════════════════════════════
        // Criterion 8: Events — Clicked += → feed Click LoomEvent → Pump → fires + Target correct.
        // ═════════════════════════════════════════════════════════════════

        [Fact]
        public void Criterion8_Events()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                RegisterDefaultFont(h);
                Container instRoot = InstantiateFixture(h, ctx);
                Button btn = instRoot.Get<Button>("btn");
                Assert.NotNull(btn);

                // Subscribe via semantic sugar Clicked (Action, parameterless).
                bool clickedFired = false;
                btn.Clicked += () => clickedFired = true;

                // Also subscribe via typed On<ClickEvent> to verify Target.
                ClickEvent received = default;
                btn.On<ClickEvent>(e => received = e);

                // Feed a Click event targeting the button via native buffer → demux → dispatch.
                using (var buf = new NativeEventBuffer())
                {
                    buf.AddClick(btn._id);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.True(clickedFired, "Button.Clicked should fire on ClickEvent");
                Assert.NotEqual(default, received);
                Assert.Same(btn, received.Target);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ═════════════════════════════════════════════════════════════════
        // Criterion 9: Inline inheritance
        // Parent Style.Color = red → Tick → child computed color = red
        // (verifies inline_override folded into set_map then propagated by cascade).
        // ═════════════════════════════════════════════════════════════════

        [Fact]
        public void Criterion9_InlineInheritance()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                RegisterDefaultFont(h);
                Container instRoot = InstantiateFixture(h, ctx);
                Container child = instRoot.Get<Container>("child");

                // Set parent Style.Color to blue (0,0,1,1) via inline override.
                // This must override the CSS class .highlight{color:red} on root
                // and propagate to child via cascade inheritance.
                instRoot.Style.Color = new Color(0, 0, 1, 1);
                Tick(h, ctx);

                ComputedNodeStyleRepr rootCs = GetComputedStyle(h, instRoot._id);
                Assert.True(rootCs.color[2] >= 0.99f,
                    $"root inline blue: color.B should be ~1.0; got {rootCs.color[2]}");

                // Child has no explicit color style → inherits parent's computed blue.
                ComputedNodeStyleRepr childCs = GetComputedStyle(h, child._id);
                Assert.True(childCs.color[0] <= 0.01f,
                    $"child color.R should be ~0.0 (inherited blue, not red); got {childCs.color[0]}");
                Assert.True(childCs.color[2] >= 0.99f,
                    $"child color.B should inherit blue (~1.0) from parent inline; got {childCs.color[2]}");
                Assert.True(childCs.color[3] >= 0.99f,
                    $"child color.A should be ~1.0; got {childCs.color[3]}");
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── helpers ──────────────────────────────────────────────────────

        /// <summary>
        /// Register DejaVuSans.ttf as default font (required for text measurement;
        /// tick panics without a default font registered).
        /// </summary>
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

        /// <summary>Create a div scene root node and return its id.</summary>
        private static uint CreateRoot(StageHandle* h)
        {
            byte[] k = Encoding.UTF8.GetBytes("div");
            fixed (byte* kp = k)
                return Native.loomgui_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        /// <summary>Append child to parent via FFI. Throws on failure.</summary>
        private static void AppendChild(StageHandle* h, uint parent, uint child)
        {
            int rc = Native.loomgui_stage_append_child(h, parent, child);
            if (rc != 0)
                throw new InvalidOperationException(
                    $"append_child(parent={parent}, child={child}) failed rc={rc}");
        }

        /// <summary>Single tick (16ms ≈ 60fps). Flushes pending writes then triggers cascade + solve + compute_world_transforms.</summary>
        private static void Tick(StageHandle* h, UIContext ctx)
        {
            ctx.FlushPendingWrites();
            Native.loomgui_stage_tick(h, 0.016f);
        }

        /// <summary>FFI read computed style for a node. Returns the struct; throws on non-zero rc.</summary>
        private static ComputedNodeStyleRepr GetComputedStyle(StageHandle* h, uint id)
        {
            ComputedNodeStyleRepr repr;
            int rc = Native.loomgui_stage_get_node_computed_style(h, id, &repr);
            if (rc != 0)
                throw new InvalidOperationException(
                    $"get_node_computed_style(id={id}) failed rc={rc}");
            return repr;
        }

        /// <summary>
        /// Common setup for each criterion: create scene root, load fixture pkg,
        /// instantiate test template, append to scene root, tick once.
        /// Returns the instantiated root Container.
        /// </summary>
        private static Container InstantiateFixture(StageHandle* h, UIContext ctx)
        {
            // Create scene root and set it as the context's root node.
            uint sceneRootId = CreateRoot(h);
            ctx._rootId = sceneRootId;
            Container sceneRoot = (Container)ctx._registry.GetOrCreate(sceneRootId);

            // Load fixture package.
            string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "test.pkg.bin");
            Assert.True(File.Exists(fixturePath),
                $"fixture pkg.bin not found at {fixturePath}. " +
                "Ensure csproj <None CopyToOutputDirectory> is configured.");

            byte[] pkgBytes = File.ReadAllBytes(fixturePath);
            Assert.True(pkgBytes.Length > 0, "fixture pkg.bin is empty");

            UIPackage pkg = ctx.LoadPackage("test", pkgBytes);
            Assert.NotNull(pkg);

            // Instantiate template and attach to scene root so layout works.
            Container instRoot = pkg.Instantiate("test");
            Assert.NotNull(instRoot);
            AppendChild(h, sceneRoot._id, instRoot._id);

            // Initial tick to run cascade + solve.
            Tick(h, ctx);

            return instRoot;
        }
    }
}
