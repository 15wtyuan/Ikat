using System;
using System.IO;
using System.Text;
using LoomGUI.Bindings;
using Xunit;

namespace LoomGUI.HeadlessTests
{
    /// <summary>
    /// P1 C2 acceptance: real CSS block layout ignores flex-grow on children.
    ///
    /// Discriminator (empirically confirmed via dump_p1block_probe under the
    /// current pseudo-block mapping — display:block → taffy Flex column):
    ///   - pseudo-block honors flex-grow: in a 280px-tall .stack, two flex-grow:1
    ///     children split the free space and each grow to h≈140.
    ///   - real block ignores flex-grow: children keep their explicit height:40.
    /// width (explicit 100px) and vertical stacking do NOT discriminate — both
    /// modes leave width unchanged and stack children on the y axis.
    ///
    /// This test is the TDD red light for C2: it MUST fail on the current
    /// pseudo-block build (c1.height≈140 &gt; 41). Task 5 switches the block
    /// mapping to a real Block strategy that ignores flex-grow, at which point
    /// c1.height becomes 40 and this test goes green.
    /// </summary>
    public unsafe class BlockLayoutTests
    {
        [Fact]
        public void BlockIgnoresFlexGrow()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                RegisterDefaultFont(h);
                Container root = InstantiateBlockFixture(h, ctx);

                Container stack = root.Get<Container>("stack");
                Container c1 = stack.Get<Container>("c1");

                // Run layout so flex/block strategies resolve child heights.
                Native.loomgui_stage_tick(h, 0.016f);

                // Real block ignores flex-grow → c1 keeps explicit height:40 (≤41 tolerance).
                // Pseudo-block (flex-column) honors flex-grow → c1 grows to ~140 and fails here.
                Assert.True(c1.Geometry.LayoutRect.Height <= 41.0f,
                    $"block: c1.height ({c1.Geometry.LayoutRect.Height}) should stay ~40px " +
                    $"(real block ignores flex-grow); got pseudo-block flex-column flex-grow → ~140");
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── helpers ──────────────────────────────────────────────────────

        /// <summary>
        /// Register DejaVuSans.ttf as default font (tick panics without a default font).
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

        /// <summary>Create a div scene root and return its id.</summary>
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

        /// <summary>
        /// Load fixtures/p1-block.pkg.bin (package "p1-block", template "p1-block-acceptance"
        /// — template name is the html file stem), instantiate, attach to a fresh scene root,
        /// and tick once. Mirrors AcceptanceGateTests.InstantiateFixture.
        /// </summary>
        private static Container InstantiateBlockFixture(StageHandle* h, UIContext ctx)
        {
            uint sceneRootId = CreateRoot(h);
            ctx._rootId = sceneRootId;
            Container sceneRoot = (Container)ctx._registry.GetOrCreate(sceneRootId);

            string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "p1-block.pkg.bin");
            Assert.True(File.Exists(fixturePath),
                $"fixture pkg.bin not found at {fixturePath}. " +
                "Ensure csproj <None CopyToOutputDirectory> is configured.");

            byte[] pkgBytes = File.ReadAllBytes(fixturePath);
            Assert.True(pkgBytes.Length > 0, "p1-block.pkg.bin is empty");

            UIPackage pkg = ctx.LoadPackage("p1-block", pkgBytes);
            Assert.NotNull(pkg);

            // Template name = html file stem (p1-block-acceptance.html → "p1-block-acceptance").
            Container instRoot = pkg.Instantiate("p1-block-acceptance");
            Assert.NotNull(instRoot);
            AppendChild(h, sceneRoot._id, instRoot._id);

            Native.loomgui_stage_tick(h, 0.016f);
            return instRoot;
        }
    }
}
