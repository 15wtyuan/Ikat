using System;
using System.Text;
using Ikat.Bindings;
using Xunit;

namespace Ikat.HeadlessTests
{
    /// <summary>
    /// C4 投影层验收：NodeGeometry（readonly struct 直读 FFI 快照）+ NodeTransform（标脏不 flush）。
    ///
    /// 每条 Fact 验一条不变量：
    /// - Geometry.LayoutRect 直读 FFI（get_node_layout_rect），反映 Style 写入经 tick 后的 layout 产物。
    /// - Geometry 滞后一帧（web-reflow 语义）：写 Style 后本帧 Geometry 不变，下帧（tick 后）才反映。
    /// - Geometry 每次 node.Geometry 返 fresh struct（snapshot），不缓存身份——struct 值语义。
    /// - Geometry.WorldRect / LocalToGlobal / GlobalToLocal：经 get_node_world_matrix + Affine2 变换。
    /// - NodeTransform setter 只存镜像、不调 FFI（set_transform 推后——ponytail 标脏不 flush）。
    /// - Node.Transform 同一 Node 多次访问返同一实例（projection §2.5 稳定单一实例）。
    ///
    /// 全部经 headless harness P/Invoke 真 dll，不启 Unity。
    /// </summary>
    public unsafe class NodeGeometryTransformTests
    {
        // lib.rs create_root 失败哨兵（与 parent 哨兵同值）。
        private const ulong InvalidNodeId = ulong.MaxValue;

        // ── Geometry.LayoutRect：直读 FFI ──────────────────────────────

        /// <summary>
        /// Geometry.LayoutRect 经 node.Geometry 读 layout 产物：写子 div width:100/height:50 → tick →
        /// Geometry.LayoutRect.Width/Height 反映。验 LayoutRect 走 get_node_layout_rect FFI（非 stub）。
        ///挂在 root 的子 div 上：root layout 强制 viewport 不受 inline 改；子 div inline 经 solve 生效。
        /// </summary>
        [Fact]
        public void GeometryLayoutRectReadsFFI()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node child = AppendChildDiv(stage, ctx);
                child.Style.Width = Length.Px(100);
                child.Style.Height = Length.Px(50);

                Tick(stage, ctx);
                IkatRect lr = child.Geometry.LayoutRect;
                Assert.InRange(lr.Width, 99, 101);
                Assert.InRange(lr.Height, 49, 51);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Geometry 滞后一帧（web-reflow 语义）：先写 width:50 + tick 让 layout 稳定到 50；
        /// 再改 width:100 但不 tick——Geometry 该帧仍反映旧值 50；tick 后才反映 100。
        /// projection §2.6 读时序：getter 读最近一次 solve 结果，本帧 Style 改下帧才反映。
        /// 用已稳定的旧值验"滞后"，不依赖默认 auto-width 语义（flex column 子项默认 stretch → viewport 宽）。
        /// </summary>
        [Fact]
        public void GeometryLagsOneFrame()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node child = AppendChildDiv(stage, ctx);
                child.Style.Width = Length.Px(50);
                Tick(stage, ctx);
                Assert.InRange(child.Geometry.LayoutRect.Width, 49, 51);   // 旧值已 settle 到 50

                // 改 width:100，不 tick——Geometry 该帧应仍反映旧值 50。
                child.Style.Width = Length.Px(100);
                float sameFrame = child.Geometry.LayoutRect.Width;
                Assert.InRange(sameFrame, 49, 51);   // 滞后：仍 50，未 solve

                // tick 后 Geometry 反映新值 100。
                Tick(stage, ctx);
                float nextFrame = child.Geometry.LayoutRect.Width;
                Assert.InRange(nextFrame, 99, 101);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// node.Geometry 每次访问返 fresh struct（snapshot）。struct 是值类型无身份，但验"每次新构造"
        /// 而非缓存同一可变对象：两次访问拿到的是 independent snapshot（在 FFI 数据未变时值相等）。
        /// </summary>
        [Fact]
        public void GeometryReturnsFreshSnapshot()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node child = AppendChildDiv(stage, ctx);
                child.Style.Width = Length.Px(100);
                Tick(stage, ctx);

                IkatRect lr1 = child.Geometry.LayoutRect;
                IkatRect lr2 = child.Geometry.LayoutRect;
                // 值相等（同 FFI 数据，两次读应一致）；struct 值语义，等号比较字段。
                Assert.Equal(lr1.Width, lr2.Width);
                Assert.InRange(lr1.Width, 99, 101);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── Geometry.WorldRect / LocalToGlobal / GlobalToLocal ─────────

        /// <summary>
        /// Geometry.WorldRect / LocalToGlobal / GlobalToLocal 经 get_node_world_matrix FFI + Affine2 变换。
        /// root 节点 world_matrix 通常 = identity（无父变换）——LocalToGlobal(p) == p，GlobalToLocal(p) == p。
        /// 验矩阵 FFI 通路 + apply_point 公式（identity 路径）。
        /// </summary>
        [Fact]
        public void GeometryRootWorldMatrixIsIdentity()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node root = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Tick(stage, ctx);   // compute_world_transforms 跑

                var p = new IkatVector2(10f, 20f);
                Assert.Equal(p, root.Geometry.LocalToGlobal(p));
                Assert.Equal(p, root.Geometry.GlobalToLocal(p));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// 子节点 world_matrix 至少含父 translate（root 占 viewport 0,0 + 子默认 0,0 → 子 world 应 ≈ local）。
        /// LocalToGlobal(IkatVector2) 与 LocalToGlobal(IkatRect) 一致性：LocalToGlobal(rect).Position == LocalToGlobal(rect.Position)。
        /// </summary>
        [Fact]
        public void GeometryLocalToGlobalRectConsistentWithPoint()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node child = AppendChildDiv(stage, ctx);
                child.Style.Width = Length.Px(40);
                child.Style.Height = Length.Px(30);
                Tick(stage, ctx);

                IkatRect lr = child.Geometry.LayoutRect;
                IkatRect world = child.Geometry.WorldRect;
                // WorldRect.Position 应等于 LocalToGlobal(LayoutRect.Position)。
                IkatVector2 worldOrigin = child.Geometry.LocalToGlobal(new IkatVector2(lr.X, lr.Y));
                Assert.Equal(worldOrigin.X, world.X, 2);
                Assert.Equal(worldOrigin.Y, world.Y, 2);
                // 尺寸在纯 translate 下不变。
                Assert.Equal(lr.Width, world.Width, 2);
                Assert.Equal(lr.Height, world.Height, 2);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// GlobalToLocal 是 LocalToGlobal 的逆：对任意 p，GlobalToLocal(LocalToGlobal(p)) ≈ p。
        /// 验 inverse 矩阵公式正确（非退化情形）。
        /// </summary>
        [Fact]
        public void GeometryLocalToGlobalRoundTrip()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node child = AppendChildDiv(stage, ctx);
                Tick(stage, ctx);

                var p = new IkatVector2(123.4f, 567.8f);
                IkatVector2 roundTrip = child.Geometry.GlobalToLocal(child.Geometry.LocalToGlobal(p));
                Assert.Equal(p.X, roundTrip.X, 2);
                Assert.Equal(p.Y, roundTrip.Y, 2);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── NodeTransform：标脏不 flush ────────────────────────────────

        /// <summary>
        /// Transform setter 写 Position → getter 读回同值（mirror 即时反映，不调 FFI flush）。
        /// 验 setter 走存镜像路径：set_transform 推后（ponytail: 4a 不 flush，留第一个逐帧 transform 控件）。
        /// </summary>
        [Fact]
        public void TransformSetterStoresMirrorNoFlush()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                var pos = new IkatVector2(10f, 20f);
                n.Transform.Position = pos;

                Assert.Equal(pos, n.Transform.Position);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Transform 全属性 round-trip：Position/Scale/Rotation/Origin 各写各的，互不干扰。
        /// 验镜像多字段共存 + 各 getter 读对应字段。
        /// </summary>
        [Fact]
        public void TransformAllPropsRoundTrip()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                var pos = new IkatVector2(1f, 2f);
                var scale = new IkatVector2(3f, 4f);
                var origin = new IkatVector2(5f, 6f);
                n.Transform.Position = pos;
                n.Transform.Scale = scale;
                n.Transform.Rotation = 0.5f;
                n.Transform.Origin = origin;

                Assert.Equal(pos, n.Transform.Position);
                Assert.Equal(scale, n.Transform.Scale);
                Assert.Equal(0.5f, n.Transform.Rotation);
                Assert.Equal(origin, n.Transform.Origin);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// node.Transform 多次访问返同一实例（projection §2.5：node.Transform.Position=X 与 .Scale=Y
        /// 必须改同一 NodeTransform）。若每次返新实例则镜像状态丢失。
        /// </summary>
        [Fact]
        public void TransformReturnsSameInstance()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Assert.Same(n.Transform, n.Transform);

                // 两次写不同属性经同一 mirror：都能读回（证明同一 NodeTransform 实例）。
                n.Transform.Position = new IkatVector2(10f, 20f);
                n.Transform.Scale = new IkatVector2(2f, 2f);
                Assert.Equal(new IkatVector2(10f, 20f), n.Transform.Position);
                Assert.Equal(new IkatVector2(2f, 2f), n.Transform.Scale);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Transform 未写过属性 getter 返默认值（Position/Scale=Zero? — Scale default 应是 One 才合理；
        /// Rotation=0；Origin=Zero）。验镜像 default 与业务语义对齐（Scale 默认 One = 不缩放）。
        /// </summary>
        [Fact]
        public void TransformUnwrittenReturnsDefaults()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Assert.Equal(IkatVector2.Zero, n.Transform.Position);
                Assert.Equal(IkatVector2.One, n.Transform.Scale);
                Assert.Equal(0f, n.Transform.Rotation);
                Assert.Equal(IkatVector2.Zero, n.Transform.Origin);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Geometry / Transform 在 Node Dispose 后访问抛 ObjectDisposedException（C1 ThrowIfDisposed 套到入口）。
        /// </summary>
        [Fact]
        public void GeometryTransformPostDisposeThrowsObjectDisposed()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                n.Dispose();
                Assert.Throws<ObjectDisposedException>(() => { var _ = n.Geometry; });
                Assert.Throws<ObjectDisposedException>(() => { var _ = n.Transform; });
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── helpers ──────────────────────────────────────────────────────

        private static ulong CreateRoot(IntPtr stage, string kind)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            byte[] k = Encoding.UTF8.GetBytes(kind);
            fixed (byte* kp = k)
                return Native.ikat_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        /// <summary>
        /// 建 root div + 子 div（append），返子节点的 typed wrapper。子 div 用来测 inline override
        /// 真实影响 layout（root layout 强制 viewport，inline 改 root 宽无效）。
        /// </summary>
        private static Node AppendChildDiv(IntPtr stage, UIContext ctx)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            ulong root = CreateRoot(stage, "div");

            byte[] k = Encoding.UTF8.GetBytes("div");
            ulong child;
            fixed (byte* kp = k)
                child = Native.ikat_stage_create_node(h, kp, (nuint)k.Length, null, 0);
            if (child == InvalidNodeId)
                throw new InvalidOperationException("create_node(div) failed");

            int rc = Native.ikat_stage_append_child(h, root, child);
            if (rc != 0)
                throw new InvalidOperationException($"append_child(parent={root}, child={child}) failed rc={rc}");

            return ctx._registry.GetOrCreate(child);
        }

        private static void Tick(IntPtr stage, UIContext ctx)
        {
            ctx.FlushPendingWrites();
            Native.ikat_stage_tick((StageHandle*)stage.ToPointer(), 0.016f);
        }
    }
}
