using System;
using System.IO;
using System.Text;
using LoomGUI.Bindings;
using Xunit;

namespace LoomGUI.HeadlessTests
{
    /// <summary>
    /// M2 Task 13: AnimationHandle 句柄 L3 全套 C# 验收（spec §9.1 headless harness + §7.2 class + §7.6 生命周期）。
    ///
    /// fixture：animation.pkg.bin（@keyframes fade + hookanim，后者带 @loom-hook "half" 锚在 50% stop），
    /// 经 LoadPackage+Instantiate 拿到 #target 节点。node.Play("fade") 走程序化 player
    /// （core play_programmatic 默认 spec = 1s / fill both / cubic-out / 单次 iteration，spec §7.3）。
    ///
    /// 关键机制：tick 驱动 core player.update → emit 动画事件进 borrow_events buffer →
    /// <see cref="TickAndPump"/>（tick + borrow_events + demux.Pump）把事件路由到句柄私有回调
    /// （FireStart/FireEnd/FireKey/FireHook，按 playerKey 查 <see cref="UIContext"/> 注册表）
    /// 以及 EventBus 广播（On&lt;AnimationEndEvent&gt;）。**必须每帧 tick 后立即 borrow+Pump**——
    /// core 在下帧 tick 开头 reset 借出 buffer，跨 tick 不 Pump 会丢上一帧事件。
    ///
    /// 全部经 headless harness P/Invoke 真 dll，不启 Unity。
    /// </summary>
    public unsafe class AnimationHandleTests
    {
        const float Eps = 5e-3f;   // 单帧 dt 累积浮点容差

        // ── Play / IsPlaying / 未知名 ──────────────────────────────────

        /// <summary>
        /// Play 返句柄；programmatic player 出生即 Playing（state=0），IsPlaying=true。
        /// Name 回传 Play 参数。验 FFI play_animation 全链 + 注册表登记。
        /// </summary>
        [Fact]
        public void Play_returns_playing_handle_with_name()
        {
            var (stage, ctx, root) = LoadFixture();
            try
            {
                var target = root.Get<Container>("target");
                var anim = target.Play("fade");

                Assert.Equal("fade", anim.Name);
                Assert.True(anim.IsPlaying);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Play 未知名（keyframes 表无此 name）抛 UIContractException（同 Get&lt;T&gt; 未命中语义，
        /// 调用方写错而非运行时异常）。钉死 spec §7.3 + public-api §9.1。
        /// </summary>
        [Fact]
        public void Play_unknown_name_throws_contract_exception()
        {
            var (stage, ctx, root) = LoadFixture();
            try
            {
                var target = root.Get<Container>("target");
                Assert.Throws<UIContractException>(() => target.Play("nonexistent"));
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── OnStart ─────────────────────────────────────────────────────

        /// <summary>
        /// OnStart 在首次 tick（player 首帧 advance，fired_start 由 false→true）触发；
        /// Play 后、tick 前不触发。验 START 事件经 demux→FireStart 路由通。
        /// </summary>
        [Fact]
        public void OnStart_fires_on_first_tick_only()
        {
            var (stage, ctx, root) = LoadFixture();
            try
            {
                var target = root.Get<Container>("target");
                int fired = 0;
                var anim = target.Play("fade").OnStart(() => fired++);

                Assert.Equal(0, fired);            // pre-tick: START 尚未 emit
                TickAndPump(stage, ctx, 0.016f);
                Assert.Equal(1, fired);            // 首帧 advance emit START 一次
                TickAndPump(stage, ctx, 0.016f);
                Assert.Equal(1, fired);            // 后续帧不重发（fired_start 防重）
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── Pause / Resume / Time 冻结 ──────────────────────────────────

        /// <summary>
        /// Pause → IsPlaying=false（Paused=1 != Playing=0）；之后 tick elapsed 不推进（advance
        /// Paused 幂等），Time 冻结。验 FFI pause_animation + core advance Paused 短路。
        /// </summary>
        [Fact]
        public void Pause_freezes_time_and_clears_IsPlaying()
        {
            var (stage, ctx, root) = LoadFixture();
            try
            {
                var target = root.Get<Container>("target");
                var anim = target.Play("fade");

                TickAndPump(stage, ctx, 0.3f);
                Assert.True(anim.IsPlaying);
                float t0 = anim.Time;
                Assert.InRange(t0, 0.3f - Eps, 0.3f + Eps);

                anim.Pause();
                Assert.False(anim.IsPlaying);      // Paused state

                TickAndPump(stage, ctx, 0.5f);
                Assert.InRange(anim.Time, t0 - Eps, t0 + Eps);   // elapsed 冻结
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Resume → IsPlaying=true；之后 tick elapsed 继续推进。验 FFI resume_animation
        /// （Paused→Playing 转换）+ advance 恢复推进。
        /// </summary>
        [Fact]
        public void Resume_advances_time_again()
        {
            var (stage, ctx, root) = LoadFixture();
            try
            {
                var target = root.Get<Container>("target");
                var anim = target.Play("fade");

                TickAndPump(stage, ctx, 0.3f);
                anim.Pause();
                TickAndPump(stage, ctx, 0.5f);     // 冻结
                float t0 = anim.Time;

                anim.Resume();
                Assert.True(anim.IsPlaying);

                TickAndPump(stage, ctx, 0.3f);
                Assert.InRange(anim.Time, t0 + 0.3f - Eps, t0 + 0.3f + Eps);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── Time seek ───────────────────────────────────────────────────

        /// <summary>
        /// Time setter = seek：直接改 core player.elapsed（单一时间源头），getter 回读新值。
        /// 验 FFI set/get_animation_time round-trip。
        /// </summary>
        [Fact]
        public void Time_set_seeks_player_position()
        {
            var (stage, ctx, root) = LoadFixture();
            try
            {
                var target = root.Get<Container>("target");
                var anim = target.Play("fade");

                TickAndPump(stage, ctx, 0.2f);
                Assert.InRange(anim.Time, 0.2f - Eps, 0.2f + Eps);

                anim.Time = 0.5f;                   // seek 到中点
                Assert.InRange(anim.Time, 0.5f - Eps, 0.5f + Eps);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── Stop（终态）──────────────────────────────────────────────────

        /// <summary>
        /// Stop = scene 层终态：FFI stop_animation 同步失效句柄（Invalidate）+ core 下帧回收 player。
        /// IsPlaying 立即 false（disposed 守卫），后续帧恒 false。spec §7.6。
        /// </summary>
        [Fact]
        public void Stop_invalidates_handle_and_recycles_player()
        {
            var (stage, ctx, root) = LoadFixture();
            try
            {
                var target = root.Get<Container>("target");
                var anim = target.Play("fade");
                Assert.True(anim.IsPlaying);

                anim.Stop();
                Assert.False(anim.IsPlaying);      // 同步 Invalidate

                TickAndPump(stage, ctx, 0.1f);     // core 回收 player
                Assert.False(anim.IsPlaying);      // 恒 false（disposed 守卫）
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── OnEnd（完成）+ 默认 1s duration 钉死 ────────────────────────

        /// <summary>
        /// OnEnd 在 player 完成帧触发（frame.completed && !was_completed，一次性）。
        /// 默认 programmatic spec duration=1s（T10 concern 1 钉死）：0.9s 不完成、1.1s 完成。
        /// 完成后句柄被 FireEnd 失效 → IsPlaying=false。
        /// </summary>
        [Fact]
        public void OnEnd_fires_on_completion_after_default_duration()
        {
            var (stage, ctx, root) = LoadFixture();
            try
            {
                var target = root.Get<Container>("target");
                int fired = 0;
                var anim = target.Play("fade").OnEnd(() => fired++);

                TickAndPump(stage, ctx, 0.9f);     // < 1s 默认时长
                Assert.Equal(0, fired);
                Assert.True(anim.IsPlaying);

                TickAndPump(stage, ctx, 0.2f);     // total 1.1s → 完成
                Assert.Equal(1, fired);
                Assert.False(anim.IsPlaying);      // FireEnd → Invalidate
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── OnKey（半 FFI，pct 精确匹配）──────────────────────────────────

        /// <summary>
        /// OnKey(0.5)：core 检测 last_progress→cur 跨越 0.5 时 emit KEY，demux 按 playerKey 查句柄
        /// 触发 FireKey(0.5)；pct 是同一 f32 值（core on_key_percents 原样发出），精确 == 匹配。
        /// 未跨越的 pct（0.8）不触发。
        /// </summary>
        [Fact]
        public void OnKey_fires_when_crossing_registered_percent()
        {
            var (stage, ctx, root) = LoadFixture();
            try
            {
                var target = root.Get<Container>("target");
                float firedAt = -1f;
                var anim = target.Play("fade");
                anim.OnKey(0.5f, () => firedAt = anim.Time);   // 闭包读句柄 Time，须 Play 后声明

                TickAndPump(stage, ctx, 0.5f);     // 跨 50% of 1s → KEY(0.5)
                Assert.InRange(firedAt, 0.5f - Eps, 0.5f + Eps);

                // 未跨越的 pct 不触发：注册 0.8，推进到 0.7 仍不触发。
                anim.OnKey(0.8f, () => firedAt = -2f);
                TickAndPump(stage, ctx, 0.2f);     // progress 0.7
                Assert.InRange(firedAt, 0.5f - Eps, 0.5f + Eps);   // 仍为首发的 0.5
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── On&lt;AnimationEndEvent&gt;（EventBus 广播）─────────────────

        /// <summary>
        /// class 触发 + node.Play 都广播 AnimationEndEvent（spec §7.1 双路由）。
        /// demux 把 EventRecord 翻译为 AnimationEndEvent 并 EventBus.Dispatch（AnimationName 从
        /// 字符串表读回）。订阅 node.On&lt;AnimationEndEvent&gt; 收到 Target/AnimationName 正确。
        /// </summary>
        [Fact]
        public void AnimationEndEvent_broadcast_via_event_bus()
        {
            var (stage, ctx, root) = LoadFixture();
            try
            {
                var target = root.Get<Container>("target");
                AnimationEndEvent received = default;
                bool got = false;
                target.On<AnimationEndEvent>(e => { received = e; got = true; });

                var anim = target.Play("fade");
                TickAndPump(stage, ctx, 0.9f);
                Assert.False(got);
                TickAndPump(stage, ctx, 0.2f);     // 完成 → END 广播
                Assert.True(got);
                Assert.Equal("fade", received.AnimationName);
                Assert.Same(target, received.Target);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── OnHook（@loom-hook 锚点）──────────────────────────────────────

        /// <summary>
        /// @loom-hook 锚点：fixture hookanim 的 50% stop 带 hook="half"（注释挂前一个 stop，
        /// 故 50%{}/* @loom-hook half */ to{} → half 锚 50%）。player 跨 50% 时 emit HOOK，
        /// demux 按 playerKey 查句柄触发 FireHook("half")，按 name 匹配 onHook 回调。
        /// 0% (from) hook 恒不触发（crossing 语义起点不计），故锚点须在 mid/late stop。
        /// </summary>
        [Fact]
        public void OnHook_fires_when_crossing_hooked_stop()
        {
            var (stage, ctx, root) = LoadFixture();
            try
            {
                var target = root.Get<Container>("target");
                bool halfFired = false;
                target.Play("hookanim").OnHook("half", () => halfFired = true);

                TickAndPump(stage, ctx, 0.5f);     // 跨 50% stop → HOOK("half")
                Assert.True(halfFired);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── 句柄失效后 no-op（§7.6）──────────────────────────────────────

        /// <summary>
        /// END 后句柄失效（disposed 守卫）：所有成员调用 no-op，不抛（§7.6「调用 no-op」）。
        /// 验 Pause/Resume/Stop/Time/OnStart 在 disposed 句柄上无副作用无异常。
        /// </summary>
        [Fact]
        public void handle_members_no_op_after_end()
        {
            var (stage, ctx, root) = LoadFixture();
            try
            {
                var target = root.Get<Container>("target");
                var anim = target.Play("fade");
                TickAndPump(stage, ctx, 1.1f);     // 完成 → END → Invalidate
                Assert.False(anim.IsPlaying);

                // 全部 no-op，不抛
                anim.Pause();
                anim.Resume();
                anim.Stop();
                anim.Time = 0.5f;
                _ = anim.Time;
                anim.OnStart(() => throw new InvalidOperationException("disposed handle fired cb"));
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── fixture 加载 + helpers ───────────────────────────────────────

        static (IntPtr stage, UIContext ctx, Container root) LoadFixture()
        {
            var (stage, ctx) = StageHarness.Create();
            StageHandle* h = (StageHandle*)stage.ToPointer();

            string fontPath = Path.Combine(AppContext.BaseDirectory, "fixtures", "fonts", "DejaVuSans.ttf");
            if (File.Exists(fontPath))
                RegisterFont(h, fontPath);

            ulong sceneRootId = CreateRoot(h, "div");
            ctx._rootId = sceneRootId;
            Container sceneRoot = (Container)ctx._registry.GetOrCreate(sceneRootId);

            string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "animation.pkg.bin");
            Assert.True(File.Exists(fixturePath), $"fixture animation.pkg.bin not found at {fixturePath}");

            byte[] pkgBytes = File.ReadAllBytes(fixturePath);
            UIPackage pkg = ctx.LoadPackage("animation", pkgBytes);
            Container instRoot = pkg.Instantiate("animation");
            AppendChild(h, sceneRoot._id, instRoot._id);
            Tick(h);   // cascade + solve（fixture 无 animation 声明 → 无 player，无事件待 pump）
            return (stage, ctx, instRoot);
        }

        /// <summary>
        /// tick 推进 core player + 立即借出本帧事件并 demux 路由（句柄私有回调 + EventBus 广播）。
        /// 必须每帧 tick 后即 Pump——core 下帧 tick 开头 reset borrow_events buffer，跨 tick 不 Pump 丢事件。
        /// </summary>
        static void TickAndPump(IntPtr stage, UIContext ctx, float dt)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            Native.loomgui_stage_tick(h, dt);
            nuint evLen = 0;
            byte* evPtr = Native.loomgui_stage_borrow_events(h, &evLen);
            ctx._eventDemuxer.Pump((IntPtr)evPtr, (int)evLen);
        }

        static void Tick(StageHandle* h) => Native.loomgui_stage_tick(h, 0.016f);

        static void RegisterFont(StageHandle* h, string fontPath)
        {
            byte[] fontBytes = File.ReadAllBytes(fontPath);
            byte[] family = Encoding.UTF8.GetBytes("DejaVuSans");
            fixed (byte* fp = family)
            fixed (byte* bp = fontBytes)
            {
                Native.loomgui_stage_register_font(
                    h, fp, (nuint)family.Length, bp, (nuint)fontBytes.Length, is_default: 1);
            }
        }

        static ulong CreateRoot(StageHandle* h, string kind)
        {
            byte[] k = Encoding.UTF8.GetBytes(kind);
            fixed (byte* kp = k)
                return Native.loomgui_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        static void AppendChild(StageHandle* h, ulong parent, ulong child)
        {
            int rc = Native.loomgui_stage_append_child(h, parent, child);
            if (rc != 0)
                throw new InvalidOperationException($"append_child(parent={parent}, child={child}) failed rc={rc}");
        }
    }
}
