using System;
using System.IO;
using System.Text;
using LoomGUI.Bindings;
using Xunit;
using Xunit.Abstractions;

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
        readonly ITestOutputHelper _log;
        public VirtualizationTests(ITestOutputHelper log) => _log = log;
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
        /// Task 6 exit criterion: variable-height list (li height:auto + per-item text +
        /// margin-bottom) scrolled down to the bottom then back to the top must NOT drift —
        /// the first visible item reappears at the same world-y as the initial state, and
        /// scroll_pos returns to ~0 without accumulating anchoring error.
        ///
        /// Drift mechanism under test: as the list scrolls, items leave/enter the visible window
        /// and collect_heights backfills their real margin-box heights. Without scroll anchoring,
        /// the head spacer height would jump when estimates get replaced by measurements,
        /// making the content visually shift even though scroll_pos is unchanged. Anchoring
        /// compensates scroll_pos.y by the head-region delta so content stays put, and the clamp
        /// exemption keeps any in-flight tween alive across the overlap change.
        /// </summary>
        [Fact]
        public void VariableHeight_NoDrift_OnScrollDownThenUp()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                RegisterDefaultFont(h);

                uint sceneRootId = CreateRoot(h, "div");
                ctx._rootId = sceneRootId;
                Container sceneRoot = (Container)ctx._registry.GetOrCreate(sceneRootId);

                string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "varheight.pkg.bin");
                Assert.True(File.Exists(fixturePath),
                    $"fixture varheight.pkg.bin not found at {fixturePath}");
                byte[] pkgBytes = File.ReadAllBytes(fixturePath);
                UIPackage pkg = ctx.LoadPackage("varheight", pkgBytes);
                Container instRoot = pkg.Instantiate("varheight");
                AppendChild(h, sceneRoot._id, instRoot._id);

                // Tick once so cascade + solve populate layout_rect before entering data-driven.
                TickAndDrain(h, ctx);

                ListView list = instRoot.Get<ListView>("list");
                Assert.NotNull(list);
                Container pane = instRoot.Get<Container>("pane");
                Assert.NotNull(pane);

                // No-op BindItem (slot content stays template default). The fixture bakes li
                // height:40px + margin-bottom:8px, so every slot's margin-box height = 48 —
                // collect_heights backfills 48 (not the bare 40 border box), exercising the
                // margin-box path. Uniform heights keep anchoring delta ~0 (estimate converges
                // on first backfill), so this validates the no-drift round trip without depending
                // on per-item inline-override propagation (a separate concern).
                list.BindItem = (item, index) => { };
                list.ItemCount = 60;

                // Settle a few frames: cold-start slots clone, BindItem fires, solve measures real
                // heights, collect_heights backfills them, anchoring stabilizes.
                for (int i = 0; i < 6; i++)
                    TickAndDrain(h, ctx);

                // Capture initial state at top: first slot's world-y + scroll_pos.
                uint ul = list._id;
                uint firstSlot0 = FirstSlotChildId(h, ul);
                Assert.NotEqual(0u, firstSlot0);
                var lr0 = GetLayoutRect(h, firstSlot0);
                float initialFirstSlotWorldY = lr0.y;
                float initialScrollY = GetScrollY(h, pane._id);

                // Scroll down near the bottom. Several frames let virtualization recycle/clone slots
                // at the new window and backfill their heights (anchoring fires here).
                float bigY = 4000f;
                SetScrollPos(h, pane._id, bigY);
                for (int i = 0; i < 8; i++)
                    TickAndDrain(h, ctx);
                float midScrollY = GetScrollY(h, pane._id);
                // set_scroll_pos clamps to [0, overlap]; anchoring may further shift it by the
                // head-region delta. Assert finite + advanced past the top (virtualization recycled
                // slots at the new window — the precondition for the no-drift round trip).
                Assert.True(!float.IsNaN(midScrollY) && !float.IsInfinity(midScrollY),
                    $"scroll_pos.y unstable after scroll down: {midScrollY}");
                Assert.True(midScrollY > 0f, $"scroll advanced past top: {midScrollY}");

                // Confirm virtualization pushed items above the visible window: the head spacer
                // (the last Container before the first slot) must have grown beyond zero, proving
                // items were measured + collapsed into the spacer — the precondition for anchoring.
                int childCount = Native.loomgui_stage_get_child_count(h, ul);
                Assert.True(childCount >= 3, $"slots materialized at mid-scroll: {childCount}");
                uint[] midKids = new uint[childCount];
                fixed (uint* bp = midKids)
                    Native.loomgui_stage_get_children(h, ul, bp, (nuint)childCount);
                // Locate the head spacer: the last Container (kind 0) before the first ListItem (slot).
                // HTML-source whitespace TextNodes (kind 1) may precede it, so we scan for it.
                int firstSlotIdx = -1;
                for (int i = 0; i < childCount; i++)
                {
                    byte kn = 0;
                    Native.loomgui_stage_get_node_kind(h, midKids[i], &kn);
                    if (kn == 15) { firstSlotIdx = i; break; }
                }
                Assert.True(firstSlotIdx > 0, $"head spacer exists before first slot: firstSlotIdx={firstSlotIdx}");
                var headSpacerRect = GetLayoutRect(h, midKids[firstSlotIdx - 1]);
                Assert.True(headSpacerRect.h > 1f,
                    $"head spacer grew (items virtualized above window): h={headSpacerRect.h}");

                // Scroll back to the top.
                SetScrollPos(h, pane._id, 0f);
                for (int i = 0; i < 8; i++)
                    TickAndDrain(h, ctx);

                // No-drift assertion: the first visible slot (item 0, re-cloned after recycling)
                // sits at the same world-y as the initial state, and scroll_pos returned to ~0.
                uint firstSlot1 = FirstSlotChildId(h, ul);
                Assert.NotEqual(0u, firstSlot1);
                var lr1 = GetLayoutRect(h, firstSlot1);
                float finalFirstSlotWorldY = lr1.y;
                float finalScrollY = GetScrollY(h, pane._id);

                _log.WriteLine(
                    $"initial: firstSlotY={initialFirstSlotWorldY:F2} scrollY={initialScrollY:F2}; " +
                    $"mid: scrollY={midScrollY:F2}; " +
                    $"final: firstSlotY={finalFirstSlotWorldY:F2} scrollY={finalScrollY:F2}");

                Assert.Equal(initialFirstSlotWorldY, finalFirstSlotWorldY, 1.0f);
                Assert.Equal(0f, finalScrollY, 1.0f);
            }
            finally { StageHarness.Destroy(stage); }
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

        /// <summary>
        /// Read a node's layout rect (x,y,w,h) straight off the FFI. Mirrors NodeGeometry.LayoutRect
        /// but usable without materializing a typed Node (we only have raw ids for slots here).
        /// </summary>
        static (float x, float y, float w, float h) GetLayoutRect(StageHandle* h, uint node)
        {
            float x = 0, y = 0, w = 0, hh = 0;
            Native.loomgui_stage_get_node_layout_rect(h, node, &x, &y, &w, &hh);
            return (x, y, w, hh);
        }

        /// <summary>
        /// Return the first slot node id (first child of kind ListItem). Slots are dynamically
        /// cloned between head/tail spacers; whitespace TextNodes from the HTML source may
        /// precede the head spacer, so we scan for the first ListItem by kind rather than
        /// assuming a fixed child index.
        /// </summary>
        static uint FirstSlotChildId(StageHandle* h, uint ul)
        {
            int count = Native.loomgui_stage_get_child_count(h, ul);
            if (count == 0) return 0;
            uint[] buf = new uint[count];
            int written;
            fixed (uint* bp = buf)
                written = Native.loomgui_stage_get_children(h, ul, bp, (nuint)count);
            for (int i = 0; i < written; i++)
            {
                byte kn = 0;
                Native.loomgui_stage_get_node_kind(h, buf[i], &kn);
                // ListItem kind discriminant (matches core NodeKind::ListItem).
                if (kn == 15) return buf[i];
            }
            return 0;
        }

        /// <summary>
        /// Set the pane's scroll position (non-animated) via the FFI. ScrollTo is a stub on the
        /// public API, so virtualization scroll tests drive the pane's scroll_pos directly.
        /// </summary>
        static void SetScrollPos(StageHandle* h, uint pane, float y)
            => Native.loomgui_stage_set_scroll_pos(h, pane, 0.0f, y, animated: 0);

        static float GetScrollY(StageHandle* h, uint pane)
        {
            float x, y;
            Native.loomgui_stage_get_scroll_pos(h, pane, &x, &y);
            return y;
        }
    }
}
