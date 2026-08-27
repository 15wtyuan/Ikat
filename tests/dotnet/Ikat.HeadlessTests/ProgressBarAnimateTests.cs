using System;
using System.IO;
using System.Text;
using Ikat.Bindings;
using Xunit;

namespace Ikat.HeadlessTests
{
    /// <summary>
    /// ProgressBar.AnimateValue 演出糖（Field Notes N9）：Value 走布局通道无 CSS 过渡，
    /// C# 投影层缓动。契约：动画期间 Value 读回缓存目标（数据值），插值中间值经 FFI
    /// 只喂渲染；直接赋 Value 显式获胜（取消动画）；进行中重复调用重锚。
    /// 手动 PumpLogic 泵帧（OnUpdate 调度器住投影层，同 SchedulerAndLifecycleTests 模式）。
    /// </summary>
    public unsafe class ProgressBarAnimateTests
    {
        // fixture controls.pkg.bin 的 prog：初始 Value=40（aria-valuenow 烘入）。
        static (IntPtr stage, UIContext ctx, Container root) LoadControlsFixture()
        {
            var (stage, ctx) = StageHarness.Create();
            StageHandle* h = (StageHandle*)stage.ToPointer();
            string fontPath = Path.Combine(AppContext.BaseDirectory, "fixtures", "fonts", "DejaVuSans.ttf");
            if (File.Exists(fontPath))
            {
                byte[] fontBytes = File.ReadAllBytes(fontPath);
                byte[] family = Encoding.UTF8.GetBytes("DejaVuSans");
                fixed (byte* fp = family)
                fixed (byte* bp = fontBytes)
                    Native.ikat_stage_register_font(
                        h, fp, (nuint)family.Length, bp, (nuint)fontBytes.Length, is_default: 1);
            }
            byte[] k = Encoding.UTF8.GetBytes("div");
            ulong sceneRootId;
            fixed (byte* kp = k)
                sceneRootId = Native.ikat_stage_create_root(h, kp, (nuint)k.Length, null, 0);
            ctx._rootId = sceneRootId;
            Container sceneRoot = (Container)ctx._registry.GetOrCreate(sceneRootId);

            string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "controls.pkg.bin");
            Assert.True(File.Exists(fixturePath), $"fixture controls.pkg.bin not found at {fixturePath}");
            UIPackage pkg = ctx.LoadPackage("controls", File.ReadAllBytes(fixturePath));
            Container instRoot = pkg.Instantiate("controls");
            Native.ikat_stage_append_child(h, sceneRoot._id, instRoot._id);
            Native.ikat_stage_tick(h, 0.016f);
            return (stage, ctx, instRoot);
        }

        [Fact]
        public void animate_interpolates_then_settles()
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                var prog = root.Get<ProgressBar>("prog");
                Assert.Equal(40f, prog.Value);
                prog.AnimateValue(80f, 1.0f);

                // 半程：公共读回 = 目标（数据值）；FFI 显示值在中间（easeOut 已越过中点）。
                ctx.PumpLogic(0.5f);
                Assert.Equal(80f, prog.Value, 5);
                float display = prog.GetControlValue();
                Assert.True(display > 40f && display < 80f, $"mid display {display} 应在 (40,80)");

                // 走完：动画收尾直写目标，_animating 清，读回回落 FFI。
                ctx.PumpLogic(0.6f);
                Assert.Equal(80f, prog.Value, 5);
                Assert.Equal(80f, prog.GetControlValue(), 5);
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void explicit_value_set_cancels_animation()
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                var prog = root.Get<ProgressBar>("prog");
                prog.AnimateValue(80f, 1.0f);
                ctx.PumpLogic(0.3f);

                prog.Value = 10f; // 显式获胜：取消动画、直写
                ctx.PumpLogic(1.0f);
                Assert.Equal(10f, prog.Value, 5);
                Assert.Equal(10f, prog.GetControlValue(), 5);
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void retarget_anchors_from_current_display()
        {
            var (stage, ctx, root) = LoadControlsFixture();
            try
            {
                var prog = root.Get<ProgressBar>("prog");
                prog.AnimateValue(80f, 1.0f);
                ctx.PumpLogic(0.25f);
                float before = prog.GetControlValue();

                prog.AnimateValue(20f, 1.0f); // 重锚：从当前插值位置转向
                ctx.PumpLogic(0.1f);
                float after = prog.GetControlValue();
                Assert.True(after < before, $"转向 20 后应下行：before={before} after={after}");

                ctx.PumpLogic(1.2f);
                Assert.Equal(20f, prog.GetControlValue(), 5);
            }
            finally { StageHarness.Destroy(stage); }
        }
    }
}
