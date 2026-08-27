using System;
using System.IO;
using System.Text;
using Ikat.Bindings;
using Xunit;

namespace Ikat.HeadlessTests
{
    /// <summary>
    /// NodeStyle.ZIndex（z-index 便签层）行为测试：set → 帧末 flush seam → core inline
    /// override 应用（u64 位图 bit 32）；getter mirror round-trip；回落显式 0。
    /// </summary>
    public unsafe class ZIndexTests
    {
        static StageHandle* H(UIContext ctx) => (StageHandle*)ctx._stage.ToPointer();

        static ulong CreateRoot(UIContext ctx)
        {
            byte[] k = Encoding.UTF8.GetBytes("div");
            fixed (byte* kp = k)
                return Native.ikat_stage_create_root(H(ctx), kp, (nuint)k.Length, null, 0);
        }

        static void Tick(UIContext ctx, float dt = 0.016f) => Native.ikat_stage_tick(H(ctx), dt);

        static byte[] FixtureBytes(string name)
        {
            string p = Path.Combine(AppContext.BaseDirectory, "fixtures", name);
            Assert.True(File.Exists(p), $"fixture {name} not found at {p}");
            return File.ReadAllBytes(p);
        }

        static void RegisterDefaultFont(UIContext ctx)
        {
            string fontPath = Path.Combine(AppContext.BaseDirectory, "fixtures", "fonts", "DejaVuSans.ttf");
            if (!File.Exists(fontPath)) return;
            byte[] fontBytes = File.ReadAllBytes(fontPath);
            byte[] family = Encoding.UTF8.GetBytes("DejaVuSans");
            fixed (byte* fp = family)
            fixed (byte* bp = fontBytes)
                Native.ikat_stage_register_font(
                    H(ctx), fp, (nuint)family.Length, bp, (nuint)fontBytes.Length, is_default: 1);
        }

        [Fact]
        public void ZIndexRoundTripsThroughInlineOverride()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                RegisterDefaultFont(ctx);
                ulong rootId = CreateRoot(ctx);
                ctx._rootId = rootId;
                Container root = (Container)ctx._registry.GetOrCreate(rootId);
                UIPackage pkg = ctx.LoadPackage("dropdown", FixtureBytes("dropdown.pkg.bin"));
                Container inst = pkg.Instantiate("dropdown");
                Native.ikat_stage_append_child(H(ctx), rootId, inst._id);
                Tick(ctx);
                var sel = inst.Get<Dropdown>("sel");

                // 未写过 → 0（CSS 初始值；getter 只反映 setter，mirror 稀疏语义）
                Assert.Equal(0, sel.Style.ZIndex);

                // set → mirror round-trip；帧末 flush seam 送 core（int arm of CssValueConvert）
                sel.Style.ZIndex = 5;
                Assert.Equal(5, sel.Style.ZIndex);
                ctx.FlushPendingWrites();
                Tick(ctx);
                Assert.Equal(5, sel.Style.ZIndex);

                // 负值合法（CSS <integer>）；显式 0 = 等效默认
                sel.Style.ZIndex = -3;
                ctx.FlushPendingWrites();
                Tick(ctx);
                Assert.Equal(-3, sel.Style.ZIndex);
                sel.Style.ZIndex = 0;
                ctx.FlushPendingWrites();
                Tick(ctx);
                Assert.Equal(0, sel.Style.ZIndex);
            }
            finally { StageHarness.Destroy(stage); }
        }
    }
}
