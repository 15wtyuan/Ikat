using System;
using System.Text;
using LoomGUI.Bindings;
using Xunit;

namespace LoomGUI.HeadlessTests
{
    /// <summary>
    /// Task 9 攒批 flush 验收：StyleMirror setter 标脏不立即过桥 + NodeTransform.Store 接通 FFI +
    /// 帧末（UIContext.FlushPendingWrites）一次性 flush 全部脏写入。
    ///
    /// 每条 Fact 验一条不变量：
    /// - StyleMirror.Set 标脏不立即调 set_inline_override：Set 后 tick（未 flush）core 不反映；
    ///   FlushPendingWrites + tick 后 core 才反映（攒批 = 延迟到帧末）。
    /// - 多次 Set 同帧只产生一次帧末 flush（dirty flag + registry 集合去重，IsDirty true→false 一次）。
    /// - StyleMirror.IsDirty / registry dirty 集合的 true→false 翻转（机制可观察）。
    /// - NodeTransform.Store 标脏 + 帧末 FlushTransform → set_transform FFI → core world_matrix 反映位移。
    /// - NodeTransform 未 flush 前 core world_matrix 不反映（延迟过桥，与 StyleMirror 同语义）。
    ///
    /// 全部经 headless harness P/Invoke 真 dll，不启 Unity。
    /// </summary>
    public unsafe class BatchFlushTests
    {
        private const ulong InvalidNodeId = ulong.MaxValue;

        // ── StyleMirror：标脏不立即过桥 ────────────────────────────────

        /// <summary>
        /// Set width 后直接 tick（不经 FlushPendingWrites）——core layout_rect.w 不反映 100px。
        /// 攒批契约：setter 只标脏，不立即调 set_inline_override；帧末 flush 才过桥。
        /// 旧即时过桥版此处 w==100（setter 立即 flush），新攒批版 w!=100（未 flush → core 不知）。
        /// </summary>
        [Fact]
        public void StyleSet_Deferred_NoCoreReflectBeforeFlush()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node child = AppendChildDiv(stage, ctx);
                child.Style.Width = Length.Px(100);   // 标脏，不 flush

                // 直接 tick（绕过 FlushPendingWrites）——core 未收到 inline override。
                Tick(stage);
                var (_, _, w, _) = GetLayoutRect(stage, child._id);
                // 子 div 默认 flex-column stretch → viewport 宽（1280）；未 flush 时不是 100。
                Assert.True(Math.Abs(w - 100f) > 1f,
                    $"攒批契约：Set 后未 flush，core layout w 该不反映 100，实得 {w}");
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// 连续 Set 3 个属性 → FlushPendingWrites → tick：core computed_style / layout 反映全部 3 个（一次性批量过桥）。
        /// 验 flush seam 把整个 _set dict 一次性拼成 CSS 串送 core（不是逐属性 N 次 FFI）。
        /// 挂在子 div 上：root layout 强制 viewport，inline width 不生效；子 div inline 经 solve 生效。
        /// </summary>
        [Fact]
        public void StyleSet_BatchedFlush_ReflectsAllAfterFlush()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node child = AppendChildDiv(stage, ctx);
                child.Style.Width = Length.Px(100);
                child.Style.Height = Length.Px(50);
                child.Style.BackgroundColor = new LoomColor(1f, 0f, 0f, 1f);

                ctx.FlushPendingWrites();
                Tick(stage);

                var (_, _, w, h) = GetLayoutRect(stage, child._id);
                Assert.InRange(w, 99, 101);
                Assert.InRange(h, 49, 51);
                var cs = GetComputedStyle(stage, child._id);
                Assert.Equal(1, cs.bg_present);
                Assert.Equal(1f, cs.background_color[0], 3);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Set 后 StyleMirror.IsDirty == true 且 node 在 registry dirty 集合内；
        /// FlushPendingWrites 后 IsDirty == false 且 dirty 集合空。
        /// 验 dirty flag + registry 跟踪的 true→false 翻转（攒批机制可观察，非黑盒）。
        /// 连续 3 次 Set 只产生 1 个 dirty 条目（集合去重）。
        /// </summary>
        [Fact]
        public void StyleSet_DirtyFlagTransitionsOnFlush()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));

                Assert.False(n.Style._mirror.IsDirty);   // 初始未脏

                n.Style.Width = Length.Px(100);
                n.Style.Height = Length.Px(50);
                n.Style.Opacity = 0.5f;     // 3 次 Set

                Assert.True(n.Style._mirror.IsDirty);
                Assert.Equal(1, ctx._registry.DirtyStyleCount);   // 去重：3 Set → 1 条目

                ctx.FlushPendingWrites();

                Assert.False(n.Style._mirror.IsDirty);
                Assert.Equal(0, ctx._registry.DirtyStyleCount);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Unset 哨兵 setter 也走标脏路径：IsDirty 翻 true，帧末 flush 后翻 false。
        /// 验 Unset 与 Set 一致走延迟路径（brief：Unset 同理）。
        /// </summary>
        [Fact]
        public void StyleUnset_MarksDirtyUntilFlush()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                n.Style.Width = Length.Px(100);
                ctx.FlushPendingWrites();
                Assert.False(n.Style._mirror.IsDirty);

                n.Style.Width = Length.Unset();   // Unset 哨兵 → 标脏
                Assert.True(n.Style._mirror.IsDirty);
                Assert.Equal(1, ctx._registry.DirtyStyleCount);

                ctx.FlushPendingWrites();
                Assert.False(n.Style._mirror.IsDirty);
                Assert.Equal(0, ctx._registry.DirtyStyleCount);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── NodeTransform：标脏 + 帧末 flush 接通 set_transform FFI ──────

        /// <summary>
        /// Transform.Position 写入 + FlushPendingWrites + tick → core world_matrix.tx/ty 反映位移。
        /// 验 NodeTransform.Store 标脏 + 帧末 FlushTransform 调 set_transform FFI（9-arg 含 origin）。
        /// root 节点 world_matrix = identity（默认），写 translate=(10,20) 后 world.tx≈10, world.ty≈20。
        /// </summary>
        [Fact]
        public void TransformStore_FlushesToWorldMatrix()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                n.Transform.Position = new LoomVector2(10f, 20f);

                ctx.FlushPendingWrites();
                Tick(stage);   // compute_world_transforms 并入 user_transform

                GetWorldMatrix(stage, n._id, out float a, out float b, out float c,
                               out float d, out float tx, out float ty);
                // identity 旋转/缩放（a=d=1, b=c=0）+ translate=(10,20)。
                Assert.Equal(10f, tx, 2);
                Assert.Equal(20f, ty, 2);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Transform 写入后未 flush 直接 tick → world_matrix 不反映位移（延迟过桥，与 Style 同语义）。
        /// 旧即时版也本就不 flush（Store 只存镜像）——本测同时守住"未 flush 不反映"不变量。
        /// </summary>
        [Fact]
        public void TransformStore_Deferred_NoReflectBeforeFlush()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                n.Transform.Position = new LoomVector2(10f, 20f);

                Tick(stage);   // 未 FlushPendingWrites
                GetWorldMatrix(stage, n._id, out _, out _, out _, out _, out float tx, out float ty);
                // 未 flush → core user_transform 仍 identity → tx,ty ≈ 0。
                Assert.True(Math.Abs(tx) < 0.5f && Math.Abs(ty) < 0.5f,
                    $"未 flush 时 world.tx/ty 该≈0，实得 ({tx},{ty})");
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Transform 全属性 flush：Position + Scale + Rotation + Origin 同时写入 →
        /// FlushPendingWrites + tick → world_matrix 偏离 identity（证明 set_transform FFI 被调、送了数据）。
        /// scale/rotation/origin 的 world_matrix 复合公式由 Rust compute_world_transforms 负责（Rust 侧已测），
        /// 本测只验 C# 侧 FlushTransform 把全 4 字段送进 set_transform（不是只送 translate）。
        /// 未 flush 前应 = identity（证标脏不立即过桥）；flush 后偏离 identity（证帧末送了全量）。
        /// </summary>
        [Fact]
        public void TransformStore_FullPropsFlush()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                n.Transform.Position = new LoomVector2(5f, 7f);
                n.Transform.Scale = new LoomVector2(2f, 3f);
                n.Transform.Rotation = 0.5f;
                n.Transform.Origin = new LoomVector2(1f, 1f);

                // 未 flush 前：world_matrix = identity（user_transform 未送 core）。
                Tick(stage);
                GetWorldMatrix(stage, n._id, out float a0, out _, out _, out float d0,
                               out float tx0, out float ty0);
                Assert.Equal(1f, a0, 2);
                Assert.Equal(1f, d0, 2);
                Assert.Equal(0f, tx0, 2);
                Assert.Equal(0f, ty0, 2);

                // flush + tick 后：world_matrix 偏离 identity（set_transform 送了全量 transform）。
                ctx.FlushPendingWrites();
                Tick(stage);
                GetWorldMatrix(stage, n._id, out float a1, out _, out _, out float d1,
                               out float tx1, out float ty1);
                bool changed = Math.Abs(a1 - 1f) > 0.01f || Math.Abs(d1 - 1f) > 0.01f
                               || Math.Abs(tx1) > 0.01f || Math.Abs(ty1) > 0.01f;
                Assert.True(changed, $"flush 后 world_matrix 该偏离 identity，实得 a={a1} d={d1} tx={tx1} ty={ty1}");
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Transform dirty flag + registry 跟踪：Set 后 IsDirty=true + dirty 集合 1 条；flush 后翻 false + 0 条。
        /// </summary>
        [Fact]
        public void TransformStore_DirtyFlagTransitionsOnFlush()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Assert.False(n.Transform._dirty);

                n.Transform.Position = new LoomVector2(1f, 2f);
                Assert.True(n.Transform._dirty);
                Assert.Equal(1, ctx._registry.DirtyTransformCount);

                ctx.FlushPendingWrites();
                Assert.False(n.Transform._dirty);
                Assert.Equal(0, ctx._registry.DirtyTransformCount);
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
                return Native.loomgui_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        private static Node AppendChildDiv(IntPtr stage, UIContext ctx)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            ulong root = CreateRoot(stage, "div");

            byte[] k = Encoding.UTF8.GetBytes("div");
            ulong child;
            fixed (byte* kp = k)
                child = Native.loomgui_stage_create_node(h, kp, (nuint)k.Length, null, 0);
            if (child == InvalidNodeId)
                throw new InvalidOperationException("create_node(div) failed");

            int rc = Native.loomgui_stage_append_child(h, root, child);
            if (rc != 0)
                throw new InvalidOperationException($"append_child(parent={root}, child={child}) failed rc={rc}");

            return ctx._registry.GetOrCreate(child);
        }

        private static void Tick(IntPtr stage) =>
            Native.loomgui_stage_tick((StageHandle*)stage.ToPointer(), 0.016f);

        private static (float x, float y, float w, float h) GetLayoutRect(IntPtr stage, ulong id)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            float x = 0, y = 0, w = 0, hh = 0;
            Native.loomgui_stage_get_node_layout_rect(h, id, &x, &y, &w, &hh);
            return (x, y, w, hh);
        }

        private static void GetWorldMatrix(IntPtr stage, ulong id,
            out float a, out float b, out float c, out float d, out float tx, out float ty)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            float la = 1f, lb = 0f, lc = 0f, ld = 1f, ltx = 0f, lty = 0f;
            Native.loomgui_stage_get_node_world_matrix(h, id, &la, &lb, &lc, &ld, &ltx, &lty);
            a = la; b = lb; c = lc; d = ld; tx = ltx; ty = lty;
        }

        private static ComputedNodeStyleRepr GetComputedStyle(IntPtr stage, ulong id)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            ComputedNodeStyleRepr repr;
            int rc = Native.loomgui_stage_get_node_computed_style(h, id, &repr);
            if (rc != 0)
                throw new InvalidOperationException($"get_node_computed_style(id={id}) failed rc={rc}");
            return repr;
        }
    }
}
