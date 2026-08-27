using System;
using System.Text;
using Ikat.Bindings;
using Xunit;

namespace Ikat.HeadlessTests
{
    /// <summary>
    /// C5 投影层验收：ClassList 即时过桥（Add/Remove/Contains/Toggle/Set/Replace → class FFI）。
    ///
    /// 每条 Fact 验一条不变量：
    /// - Add/Remove/Contains 经 add_class/remove_class/has_class FFI round-trip（core 状态真反映）。
    /// - Toggle/Set/Replace 是 C# 组合（Contains + Add/Remove）语义正确。
    /// - node.Classes 多次访问返同一 ClassList 实例（projection §2.5 稳定单一实例）。
    /// - Dispose 后访问 Classes 抛 ObjectDisposedException（C1 ThrowIfDisposed 套到 Classes 入口）。
    ///
    /// ClassAffectsComputedStyle（class→cascade→computed_style）defer 到 E：本仓库无运行时 StyleSheet
    /// 注入 FFI（lib.rs grep 无 add_rule/inject_rule；UIContext.StyleSheet throw NE），规则仅打包期
    /// bake 进 pkg.bin，要验 class 驱动 cascade 需 E2 造含 .foo{color:red} 规则的 fixture pkg。
    ///
    /// 全部经 headless harness P/Invoke 真 dll，不启 Unity。
    /// </summary>
    public unsafe class NodeClassListTests
    {
        // lib.rs create_root 失败哨兵（与 parent 哨兵同值）。
        private const ulong InvalidNodeId = ulong.MaxValue;

        // ── Add / Contains round-trip ───────────────────────────────────

        /// <summary>
        /// Add("hi") → Contains("hi") == true；未加的 class 名 Contains == false。
        /// 验 add_class + has_class FFI round-trip 全链通（含 Node.Classes lazy 造 + CallAdd 路径）。
        /// </summary>
        [Fact]
        public void AddThenContainsReturnsTrue()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));

                Assert.False(n.Classes.Contains("hi"));   // 加之前 false
                n.Classes.Add("hi");

                Assert.True(n.Classes.Contains("hi"));    // 加之后 true
                Assert.False(n.Classes.Contains("bye"));  // 其它 class 仍 false
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── Remove ──────────────────────────────────────────────────────

        /// <summary>
        /// Add + Remove → Contains 翻回 false。验 remove_class FFI 路径 + core 状态真清除。
        /// </summary>
        [Fact]
        public void RemoveClearsContains()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                n.Classes.Add("hi");
                Assert.True(n.Classes.Contains("hi"));

                n.Classes.Remove("hi");
                Assert.False(n.Classes.Contains("hi"));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// Remove 未加的 class → no-op（不抛、Contains 仍 false）。对齐 DOM classList.remove 习惯。
        /// </summary>
        [Fact]
        public void RemoveAbsentClassIsNoOp()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                n.Classes.Remove("never-added");   // 不抛
                Assert.False(n.Classes.Contains("never-added"));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── Toggle（C# 组合）────────────────────────────────────────────

        /// <summary>
        /// Toggle 在加/未加间翻转：首次 Toggle("x") 加 → true；二次 Toggle("x") 移除 → false。
        /// 验 Toggle 是 Contains + Add/Remove 的正确组合。
        /// </summary>
        [Fact]
        public void ToggleFlipsMembership()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));

                n.Classes.Toggle("x");
                Assert.True(n.Classes.Contains("x"));

                n.Classes.Toggle("x");
                Assert.False(n.Classes.Contains("x"));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── Set（C# 组合）───────────────────────────────────────────────

        /// <summary>
        /// Set("x", true) 加；Set("x", false) 移除。验 Set 是 on?Add:Remove 的正确分支。
        /// </summary>
        [Fact]
        public void SetOnOffTogglesMembership()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));

                n.Classes.Set("x", on: true);
                Assert.True(n.Classes.Contains("x"));

                n.Classes.Set("x", on: false);
                Assert.False(n.Classes.Contains("x"));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── Replace（C# 组合）──────────────────────────────────────────

        /// <summary>
        /// Add a → Replace(a, b) → a 不在、b 在。验 Replace = Remove(old) + Add(new) 语义。
        /// 替换不存在的 oldName 等同单纯 Add newName（对齐 DOM classList.replace 容错）。
        /// </summary>
        [Fact]
        public void ReplaceSwapsMembership()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                n.Classes.Add("a");

                n.Classes.Replace("a", "b");

                Assert.False(n.Classes.Contains("a"));
                Assert.True(n.Classes.Contains("b"));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── 多 class 共存 ───────────────────────────────────────────────

        /// <summary>
        /// Add 多个不同 class 名 → 全部独立 Contains=true（core 侧 class 集合不互斥）。
        /// </summary>
        [Fact]
        public void MultipleClassesCoexist()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                n.Classes.Add("foo");
                n.Classes.Add("bar");
                n.Classes.Add("baz");

                Assert.True(n.Classes.Contains("foo"));
                Assert.True(n.Classes.Contains("bar"));
                Assert.True(n.Classes.Contains("baz"));

                n.Classes.Remove("bar");
                Assert.False(n.Classes.Contains("bar"));
                Assert.True(n.Classes.Contains("foo"));   // 移一个不影响其它
                Assert.True(n.Classes.Contains("baz"));
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── 稳定单一实例（projection §2.5）──────────────────────────────

        /// <summary>
        /// node.Classes 多次访问返同一实例。若每次返新实例，跨调用的镜像/句柄状态会丢——
        /// 虽然 ClassList 当前无镜像（直 FFI），但稳定实例是 projection §2.5 的不变量，
        /// 加缓存或帧末 seam 时不能退化为每访问新实例。
        /// </summary>
        [Fact]
        public void ClassesReturnsSameInstance()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                Assert.Same(n.Classes, n.Classes);

                // 经一次 Add 后再访问仍同一实例（lazy ??= 不会被覆盖）。
                n.Classes.Add("x");
                Assert.Same(n.Classes, n.Classes);
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        // ── Dispose 后访问抛 ────────────────────────────────────────────

        /// <summary>
        /// Dispose 后访问 node.Classes 抛 ObjectDisposedException（C1 ThrowIfDisposed 套到 Classes 入口）。
        /// </summary>
        [Fact]
        public void ClassesPostDisposeThrowsObjectDisposed()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                n.Dispose();
                Assert.Throws<ObjectDisposedException>(() => { var _ = n.Classes; });
            }
            finally
            {
                StageHarness.Destroy(stage);
            }
        }

        /// <summary>
        /// 业务捕获 `var cl = node.Classes;` 后 Dispose——后续每个公共方法（Add/Remove/Contains/
        /// Toggle/Set/Replace）必须抛 ObjectDisposedException，不能让 FFI 走 disposed NodeId。
        /// 覆盖 C5 review Minor #1：Node.Classes getter 的 ThrowIfDisposed 只拦 getter 入口，
        /// 持引用跨 Dispose 这条路径靠 ClassList 各方法入口的 ThrowIfDisposed 兜。
        /// </summary>
        [Fact]
        public void CapturedClassListPostDisposeThrowsOnEveryPublicMethod()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                ClassList cl = n.Classes;   // 先捕获引用
                n.Dispose();

                Assert.Throws<ObjectDisposedException>(() => cl.Add("x"));
                Assert.Throws<ObjectDisposedException>(() => cl.Remove("x"));
                Assert.Throws<ObjectDisposedException>(() => cl.Contains("x"));
                Assert.Throws<ObjectDisposedException>(() => cl.Toggle("x"));
                Assert.Throws<ObjectDisposedException>(() => cl.Set("x", true));
                Assert.Throws<ObjectDisposedException>(() => cl.Replace("a", "b"));
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
    }
}
