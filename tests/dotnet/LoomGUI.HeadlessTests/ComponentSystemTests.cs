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
using LoomGUI.Bindings;
using Xunit;

namespace LoomGUI.HeadlessTests
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
                return Native.loomgui_stage_create_root(h, kp, (nuint)k.Length, null, 0);
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
}
