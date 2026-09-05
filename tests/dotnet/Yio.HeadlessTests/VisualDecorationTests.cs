using System;
using System.IO;
using System.Text;
using Yio.Bindings;
using Xunit;

namespace Yio.HeadlessTests
{
    /// <summary>
    /// P2-A acceptance: border_ring consumes border-radius + box_shadow_quad rounds.
    ///
    /// Honest layering — this headless test does NOT verify rounded-corner geometry:
    ///   - Rounded-corner vertex geometry → crates/core/src/render/tests.rs (P2 Task 1-2).
    ///   - Visual parity (border corner no longer leaks past the radius) → Unity PlayMode.
    /// What this test does verify, end-to-end through the real dll:
    ///   - The acceptance page (border-radius + border + box-shadow) packages and
    ///     instantiates without panic — smoke for the whole parse → cascade → layout chain.
    ///   - The .rounded-border / .rounded-shadow class selectors match (nodes found by id).
    ///   - CSS width:200px reaches layout (rect in the 200px magnitude).
    ///   - The `border` declaration reaches computed style (border_present=1, red) — the
    ///     precondition for border_ring to have material to draw a ring from.
    /// ComputedNodeStyleRepr exposes no box-shadow / border-radius field, so the rounded-shadow
    /// case is smoke-only here; its rounding geometry is covered by render/tests.rs.
    /// </summary>
    public unsafe class VisualDecorationTests
    {
        [Fact]
        public void RoundedBorder_ReachesComputedStyleAndLayout()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                RegisterDefaultFont(h);
                Container root = InstantiateVisualFixture(h, ctx);

                // Class selectors matched → both nodes found by id (Instantiate builds the tree;
                // lookup does not depend on tick).
                Container rb = root.Get<Container>("rb");
                Container rs = root.Get<Container>("rs");
                Assert.NotNull(rb);
                Assert.NotNull(rs);

                // Run layout + rematch so layout_rect / computed_style reflect the CSS.
                Native.yio_stage_tick(h, 0.016f);

                // CSS width:200px reaches layout. Tolerance covers content-box vs border-box
                // rect convention (200 content vs 212 border-box with a 6px border on #rb).
                Assert.InRange(rb.Geometry.LayoutRect.Width, 199f, 213f);
                Assert.InRange(rs.Geometry.LayoutRect.Width, 199f, 213f);

                // border:6px solid #ff0000 reaches computed style — border_ring's input material.
                ComputedNodeStyleRepr rbStyle = GetComputedStyle(h, rb._id);
                Assert.Equal(1, rbStyle.border_present);
                Assert.Equal(1f, rbStyle.border_color[0], 3);   // R
                Assert.Equal(0f, rbStyle.border_color[1], 3);   // G
                Assert.Equal(0f, rbStyle.border_color[2], 3);   // B
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── helpers ──────────────────────────────────────────────────────

        /// <summary>Register DejaVuSans.ttf as default font (tick panics without one).</summary>
        private static void RegisterDefaultFont(StageHandle* h)
        {
            string fontPath = Path.Combine(AppContext.BaseDirectory, "fixtures", "fonts", "DejaVuSans.ttf");
            byte[] fontBytes = File.ReadAllBytes(fontPath);
            byte[] family = Encoding.UTF8.GetBytes("DejaVuSans");
            fixed (byte* fp = family)
            fixed (byte* bp = fontBytes)
            {
                int rc = Native.yio_stage_register_font(
                    h, fp, (nuint)family.Length, bp, (nuint)fontBytes.Length, is_default: 1);
                if (rc != 0)
                    throw new InvalidOperationException(
                        $"register_font failed rc={rc}; font path={fontPath}");
            }
        }

        private static ulong CreateRoot(StageHandle* h)
        {
            byte[] k = Encoding.UTF8.GetBytes("div");
            fixed (byte* kp = k)
                return Native.yio_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        private static void AppendChild(StageHandle* h, ulong parent, ulong child)
        {
            int rc = Native.yio_stage_append_child(h, parent, child);
            if (rc != 0)
                throw new InvalidOperationException(
                    $"append_child(parent={parent}, child={child}) failed rc={rc}");
        }

        /// <summary>
        /// Load fixtures/p2-visual.pkg.bin (package "p2-visual", template "p2-visual-acceptance"
        /// — template name is the html file stem), instantiate, and attach to a fresh scene root.
        /// Mirrors BlockLayoutTests.InstantiateBlockFixture.
        /// </summary>
        private static Container InstantiateVisualFixture(StageHandle* h, UIContext ctx)
        {
            ulong sceneRootId = CreateRoot(h);
            ctx._rootId = sceneRootId;
            Container sceneRoot = (Container)ctx._registry.GetOrCreate(sceneRootId);

            string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "p2-visual.pkg.bin");
            Assert.True(File.Exists(fixturePath),
                $"fixture pkg.bin not found at {fixturePath}. " +
                "Ensure csproj <None CopyToOutputDirectory> is configured.");

            byte[] pkgBytes = File.ReadAllBytes(fixturePath);
            Assert.True(pkgBytes.Length > 0, "p2-visual.pkg.bin is empty");

            UIPackage pkg = ctx.LoadPackage("p2-visual", pkgBytes);
            Assert.NotNull(pkg);

            Container instRoot = pkg.Instantiate("p2-visual-acceptance");
            Assert.NotNull(instRoot);
            AppendChild(h, sceneRoot._id, instRoot._id);
            return instRoot;
        }

        private static ComputedNodeStyleRepr GetComputedStyle(StageHandle* h, ulong id)
        {
            ComputedNodeStyleRepr repr;
            int rc = Native.yio_stage_get_node_computed_style(h, id, &repr);
            if (rc != 0)
                throw new InvalidOperationException($"get_node_computed_style(id={id}) failed rc={rc}");
            return repr;
        }
    }
}
