using NUnit.Framework;
using UnityEngine;

namespace Ikat.Tests
{
    public class MaterialManagerTests
    {
        [Test]
        public void SameKeyReturnsSameMaterial()
        {
            var mm = new MaterialManager(Shader.Find("Ikat/Unlit"));
            var white = Texture2D.whiteTexture;
            var a = mm.Get(program: 0, white, maskContext: 0, false);
            var b = mm.Get(program: 0, white, maskContext: 0, false);
            Assert.AreSame(a, b);
        }

        [Test]
        public void DifferentMaskContextReturnsDifferentMaterial()
        {
            var mm = new MaterialManager(Shader.Find("Ikat/Unlit"));
            var white = Texture2D.whiteTexture;
            var a = mm.Get(0, white, 0, false);
            var b = mm.Get(0, white, 1, false);
            Assert.AreNotSame(a, b);
        }

        /// ctx>0 material 创建即 EnableKeyword("CLIPPED")（激活 shader multi_compile _ CLIPPED variant）。
        [Test]
        public void CtxGtZero_MaterialHasClippedKeyword()
        {
            var mm = new MaterialManager(Shader.Find("Ikat/Unlit"));
            var white = Texture2D.whiteTexture;
            var m0 = mm.Get(0, white, 0, false);
            var m1 = mm.Get(0, white, 1, false);
            Assert.IsFalse(m0.IsKeywordEnabled("CLIPPED"), "ctx=0: 不裁剪，无 CLIPPED keyword");
            Assert.IsTrue(m1.IsKeywordEnabled("CLIPPED"), "ctx>0: 启用 CLIPPED variant");
        }

        /// SetClipEntries 后该 ctx material 的 clip 链数组被刷新（SetVectorArray 落到
        /// 缓存实例）——Set-after-Get 顺序（后续帧路径）。
        [Test]
        public void SetClipEntries_UpdatesCachedMaterialArrays()
        {
            var mm = new MaterialManager(Shader.Find("Ikat/Unlit"));
            var white = Texture2D.whiteTexture;
            var m = mm.Get(0, white, 7, false);   // 建 ctx=7 material
            var entries = new System.Collections.Generic.List<ClipEntryView>
            {
                new ClipEntryView
                {
                    HasRect = true, W = 100f, H = 80f,
                    A = 1f, D = 1f,   // identity frame（B=C=0, Tx=Ty=0）
                },
            };
            mm.SetClipEntries(7, entries);
            Assert.AreEqual(1f, m.GetFloat("_ClipCount"), "clip 链 entry 数写入");
            var f0 = m.GetVectorArray("_ClipFrame0");
            Assert.AreEqual(new Vector4(1f, 0f, 0f, 0f), f0[0], "frame0=(A,C,Tx,shapeKind=0)");
            var rect = m.GetVectorArray("_ClipRect");
            Assert.AreEqual(new Vector4(100f, 80f, 0f, 0f), rect[0], "rect=(w,h,poly_count,_)");
        }

        /// SetClipEntries-before-Get 顺序：链先进 dict，Get 建材质时读取（首帧路径）。
        [Test]
        public void SetClipEntries_BeforeGet_AppliedOnCreation()
        {
            var mm = new MaterialManager(Shader.Find("Ikat/Unlit"));
            var white = Texture2D.whiteTexture;
            var entries = new System.Collections.Generic.List<ClipEntryView>
            {
                new ClipEntryView
                {
                    HasShape = true, ShapeKind = 0, CircleCx = 50f, CircleCy = 50f, CircleR = 50f,
                    A = 1f, D = 1f,
                },
            };
            mm.SetClipEntries(3, entries);
            var m = mm.Get(0, white, 3, false);
            Assert.AreEqual(1f, m.GetFloat("_ClipCount"), "首帧：链先于 Get，Get 建材质时带链");
            var circ = m.GetVectorArray("_ClipCircle");
            Assert.AreEqual(new Vector4(50f, 50f, 50f, 0f), circ[0]);
            Assert.IsTrue(m.IsKeywordEnabled("CLIPPED"));
        }

        /// 多 entry 链（rect + polygon）全量写入：_ClipCount=2、frame kind 双独立
        /// （frame0.w=shapeKind / frame1.w=rectKind）、polygon 点两点一 float4 落槽。
        [Test]
        public void SetClipEntries_MultiEntry_RoundAndPolygon()
        {
            var mm = new MaterialManager(Shader.Find("Ikat/Unlit"));
            var white = Texture2D.whiteTexture;
            var m = mm.Get(0, white, 5, false);
            var entries = new System.Collections.Generic.List<ClipEntryView>
            {
                new ClipEntryView
                {
                    HasRect = true, HasRadii = true, W = 200f, H = 100f,
                    RadiiTlTr = new Vector4(10f, 12f, 14f, 16f),
                    RadiiBrBl = new Vector4(18f, 20f, 22f, 24f),
                    A = 1f, D = 1f,
                },
                new ClipEntryView
                {
                    HasShape = true, ShapeKind = 1,
                    Poly = new[] { new Vector2(50f, 0f), new Vector2(100f, 50f), new Vector2(50f, 100f), new Vector2(0f, 50f) },
                    A = 1f, D = 1f,
                },
            };
            mm.SetClipEntries(5, entries);
            Assert.AreEqual(2f, m.GetFloat("_ClipCount"));
            var f0 = m.GetVectorArray("_ClipFrame0");
            var f1 = m.GetVectorArray("_ClipFrame1");
            Assert.AreEqual(0f, f0[0].w, "entry0 无 shape");
            Assert.AreEqual(2f, f1[0].w, "entry0 rectKind=2（圆角）");
            Assert.AreEqual(2f, f0[1].w, "entry1 shapeKind=2（polygon）");
            Assert.AreEqual(0f, f1[1].w, "entry1 无 rect");
            var poly = m.GetVectorArray("_ClipPoly");
            // entry1 槽基址 = 1×8；点 0/1 打包进第一个 float4。
            Assert.AreEqual(new Vector4(50f, 0f, 100f, 50f), poly[8], "poly 两点一 float4");
            Assert.AreEqual(new Vector4(50f, 100f, 0f, 50f), poly[9]);
        }
    }
}
