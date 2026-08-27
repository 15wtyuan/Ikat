using System;
using System.Text;
using Ikat.Bindings;
using Xunit;

namespace Ikat.HeadlessTests
{
    /// <summary>
    /// C6 投影层验收：Container 树写操作（AddChild / InsertChild / RemoveChild /
    /// SetChildIndex / SwapChildren / SwapChildrenAt）。
    ///
    /// 每条 Fact 验一条投影层不变量：
    /// - AddChild/InsertChild 返操作的 Node（registry 身份稳定——同一 NodeId 永远返同一实例）。
    /// - RemoveChild 不 Dispose 子（DOM 语义：可重挂到别处）。与 C1 Node.Dispose 区别（递归永久销毁）。
    /// - SetChildIndex = RemoveChild + InsertChild 组合，索引偏移经 Remove 后 count-1 自然吸收。
    /// - SwapChildren 索引偏移：先移高位再移低位（移高位不影低位索引），再按原索引插回。
    ///
    /// 全部经 headless harness P/Invoke 真 dll，不启 Unity。setup 走 raw FFI 建独立节点 + C# wrapper 写。
    /// </summary>
    public unsafe class ContainerTreeWriteOpsTests
    {
        // lib.rs create_root/create_node 失败哨兵（与 root parent 哨兵同值 0xFFFF_FFFF）。
        private const ulong InvalidNodeId = ulong.MaxValue;

        // ── AddChild ─────────────────────────────────────────────────────

        /// <summary>
        /// AddChild 返回操作的 Node（同一对象——identity stable），并挂到末尾。
        /// 验 DOM AppendChild 语义 + projection §2.4 身份稳定不变量。
        /// </summary>
        [Fact]
        public void AddChildReturnsSameNodeAndAppendsToEnd()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Container a = (Container)ctx._registry.GetOrCreate(CreateNode(stage, "div"));
                Container b = (Container)ctx._registry.GetOrCreate(CreateNode(stage, "div"));

                Assert.Same(a, parent.AddChild(a));   // 返同一引用
                Assert.Same(b, parent.AddChild(b));
                Assert.Equal(2, parent.ChildCount);
                Assert.Same(a, parent.GetChildAt(0));   // 按 append 顺序
                Assert.Same(b, parent.GetChildAt(1));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// AddChild null 抛 ArgumentNullException（.NET 习惯参数校）。
        /// </summary>
        [Fact]
        public void AddChildNullThrowsArgumentNull()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Assert.Throws<ArgumentNullException>(() => parent.AddChild<Node>(null));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// AddChild 已 Dispose 的子抛 ObjectDisposedException（stale 句柄不该再传 API）。
        /// </summary>
        [Fact]
        public void AddChildDisposedChildThrowsObjectDisposed()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Container child = (Container)ctx._registry.GetOrCreate(CreateNode(stage, "div"));
                child.Dispose();
                Assert.Throws<ObjectDisposedException>(() => parent.AddChild(child));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── InsertChild ──────────────────────────────────────────────────

        /// <summary>
        /// InsertChild(c, i) 把 c 插到「当前第 i 子之前」。i=0 头插，i=Count 末尾追加。
        /// 验 insert_before FFI 的 refChild 查找 + index→ref_id 转换。
        /// </summary>
        [Fact]
        public void InsertChildInsertsBeforeNthChild()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Container a = (Container)ctx._registry.GetOrCreate(CreateNode(stage, "div"));
                Container b = (Container)ctx._registry.GetOrCreate(CreateNode(stage, "div"));
                Container c = (Container)ctx._registry.GetOrCreate(CreateNode(stage, "div"));
                parent.AddChild(a);
                parent.AddChild(b);   // 当前 [a, b]

                Assert.Same(c, parent.InsertChild(c, 1));   // 返同一引用
                Assert.Equal(3, parent.ChildCount);
                Assert.Same(a, parent.GetChildAt(0));
                Assert.Same(c, parent.GetChildAt(1));   // c 插到 b 之前
                Assert.Same(b, parent.GetChildAt(2));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// InsertChild(c, ChildCount) 等价 append——insert_before ref_id=INVALID。
        /// </summary>
        [Fact]
        public void InsertChildAtCountAppends()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Container a = (Container)ctx._registry.GetOrCreate(CreateNode(stage, "div"));
                Container b = (Container)ctx._registry.GetOrCreate(CreateNode(stage, "div"));
                parent.AddChild(a);

                parent.InsertChild(b, parent.ChildCount);   // i == 1 == count
                Assert.Same(b, parent.GetChildAt(1));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// InsertChild 越界（负数 / &gt; ChildCount）抛 ArgumentOutOfRangeException。
        /// </summary>
        [Theory]
        [InlineData(-1)]
        [InlineData(2)]      // 空 parent，count=0；i=2 越界
        [InlineData(100)]
        public void InsertChildOutOfRangeThrows(int badIndex)
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Container c = (Container)ctx._registry.GetOrCreate(CreateNode(stage, "div"));
                Assert.Throws<ArgumentOutOfRangeException>(() => parent.InsertChild(c, badIndex));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── RemoveChild（DOM 语义：不 Dispose）───────────────────────────

        /// <summary>
        /// RemoveChild 摘子并使 ChildCount 递减。其余子保持原位（append 顺序）。
        /// </summary>
        [Fact]
        public void RemoveChildDecrementsCountAndKeepsRemaining()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Container a = (Container)ctx._registry.GetOrCreate(CreateNode(stage, "div"));
                Container b = (Container)ctx._registry.GetOrCreate(CreateNode(stage, "div"));
                parent.AddChild(a);
                parent.AddChild(b);

                parent.RemoveChild(a);
                Assert.Equal(1, parent.ChildCount);
                Assert.Same(b, parent.GetChildAt(0));   // b 保持原位（移到 0）
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// RemoveChild 不 Dispose 子：c.IsDisposed == false。区别于 Node.Dispose（C1 永久销毁）。
        /// 验 DOM 语义——子仍 live，可后续访问 Text/Style 等。
        /// </summary>
        [Fact]
        public void RemoveChildKeepsNodeAlive()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Container child = (Container)ctx._registry.GetOrCreate(CreateNode(stage, "div"));
                parent.AddChild(child);

                parent.RemoveChild(child);
                Assert.False(child.IsDisposed);   // DOM 语义：摘 ≠ 销毁
                Assert.Null(child.Parent);         // parent 指针清
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// RemoveChild 后的子可重挂到别处（DOM re-parentable）。
        /// 验 RemoveChild ≠ Dispose 的可观察后果——重挂成功且新 parent.Children 含 c。
        /// </summary>
        [Fact]
        public void RemovedChildIsReparentable()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parentA = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Container parentB = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Container child = (Container)ctx._registry.GetOrCreate(CreateNode(stage, "div"));

                parentA.AddChild(child);
                parentA.RemoveChild(child);
                parentB.AddChild(child);   // 重挂——成功（child.parent 已 None）

                Assert.Equal(1, parentB.ChildCount);
                Assert.Same(child, parentB.GetChildAt(0));
                Assert.Same(parentB, child.Parent);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// RemoveChild 非 直系子 抛 ArgumentException（DOM NotFoundError 等价）。
        /// 含：别的 parent 的子 / 根节点 / 无关节点。
        /// </summary>
        [Fact]
        public void RemoveChildNonChildThrowsArgument()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Container otherRoot = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));

                Assert.Throws<ArgumentException>(() => parent.RemoveChild(otherRoot));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// RemoveChild null 抛 ArgumentNullException。
        /// </summary>
        [Fact]
        public void RemoveChildNullThrowsArgumentNull()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Assert.Throws<ArgumentNullException>(() => parent.RemoveChild(null));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── SetChildIndex ────────────────────────────────────────────────

        /// <summary>
        /// SetChildIndex(c, i) 把 c 移到最终位置 i。前移 / 后移 / 同位 no-op 三种情形。
        /// 组合实现：RemoveChild + InsertChild；Remove 后 count-1 自然吸收索引偏移。
        /// </summary>
        [Theory]
        [InlineData(0, 2, new[] { "b", "c", "a", "d" })]    // 前移：a 0→2
        [InlineData(3, 1, new[] { "a", "d", "b", "c" })]    // 后移：d 3→1
        [InlineData(1, 1, new[] { "a", "b", "c", "d" })]    // 同位 no-op
        public void SetChildIndexReorders(int from, int to, string[] expectedOrder)
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                // 建 4 子（按字母 a/b/c/d），用 class 标识便于断言最终顺序。
                Container a = Tag(stage, ctx, "a");
                Container b = Tag(stage, ctx, "b");
                Container c = Tag(stage, ctx, "c");
                Container d = Tag(stage, ctx, "d");
                parent.AddChild(a);
                parent.AddChild(b);
                parent.AddChild(c);
                parent.AddChild(d);

                Container moving = (Container)parent.GetChildAt(from);
                parent.SetChildIndex(moving, to);

                Assert.Equal(expectedOrder,
                    new[] { TagOf(parent.GetChildAt(0)), TagOf(parent.GetChildAt(1)),
                            TagOf(parent.GetChildAt(2)), TagOf(parent.GetChildAt(3)) });
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// SetChildIndex 越界（i &gt;= ChildCount）抛 ArgumentOutOfRange——c 已占一槽，最终位数同 ChildCount。
        /// </summary>
        [Fact]
        public void SetChildIndexOutOfRangeThrows()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Container a = Tag(stage, ctx, "a");
                Container b = Tag(stage, ctx, "b");
                parent.AddChild(a);
                parent.AddChild(b);

                Assert.Throws<ArgumentOutOfRangeException>(() => parent.SetChildIndex(a, 2));   // count=2，i=2 越界
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── SwapChildren（索引偏移处理）──────────────────────────────────

        /// <summary>
        /// SwapChildren(a, b) 交换 a/b 位置。覆盖：首末交换 / 相邻交换 / 含中位交换。
        /// 算法：先移高位（不影低位索引），再移低位，最后按原索引插回。
        /// </summary>
        [Theory]
        [InlineData(0, 3, new[] { "d", "b", "c", "a" })]    // 首末
        [InlineData(1, 2, new[] { "a", "c", "b", "d" })]    // 相邻中位
        [InlineData(0, 1, new[] { "b", "a", "c", "d" })]    // 相邻首
        [InlineData(2, 3, new[] { "a", "b", "d", "c" })]    // 相邻末
        public void SwapChildrenSwapsPositions(int ia, int ib, string[] expectedOrder)
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Container a = Tag(stage, ctx, "a");
                Container b = Tag(stage, ctx, "b");
                Container c = Tag(stage, ctx, "c");
                Container d = Tag(stage, ctx, "d");
                parent.AddChild(a);
                parent.AddChild(b);
                parent.AddChild(c);
                parent.AddChild(d);

                parent.SwapChildren(parent.GetChildAt(ia), parent.GetChildAt(ib));

                Assert.Equal(expectedOrder,
                    new[] { TagOf(parent.GetChildAt(0)), TagOf(parent.GetChildAt(1)),
                            TagOf(parent.GetChildAt(2)), TagOf(parent.GetChildAt(3)) });
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// SwapChildren 同节点（ReferenceEquals）no-op——索引相同早退路径。
        /// </summary>
        [Fact]
        public void SwapChildrenSameNodeIsNoOp()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Container a = Tag(stage, ctx, "a");
                Container b = Tag(stage, ctx, "b");
                parent.AddChild(a);
                parent.AddChild(b);

                parent.SwapChildren(a, a);   // 同节点
                Assert.Equal(new[] { "a", "b" },
                    new[] { TagOf(parent.GetChildAt(0)), TagOf(parent.GetChildAt(1)) });
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// SwapChildrenAt(a, b) 按索引交换——委托 SwapChildren。索引越界抛（GetChildAt 路径）。
        /// </summary>
        [Fact]
        public void SwapChildrenAtSwaps()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Container a = Tag(stage, ctx, "a");
                Container b = Tag(stage, ctx, "b");
                Container c = Tag(stage, ctx, "c");
                parent.AddChild(a);
                parent.AddChild(b);
                parent.AddChild(c);

                parent.SwapChildrenAt(0, 2);
                Assert.Equal(new[] { "c", "b", "a" },
                    new[] { TagOf(parent.GetChildAt(0)), TagOf(parent.GetChildAt(1)), TagOf(parent.GetChildAt(2)) });
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── Dispose 闸门（C1 ThrowIfDisposed 套用到 C6 写操作）──────────

        /// <summary>
        /// Container 自身 Dispose 后调写操作抛 ObjectDisposedException（C1 ThrowIfDisposed 套用到 C6 入口）。
        /// </summary>
        [Fact]
        public void PostDisposeWriteThrowsObjectDisposed()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Container child = (Container)ctx._registry.GetOrCreate(CreateNode(stage, "div"));
                parent.AddChild(child);
                parent.Dispose();

                Container other = (Container)ctx._registry.GetOrCreate(CreateNode(stage, "div"));
                Assert.Throws<ObjectDisposedException>(() => parent.AddChild(other));
                Assert.Throws<ObjectDisposedException>(() => parent.InsertChild(other, 0));
                Assert.Throws<ObjectDisposedException>(() => parent.RemoveChild(child));
                Assert.Throws<ObjectDisposedException>(() => parent.SetChildIndex(child, 0));
                Assert.Throws<ObjectDisposedException>(() => parent.SwapChildren(child, other));
                Assert.Throws<ObjectDisposedException>(() => parent.SwapChildrenAt(0, 1));
                Assert.Throws<ObjectDisposedException>(() => { parent.TextContent = "x"; });
                Assert.Throws<ObjectDisposedException>(() => { var _ = parent.TextContent; });
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── helpers ──────────────────────────────────────────────────────

        /// <summary>建无父 div 节点，挂一个 class=tag 的标记（用 class 作 label 便于断言顺序）。</summary>
        private static Container Tag(IntPtr stage, UIContext ctx, string tag)
        {
            ulong id = CreateNode(stage, "div");
            Node n = ctx._registry.GetOrCreate(id);
            n.Classes.Add(tag);
            return (Container)n;
        }

        /// <summary>取首个 class 作 label（测试 fixture 保证每节点只挂一个 tag class）。</summary>
        private static string TagOf(Node n)
        {
            foreach (string cls in new[] { "a", "b", "c", "d" })
                if (n.Classes.Contains(cls)) return cls;
            return "?";
        }

        /// <summary>建根节点（无 CSS）。返 NodeId；0xFFFF_FFFF = 失败。</summary>
        private static ulong CreateRoot(IntPtr stage, string kind)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            byte[] k = Encoding.UTF8.GetBytes(kind ?? "");
            fixed (byte* kp = k)
                return Native.ikat_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        /// <summary>建无父节点（后续 AddChild 挂父）。返 NodeId；0xFFFF_FFFF = 失败。</summary>
        private static ulong CreateNode(IntPtr stage, string kind)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            byte[] k = Encoding.UTF8.GetBytes(kind ?? "");
            fixed (byte* kp = k)
                return Native.ikat_stage_create_node(h, kp, (nuint)k.Length, null, 0);
        }
    }
}
