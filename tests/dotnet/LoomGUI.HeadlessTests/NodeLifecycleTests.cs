using System;
using System.Runtime.InteropServices;
using System.Text;
using LoomGUI.Bindings;
using Xunit;

namespace LoomGUI.HeadlessTests
{
    /// <summary>
    /// C1 投影壳基础验收：NodeRegistry（对象身份缓存）+ NodeFactory（kind byte → typed 子类）
    /// + Node 生命周期（Dispose/RemoveFromParent/Context/Parent/IsDisposed）。
    ///
    /// 全部经 headless harness P/Invoke 真 dll，不启 Unity。每个 Fact 验一条投影层不变量：
    /// 身份稳定 / kind dispatch / Dispose 后访问抛 / 根 Parent null。
    /// </summary>
    public unsafe class NodeLifecycleTests
    {
        // lib.rs:429 root parent 哨兵（与 create_root 失败哨兵 0xFFFF_FFFF 同值）。
        private const ulong InvalidNodeId = ulong.MaxValue;

        // ── NodeFactory：kind byte → typed C# 子类 ───────────────────────

        /// <summary>
        /// create_root("div") → Container（kind=0）。最常见路径，先确保它通。
        /// </summary>
        [Fact]
        public void KindDivDispatchesToContainer()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                ulong id = CreateRoot(stage, "div", "");
                Assert.NotEqual(InvalidNodeId, id);

                Node n = ctx._registry.GetOrCreate(id);
                Assert.IsType<Container>(n);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// create_root("button") → Button（kind=6）。验 control 变体也走 NodeFactory switch。
        /// </summary>
        [Fact]
        public void KindButtonDispatchesToButton()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                ulong id = CreateRoot(stage, "button", "");
                Assert.NotEqual(InvalidNodeId, id);

                Node n = ctx._registry.GetOrCreate(id);
                Assert.IsType<Button>(n);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// create_root("img") → Image（kind=8）。验叶子绘制节点 dispatch。
        /// </summary>
        [Fact]
        public void KindImgDispatchesToImage()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                ulong id = CreateRoot(stage, "img", "");
                Assert.NotEqual(InvalidNodeId, id);

                Node n = ctx._registry.GetOrCreate(id);
                Assert.IsType<Image>(n);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// create_root("span") → TextNode（kind=1）。验文本节点 dispatch（围栏 span→TextNode 映射）。
        /// </summary>
        [Fact]
        public void KindSpanDispatchesToTextNode()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                ulong id = CreateRoot(stage, "span", "");
                Assert.NotEqual(InvalidNodeId, id);

                Node n = ctx._registry.GetOrCreate(id);
                Assert.IsType<TextNode>(n);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── NodeRegistry：对象身份稳定 ──────────────────────────────────

        /// <summary>
        /// 投影层不变量（projection §2.4）：GetOrCreate 对同一 NodeId 必须返回同一 C# 实例。
        /// 订阅 / 镜像挂对象上，若每次返不同实例则丢失。强引用缓存兑现本不变量。
        /// </summary>
        [Fact]
        public void RegistryReturnsSameInstanceForSameId()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                ulong id = CreateRoot(stage, "div", "");
                Node first = ctx._registry.GetOrCreate(id);
                Node second = ctx._registry.GetOrCreate(id);
                Assert.Same(first, second);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// TryGet 在未缓存时返 false；GetOrCreate 后返 true + 同一实例。
        /// </summary>
        [Fact]
        public void TryGetReflectsCacheState()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                ulong id = CreateRoot(stage, "div", "");

                Assert.False(ctx._registry.TryGet(id, out var pre));
                Assert.Null(pre);

                Node created = ctx._registry.GetOrCreate(id);
                Assert.True(ctx._registry.TryGet(id, out var cached));
                Assert.Same(created, cached);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── Node 生命周期 ───────────────────────────────────────────────

        /// <summary>
        /// 根节点 Parent == null（FFI node_parent 返 sentinel 0xFFFF_FFFF）。
        /// 投影层契约：根无父，返 null 不抛。
        /// </summary>
        [Fact]
        public void ParentOfRootIsNull()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                ulong id = CreateRoot(stage, "div", "");
                Node n = ctx._registry.GetOrCreate(id);
                Assert.Null(n.Parent);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Dispose 标 IsDisposed + 从 registry 移除（再 GetOrCreate 会造新实例，因旧已 evict）。
        /// </summary>
        [Fact]
        public void DisposeMarksIsDisposedAndEvictsFromCache()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                ulong id = CreateRoot(stage, "div", "");
                Node n = ctx._registry.GetOrCreate(id);

                Assert.False(n.IsDisposed);
                Assert.True(ctx._registry.TryGet(id, out _));

                n.Dispose();

                Assert.True(n.IsDisposed);
                Assert.False(ctx._registry.TryGet(id, out _));   // 缓存已 evict
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Dispose 后任意公共读操作抛 ObjectDisposedException（C1 在 Context/Parent 上加 check）。
        /// 后续 C2-C7 各自在自己加的方法上补 check。
        /// </summary>
        [Fact]
        public void PostDisposeAccessThrowsObjectDisposedException()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                ulong id = CreateRoot(stage, "div", "");
                Node n = ctx._registry.GetOrCreate(id);
                n.Dispose();

                Assert.Throws<ObjectDisposedException>(() => { var _ = n.Context; });
                Assert.Throws<ObjectDisposedException>(() => { var _ = n.Parent; });
                Assert.Throws<ObjectDisposedException>(() => n.RemoveFromParent());
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Dispose 幂等：二次 Dispose 不抛（防业务方 try/finally 重复释放）。
        /// </summary>
        [Fact]
        public void DisposeIsIdempotent()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div", ""));
                n.Dispose();
                n.Dispose();   // 不抛
                Assert.True(n.IsDisposed);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── helpers ──────────────────────────────────────────────────────

        /// <summary>
        /// 调 loomgui_stage_create_root（与 HarnessSmokeTests.CreateRoot 同风格：
        /// UTF-8 字节 + fixed 钉住 + ptr+len）。返 NodeId；0xFFFF_FFFF = 失败。
        /// </summary>
        private static ulong CreateRoot(IntPtr stage, string kind, string css)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            byte[] k = Encoding.UTF8.GetBytes(kind ?? "");
            byte[] c = Encoding.UTF8.GetBytes(css ?? "");
            fixed (byte* kp = k, cp = c)
                return Native.loomgui_stage_create_root(h, kp, (nuint)k.Length, cp, (nuint)c.Length);
        }
    }
}
