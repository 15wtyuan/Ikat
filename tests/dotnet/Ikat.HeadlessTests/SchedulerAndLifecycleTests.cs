using System;
using System.IO;
using System.Text;
using Ikat.Bindings;
using Xunit;

namespace Ikat.HeadlessTests
{
    /// <summary>
    /// 逻辑调度三件套（OnUpdate / CallLater / CallNextFrame）、包生命周期（UnloadPackage）、
    /// option/tab 派生 getter（Value/Selected）、Container.GetTemplate 的行为测试。
    ///
    /// 调度器住在 C# 投影层：headless 测试手动调 PumpLogic（同 FlushPendingWrites 模式），
    /// 生产路径由 IkatHost.Step 帧头泵。
    /// </summary>
    public unsafe class SchedulerAndLifecycleTests
    {
        const ulong RootSentinel = ulong.MaxValue;

        // ── helpers（同 OptionItemDispatchTests 的 fixture 模式）────────────

        static StageHandle* H(UIContext ctx) => (StageHandle*)ctx._stage.ToPointer();

        static ulong CreateRoot(UIContext ctx)
        {
            byte[] k = Encoding.UTF8.GetBytes("div");
            fixed (byte* kp = k)
                return Native.ikat_stage_create_root(H(ctx), kp, (nuint)k.Length, null, 0);
        }

        static void Tick(UIContext ctx, float dt = 0.016f) => Native.ikat_stage_tick(H(ctx), dt);

        static byte[] FixtureBytes(string name)
        {
            string p = Path.Combine(AppContext.BaseDirectory, "fixtures", name);
            Assert.True(File.Exists(p), $"fixture {name} not found at {p}");
            return File.ReadAllBytes(p);
        }

        /// <summary>注册默认字体（tick 前置契约：文本测量无 default font 即 panic）。</summary>
        static void RegisterDefaultFont(UIContext ctx)
        {
            string fontPath = Path.Combine(AppContext.BaseDirectory, "fixtures", "fonts", "DejaVuSans.ttf");
            if (!File.Exists(fontPath)) return;
            byte[] fontBytes = File.ReadAllBytes(fontPath);
            byte[] family = Encoding.UTF8.GetBytes("DejaVuSans");
            fixed (byte* fp = family)
            fixed (byte* bp = fontBytes)
                Native.ikat_stage_register_font(
                    H(ctx), fp, (nuint)family.Length, bp, (nuint)fontBytes.Length, is_default: 1);
        }

        /// <summary>dropdown fixture（3 option：value=alpha-val / value=beta-val+selected / 无 value）。</summary>
        static (IntPtr stage, UIContext ctx, Container root, Dropdown sel) LoadDropdown()
        {
            var (stage, ctx) = StageHarness.Create();
            RegisterDefaultFont(ctx);
            ulong rootId = CreateRoot(ctx);
            ctx._rootId = rootId;
            Container root = (Container)ctx._registry.GetOrCreate(rootId);
            UIPackage pkg = ctx.LoadPackage("dropdown", FixtureBytes("dropdown.pkg.bin"));
            Container inst = pkg.Instantiate("dropdown");
            Native.ikat_stage_append_child(H(ctx), rootId, inst._id);
            Tick(ctx);
            return (stage, ctx, root, inst.Get<Dropdown>("sel"));
        }

        // ── CallAfterLayout（N26：Instantiate 后同帧拿实测几何）─────────────

        /// <summary>
        /// 新挂载子树：帧头 CallNextFrame 回调先于首次 solve（Geometry 全零——行为契约），
        /// tick 后 CallAfterLayout 回调读到已解算几何。业务由此免自旋等待。
        /// </summary>
        [Fact]
        public void CallAfterLayoutSeesSolvedGeometrySameFrame()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                RegisterDefaultFont(ctx);
                ulong rootId = CreateRoot(ctx);
                ctx._rootId = rootId;
                UIPackage pkg = ctx.LoadPackage("dropdown", FixtureBytes("dropdown.pkg.bin"));
                Container inst = pkg.Instantiate("dropdown");
                Native.ikat_stage_append_child(H(ctx), rootId, inst._id);
                // 尚未 tick：新子树布局未解算。

                float wAfterLayout = -1f, wNextFrame = -1f;
                ctx.CallNextFrame(() => wNextFrame = inst.Geometry.WorldRect.Width);
                ctx.CallAfterLayout(() => wAfterLayout = inst.Geometry.WorldRect.Width);

                // 模拟 IkatHost.Step 序：PumpLogic（帧头）→ flush → tick → PumpAfterLayout。
                ctx.PumpLogic(0.016f);
                ctx.FlushPendingWrites();
                Tick(ctx);
                ctx.PumpAfterLayout();

                Assert.True(wAfterLayout > 0f,
                    $"after-layout 回调应读到已解算宽度，got {wAfterLayout}");
                Assert.Equal(0f, wNextFrame, 3); // 帧头回调先于首次 solve → 全零
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── OnUpdate ────────────────────────────────────────────────────────

        [Fact]
        public void OnUpdateFiresEachPumpWithDt()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                var root = (Container)ctx._registry.GetOrCreate(CreateRoot(ctx));
                float lastDt = -1f;
                int fires = 0;
                root.OnUpdate(dt => { lastDt = dt; fires++; });

                ctx.PumpLogic(0.1f);
                ctx.PumpLogic(0.2f);
                Assert.Equal(2, fires);
                Assert.Equal(0.2f, lastDt, 5);
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void OnUpdateSubscriptionDisposeStops()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                var root = (Container)ctx._registry.GetOrCreate(CreateRoot(ctx));
                int fires = 0;
                var reg = root.OnUpdate(_ => fires++);
                ctx.PumpLogic(0.016f);
                reg.Dispose();
                ctx.PumpLogic(0.016f);
                Assert.Equal(1, fires);
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void OnUpdateClearedOnNodeDisposeButNotOnRemoveFromParent()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                var root = (Container)ctx._registry.GetOrCreate(CreateRoot(ctx));
                var child = ctx.Create<Container>();
                root.AddChild(child);
                int fires = 0;
                child.OnUpdate(_ => fires++);

                child.RemoveFromParent();       // 不清订阅（契约）
                ctx.PumpLogic(0.016f);
                Assert.Equal(1, fires);

                child.Dispose();                // Dispose 清订阅（契约）
                ctx.PumpLogic(0.016f);
                Assert.Equal(1, fires);
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void OnUpdateExceptionIsolated()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                var root = (Container)ctx._registry.GetOrCreate(CreateRoot(ctx));
                int after = 0;
                root.OnUpdate(_ => throw new InvalidOperationException("boom"));
                root.OnUpdate(_ => after++);
                ctx.PumpLogic(0.016f);          // 第一个回调抛不阻断第二个 + 后续帧
                ctx.PumpLogic(0.016f);
                Assert.Equal(2, after);
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void OnUpdateStyleChangeSolvedSameFrame()
        {
            // 帧头泵的语义红利：回调内改 Style → FlushPendingWrites → 同一次 tick solve 生效。
            // 镜像 BatchFlushTests 的攒批验证法（绕过 flush 则本帧不生效）。
            var (stage, ctx) = StageHarness.Create();
            try
            {
                var root = (Container)ctx._registry.GetOrCreate(CreateRoot(ctx));
                var child = ctx.Create<Container>();
                root.AddChild(child);
                child.OnUpdate(_ => child.Style.Width = Length.Px(120));

                ctx.PumpLogic(0.016f);          // 回调改宽（标脏）
                ctx.FlushPendingWrites();       // flush seam（IkatHost.Step 在泵后自动调）
                Tick(ctx);
                Assert.Equal(120f, child.Geometry.LayoutRect.Width, 3);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── UnloadPackage 生命周期 ─────────────────────────────────────────

        [Fact]
        public void UnloadPackageLifecycle()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                ulong rootId = CreateRoot(ctx);
                UIPackage pkg = ctx.LoadPackage("dd", FixtureBytes("dropdown.pkg.bin"));
                Container inst = pkg.Instantiate("dropdown");
                Native.ikat_stage_append_child(H(ctx), rootId, inst._id);

                ctx.UnloadPackage("dd");        // ok：模板注册移除
                Assert.Throws<UIContractException>(() => ctx.UnloadPackage("dd"));   // 双卸抛
                // prefab 删除语义：旧句柄再实例化 → 抛；已实例化活节点不受影响。
                Assert.Throws<UIPackageException>(() => pkg.Instantiate("dropdown"));
                Assert.True(inst.ChildCount >= 1, "live instance survives unload");

                ctx.LoadPackage("dd", FixtureBytes("dropdown.pkg.bin"));   // 重载同名 ok
                Container inst2 = pkg.Instantiate("dropdown");
                Assert.NotNull(inst2);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── option / tab 派生 getter ───────────────────────────────────────

        [Fact]
        public void DropdownSelectedValuePrefersValueAttr()
        {
            var (stage, ctx, root, sel) = LoadDropdown();
            try
            {
                // fixture：opt-b value="beta-val" aria-selected → 选中项 value 直读。
                Assert.Equal("beta-val", sel.SelectedValue);
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void OptionValueFallsBackToText()
        {
            var (stage, ctx, root, sel) = LoadDropdown();
            try
            {
                var a = sel.Get<OptionItem>("opt-a");
                var c = sel.Get<OptionItem>("opt-c");
                Assert.Equal("alpha-val", a.Value);      // value 属性
                Assert.Equal("Gamma", c.Value);          // 无 value → 回落文本
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void OptionSelectedDerivesFromParent()
        {
            var (stage, ctx, root, sel) = LoadDropdown();
            try
            {
                var a = sel.Get<OptionItem>("opt-a");
                var b = sel.Get<OptionItem>("opt-b");
                Assert.False(a.Selected);
                Assert.True(b.Selected);                 // fixture bake：aria-selected on opt-b
                sel.SelectedIndex = 0;                  // 改选 → 合成值跟随
                Assert.True(a.Selected);
                Assert.False(b.Selected);
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void TabSelectedDerivesFromParentTabList()
        {
            var (stage, ctx) = StageHarness.Create();
            RegisterDefaultFont(ctx);
            try
            {
                ulong rootId = CreateRoot(ctx);
                ctx._rootId = rootId;
                Container root = (Container)ctx._registry.GetOrCreate(rootId);
                UIPackage pkg = ctx.LoadPackage("tablist", FixtureBytes("tablist.pkg.bin"));
                Container inst = pkg.Instantiate("tablist");
                Native.ikat_stage_append_child(H(ctx), rootId, inst._id);
                Tick(ctx);

                // fixture（tablist.workspace）：role=tablist id=tl 含 tab 子。
                var tablist = inst.Get<TabList>("tl");
                Assert.NotNull(tablist);
                var tabs = tablist.Query<Tab>();
                Assert.True(tabs.Count >= 2, "fixture should have >= 2 tabs");
                int selectedCount = 0;
                foreach (var t in tabs) if (t.Selected) selectedCount++;
                Assert.Equal(1, selectedCount);
                // 切换 → 合成值跟随（非字面存储）。
                tablist.SelectedIndex = tablist.SelectedIndex == 0 ? 1 : 0;
                selectedCount = 0;
                foreach (var t in tabs) if (t.Selected) selectedCount++;
                Assert.Equal(1, selectedCount);
            }
            finally { StageHarness.Destroy(stage); }
        }

        static T FindFirst<T>(Container from) where T : Node
        {
            foreach (var n in from.Query<T>()) return n;
            return null;
        }

        // ── Container.GetTemplate（设计期具名模板）─────────────────────────

        [Fact]
        public void GetTemplateReturnsCloneableBlueprint()
        {
            var (stage, ctx) = StageHarness.Create();
            RegisterDefaultFont(ctx);
            try
            {
                ulong rootId = CreateRoot(ctx);
                ctx._rootId = rootId;
                Container root = (Container)ctx._registry.GetOrCreate(rootId);
                UIPackage pkg = ctx.LoadPackage("templates", FixtureBytes("templates.pkg.bin"));
                Container inst = pkg.Instantiate("templates");
                Native.ikat_stage_append_child(H(ctx), rootId, inst._id);
                Tick(ctx);

                var tpl = inst.GetTemplate("row-tpl");
                Assert.NotNull(tpl);
                // 克隆：蓝图根是 listitem（模板的单元素子）。克隆游离；挂到 inst 内（组件
                // scoped CSS 的正确用法——[role=listitem] 规则锚定在实例域，挂外面吃不到）
                // + tick 后吃到 fixture CSS（listitem 100×30）才有 layout 产物。
                Container clone = tpl.Instantiate();
                inst.AddChild(clone);
                ctx.FlushPendingWrites();
                Tick(ctx);
                Assert.Equal(100f, clone.Geometry.LayoutRect.Width, 3);
                // 多次克隆互为独立副本。
                Container clone2 = tpl.Instantiate();
                Assert.NotSame(clone, clone2);
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void GetTemplateMissingThrowsAndNonTemplateIdThrows()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                ulong rootId = CreateRoot(ctx);
                ctx._rootId = rootId;
                Container root = (Container)ctx._registry.GetOrCreate(rootId);
                UIPackage pkg = ctx.LoadPackage("templates", FixtureBytes("templates.pkg.bin"));
                Container inst = pkg.Instantiate("templates");
                Native.ikat_stage_append_child(H(ctx), rootId, inst._id);

                Assert.Throws<UIContractException>(() => inst.GetTemplate("no-such-template"));
            }
            finally { StageHarness.Destroy(stage); }
        }
    }
}
