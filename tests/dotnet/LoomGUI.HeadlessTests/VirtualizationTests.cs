using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
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
        /// Task 6 exit criterion: a genuinely variable-height list (per-item inline height set
        /// in BindItem + margin-bottom) must keep the first visible item's world-y stable across
        /// a scroll-down-then-up round trip, AND the scroll-anchoring compensation must actually
        /// fire mid-scroll (otherwise the test is tautological — it would pass with the anchoring
        /// code deleted, as the previous fixed-height version did).
        ///
        /// Two load-bearing assertions:
        /// 1. <b>Anchoring-fires (delete-gate)</b>: after snapping scroll_pos to a mid-range value
        ///    and ticking ONE frame, scroll_pos.y must DIFFER from the snapped value. During a
        ///    down-scroll the virtualized content only ever grows (more items get measured and
        ///    folded into the spacers → overlap grows), so the refresh_content_sizes clamp cannot
        ///    be what moved an in-range scroll_pos. The only remaining modifier is collect_heights'
        ///    anchoring delta (scroll_pos.y += head-region height delta). With anchoring deleted,
        ///    scroll_pos would stay exactly at the snapped value → assertion fails.
        /// 2. <b>No-drift round-trip</b>: scroll to the bottom then back to the top; the first
        ///    visible item reappears at the same world-y as the initial state.
        /// Plus a sanity check that measured heights are genuinely non-uniform.
        /// </summary>
        [Fact]
        public void VariableHeight_NoDrift_OnScrollDownThenUp()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                RegisterDefaultFont(h);

                ulong sceneRootId = CreateRoot(h, "div");
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

                // Per-item variable height via inline override (Option B). The periodic pattern
                // (period 4: 40/70/100/130) guarantees every visible window contains a mix of
                // short and tall items, so the non-uniform sanity check holds at any scroll
                // position. Crucially, the estimate (mean of the few measured items) never
                // exactly matches the next unmeasured item, so as each new region scrolls into
                // view and gets measured, the head-region height sum shifts → anchoring must
                // compensate. With uniform heights the delta converges to ~0 and the test would
                // pass with the anchoring code deleted.
                list.BindItem = (item, index) =>
                {
                    float hh = 40f + (index % 4) * 30f;
                    item.Style.Height = Length.Px(hh);
                };
                list.ItemCount = 200;

                // Settle a few frames at the top: cold-start slots clone, BindItem sets heights,
                // solve measures them, collect_heights backfills, anchoring stabilizes.
                for (int i = 0; i < 4; i++)
                    TickAndDrain(h, ctx);

                ulong ul = list._id;

                // ── Sanity: measured heights are genuinely non-uniform ──────────────────
                // Read each visible slot's border-box height; at least two must differ, proving
                // the dataset is variable (guards against a regression that silently re-bakes a
                // fixed height and makes the anchoring assertion vacuous).
                var slotRects0 = SlotLayoutRects(h, ul);
                Assert.True(slotRects0.Count >= 2, $"at least 2 slots visible at top: {slotRects0.Count}");
                float minH = slotRects0.Min(r => r.h);
                float maxH = slotRects0.Max(r => r.h);
                Assert.True(maxH - minH > 1f,
                    $"heights must be non-uniform to exercise anchoring; min={minH} max={maxH}");

                // Capture initial state at top: first slot's world-y + scroll_pos.
                ulong firstSlot0 = FirstSlotChildId(h, ul);
                Assert.NotEqual(0u, firstSlot0);
                float initialFirstSlotWorldY = GetLayoutRect(h, firstSlot0).y;
                float initialScrollY = GetScrollY(h, pane._id);

                // ── Anchoring-fires (delete-gate) ────────────────────────────────────────
                // Snap scroll_pos to a value comfortably inside the scroll range (the tail spacer
                // already makes overlap large at the top). One tick materializes the new window's
                // slots, measures them, and collect_heights must compensate scroll_pos by the
                // head-region delta. Read scroll_pos back: it must have moved off the snapped
                // value. clamp cannot be responsible (content only grows on a down-scroll), so a
                // non-zero delta proves anchoring ran. Deleting the anchoring code leaves
                // scroll_pos untouched → this assertion fails.
                float snapY = 1500f;
                SetScrollPos(h, pane._id, snapY);
                float snappedY = GetScrollY(h, pane._id); // post-snap (already clamped to range)
                Assert.True(snappedY > initialScrollY + 50f,
                    $"snap advanced past top: snapped={snappedY} initial={initialScrollY}");
                TickAndDrain(h, ctx);
                float afterOneTickY = GetScrollY(h, pane._id);
                Assert.True(!float.IsNaN(afterOneTickY) && !float.IsInfinity(afterOneTickY),
                    $"scroll_pos.y finite after tick: {afterOneTickY}");
                Assert.True(MathF.Abs(afterOneTickY - snappedY) > 0.5f,
                    $"anchoring must shift scroll_pos off the snapped value " +
                    $"(snapped={snappedY}, afterTick={afterOneTickY}); " +
                    $"a no-op means anchoring never fired (delete-gate)");

                // Continue scrolling to near the bottom so the whole short→tall transition is
                // traversed and every region's heights get measured.
                SetScrollPos(h, pane._id, 20000f);
                for (int i = 0; i < 8; i++)
                    TickAndDrain(h, ctx);
                float midScrollY = GetScrollY(h, pane._id);
                Assert.True(midScrollY > snappedY,
                    $"scroll advanced toward bottom: mid={midScrollY} snapped={snappedY}");

                // Confirm virtualization pushed items above the visible window (head spacer grew).
                Assert.True(HeadSpacerHeight(h, ul) > 1f, "head spacer grew (items virtualized above)");

                // Scroll back to the top.
                SetScrollPos(h, pane._id, 0f);
                for (int i = 0; i < 8; i++)
                    TickAndDrain(h, ctx);

                // No-drift assertion: the first visible slot (item 0, re-cloned after recycling)
                // sits at the same world-y as the initial state, and scroll_pos returned to ~0.
                ulong firstSlot1 = FirstSlotChildId(h, ul);
                Assert.NotEqual(0u, firstSlot1);
                float finalFirstSlotWorldY = GetLayoutRect(h, firstSlot1).y;
                float finalScrollY = GetScrollY(h, pane._id);

                _log.WriteLine(
                    $"initial: firstSlotY={initialFirstSlotWorldY:F2} scrollY={initialScrollY:F2}; " +
                    $"snap={snappedY:F2} after1Tick={afterOneTickY:F2} (delta={afterOneTickY - snappedY:F2}); " +
                    $"mid: scrollY={midScrollY:F2}; " +
                    $"final: firstSlotY={finalFirstSlotWorldY:F2} scrollY={finalScrollY:F2}");

                Assert.Equal(initialFirstSlotWorldY, finalFirstSlotWorldY, 1.0f);
                Assert.Equal(0f, finalScrollY, 1.0f);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Task 7 Step 7: NotifyInserted must not corrupt scroll position or visible content.
        /// Inserts items at the END of the list (after the visible window) — nothing above the
        /// viewport changes, so scroll_pos.y and the visible slot set must be byte-for-byte
        /// preserved across the notify + tick. This is the robust, unambiguous assertion of the
        /// Notify plumbing (insert-before-visible would require scroll anchoring for insertions,
        /// which is out of Task 7 scope). Also verifies ItemCount cache stays in sync and the
        /// list keeps ticking cleanly after the notify.
        /// </summary>
        [Fact]
        public void NotifyInserted_AtTail_PreservesScrollPositionAndVisibleSlots()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                RegisterDefaultFont(h);

                ulong sceneRootId = CreateRoot(h, "div");
                ctx._rootId = sceneRootId;
                Container sceneRoot = (Container)ctx._registry.GetOrCreate(sceneRootId);

                string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "varheight.pkg.bin");
                Assert.True(File.Exists(fixturePath), $"fixture missing: {fixturePath}");
                byte[] pkgBytes = File.ReadAllBytes(fixturePath);
                UIPackage pkg = ctx.LoadPackage("varheight", pkgBytes);
                Container instRoot = pkg.Instantiate("varheight");
                AppendChild(h, sceneRoot._id, instRoot._id);

                TickAndDrain(h, ctx);

                ListView list = instRoot.Get<ListView>("list");
                Container pane = instRoot.Get<Container>("pane");
                list.BindItem = (item, index) => { item.Style.Height = Length.Px(40f + (index % 4) * 30f); };
                list.ItemCount = 200;

                // Settle at top first: cold-start slots clone, BindItem sets heights, solve
                // measures them, collect_heights backfills, the tail spacer grows the pane's
                // overlap so a mid-list scroll_pos is actually reachable (otherwise set_scroll_pos
                // clamps to overlap=0 → stays at top).
                for (int i = 0; i < 4; i++)
                    TickAndDrain(h, ctx);

                // Now scroll to mid-list.
                SetScrollPos(h, pane._id, 1500f);
                for (int i = 0; i < 4; i++)
                    TickAndDrain(h, ctx);

                float scrollYBefore = GetScrollY(h, pane._id);
                int slotCountBefore = SlotLayoutRects(h, list._id).Count;
                int itemCountBefore = list.ItemCount;
                Assert.True(scrollYBefore > 100f, $"precondition: scrolled to mid-list (y={scrollYBefore})");
                Assert.True(slotCountBefore > 0, "precondition: slots visible");

                // Insert 5 items at the END (after all visible content).
                list.NotifyInserted(itemCountBefore, 5);

                // ItemCount cache updated synchronously.
                Assert.Equal(itemCountBefore + 5, list.ItemCount);

                // Tick a frame: notify shifts no visible slot (insert is past the viewport),
                // so scroll_pos and visible slot set must be unchanged.
                TickAndDrain(h, ctx);

                float scrollYAfter = GetScrollY(h, pane._id);
                int slotCountAfter = SlotLayoutRects(h, list._id).Count;
                Assert.Equal(scrollYBefore, scrollYAfter, 0.5f);
                Assert.Equal(slotCountBefore, slotCountAfter);

                _log.WriteLine(
                    $"scroll y: {scrollYBefore:F2}→{scrollYAfter:F2}; " +
                    $"slots: {slotCountBefore}→{slotCountAfter}; " +
                    $"items: {itemCountBefore}→{list.ItemCount}");
            }
            finally { StageHarness.Destroy(stage); }
        }

        static int CountRenderNodesAfterTick(int itemCount)
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                RegisterDefaultFont(h);

                ulong sceneRootId = CreateRoot(h, "div");
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

        static ulong CreateRoot(StageHandle* h, string kind)
        {
            byte[] k = Encoding.UTF8.GetBytes(kind);
            fixed (byte* kp = k)
                return Native.loomgui_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        static void AppendChild(StageHandle* h, ulong parent, ulong child)
        {
            int rc = Native.loomgui_stage_append_child(h, parent, child);
            if (rc != 0)
                throw new InvalidOperationException($"append_child(parent={parent}, child={child}) failed rc={rc}");
        }

        /// <summary>
        /// Read a node's layout rect (x,y,w,h) straight off the FFI. Mirrors NodeGeometry.LayoutRect
        /// but usable without materializing a typed Node (we only have raw ids for slots here).
        /// </summary>
        static (float x, float y, float w, float h) GetLayoutRect(StageHandle* h, ulong node)
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
        static ulong FirstSlotChildId(StageHandle* h, ulong ul)
        {
            int count = Native.loomgui_stage_get_child_count(h, ul);
            if (count == 0) return 0;
            ulong[] buf = new ulong[count];
            int written;
            fixed (ulong* bp = buf)
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
        static void SetScrollPos(StageHandle* h, ulong pane, float y)
            => Native.loomgui_stage_set_scroll_pos(h, pane, 0.0f, y, animated: 0);

        static float GetScrollY(StageHandle* h, ulong pane)
        {
            float x, y;
            Native.loomgui_stage_get_scroll_pos(h, pane, &x, &y);
            return y;
        }

        /// <summary>
        /// Collect the layout rects of every slot (ListItem child of ul). Used for the
        /// non-uniform-height sanity check: confirms the dataset is genuinely variable so the
        /// anchoring assertion is not vacuous.
        /// </summary>
        static List<(float x, float y, float w, float h)> SlotLayoutRects(StageHandle* h, ulong ul)
        {
            var result = new List<(float, float, float, float)>();
            int count = Native.loomgui_stage_get_child_count(h, ul);
            if (count == 0) return result;
            ulong[] buf = new ulong[count];
            int written;
            fixed (ulong* bp = buf)
                written = Native.loomgui_stage_get_children(h, ul, bp, (nuint)count);
            for (int i = 0; i < written; i++)
            {
                byte kn = 0;
                Native.loomgui_stage_get_node_kind(h, buf[i], &kn);
                if (kn == 15) // ListItem
                    result.Add(GetLayoutRect(h, buf[i]));
            }
            return result;
        }

        /// <summary>
        /// Height of the head spacer (the Container child immediately before the first slot).
        /// Non-zero after scrolling proves items were collapsed above the visible window.
        /// </summary>
        static float HeadSpacerHeight(StageHandle* h, ulong ul)
        {
            int count = Native.loomgui_stage_get_child_count(h, ul);
            if (count == 0) return 0f;
            ulong[] buf = new ulong[count];
            int written;
            fixed (ulong* bp = buf)
                written = Native.loomgui_stage_get_children(h, ul, bp, (nuint)count);
            int firstSlotIdx = -1;
            for (int i = 0; i < written; i++)
            {
                byte kn = 0;
                Native.loomgui_stage_get_node_kind(h, buf[i], &kn);
                if (kn == 15) { firstSlotIdx = i; break; }
            }
            if (firstSlotIdx <= 0) return 0f;
            return GetLayoutRect(h, buf[firstSlotIdx - 1]).h;
        }
    }
}
