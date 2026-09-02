using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using Ikat;
using Ikat.Bindings;
using Xunit;
using Xunit.Abstractions;

namespace Ikat.HeadlessTests
{
    /// <summary>
    /// Multi-template ListView (#12) — TemplateSelector participates in cloning.
    ///
    /// Fixture: one role="list" with two <template id> blueprints whose CSS bakes
    /// DIFFERENT heights (rowA=40px, rowB=80px). A slot's layout height therefore
    /// identifies its blueprint without any marker plumbing — the assertions read
    /// slot rect heights straight off the solved tree.
    ///
    /// Covered contract points:
    /// - GetTemplate("rowA"/"rowB") + TemplateSelector lambda dispatches per item.
    /// - Strict semantics: a set selector must answer every index (null → UIContractException).
    /// - Multiple templates without a selection → UIContractException (rc -2 mapping).
    /// - ItemTemplate set BEFORE ItemCount is buffered (no silent drop) and satisfies
    ///   the multi-template "choice given" requirement.
    /// - Re-setting the selector after enter re-materializes items on the new blueprints.
    /// - NotifyInserted after enter re-pushes using the (now source-dead) UITemplate ids —
    ///   core resolves them via the adopt registry keyed by source NodeId (generation bits
    ///   make stale ids safe to look up).
    /// </summary>
    public unsafe class MultiTemplateTests
    {
        readonly ITestOutputHelper _log;
        public MultiTemplateTests(ITestOutputHelper log) => _log = log;

        (IntPtr, UIContext, ListView, Container) LoadList()
        {
            var (stage, ctx) = StageHarness.Create();
            StageHandle* h = (StageHandle*)stage.ToPointer();
            RegisterDefaultFont(h);

            ulong sceneRootId = CreateRoot(h, "div");
            ctx._rootId = sceneRootId;
            Container sceneRoot = (Container)ctx._registry.GetOrCreate(sceneRootId);

            string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "multitpl.pkg.bin");
            Assert.True(File.Exists(fixturePath), $"fixture missing: {fixturePath}");
            byte[] pkgBytes = File.ReadAllBytes(fixturePath);
            UIPackage pkg = ctx.LoadPackage("multitpl", pkgBytes);
            Container instRoot = pkg.Instantiate("multitpl");
            AppendChild(h, sceneRoot._id, instRoot._id);

            // One tick so cascade + solve populate layout_rect before entering data-driven.
            TickAndDrain(h, ctx);

            ListView list = instRoot.Get<ListView>("list");
            Assert.NotNull(list);
            Container pane = instRoot.Get<Container>("pane");
            Assert.NotNull(pane);
            return (stage, ctx, list, pane);
        }

        [Fact]
        public void Selector_DispatchesPerItemBlueprints()
        {
            var (stage, ctx, list, _pane) = LoadList();
            StageHandle* h = (StageHandle*)stage.ToPointer();
            try
            {
                Container scope = list.Parent;
                UITemplate ta = scope.GetTemplate("rowA");
                UITemplate tb = scope.GetTemplate("rowB");
                Assert.NotNull(ta);
                Assert.NotNull(tb);

                // Alternating dispatch: even → A (40px), odd → B (80px).
                list.TemplateSelector = i => (i % 2 == 0) ? ta : tb;
                list.ItemCount = 12;

                for (int i = 0; i < 4; i++)
                    TickAndDrain(h, ctx);

                var heights = SlotHeights(h, list._id);
                Assert.True(heights.Count >= 4, $"cold-start window has slots: {heights.Count}");
                Assert.Contains(heights, x => Math.Abs(x - 40f) < 1f);
                Assert.Contains(heights, x => Math.Abs(x - 80f) < 1f);
                // Alternation sanity: window starts at item 0 → first slot is an A row.
                Assert.True(Math.Abs(heights[0] - 40f) < 1f, $"first slot from blueprint A: {heights[0]}");

                // Virtualization still holds with mixed templates: bounded slot count.
                Assert.True(heights.Count < 12, "visible window is bounded (virtualized)");

                // NotifyInserted after enter: re-push rides on the adopt registry
                // (template source nodes died at enter — registry keys still resolve).
                list.NotifyInserted(6, 4);
                Assert.Equal(16, list.ItemCount);
                for (int i = 0; i < 3; i++)
                    TickAndDrain(h, ctx);
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void SelectorNull_ThrowsWithIndex()
        {
            var (stage, ctx, list, _pane) = LoadList();
            StageHandle* h = (StageHandle*)stage.ToPointer();
            try
            {
                Container scope = list.Parent;
                UITemplate ta = scope.GetTemplate("rowA");
                list.TemplateSelector = i => (i == 3) ? null : ta;
                var ex = Assert.Throws<UIContractException>(() => list.ItemCount = 10);
                Assert.Contains("index 3", ex.Message);
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void MultiTemplateWithoutSelection_Throws()
        {
            var (stage, ctx, list, _pane) = LoadList();
            StageHandle* h = (StageHandle*)stage.ToPointer();
            try
            {
                var ex = Assert.Throws<UIContractException>(() => list.ItemCount = 10);
                Assert.Contains("multiple <template>", ex.Message);
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void PreEnterItemTemplate_IsBuffered_AndSatisfiesChoice()
        {
            var (stage, ctx, list, _pane) = LoadList();
            StageHandle* h = (StageHandle*)stage.ToPointer();
            try
            {
                Container scope = list.Parent;
                UITemplate tb = scope.GetTemplate("rowB");
                // Set BEFORE ItemCount: buffered by core, consumed at enter — and counts as
                // the multi-template "choice" (no UIContractException).
                list.ItemTemplate = tb;
                list.ItemCount = 8;

                for (int i = 0; i < 3; i++)
                    TickAndDrain(h, ctx);

                var heights = SlotHeights(h, list._id);
                Assert.True(heights.Count >= 3, $"slots visible: {heights.Count}");
                Assert.All(heights, x => Assert.True(Math.Abs(x - 80f) < 1f, $"all slots from override blueprint B: {x}"));
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void ReselectAfterEnter_RematerializesOnNewBlueprints()
        {
            var (stage, ctx, list, _pane) = LoadList();
            StageHandle* h = (StageHandle*)stage.ToPointer();
            try
            {
                Container scope = list.Parent;
                UITemplate ta = scope.GetTemplate("rowA");
                UITemplate tb = scope.GetTemplate("rowB");

                list.TemplateSelector = i => (i % 2 == 0) ? ta : tb;
                list.ItemCount = 12;
                for (int i = 0; i < 3; i++)
                    TickAndDrain(h, ctx);
                Assert.Contains(SlotHeights(h, list._id), x => Math.Abs(x - 40f) < 1f);

                // Swap the selector post-enter: mismatched active slots get parked and the
                // items re-materialize on blueprint B.
                list.TemplateSelector = _ => tb;
                for (int i = 0; i < 4; i++)
                    TickAndDrain(h, ctx);

                var heights = SlotHeights(h, list._id);
                Assert.True(heights.Count >= 3, $"slots visible after remap: {heights.Count}");
                Assert.All(heights, x => Assert.True(Math.Abs(x - 80f) < 1f, $"all slots re-materialized on B: {x}"));
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── helpers (mirror VirtualizationTests) ───────────────────────────────────

        static void TickAndDrain(StageHandle* h, UIContext ctx)
        {
            ctx.FlushPendingWrites();
            ctx.DrainPendingBinds();
            Native.ikat_stage_tick(h, 0.016f);
        }

        static List<float> SlotHeights(StageHandle* h, ulong ul)
        {
            var result = new List<float>();
            int count = Native.ikat_stage_get_child_count(h, ul);
            if (count == 0) return result;
            ulong[] buf = new ulong[count];
            int written;
            fixed (ulong* bp = buf)
                written = Native.ikat_stage_get_children(h, ul, bp, (nuint)count);
            // Walk in DOM order; spacers are divs (kind 0), slots are ListItem (kind 15).
            // Parked (display:none) slots have zero rect — collect active slots in visual
            // order by taking ListItem children with non-zero height.
            for (int i = 0; i < written; i++)
            {
                byte kn = 0;
                Native.ikat_stage_get_node_kind(h, buf[i], &kn);
                if (kn == 15)
                {
                    var r = GetLayoutRect(h, buf[i]);
                    if (r.h > 0.5f)
                        result.Add(r.h);
                }
            }
            return result;
        }

        static (float x, float y, float w, float h) GetLayoutRect(StageHandle* h, ulong node)
        {
            float x = 0, y = 0, w = 0, hgt = 0;
            Native.ikat_stage_get_node_layout_rect(h, node, &x, &y, &w, &hgt);
            return (x, y, w, hgt);
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
    }
}
