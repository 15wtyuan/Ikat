// Component system headless tests（component-system spec T5）：
// fixture = component.pkg.bin（页面 2 个 game-item-card host + slot 投影按钮），
// 打包期展开（components/ 注册表）产物经真实 pkg → instantiate → C# 投影验证。
//
// 覆盖：CustomElement.Tag 读数、host 硬墙作用域（page.Get 不穿透 / 两跳可达）、
// 同组件多实例 id 独立、slot 投影内容归组件域。

using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using Ikat.Bindings;
using Xunit;

namespace Ikat.HeadlessTests
{
    public unsafe class ComponentSystemTests : IDisposable
    {
        IntPtr _stage;
        UIContext _ctx;
        Container _page;

        public ComponentSystemTests()
        {
            (_stage, _ctx) = StageHarness.Create();
            StageHandle* h = (StageHandle*)_stage.ToPointer();
            // Instantiate 前需 scene root（同 AnimationHandleTests.LoadFixture 模式）。
            ulong sceneRootId = CreateRoot(h, "div");
            _ctx._rootId = sceneRootId;
            string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "component.pkg.bin");
            Assert.True(File.Exists(fixturePath), $"fixture component.pkg.bin not found at {fixturePath}");
            byte[] pkgBytes = File.ReadAllBytes(fixturePath);
            UIPackage pkg = _ctx.LoadPackage("component", pkgBytes);
            _page = pkg.Instantiate("component");
        }

        static ulong CreateRoot(StageHandle* h, string kind)
        {
            byte[] k = Encoding.UTF8.GetBytes(kind);
            fixed (byte* kp = k)
                return Native.ikat_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        public void Dispose()
        {
            if (_stage != IntPtr.Zero)
            {
                StageHarness.Destroy(_stage);
                _stage = IntPtr.Zero;
            }
        }

        /// <summary>
        /// Tag 读原始 hyphen 标签字面量（pkg v35 展开保留）；NodeFactory 派发到 CustomElement 类型。
        /// </summary>
        [Fact]
        public void CustomElementTagReadsHyphenLiteral()
        {
            CustomElement card = _page.Get<CustomElement>("card");
            Assert.Equal("game-item-card", card.Tag);
            CustomElement card2 = _page.Get<CustomElement>("card2");
            Assert.Equal("game-item-card", card2.Tag);
        }

        /// <summary>
        /// 硬墙作用域：page.Get 能命中 host 自身（host 归页面域），不穿透进组件内部
        ///（投影按钮 id 只在组件实例域，须两跳访问）。
        /// </summary>
        [Fact]
        public void PageGetHitsHostButNotProjectedContent()
        {
            CustomElement card = _page.Get<CustomElement>("card");
            Assert.NotNull(card);

            // 投影内容 id 不在页面域（component.Get 穿透 = 旧 L1 行为，L3 已消除）
            Assert.Throws<UIContractException>(() => _page.Get<Button>("equip"));

            // 两跳：host 内 Get 命中投影按钮
            Button equip = card.Get<Button>("equip");
            Assert.NotNull(equip);

            // 实例 2 的 equip2 不在实例 1 的域里
            Assert.Throws<UIContractException>(() => card.Get<Button>("equip2"));
            Button equip2 = _page.Get<CustomElement>("card2").Get<Button>("equip2");
            Assert.NotNull(equip2);
        }

        /// <summary>
        /// 多实例隔离：同名内部结构（fallback 默认标题 span）在两个实例里是不同节点
        ///（各实例独立对象树 + ID 作用域，main-design §4.3）。
        /// </summary>
        [Fact]
        public void MultiInstanceIdsAreIndependent()
        {
            // card 没投影 title → fallback 默认标题在；card2 同理。两实例的同 class 节点互异。
            var titles1 = _page.Get<CustomElement>("card").Query<TextElement>();
            var titles2 = _page.Get<CustomElement>("card2").Query<TextElement>();
            Assert.NotEmpty(titles1);
            Assert.NotEmpty(titles2);
            Assert.DoesNotContain(titles1[0], titles2);

            // Query 同样守边界：页面级 Query 看不到 host 内部按钮
            var pageButtons = _page.Query<Button>();
            Assert.DoesNotContain(pageButtons, b => ReferenceEquals(b, _page.Get<CustomElement>("card").Get<Button>("equip")));
        }
    }

    /// <summary>
    /// RegisterComponent 类绑定 + 生命周期（#20）：工厂委托构造派生类（AOT 零反射）、
    /// OnConnected 在 instantiate 时 eager fire、OnDisconnected 双路径（Dispose 同步 /
    /// Rust 侧死亡经 PumpRemovedNodes 帧泵 + 天然去重）、晚注册不追改已构造 wrapper、
    /// 重复注册 fail loud。fixture 同上（component.pkg.bin，2 个 game-item-card host）。
    /// </summary>
    public unsafe class RegisterComponentLifecycleTests : IDisposable
    {
        // 静态计数器（派生类回调写）：本类测试串行跑，逐 test 归零防串扰。
        class LifecycleCard : CustomElement
        {
            public static int Connected;
            public static int Disconnected;
            public LifecycleCard(UIContext ctx, ulong id) : base(ctx, id) { }
            protected override void OnConnected() => Connected++;
            protected override void OnDisconnected() => Disconnected++;
        }

        IntPtr _stage;
        UIContext _ctx;
        Container _page;

        // 注册→load→instantiate（顺序是本组测试的被测对象，不能复用类 ctor 的既成实例）。
        (UIContext, Container) SetupRegistered()
        {
            LifecycleCard.Connected = 0;
            LifecycleCard.Disconnected = 0;
            (_stage, _ctx) = StageHarness.Create();
            StageHandle* h = (StageHandle*)_stage.ToPointer();
            _ctx._rootId = CreateRoot(h, "div");
            string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "component.pkg.bin");
            UIPackage pkg = _ctx.LoadPackage("component", File.ReadAllBytes(fixturePath));
            _ctx.RegisterComponent("game-item-card", (c, id) => new LifecycleCard(c, id));
            return (_ctx, pkg.Instantiate("component"));
        }

        static ulong CreateRoot(StageHandle* h, string kind)
        {
            byte[] k = Encoding.UTF8.GetBytes(kind);
            fixed (byte* kp = k)
                return Native.ikat_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        public void Dispose()
        {
            if (_stage != IntPtr.Zero)
            {
                StageHarness.Destroy(_stage);
                _stage = IntPtr.Zero;
            }
        }

        /// <summary>
        /// 注册后 instantiate：eager 构造派生类（DoInstantiate 的 MaterializeCustomElements
        /// 不等首次访问）+ 每实例 fire OnConnected。再实例化一页 = 新实例再 connect。
        /// </summary>
        [Fact]
        public void RegisterBeforeInstantiate_EagerlyConstructsDerivedAndFiresConnected()
        {
            var (_, page) = SetupRegistered();
            Assert.Equal(2, LifecycleCard.Connected);
            CustomElement card = page.Get<CustomElement>("card");
            Assert.IsType<LifecycleCard>(card);

            string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "component.pkg.bin");
            UIPackage pkg = _ctx.LoadPackage("component2", File.ReadAllBytes(fixturePath));
            Container page2 = pkg.Instantiate("component");
            Assert.Equal(4, LifecycleCard.Connected);
            Assert.IsType<LifecycleCard>(page2.Get<CustomElement>("card"));
        }

        /// <summary>
        /// 晚注册语义：instantiate 之后注册只影响未来构造——已构造 wrapper 不追改
        ///（身份缓存不可破坏），无 OnConnected 补发。
        /// </summary>
        [Fact]
        public void RegisterAfterInstantiate_DoesNotRetrofitExistingWrappers()
        {
            LifecycleCard.Connected = 0;
            (_stage, _ctx) = StageHarness.Create();
            StageHandle* h = (StageHandle*)_stage.ToPointer();
            _ctx._rootId = CreateRoot(h, "div");
            string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "component.pkg.bin");
            UIPackage pkg = _ctx.LoadPackage("component", File.ReadAllBytes(fixturePath));
            _page = pkg.Instantiate("component");

            _ctx.RegisterComponent("game-item-card", (c, id) => new LifecycleCard(c, id));
            CustomElement card = _page.Get<CustomElement>("card");
            Assert.Equal(typeof(CustomElement), card.GetType());
            Assert.Equal(0, LifecycleCard.Connected);
        }

        /// <summary>
        /// 重复注册 / null tag / null 工厂 → UIContractException（fail loud，同严格派哲学）。
        /// </summary>
        [Fact]
        public void RegisterComponent_ArgumentContracts()
        {
            var (ctx, _) = SetupRegistered();
            Assert.Throws<UIContractException>(
                () => ctx.RegisterComponent("game-item-card", (c, id) => new LifecycleCard(c, id)));
            Assert.Throws<UIContractException>(
                () => ctx.RegisterComponent("", (c, id) => new LifecycleCard(c, id)));
            Assert.Throws<UIContractException>(
                () => ctx.RegisterComponent("other-widget", null));
        }

        /// <summary>
        /// 用户 Dispose = 同步 OnDisconnected（回调时 core 节点已删）；随后泵一次不双发
        ///（Dispose 已 evict，PumpRemovedNodes 无 wrapper 可命中 = 天然去重）。
        /// </summary>
        [Fact]
        public void DisposeFiresDisconnectedSynchronously_PumpDoesNotDoubleFire()
        {
            var (_, page) = SetupRegistered();
            LifecycleCard card = (LifecycleCard)page.Get<CustomElement>("card");
            card.Dispose();
            Assert.Equal(1, LifecycleCard.Disconnected);
            Assert.True(card.IsDisposed);

            _ctx.PumpRemovedNodes();
            Assert.Equal(1, LifecycleCard.Disconnected);
        }

        /// <summary>
        /// Rust 侧死亡（不经 C# Dispose）：PumpRemovedNodes evict wrapper + fire
        /// OnDisconnected；子树内已物化的非组件 wrapper（投影按钮）顺带 evict 标
        /// _disposed——死亡变显式。子树删除顺序：后代先于 host（释放序）。
        /// </summary>
        [Fact]
        public void RustSideDeath_PumpFiresDisconnectedAndEvictsSubtreeWrappers()
        {
            var (_, page) = SetupRegistered();
            CustomElement card = page.Get<CustomElement>("card");
            Button equip = card.Get<Button>("equip");
            Assert.False(equip.IsDisposed);

            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Assert.Equal(0, Native.ikat_stage_remove_node(h, card._id));

            Assert.Equal(0, LifecycleCard.Disconnected);
            _ctx.PumpRemovedNodes();
            Assert.Equal(1, LifecycleCard.Disconnected);
            Assert.True(card.IsDisposed);
            Assert.True(equip.IsDisposed, "子树已物化 wrapper 顺带 evict（死亡显式化）");
        }
    }
}
