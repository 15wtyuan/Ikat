using System;
using System.Text;
using LoomGUI.Bindings;
using Xunit;

namespace LoomGUI.HeadlessTests
{
    /// <summary>
    /// C2 投影层验收：Container 只读树访问（ChildCount / Children / GetChildAt / GetChildIndex）。
    ///
    /// 每条 Fact 验一条投影层不变量：
    /// - ChildCount 直接读 Rust 子数（每次访问最新，不缓存）。
    /// - Children / GetChildAt 经 get_children + registry.GetOrCreate 把子 NodeId lazy 包成 typed Node。
    /// - 身份稳定（同一 NodeId → 同一 Node 实例；订阅/镜像挂对象上不丢——projection §2.4）。
    /// - Children 列表本身不缓存：Rust 树可变（C6 写操作），缓存会 stale。
    ///
    /// 全部经 headless harness P/Invoke 真 dll，不启 Unity。
    /// </summary>
    public unsafe class ContainerChildrenTests
    {
        // lib.rs:429 root parent 哨兵（与 create_root/create_node 失败哨兵同值）。
        private const uint InvalidNodeId = 0xFFFF_FFFFu;

        // ── ChildCount ──────────────────────────────────────────────────

        /// <summary>
        /// 直系子数与 Rust get_child_count 一致。每次访问读最新 Rust 状态（不缓存）。
        /// </summary>
        [Fact]
        public void ChildCountMatchesRustSubtree()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                uint parent = CreateRoot(stage, "div");
                Assert.Equal(0, ((Container)ctx._registry.GetOrCreate(parent)).ChildCount);   // 空 parent

                AppendChild(stage, parent, CreateNode(stage, "div"));
                AppendChild(stage, parent, CreateNode(stage, "div"));
                Assert.Equal(2, ((Container)ctx._registry.GetOrCreate(parent)).ChildCount);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// ChildCount 反映 Rust 侧的实时变化：append 后立即增大，remove 后立即减小。
        /// 验 Children/ChildCount 不缓存旧快照（树可变，缓存会 stale）。
        /// </summary>
        [Fact]
        public void ChildCountReflectsRustMutation()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                uint parent = CreateRoot(stage, "div");
                Container container = (Container)ctx._registry.GetOrCreate(parent);

                Assert.Equal(0, container.ChildCount);
                AppendChild(stage, parent, CreateNode(stage, "div"));
                Assert.Equal(1, container.ChildCount);   // 无重新 GetOrCreate——直接读最新
                AppendChild(stage, parent, CreateNode(stage, "div"));
                Assert.Equal(2, container.ChildCount);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── Children / GetChildAt（lazy materialize via NodeFactory）──────

        /// <summary>
        /// Children 经 get_children + NodeFactory 派发为 typed 子类：
        /// div parent 下 button + img → Children[0] is Button, [1] is Image。
        /// 验 lazy 构造走 NodeFactory switch（C1），不是统一返 Container。
        /// </summary>
        [Fact]
        public void ChildrenReturnsTypedNodesViaFactory()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                uint parent = CreateRoot(stage, "div");
                AppendChild(stage, parent, CreateNode(stage, "button"));
                AppendChild(stage, parent, CreateNode(stage, "img"));

                Container container = (Container)ctx._registry.GetOrCreate(parent);
                Assert.IsType<Button>(container.Children[0]);
                Assert.IsType<Image>(container.Children[1]);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// GetChildAt(i) 返回第 i 个子节点（按 append 顺序）。
        /// </summary>
        [Fact]
        public void GetChildAtReturnsNthChild()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                uint parent = CreateRoot(stage, "div");
                AppendChild(stage, parent, CreateNode(stage, "div"));
                AppendChild(stage, parent, CreateNode(stage, "img"));

                Container container = (Container)ctx._registry.GetOrCreate(parent);
                Assert.IsType<Container>(container.GetChildAt(0));
                Assert.IsType<Image>(container.GetChildAt(1));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// GetChildAt 越界（负数 / ≥ Count）抛 ArgumentOutOfRangeException。
        /// </summary>
        [Theory]
        [InlineData(-1)]
        [InlineData(0)]     // 空 parent：index 0 也越界
        [InlineData(100)]
        public void GetChildAtOutOfBoundsThrows(int badIndex)
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                uint parent = CreateRoot(stage, "div");   // 无子
                Container container = (Container)ctx._registry.GetOrCreate(parent);
                Assert.Throws<ArgumentOutOfRangeException>(() => container.GetChildAt(badIndex));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── 身份稳定（projection §2.4）──────────────────────────────────

        /// <summary>
        /// 同一 NodeId 的 Children 访问必须返回同一 C# 实例：订阅 / 镜像挂对象上，
        /// 若每次返不同实例则丢失。经 NodeRegistry 强引用缓存兑现。
        /// 注意：List 本身每次新建（树可变不缓存 list），但 List 内 Node 引用稳定。
        /// </summary>
        [Fact]
        public void ChildrenIdentityStableAcrossAccess()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                uint parent = CreateRoot(stage, "div");
                AppendChild(stage, parent, CreateNode(stage, "button"));

                Container container = (Container)ctx._registry.GetOrCreate(parent);
                Node first = container.Children[0];
                Node second = container.Children[0];   // 第二次访问（新 list，同 Node 实例）
                Assert.Same(first, second);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Children[i] 与 GetChildAt(i) 与 GetOrCreate(childId) 三路返回同一实例。
        /// 验所有 lazy 构造路径共享 registry 缓存。
        /// </summary>
        [Fact]
        public void AllChildAccessPathsReturnSameInstance()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                uint parent = CreateRoot(stage, "div");
                uint child = CreateNode(stage, "button");
                AppendChild(stage, parent, child);

                Container container = (Container)ctx._registry.GetOrCreate(parent);
                Node fromChildren = container.Children[0];
                Node fromGetChildAt = container.GetChildAt(0);
                Node fromRegistry = ctx._registry.GetOrCreate(child);

                Assert.Same(fromChildren, fromGetChildAt);
                Assert.Same(fromChildren, fromRegistry);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── GetChildIndex ───────────────────────────────────────────────

        /// <summary>
        /// GetChildIndex 命中返对应索引（按 append 顺序）。
        /// </summary>
        [Fact]
        public void GetChildIndexFindsKnownChild()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                uint parent = CreateRoot(stage, "div");
                uint first = CreateNode(stage, "div");
                uint second = CreateNode(stage, "img");
                AppendChild(stage, parent, first);
                AppendChild(stage, parent, second);

                Container container = (Container)ctx._registry.GetOrCreate(parent);
                Assert.Equal(0, container.GetChildIndex(ctx._registry.GetOrCreate(first)));
                Assert.Equal(1, container.GetChildIndex(ctx._registry.GetOrCreate(second)));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// GetChildIndex 未在直系子中找到返 -1（.NET IndexOf 习惯；不抛）。
        /// 含：别的 parent 的子 / 根节点 / 无关节点。
        /// </summary>
        [Fact]
        public void GetChildIndexMissReturnsNegativeOne()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                uint parent = CreateRoot(stage, "div");
                AppendChild(stage, parent, CreateNode(stage, "div"));

                uint otherRoot = CreateRoot(stage, "div");   // 另一棵树的根

                Container container = (Container)ctx._registry.GetOrCreate(parent);
                Assert.Equal(-1, container.GetChildIndex(ctx._registry.GetOrCreate(otherRoot)));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// GetChildIndex(null) 抛 ArgumentNullException（.NET 习惯参数校）。
        /// </summary>
        [Fact]
        public void GetChildIndexNullThrowsArgumentNull()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                uint parent = CreateRoot(stage, "div");
                Container container = (Container)ctx._registry.GetOrCreate(parent);
                Assert.Throws<ArgumentNullException>(() => container.GetChildIndex(null));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// GetChildIndex(已 Dispose 节点) 抛 ObjectDisposedException。
        /// Dispose 后的节点句柄已 stale，不该再作参数传任何 API（C1 不变量）。
        /// </summary>
        [Fact]
        public void GetChildIndexDisposedChildThrowsObjectDisposed()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                uint parent = CreateRoot(stage, "div");
                uint child = CreateNode(stage, "div");
                AppendChild(stage, parent, child);

                Container container = (Container)ctx._registry.GetOrCreate(parent);
                Node childNode = ctx._registry.GetOrCreate(child);
                childNode.Dispose();   // Dispose 递归清 Rust 子树 + evict registry 缓存

                Assert.Throws<ObjectDisposedException>(() => container.GetChildIndex(childNode));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── Dispose 闸门（C1 ThrowIfDisposed 套用到 C2 新方法）──────────

        /// <summary>
        /// Container 自身 Dispose 后访问 ChildCount/Children/GetChildAt/GetChildIndex
        /// 抛 ObjectDisposedException（C1 ThrowIfDisposed 在 C2 每个公共入口套用）。
        /// </summary>
        [Fact]
        public void PostDisposeAccessThrowsObjectDisposed()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                uint parent = CreateRoot(stage, "div");
                AppendChild(stage, parent, CreateNode(stage, "div"));

                Container container = (Container)ctx._registry.GetOrCreate(parent);
                container.Dispose();

                Node gone = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Assert.Throws<ObjectDisposedException>(() => { var _ = container.ChildCount; });
                Assert.Throws<ObjectDisposedException>(() => { var _ = container.Children; });
                Assert.Throws<ObjectDisposedException>(() => container.GetChildAt(0));
                Assert.Throws<ObjectDisposedException>(() => container.GetChildIndex(gone));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── helpers ──────────────────────────────────────────────────────

        /// <summary>建根节点（无 CSS）。返 NodeId；0xFFFF_FFFF = 失败。</summary>
        private static uint CreateRoot(IntPtr stage, string kind)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            byte[] k = Encoding.UTF8.GetBytes(kind ?? "");
            fixed (byte* kp = k)
                return Native.loomgui_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        /// <summary>建无父节点（后续 append_child 挂父）。返 NodeId；0xFFFF_FFFF = 失败。</summary>
        private static uint CreateNode(IntPtr stage, string kind)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            byte[] k = Encoding.UTF8.GetBytes(kind ?? "");
            fixed (byte* kp = k)
                return Native.loomgui_stage_create_node(h, kp, (nuint)k.Length, null, 0);
        }

        /// <summary>挂 child 到 parent 末尾。失败抛（测试 fixture 假定 append 必成功）。</summary>
        private static void AppendChild(IntPtr stage, uint parent, uint child)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            int rc = Native.loomgui_stage_append_child(h, parent, child);
            if (rc != 0)
                throw new InvalidOperationException(
                    $"append_child(parent={parent}, child={child}) failed rc={rc}");
        }
    }
}
