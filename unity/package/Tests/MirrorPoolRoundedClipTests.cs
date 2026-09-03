using System.Collections.Generic;
using NUnit.Framework;
using UnityEngine;

namespace Ikat
{
    /// 圆角 clip 端到端（#52 多 entry 布局）：blob clip 表带 radii entry → MirrorPool
    /// 应建 CLIPPED 材质并写 clip 链数组（rectKind=2 圆角，stat-bar 链路的 Unity 半场守卫）。
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
            uint clipLen = 4u + 92u;   // clip_count + 1×92B entry（多 entry 布局，无 poly）
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
            b.AddRange(System.BitConverter.GetBytes(0b011u)); // flags: has_rect | has_radii
            b.AddRange(System.BitConverter.GetBytes(1f));  // inv_frame a
            b.AddRange(System.BitConverter.GetBytes(0f));  // b
            b.AddRange(System.BitConverter.GetBytes(0f));  // c
            b.AddRange(System.BitConverter.GetBytes(1f));  // d
            b.AddRange(System.BitConverter.GetBytes(-cx));  // tx（design 平移逆）
            b.AddRange(System.BitConverter.GetBytes(-cy));  // ty
            b.AddRange(System.BitConverter.GetBytes(cw));   // rect w（box-local）
            b.AddRange(System.BitConverter.GetBytes(ch));   // rect h
            for (int k = 0; k < 4; k++)
            {
                b.AddRange(System.BitConverter.GetBytes(radius));
                b.AddRange(System.BitConverter.GetBytes(radius));
            }
            b.AddRange(System.BitConverter.GetBytes(0f));  // circle cx
            b.AddRange(System.BitConverter.GetBytes(0f));  // circle cy
            b.AddRange(System.BitConverter.GetBytes(0f));  // circle r
            b.AddRange(System.BitConverter.GetBytes(0u));  // poly_count
            b.AddRange(System.BitConverter.GetBytes(0u));  // poly_off
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
                var entries = blob.ReadClipEntries(7);
                Assert.AreEqual(1, entries.Count, "ctx=7 应有 1 条 entry");
                var en = entries[0];
                Assert.IsTrue(en.HasRect && en.HasRadii, "rect + radii flags 解码");
                Assert.AreEqual(3f, en.RadiiTlTr.x, 0.001f, "TL rx 解码应得 3");
                Assert.AreEqual(546f, en.W, 0.01f);
                Assert.AreEqual(16f, en.H, 0.01f);

                pool.Sync(blob, root.transform, mm, null, fallback);

                var mr = root.transform.GetChild(0).GetComponent<MeshRenderer>();
                var mat = mr.sharedMaterial;
                Assert.IsNotNull(mat, "节点应持有材质");
                Assert.IsTrue(mat.IsKeywordEnabled("CLIPPED"),
                    "ctx>0 clip → CLIPPED 变体（多 entry 数组分派）");
                Assert.AreEqual(1f, mat.GetFloat("_ClipCount"), "链 entry 数=1");
                var f1 = mat.GetVectorArray("_ClipFrame1");
                Assert.AreEqual(2f, f1[0].w, "rectKind=2（圆角）");
                var r0 = mat.GetVectorArray("_ClipRadii0");
                Assert.AreEqual(3f, r0[0].x, 0.001f, "TL 半径透传（像素空间，不再归一化）");
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
