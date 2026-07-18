using System;
using System.Text;
using LoomGUI.Bindings;
using Xunit;

namespace LoomGUI.HeadlessTests
{
    /// <summary>
    /// C6 投影层验收：TextNode.Text + Container.TextContent（DOM textContent 语义）。
    ///
    /// 每条 Fact 验一条投影层不变量：
    /// - TextNode.Text setter 同步写穿 core（set_text FFI 标 dirty_text）+ 缓存镜像；getter 读镜像。
    /// - Container.TextContent 读=递归拼接后代 TextNode.Text（文档序）；写=清所有子 + 挂单个 TextNode。
    /// - TextContent 写是「替换子树」语义——原所有子被移除（不 Dispose，DOM 可重挂）。
    ///
    /// 读侧 C# 镜像（_text）的已知 gap 见 LoomGUI.Nodes.cs TextNode 注释——lib.rs 无 get_text FFI，
    /// Instantiate 路径的文本不进 C# 镜像（ ponytail 推后，待首个 Instantiate 文本读回场景）。
    /// 本测试集 setup 一律经 C# setter 写文本（与读镜像对称），故全绿。
    /// </summary>
    public unsafe class TextContentTests
    {
        // lib.rs create_root/create_node 失败哨兵。
        private const uint InvalidNodeId = 0xFFFF_FFFFu;

        // ── TextNode.Text round-trip ────────────────────────────────────

        /// <summary>
        /// TextNode.Text setter + getter round-trip：写 "Hello" 读回 "Hello"。
        /// setter 同步调 set_text FFI + 缓存 _text；getter 读 _text。
        /// </summary>
        [Fact]
        public void TextNodeTextRoundTrips()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                TextNode tn = (TextNode)ctx._registry.GetOrCreate(CreateNode(stage, "span"));
                Assert.Equal("", tn.Text);   // 默认空串

                tn.Text = "Hello";
                Assert.Equal("Hello", tn.Text);

                tn.Text = "World";   // 覆盖
                Assert.Equal("World", tn.Text);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// TextNode.Text = null 当空串处理（与 DOM textContent=null 一致；.NET string 默认 null 防御）。
        /// </summary>
        [Fact]
        public void TextNodeTextNullBecomesEmpty()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                TextNode tn = (TextNode)ctx._registry.GetOrCreate(CreateNode(stage, "span"));
                tn.Text = "hi";
                tn.Text = null;
                Assert.Equal("", tn.Text);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// TextNode.Text 支持 UTF-8（中文 / emoji）。setter 编码 UTF-8 字节传 FFI；getter 读镜像。
        /// </summary>
        [Fact]
        public void TextNodeTextPreservesUnicode()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                TextNode tn = (TextNode)ctx._registry.GetOrCreate(CreateNode(stage, "span"));
                tn.Text = "你好，世界 🌍";
                Assert.Equal("你好，世界 🌍", tn.Text);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// TextNode Dispose 后访问 Text 抛 ObjectDisposedException（C1 ThrowIfDisposed 套到 getter/setter）。
        /// </summary>
        [Fact]
        public void TextNodeTextPostDisposeThrows()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                TextNode tn = (TextNode)ctx._registry.GetOrCreate(CreateNode(stage, "span"));
                tn.Dispose();
                Assert.Throws<ObjectDisposedException>(() => { var _ = tn.Text; });
                Assert.Throws<ObjectDisposedException>(() => tn.Text = "x");
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── Container.TextContent 写 ────────────────────────────────────

        /// <summary>
        /// TextContent setter 清当前所有子 + 挂单个 TextNode（设 text）。最终 ChildCount == 1，
        /// 唯一子是 TextNode 且 Text == 写入值。
        /// </summary>
        [Fact]
        public void TextContentWriteReplacesChildrenWithSingleTextNode()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                parent.AddChild(ctx._registry.GetOrCreate(CreateNode(stage, "div")));   // 预占位
                parent.AddChild(ctx._registry.GetOrCreate(CreateNode(stage, "img")));
                Assert.Equal(2, parent.ChildCount);

                parent.TextContent = "Hello";

                Assert.Equal(1, parent.ChildCount);
                Node only = parent.GetChildAt(0);
                Assert.IsType<TextNode>(only);
                Assert.Equal("Hello", ((TextNode)only).Text);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// TextContent 写后立即读 round-trip——getter 递归取唯一 TextNode 的 Text。
        /// </summary>
        [Fact]
        public void TextContentWriteThenReadRoundTrips()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                parent.TextContent = "你好";
                Assert.Equal("你好", parent.TextContent);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// TextContent = "" 仍建一个 TextNode（空文本）。DOM 语义：textContent="" ≠ 无子。
        /// </summary>
        [Fact]
        public void TextContentWriteEmptyCreatesEmptyTextNode()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                parent.TextContent = "";
                Assert.Equal(1, parent.ChildCount);
                Assert.IsType<TextNode>(parent.GetChildAt(0));
                Assert.Equal("", parent.TextContent);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// TextContent = null 当空串处理（与 TextNode.Text null 一致）。
        /// </summary>
        [Fact]
        public void TextContentWriteNullBecomesEmpty()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                parent.TextContent = null;
                Assert.Equal(1, parent.ChildCount);
                Assert.Equal("", parent.TextContent);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── Container.TextContent 读 ────────────────────────────────────

        /// <summary>
        /// TextContent 读 = 拼接直系 TextNode 子（文档序）。多 TextNode 子串接无分隔。
        /// </summary>
        [Fact]
        public void TextContentReadConcatenatesDirectTextNodeChildren()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                TextNode a = MakeText(stage, ctx, "Hello");
                TextNode b = MakeText(stage, ctx, "World");
                parent.AddChild(a);
                parent.AddChild(b);

                Assert.Equal("HelloWorld", parent.TextContent);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// TextContent 读 = 递归 Container 后代（文档序深度优先）。TextBlock 子树内 TextNode 也累加。
        /// 验 DOM textContent 全子树扫描语义。
        /// </summary>
        [Fact]
        public void TextContentReadRecursesNestedContainers()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                parent.AddChild(MakeText(stage, ctx, "A"));

                Container inner = (Container)ctx._registry.GetOrCreate(CreateNode(stage, "div"));
                inner.AddChild(MakeText(stage, ctx, "B"));
                inner.AddChild(MakeText(stage, ctx, "C"));
                parent.AddChild(inner);   // 文档序：A 在前，inner(B,C) 在后

                Assert.Equal("ABC", parent.TextContent);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// TextContent 读 = 跳过非 TextNode 叶子（Image）。只累加 TextNode._text。
        /// </summary>
        [Fact]
        public void TextContentReadSkipsNonTextNodeLeaves()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                parent.AddChild(MakeText(stage, ctx, "X"));
                parent.AddChild(ctx._registry.GetOrCreate(CreateNode(stage, "img")));   // Image 叶子贡献 0 字符
                parent.AddChild(MakeText(stage, ctx, "Y"));

                Assert.Equal("XY", parent.TextContent);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// 空 Container 的 TextContent == ""（无子 = 无文字）。
        /// </summary>
        [Fact]
        public void TextContentReadEmptyContainerReturnsEmpty()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Assert.Equal("", parent.TextContent);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// TextContent 读 = 嵌套混合结构（TextBlock + TextElement + TextNode）文档序。
        /// 验 Container 子类（TextBlock/TextElement 都是 Container 派生）也走递归路径。
        /// </summary>
        [Fact]
        public void TextContentReadHandlesMixedContainerSubclasses()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Container parent = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                // p (TextBlock) > span (TextNode "Hello") + span (TextNode " ")
                Container p = (Container)ctx._registry.GetOrCreate(CreateNode(stage, "div"));
                p.AddChild(MakeText(stage, ctx, "Hello"));
                p.AddChild(MakeText(stage, ctx, " "));
                // strong (TextElement) > span (TextNode "World")
                Container strong = (Container)ctx._registry.GetOrCreate(CreateNode(stage, "div"));
                strong.AddChild(MakeText(stage, ctx, "World"));
                parent.AddChild(p);
                parent.AddChild(strong);

                Assert.Equal("Hello World", parent.TextContent);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── helpers ──────────────────────────────────────────────────────

        /// <summary>建无父 TextNode（span）+ 经 C# setter 写文本（同步 set_text FFI + 缓存镜像）。</summary>
        private static TextNode MakeText(IntPtr stage, UIContext ctx, string text)
        {
            TextNode tn = (TextNode)ctx._registry.GetOrCreate(CreateNode(stage, "span"));
            tn.Text = text;   // C# setter 路径——与读镜像对称，避免 ghost state
            return tn;
        }

        /// <summary>建根节点（无 CSS）。返 NodeId；0xFFFF_FFFF = 失败。</summary>
        private static uint CreateRoot(IntPtr stage, string kind)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            byte[] k = Encoding.UTF8.GetBytes(kind ?? "");
            fixed (byte* kp = k)
                return Native.loomgui_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        /// <summary>建无父节点。返 NodeId；0xFFFF_FFFF = 失败。</summary>
        private static uint CreateNode(IntPtr stage, string kind)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            byte[] k = Encoding.UTF8.GetBytes(kind ?? "");
            fixed (byte* kp = k)
                return Native.loomgui_stage_create_node(h, kp, (nuint)k.Length, null, 0);
        }
    }
}
