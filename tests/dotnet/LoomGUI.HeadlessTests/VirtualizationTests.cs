using System;
using System.IO;
using System.Text;
using LoomGUI.Bindings;
using Xunit;

namespace LoomGUI.HeadlessTests
{
    /// <summary>
    /// M1 ListView virtualization — the project's core essence proof (headless).
    ///
    /// Asserts the defining invariant of virtualization: the render-node count produced by
    /// the core MUST NOT grow with total item count. We instantiate the same fixture
    /// (overflow:scroll pane &gt; ul ListView &gt; li template) twice, set ItemCount to 1000 then
    /// to 10000, and assert the frame's render-node count is EQUAL across both runs.
    ///
    /// Mechanism under test: enter_data_driven (clear design-time li, back up template,
    /// build head/tail spacers) → plan_visible/execute_visible (cold-start instantiates a
    /// CONSTANT INITIAL_SLOTS regardless of ItemCount, because with a zeroed layout_rect the
    /// viewport height reads 0 → cold-start branch) → take_pending_binds (C# drains every tick
    /// → BindItem) → collect_heights (solve回填 HeightCache). The spacer heights scale with
    /// ItemCount (head/tail grow), but the INSTANCE COUNT stays constant — that is virtualization.
    ///
    /// Render-node count is read straight off the frame blob header (build_blob writes
    /// magic(4) + version(4) + node_count(4) at the start); no full deserialization needed.
    /// </summary>
    public unsafe class VirtualizationTests
    {
        [Fact]
        public void RenderNodeCount_DoesNotGrow_WithTotalItemCount()
        {
            int n1000 = CountRenderNodesAfterTick(itemCount: 1000);
            int n10000 = CountRenderNodesAfterTick(itemCount: 10000);

            // Essence assertion: count is EQUAL, not merely "below a threshold".
            // A non-virtualizing list would scale linearly (10000 ≈ 10× 1000).
            Assert.Equal(n1000, n10000);
        }

        /// <summary>
        /// Loads the virtualization fixture fresh, drives it to data-driven mode with the given
        /// ItemCount, ticks a few frames (draining pending binds each tick so BindItem fires and
        /// layout_rect settles), then returns the frame's render-node count.
        /// </summary>
        static int CountRenderNodesAfterTick(int itemCount)
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                RegisterDefaultFont(h);

                uint sceneRootId = CreateRoot(h, "div");
                ctx._rootId = sceneRootId;
                Container sceneRoot = (Container)ctx._registry.GetOrCreate(sceneRootId);

                string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "virtualization.pkg.bin");
                Assert.True(File.Exists(fixturePath),
                    $"fixture virtualization.pkg.bin not found at {fixturePath}");

                byte[] pkgBytes = File.ReadAllBytes(fixturePath);
                UIPackage pkg = ctx.LoadPackage("virtualization", pkgBytes);
                Container instRoot = pkg.Instantiate("virtualization");
                AppendChild(h, sceneRoot._id, instRoot._id);

                // Tick once so cascade + solve populate layout_rect BEFORE entering data-driven
                // mode (enter_data_driven reads the ul's computed height-auto state).
                TickAndDrain(h, ctx);

                ListView list = instRoot.Get<ListView>("list");
                Assert.NotNull(list);

                // Enter data-driven + set count. A no-op BindItem so the drain doesn't throw and
                // the slot content stays the template default (we only care about node COUNT here).
                list.BindItem = (item, index) => { };
                list.ItemCount = itemCount;

                // Tick several frames: the virtualization core plans visible, clones slots,
                // solves layout, and回填 heights. A few frames let the cold-start slots settle
                // and any pending binds drain. DrainPendingBinds must precede each raw tick
                // (mirrors LoomHost.Step's ordering).
                for (int i = 0; i < 4; i++)
                    TickAndDrain(h, ctx);

                return ReadRenderNodeCount(h);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── helpers ──────────────────────────────────────────────────────

        static void TickAndDrain(StageHandle* h, UIContext ctx)
        {
            // Same ordering as LoomHost.Step: flush writes → drain pending binds → tick.
            ctx.FlushPendingWrites();
            ctx.DrainPendingBinds();
            Native.loomgui_stage_tick(h, 0.016f);
        }

        /// <summary>
        /// Render-node count from the frame blob header. build_blob layout starts with
        /// magic(4) + version(4) + node_count(u32 LE) — node_count sits at byte offset 8.
        /// borrow_frame returns a Rust-owned ptr valid until the next tick.
        /// </summary>
        static int ReadRenderNodeCount(StageHandle* h)
        {
            nuint len = 0;
            byte* ptr = Native.loomgui_stage_borrow_frame(h, &len);
            if (ptr == null || len < 12)
                throw new InvalidOperationException("borrow_frame returned no/short blob");
            // node_count = u32 LE at offset 8 (after magic + version).
            return (int)(ptr[8] | (ptr[9] << 8) | (ptr[10] << 16) | (ptr[11] << 24));
        }

        static void RegisterDefaultFont(StageHandle* h)
        {
            string fontPath = Path.Combine(AppContext.BaseDirectory, "fixtures", "fonts", "DejaVuSans.ttf");
            if (!File.Exists(fontPath)) return;
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
    }
}
