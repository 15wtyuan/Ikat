using System.Collections.Generic;
using NUnit.Framework;
using UnityEngine;

namespace Ikat
{
    /// 圆角 clip 端到端：blob clip 表带 radii → MirrorPool 应建 CLIPPED_ROUNDED 材质
    /// 并写归一化 _CornerRadius（stat-bar fill 圆角链路的 Unity 半场守卫）。
    public class MirrorPoolRoundedClipTests
    {
        /// 单节点 blob + clip 表一条 entry（ctx=7, rect, radii）。镜像 blob.rs v13 布局
        /// （23 列：…reuse_key 后跟 effect_block 128B / shadow_params 24B / grad_params 208B）。
        static byte[] OneNodeBlobWithClip(uint id, uint maskCtx, float cx, float cy, float cw, float ch, float radius)
        {
            var b = new List<byte>();
            b.AddRange(System.BitConverter.GetBytes(0x4D4F4F4Cu));
            b.AddRange(System.BitConverter.GetBytes(13u));
            b.AddRange(System.BitConverter.GetBytes(1u));

            int[] elemSize = { 4, 4, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 1, 4, 4, 4, 1, 80, 1, 4, 128, 24, 208 };
            int colOff = 12 + elemSize.Length * 4 + 6 * 4;
            foreach (int _ in elemSize) { b.AddRange(System.BitConverter.GetBytes(colOff)); colOff += _; }

            var arena = new List<byte>();
            int arenaStart = arena.Count;
            arena.AddRange(System.BitConverter.GetBytes(4));
            arena.AddRange(System.BitConverter.GetBytes(6));
            AppendVert(arena, 0f, 0f);
            AppendVert(arena, 100f, 0f);
            AppendVert(arena, 100f, 20f);
            AppendVert(arena, 0f, 20f);
            // uvs[4×2]（纯色 quad 全图 UV）
            AppendVert(arena, 0f, 0f);
            AppendVert(arena, 1f, 0f);
            AppendVert(arena, 1f, 1f);
            AppendVert(arena, 0f, 1f);
            // colors[4×4]
            for (int v = 0; v < 16; v++) arena.AddRange(System.BitConverter.GetBytes(1f));
            // idx[6]（本 mesh 内顶点序号）
            foreach (uint ix in new uint[] { 0, 1, 2, 0, 2, 3 }) arena.AddRange(System.BitConverter.GetBytes(ix));
            int arenaLen = arena.Count - arenaStart;

            // mesh arena 在 SOA 列数据之后（header → 列数据 → mesh arena → clip → path）
            int soaLen = 0;
            foreach (int e in elemSize) soaLen += e;
            int meshArenaOff = 12 + elemSize.Length * 4 + 6 * 4 + soaLen;
            b.AddRange(System.BitConverter.GetBytes(meshArenaOff));
            b.AddRange(System.BitConverter.GetBytes(arenaLen));
            int clipOff = meshArenaOff + arenaLen;
            uint clipLen = 4u + 52u;
            b.AddRange(System.BitConverter.GetBytes(clipOff));
            b.AddRange(System.BitConverter.GetBytes(clipLen));
            int pathOff = clipOff + (int)clipLen;
            b.AddRange(System.BitConverter.GetBytes(pathOff));
            b.AddRange(System.BitConverter.GetBytes(4u));

            // SOA 列数据（每列按自己的 elemSize 顺序落在 offset 处）
            b.AddRange(System.BitConverter.GetBytes(id));
            b.AddRange(System.BitConverter.GetBytes(-1));
            b.Add(1);
            b.AddRange(System.BitConverter.GetBytes(1f));
            b.AddRange(System.BitConverter.GetBytes(0u));
            b.AddRange(System.BitConverter.GetBytes(maskCtx));
            b.AddRange(System.BitConverter.GetBytes(1f));
            b.AddRange(System.BitConverter.GetBytes(0f));
            b.AddRange(System.BitConverter.GetBytes(0f));
            b.AddRange(System.BitConverter.GetBytes(1f));
            b.AddRange(System.BitConverter.GetBytes(10f));
            b.AddRange(System.BitConverter.GetBytes(20f));
            b.Add(1);
            b.AddRange(System.BitConverter.GetBytes(0u));
            b.AddRange(System.BitConverter.GetBytes((uint)arenaLen));
            b.AddRange(System.BitConverter.GetBytes(0u));
            b.Add(0);
            for (int j = 0; j < 20; j++) b.AddRange(System.BitConverter.GetBytes(0f));
            b.Add(2);
            b.AddRange(System.BitConverter.GetBytes(0u));
            for (int j = 0; j < 128 / 4; j++) b.AddRange(System.BitConverter.GetBytes(0f));
            for (int j = 0; j < 24 / 4; j++) b.AddRange(System.BitConverter.GetBytes(0f));
            for (int j = 0; j < 208 / 4; j++) b.AddRange(System.BitConverter.GetBytes(0f));

            b.AddRange(arena);
            b.AddRange(System.BitConverter.GetBytes(1u)); // clip_count
            b.AddRange(System.BitConverter.GetBytes(7u));  // ctx
            b.AddRange(System.BitConverter.GetBytes(cx));
            b.AddRange(System.BitConverter.GetBytes(cy));
            b.AddRange(System.BitConverter.GetBytes(cw));
            b.AddRange(System.BitConverter.GetBytes(ch));
            for (int k = 0; k < 4; k++)
            {
                b.AddRange(System.BitConverter.GetBytes(radius));
                b.AddRange(System.BitConverter.GetBytes(radius));
            }
            b.AddRange(System.BitConverter.GetBytes(0u)); // path_count
            return b.ToArray();

            static void AppendVert(List<byte> a, float vx, float vy)
            {
                a.AddRange(System.BitConverter.GetBytes(vx));
                a.AddRange(System.BitConverter.GetBytes(vy));
            }
        }

        [Test]
        public void RoundedClipEntryEnablesClippedRoundedMaterial()
        {
            var root = new GameObject("root");
            var shader = Shader.Find("Ikat/Unlit");
            var mm = new MaterialManager(shader);
            var pool = new MirrorPool();
            var fallback = Texture2D.whiteTexture;

            try
            {
                // design rect 546x16，radius 3（stat-bar 实参）
                byte[] raw = OneNodeBlobWithClip(100, 7, 92f, 898f, 546f, 16f, 3f);
                var blob = new FrameBlob(raw);
                Assert.IsTrue(blob.IsValid, "magic+version v13");
                Assert.AreEqual(1, blob.NodeCount);
                Assert.AreEqual(
                    1u,
                    System.BitConverter.ToUInt32(raw, System.BitConverter.ToInt32(raw, 112)),
                    "clipOff 处应写有 clip_count=1（自洽检查）");
                Assert.AreEqual(1, blob.ClipCount, "clip 表 count=1（ClipTableOff/ClipTableLen 解码）");
                Assert.IsTrue(blob.ClipRect(7, out _, out _, out float dw, out float dh, out float cr),
                    "ClipRect 应命中 ctx=7");
                Assert.AreEqual(3f, cr, 0.001f, "radii 解码应得 3");
                Assert.AreEqual(546f, dw, 0.01f);

                pool.Sync(blob, root.transform, mm, null, fallback);

                var mr = root.transform.GetChild(0).GetComponent<MeshRenderer>();
                var mat = mr.sharedMaterial;
                Assert.IsNotNull(mat, "节点应持有材质");
                Assert.IsTrue(mat.IsKeywordEnabled("CLIPPED_ROUNDED"),
                    "radii>0 clip → CLIPPED_ROUNDED 变体");
                float normR = mat.GetFloat("_CornerRadius");
                Assert.Greater(normR, 0f, "_CornerRadius 应为归一化正值");
                // 归一化 = 3 / min(546,16)/2 = 0.375
                Assert.AreEqual(0.375f, normR, 0.01f, "归一化半径 3/(16/2)");
            }
            finally
            {
                pool.Clear();
                mm.Clear();
                Object.DestroyImmediate(root);
            }
        }
    }
}
