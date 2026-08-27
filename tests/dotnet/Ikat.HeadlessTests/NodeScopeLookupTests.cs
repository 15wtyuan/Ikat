using System;
using System.Text;
using Ikat.Bindings;
using Xunit;

namespace Ikat.HeadlessTests
{
    /// <summary>
    /// C7 投影层验收：Node 作用域查找 API（Get&lt;T&gt;/TryGet&lt;T&gt;/Query&lt;T&gt;/Query(selector)）。
    ///
    /// 每条 Fact 验一条投影层不变量：
    /// - Query&lt;T&gt; 按 C# typed 子类匹配（is T 含派生），DFS 子树文档序 pre-order，不含 self。
    /// - Query(".cls" / "tag" / "tag.cls") 经 NodeKind + Classes.Contains 匹配。
    /// - Get/TryGet 经 find_node_by_id + 父链 scope-check 限定在本子树；未命中路径抛 UIContractException / 返 false。
    /// - Dispose 后访问任一查找入口抛 ObjectDisposedException（C1 ThrowIfDisposed 套到 C7 入口）。
    ///
    /// **id-lookup 正路径（GetFindsByIdAndType / GetThrowsWhenWrongType）部分 defer 到 E2**：
    /// 4a 无 set_id_attr FFI，create_node FFI 不接 id 参数，唯一能设 live 节点 id_attr 的路径是
    /// load_package + instantiate（pkg.bin 内 TemplateNode.id_attr）—— 即 E2 的 fixture pkg 工作。
    /// 故 Get/TryGet 只测「未命中」路径（id=""、id=null、id 不存在——这些路径不需要预先设 id）。
    /// 正路径在 E2 fixture pkg 落地后补测（同 ClassAffectsComputedStyle 的 defer 模式）。
    ///
    /// 全部经 headless harness P/Invoke 真 dll，不启 Unity。
    /// </summary>
    public unsafe class NodeScopeLookupTests
    {
        // lib.rs create_root 失败哨兵（与 parent / find_node_by_id 未命中同值）。
        private const ulong InvalidNodeId = ulong.MaxValue;

        // ── Query<T>()：按类型 DFS 子树 ────────────────────────────────

        /// <summary>
        /// Query&lt;Button&gt; 在 div &gt; [button, img] 子树里返单元素 Button 列表。
        /// 验 DFS pre-order + is T 类型过滤正确。
        /// </summary>
        [Fact]
        public void QueryByTypeReturnsTypedDescendants()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container root = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                AppendChild(stage, root._id, CreateNode(stage, "button"));
                AppendChild(stage, root._id, CreateNode(stage, "img"));

                var buttons = root.Query<Button>();
                Assert.Single(buttons);
                Assert.IsType<Button>(buttons[0]);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// 子树无 T 类型节点 → Query&lt;T&gt; 返空 list（不抛、不返 null）。
        /// </summary>
        [Fact]
        public void QueryByTypeEmptyWhenNoMatch()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container root = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                AppendChild(stage, root._id, CreateNode(stage, "div"));

                Assert.Empty(root.Query<Image>());
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Query&lt;Container&gt; 含 Container 派生类（Button/Link/TextBlock/...）。
        /// is T 在 C# 自动含派生——Button : Container，故 Query&lt;Container&gt; 命中 Button。
        /// </summary>
        [Fact]
        public void QueryByTypeIncludesSubclasses()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container root = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                AppendChild(stage, root._id, CreateNode(stage, "div"));
                AppendChild(stage, root._id, CreateNode(stage, "button"));
                AppendChild(stage, root._id, CreateNode(stage, "img"));

                // Container 子类：div(Container) + button(Button : Container)；img 不算。
                var containers = root.Query<Container>();
                Assert.Equal(2, containers.Count);
                Assert.IsType<Container>(containers[0]);
                Assert.IsType<Button>(containers[1]);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Query 跨多层后代：div &gt; div &gt; button，root.Query&lt;Button&gt; 命中深层 button。
        /// 验 DFS 真递归（不是只看直系子）。
        /// </summary>
        [Fact]
        public void QueryByTypeDescendsMultipleLevels()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container root = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                ulong mid = CreateNode(stage, "div");
                AppendChild(stage, root._id, mid);
                AppendChild(stage, mid, CreateNode(stage, "button"));

                Assert.Single(root.Query<Button>());
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Query&lt;T&gt; 不含 self（与 DOM querySelectorAll 一致：element.query 只查后代）。
        /// root 是 Container —— root.Query&lt;Container&gt; 不应含 root 自身。
        /// </summary>
        [Fact]
        public void QueryByTypeExcludesSelf()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container root = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                // root 是 Container；Query<Container> 在自身上不应返 root。
                Assert.Empty(root.Query<Container>());
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// 在非 Container 叶子节点上 Query 返空（无 Children 可 DFS）。
        /// </summary>
        [Fact]
        public void QueryOnLeafNodeReturnsEmpty()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node leaf = ctx._registry.GetOrCreate(CreateRoot(stage, "img"));
                Assert.Empty(leaf.Query<Node>());
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── Query(selector)：class / tag / tag.cls ──────────────────────

        /// <summary>
        /// Query(".hi") 返所有 has_class("hi") 的后代。class 经 add_class FFI 设、has_class FFI 查。
        /// </summary>
        [Fact]
        public void QueryByClassFindsNodesWithClass()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container root = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                ulong a = CreateNode(stage, "div");
                ulong b = CreateNode(stage, "div");
                AppendChild(stage, root._id, a);
                AppendChild(stage, root._id, b);

                ctx._registry.GetOrCreate(a).Classes.Add("hi");
                ctx._registry.GetOrCreate(b).Classes.Add("hi");
                ctx._registry.GetOrCreate(b).Classes.Add("bye");

                var his = root.Query(".hi");
                Assert.Equal(2, his.Count);
                Assert.True(his[0].Classes.Contains("hi"));
                Assert.True(his[1].Classes.Contains("hi"));
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// 子树无 class 节点 → Query(".cls") 返空。
        /// </summary>
        [Fact]
        public void QueryByClassEmptyWhenNoMatch()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container root = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                AppendChild(stage, root._id, CreateNode(stage, "div"));

                Assert.Empty(root.Query(".never-added"));
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Query("button") 按 tag 匹配 —— 经 get_node_kind → NodeKind → 围栏 tag 名。
        /// 建 div + button + img 子树，"button" 只命中 button 节点。
        /// </summary>
        [Fact]
        public void QueryByTagMatchesTaggedNodes()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container root = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                AppendChild(stage, root._id, CreateNode(stage, "button"));
                AppendChild(stage, root._id, CreateNode(stage, "img"));
                AppendChild(stage, root._id, CreateNode(stage, "div"));

                var buttons = root.Query("button");
                Assert.Single(buttons);
                Assert.IsType<Button>(buttons[0]);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// tag 别名：div/header/nav 都映 NodeKind.Container。selector "div" 只命中 Container 节点
        /// （注意：button 虽然 : Container，但 NodeKind=Button，不命中 "div"——tag 匹配走 NodeKind 严格 ==）。
        /// </summary>
        [Fact]
        public void QueryByTagMatchesOnlyExactNodeKind()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container root = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                AppendChild(stage, root._id, CreateNode(stage, "div"));     // NodeKind.Container
                AppendChild(stage, root._id, CreateNode(stage, "button"));   // NodeKind.Button

                // "div" 严格匹配 NodeKind.Container，不匹配 Button（Button : Container 但 kind 不同）。
                var divs = root.Query("div");
                Assert.Single(divs);
                Assert.IsType<Container>(divs[0]);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Query("button.primary")：tag AND class 同时匹配。button 无 primary class 不命中。
        /// </summary>
        [Fact]
        public void QueryByTagAndClassRequiresBoth()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container root = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                ulong b1 = CreateNode(stage, "button");
                ulong b2 = CreateNode(stage, "button");
                AppendChild(stage, root._id, b1);
                AppendChild(stage, root._id, b2);
                ctx._registry.GetOrCreate(b2).Classes.Add("primary");

                var primaryButtons = root.Query("button.primary");
                Assert.Single(primaryButtons);
                Assert.True(primaryButtons[0].Classes.Contains("primary"));
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// 未知 tag（围栏外）→ Query 返空（容错，不抛）。
        /// </summary>
        [Fact]
        public void QueryByUnknownTagReturnsEmpty()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container root = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                AppendChild(stage, root._id, CreateNode(stage, "div"));

                Assert.Empty(root.Query("video"));   // 围栏外 tag
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// null / 空 / whitespace selector → 返空 list（容错，不抛）。
        /// </summary>
        [Theory]
        [InlineData(null)]
        [InlineData("")]
        [InlineData("   ")]
        public void QueryEmptySelectorReturnsEmpty(string selector)
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container root = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                AppendChild(stage, root._id, CreateNode(stage, "div"));

                Assert.Empty(root.Query(selector));
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── 文档序（pre-order DFS）──────────────────────────────────────

        /// <summary>
        /// Query 文档序 pre-order：先 visit 直系子（按 append 顺序），再递归各子的子树。
        /// 建 div &gt; [a, b&gt;[c, d], e]，Query&lt;Container&gt; 应返 [a, b, c, d, e] 严格按文档序。
        /// </summary>
        [Fact]
        public void QueryReturnsDocumentOrder()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container root = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                ulong a = CreateNode(stage, "div");
                ulong b = CreateNode(stage, "div");
                ulong c = CreateNode(stage, "div");
                ulong d = CreateNode(stage, "div");
                ulong e = CreateNode(stage, "div");
                AppendChild(stage, root._id, a);
                AppendChild(stage, root._id, b);
                AppendChild(stage, root._id, e);
                AppendChild(stage, b, c);
                AppendChild(stage, b, d);

                var order = root.Query<Container>();
                // 预期文档序：a, b（先 visit 直系子）→ b 的子树 c, d → e。
                // pre-order：visit(b) 后立即递归 b 的子树（c, d），再回 root 的下一子 e。
                Assert.Equal(new[] { a, b, c, d, e }, AssertConvertIds(order));
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── Get<T> / TryGet<T>：未命中路径（正路径 defer 到 E2）──────────

        /// <summary>
        /// Get 在子树内无此 id 时抛 UIContractException。
        /// 这是 4a 可测的「未命中」路径——不需要预先设 id（默认子树内无任何 id_attr）。
        /// 正路径（命中 + 类型匹配）defer 到 E2 fixture pkg。
        /// </summary>
        [Fact]
        public void GetThrowsUIContractWhenMissing()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container root = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                AppendChild(stage, root._id, CreateNode(stage, "button"));

                Assert.Throws<UIContractException>(() => root.Get<Button>("never-set-id"));
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Get(null) / Get("") 抛 UIContractException（调用方写错——DOM getElementById 习惯）。
        /// null 也能 throw NE/Lazy —— 经 TryGet 内的 IsNullOrEmpty 早返 false 后 Get 抛 UIContract。
        /// </summary>
        [Theory]
        [InlineData(null)]
        [InlineData("")]
        public void GetThrowsUIContractOnNullOrEmptyId(string badId)
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container root = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Assert.Throws<UIContractException>(() => root.Get<Button>(badId));
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// TryGet 未命中返 false + node=default。宽松路径（null/空 id 也 false，不抛）。
        /// </summary>
        [Fact]
        public void TryGetReturnsFalseWhenMissing()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container root = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                AppendChild(stage, root._id, CreateNode(stage, "button"));

                Assert.False(root.TryGet<Button>("never-set-id", out var found));
                Assert.Null(found);   // default(Button) == null（Button 是引用类型）
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// TryGet(null/empty) 返 false（不抛——TryGet 是宽松查询路径，与 Get 抛互补）。
        /// </summary>
        [Theory]
        [InlineData(null)]
        [InlineData("")]
        public void TryGetReturnsFalseOnNullOrEmptyId(string badId)
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container root = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Assert.False(root.TryGet<Button>(badId, out _));
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// L1 子树 DFS 天然隔离：root1 的 Get 查不到 root2 子树内的节点。
        /// **需要 T12 .dll 重编译**（含 ikat_make_test_pkg + find_node_by_id_in_subtree FFI）。
        /// T12 后移除 Skip 即可运行。
        /// </summary>
        [Fact(Skip = "Stub: CreateChild/RootSentinel not yet on NodeRegistry. Replace body when registry supports direct pkg instantiation.")]
        public void GetOnRoot1CannotSeeRoot2SubtreeNode()
        {
            // TODO: replace with real implementation when NodeRegistry gains CreateChild
            throw new NotImplementedException();
        }

        // ── L1 subtree find（find_node_by_id_in_subtree + Get/TryGet）──────

        /// <summary>
        /// N slot 各含内部 id="badge" 节点——每个 slot.Get&lt;Container&gt;("badge") 命中自己的。
        /// 旧全局首匹配会撞到第一个 slot 的 badge；子树 DFS 修正此 bug。
        ///
        /// **需要 T12 .dll 重编译**（含 ikat_make_test_pkg + ikat_stage_find_node_by_id_in_subtree
        /// 两个新 FFI）。T12 前本测试 Skip。
        /// </summary>
        [Fact]
        public unsafe void GetOnSlotHitsOwnBadgeNotOtherSlotsBadge()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                byte[] compName = Encoding.UTF8.GetBytes("slot");
                nuint outLen;
                byte* pkgPtr;
                fixed (byte* cp = compName)
                {
                    pkgPtr = Native.ikat_make_test_pkg(cp, (nuint)compName.Length, &outLen);
                    if (pkgPtr == null)
                        throw new InvalidOperationException("make_test_pkg returned null");
                    try
                    {
                        // 加载包 + 实例化 N 个 slot
                        int rc = Native.ikat_stage_load_package(h,
                            cp, (nuint)compName.Length, pkgPtr, outLen);
                        Assert.Equal(0, rc);

                        ulong root = CreateRoot(stage, "div");
                        Container rootNode = (Container)ctx._registry.GetOrCreate(root);

                        const int N = 2;
                        ulong[] slots = new ulong[N];
                        for (int i = 0; i < N; i++)
                        {
                            slots[i] = Native.ikat_stage_instantiate(h,
                                cp, (nuint)compName.Length,
                                cp, (nuint)compName.Length);
                            Assert.NotEqual(InvalidNodeId, slots[i]);
                            AppendChild(stage, root, slots[i]);
                        }

                        // 每个 slot 子树内 Get<Container>("badge") 应命中自己的 badge
                        for (int i = 0; i < N; i++)
                        {
                            Node slotNode = ctx._registry.GetOrCreate(slots[i]);
                            Container badge = slotNode.Get<Container>("badge");
                            Assert.NotNull(badge);
                            // 验证 badge 是该 slot 的后代（父链验证）
                            ulong parent = Native.ikat_node_parent(h, badge._id);
                            Assert.Equal(slots[i], parent);
                        }

                        // 各 slot 的 badge id 互不相同
                        Container badge0 = ctx._registry.GetOrCreate(slots[0])
                            .Get<Container>("badge");
                        Container badge1 = ctx._registry.GetOrCreate(slots[1])
                            .Get<Container>("badge");
                        Assert.NotEqual(badge0._id, badge1._id);

                        // L3 边界：root.Get 不再穿透 slot 根（LOOKUP_SCOPE）——badge 只能
                        // 经 slot 根两跳访问（契约 public-api §3.1，旧行为是穿透返首个）。
                        Assert.Throws<UIContractException>(() => rootNode.Get<Container>("badge"));
                        Container viaSlot = ctx._registry.GetOrCreate(slots[0])
                            .Get<Container>("badge");
                        Assert.NotNull(viaSlot);
                    }
                    finally
                    {
                        Native.ikat_bytes_free(pkgPtr, outLen);
                    }
                }
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// TryGet 子树命中——与 GetOnSlotHitsOwnBadge 对应的宽松路径。
        /// **需要 T12 .dll 重编译。**
        /// </summary>
        [Fact]
        public unsafe void TryGetOnSlotHitsOwnBadge()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                byte[] compName = Encoding.UTF8.GetBytes("slot");
                nuint outLen;
                byte* pkgPtr;
                fixed (byte* cp = compName)
                {
                    pkgPtr = Native.ikat_make_test_pkg(cp, (nuint)compName.Length, &outLen);
                    if (pkgPtr == null)
                        throw new InvalidOperationException("make_test_pkg returned null");
                    try
                    {
                        int rc = Native.ikat_stage_load_package(h,
                            cp, (nuint)compName.Length, pkgPtr, outLen);
                        Assert.Equal(0, rc);

                        ulong root = CreateRoot(stage, "div");
                        ulong slot0 = Native.ikat_stage_instantiate(h,
                            cp, (nuint)compName.Length, cp, (nuint)compName.Length);
                        AppendChild(stage, root, slot0);

                        bool ok = ctx._registry.GetOrCreate(slot0)
                            .TryGet<Container>("badge", out var badge);
                        Assert.True(ok);
                        Assert.NotNull(badge);
                    }
                    finally
                    {
                        Native.ikat_bytes_free(pkgPtr, outLen);
                    }
                }
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── Dispose 闸门（C1 ThrowIfDisposed 套用到 C7 新入口）──────────

        /// <summary>
        /// Dispose 后访问 Get/TryGet/Query 抛 ObjectDisposedException（ThrowIfDisposed 在每个公共入口）。
        /// </summary>
        [Fact]
        public void PostDisposeAccessThrowsObjectDisposed()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container root = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                AppendChild(stage, root._id, CreateNode(stage, "button"));
                root.Dispose();

                Assert.Throws<ObjectDisposedException>(() => root.Get<Button>("any"));
                Assert.Throws<ObjectDisposedException>(() => root.TryGet<Button>("any", out _));
                Assert.Throws<ObjectDisposedException>(() => root.Query<Button>());
                Assert.Throws<ObjectDisposedException>(() => root.Query(".cls"));
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── helpers ──────────────────────────────────────────────────────

        private static ulong[] AssertConvertIds(System.Collections.Generic.IReadOnlyList<Node> nodes)
        {
            var arr = new ulong[nodes.Count];
            for (int i = 0; i < nodes.Count; i++) arr[i] = nodes[i]._id;
            return arr;
        }

        private static ulong CreateRoot(IntPtr stage, string kind)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            byte[] k = Encoding.UTF8.GetBytes(kind);
            fixed (byte* kp = k)
                return Native.ikat_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        private static ulong CreateNode(IntPtr stage, string kind)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            byte[] k = Encoding.UTF8.GetBytes(kind);
            fixed (byte* kp = k)
                return Native.ikat_stage_create_node(h, kp, (nuint)k.Length, null, 0);
        }

        private static void AppendChild(IntPtr stage, ulong parent, ulong child)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            int rc = Native.ikat_stage_append_child(h, parent, child);
            if (rc != 0)
                throw new InvalidOperationException(
                    $"append_child(parent={parent}, child={child}) failed rc={rc}");
        }

        /// <summary>
        /// L3 查找边界（Query 侧）：instance root 带 LOOKUP_SCOPE——页面级 Query&lt;T&gt;
        /// visit 后不下钻（badge 不进结果）；slot 根自身 Query 照常见内部。
        /// 与 Get/TryGet（core DFS 剪枝）同口径（main-design §4.3）。
        /// </summary>
        [Fact]
        public void QueryPrunesAtLookupScopeBoundary()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                byte[] compName = Encoding.UTF8.GetBytes("slot");
                nuint outLen;
                byte* pkgPtr;
                fixed (byte* cp = compName)
                {
                    pkgPtr = Native.ikat_make_test_pkg(cp, (nuint)compName.Length, &outLen);
                    Assert.NotEqual(IntPtr.Zero, (IntPtr)pkgPtr);
                    try
                    {
                        int rc = Native.ikat_stage_load_package(h,
                            cp, (nuint)compName.Length, pkgPtr, outLen);
                        Assert.Equal(0, rc);

                        ulong root = CreateRoot(stage, "div");
                        Container rootNode = (Container)ctx._registry.GetOrCreate(root);
                        ulong slot = Native.ikat_stage_instantiate(h,
                            cp, (nuint)compName.Length, cp, (nuint)compName.Length);
                        Assert.NotEqual(InvalidNodeId, slot);
                        AppendChild(stage, root, slot);

                        // 页面级 Query：结果含 slot 根自身，不含其内部 badge
                        //（registry 身份稳定——同 NodeId 同实例，引用相等即同一节点）。
                        Node slotNode = ctx._registry.GetOrCreate(slot);
                        Container badge = slotNode.Get<Container>("badge");
                        var pageResults = rootNode.Query<Node>();
                        Assert.Contains(slotNode, pageResults);
                        Assert.DoesNotContain(badge, pageResults);

                        // slot 根自身 Query：照常命中内部 badge。
                        var slotResults = slotNode.Query<Node>();
                        Assert.Contains(badge, slotResults);
                    }
                    finally
                    {
                        Native.ikat_bytes_free(pkgPtr, outLen);
                    }
                }
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

    }
}
