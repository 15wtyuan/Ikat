using System.Collections.Generic;
using NUnit.Framework;
using UnityEngine;

namespace LoomGUI.Tests
{
    /// MirrorPool reuse_key 按 reuse_key 复用 GO 的 EditMode 测试。
    /// 手搓 v10 1 节点 mesh blob（20 列含 reuse_key） → 验 slot 换绑 GO 复用。
    public class MirrorPoolReuseKeyTests
    {
        /// 构造一个 v10 1 节点 Mesh blob（20 列 SOA）。
        /// v10：删 text_arena + text_off/text_len 列（22→20 列），header 116B。
        static byte[] OneNodeBlobV10(
            uint id, float x, float y, float w, float h, uint sortKey,
            byte payloadKind = 1, byte changeLevel = 2, uint reuseKey = 0)
        {
            var b = new List<byte>();

            // header: magic, version=10, node_count=1
            b.AddRange(System.BitConverter.GetBytes(0x4D4F4F4Cu));
            b.AddRange(System.BitConverter.GetBytes(10u));
            b.AddRange(System.BitConverter.GetBytes(1u));

            // header 总长 = 12 + 20*4 + 6*4 = 116。列 offset 从此起按 elemSize 递进。
            int colOff = 116;
            int[] offs = new int[20];
            // v10: 20 cols — node_id(4) parent_id(4) visible(1) alpha(4) sort_key(4) mask_context(4)
            //   m_a..m_ty(6×4) payload_kind(1) mesh_off(4) mesh_len(4) path_idx(4)
            //   program(1) color_matrix(80) change_level(1) reuse_key(4)
            int[] elemSize = { 4, 4, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 1, 4, 4, 4, 1, 80, 1, 4 };
            for (int i = 0; i < 20; i++) { offs[i] = colOff; colOff += elemSize[i]; }
            int arenaOff = colOff;

            // mesh arena：1 mesh，4 verts / 6 idx。
            var arena = new List<byte>();
            int arenaStart = arena.Count;
            arena.AddRange(System.BitConverter.GetBytes(4)); // vert_count
            arena.AddRange(System.BitConverter.GetBytes(6)); // idx_count
            AppendVert(arena, 0f, 0f);
            AppendVert(arena, w,   0f);
            AppendVert(arena, w,   h);
            AppendVert(arena, 0f,  h);
            AppendVert(arena, 0f, 0f);
            AppendVert(arena, 1f, 0f);
            AppendVert(arena, 1f, 1f);
            AppendVert(arena, 0f, 1f);
            for (int v = 0; v < 4; v++)
            {
                arena.AddRange(System.BitConverter.GetBytes(1f));
                arena.AddRange(System.BitConverter.GetBytes(1f));
                arena.AddRange(System.BitConverter.GetBytes(1f));
                arena.AddRange(System.BitConverter.GetBytes(1f));
            }
            arena.AddRange(System.BitConverter.GetBytes(0u));
            arena.AddRange(System.BitConverter.GetBytes(1u));
            arena.AddRange(System.BitConverter.GetBytes(2u));
            arena.AddRange(System.BitConverter.GetBytes(0u));
            arena.AddRange(System.BitConverter.GetBytes(2u));
            arena.AddRange(System.BitConverter.GetBytes(3u));
            int arenaLen = arena.Count - arenaStart;

            // 20 列 offset + mesh/clip/path 三 arena off+len（v10：text_arena 已删）
            foreach (var o in offs) b.AddRange(System.BitConverter.GetBytes(o));
            b.AddRange(System.BitConverter.GetBytes(arenaOff));                      // mesh_arena_off
            b.AddRange(System.BitConverter.GetBytes(arenaLen));                      // mesh_arena_len
            int clipOff = arenaOff + arenaLen;
            b.AddRange(System.BitConverter.GetBytes(clipOff));                        // clip_table_off
            b.AddRange(System.BitConverter.GetBytes(4u));                             // clip_table_len（仅 clip_count）
            int pathOff = clipOff + 4;                                                // clip_count 后
            b.AddRange(System.BitConverter.GetBytes(pathOff));                        // path_table_off
            b.AddRange(System.BitConverter.GetBytes(4u));                             // path_table_len（仅 path_count）

            // 列数据 SOA（列优先，镜像 blob.rs v10 / FrameBlob）
            b.AddRange(System.BitConverter.GetBytes(id));        // col 0: node_id
            b.AddRange(System.BitConverter.GetBytes(-1));        // col 1: parent_id
            b.Add(1);                                            // col 2: visible
            b.AddRange(System.BitConverter.GetBytes(1f));        // col 3: alpha
            b.AddRange(System.BitConverter.GetBytes(sortKey));   // col 4: sort_key
            b.AddRange(System.BitConverter.GetBytes(0u));        // col 5: mask_context
            b.AddRange(System.BitConverter.GetBytes(1f));        // col 6: m_a
            b.AddRange(System.BitConverter.GetBytes(0f));        // col 7: m_b
            b.AddRange(System.BitConverter.GetBytes(0f));        // col 8: m_c
            b.AddRange(System.BitConverter.GetBytes(1f));        // col 9: m_d
            b.AddRange(System.BitConverter.GetBytes(x));         // col 10: m_tx
            b.AddRange(System.BitConverter.GetBytes(y));         // col 11: m_ty
            b.Add(payloadKind);                                  // col 12: payload_kind
            b.AddRange(System.BitConverter.GetBytes(0u));        // col 13: mesh_off
            b.AddRange(System.BitConverter.GetBytes((uint)arenaLen)); // col 14: mesh_len
            b.AddRange(System.BitConverter.GetBytes(0u));        // col 15: path_idx
            b.Add(0);                                            // col 16: program（img/无图 Container）
            // col 17: color_matrix（[f32;20] = 80B，全零）
            for (int j = 0; j < 20; j++) b.AddRange(System.BitConverter.GetBytes(0f));
            b.Add(changeLevel);                                  // col 18: change_level
            b.AddRange(System.BitConverter.GetBytes(reuseKey));  // col 19: reuse_key

            b.AddRange(arena);
            // clip 表：仅 clip_count=0
            b.AddRange(System.BitConverter.GetBytes(0u));
            // path 表：仅 path_count=0
            b.AddRange(System.BitConverter.GetBytes(0u));
            return b.ToArray();

            static void AppendVert(List<byte> a, float vx, float vy)
            {
                a.AddRange(System.BitConverter.GetBytes(vx));
                a.AddRange(System.BitConverter.GetBytes(vy));
            }
        }

        /// reuse_key 不变、NodeId 变 → 同一 GO 复用（不销毁重建）。
        /// 构造 blob：slot 节点 reuse_key=5, node_id=100, Full。
        /// 下一帧 blob：同 reuse_key=5, node_id=200（换绑）, Full。
        /// 断言：_poolByReuse[5] 的 GO 是同一个（ReferenceEquals），只重建 mesh。
        [Test]
        public void SlotReuseKeyRecyclesGoAcrossNodeChange()
        {
            var root = new GameObject("root");
            var shader = Shader.Find("LoomGUI/Unlit");
            var mm = new MaterialManager(shader);
            var pool = new MirrorPool();
            var fallback = Texture2D.whiteTexture;

            try
            {
                // 帧 1：node_id=100, reuse_key=5, Full → 建 GO
                var blob1 = new FrameBlob(OneNodeBlobV10(
                    id: 100, x: 10f, y: 20f, w: 5f, h: 5f,
                    sortKey: 0, payloadKind: 1, changeLevel: 2, reuseKey: 5));
                Assert.AreEqual(1, blob1.NodeCount, "blob1 NodeCount=1");
                Assert.AreEqual(5u, blob1.ReuseKey(0), "blob1 reuse_key=5");
                pool.Sync(blob1, root.transform, mm, null, fallback);

                Assert.AreEqual(1, pool.Count, "帧1: pool.Count=1");
                Assert.AreEqual(1, root.transform.childCount, "帧1: 1 子 GO");
                var go1 = root.transform.GetChild(0).gameObject;

                // 帧 2：node_id=200, 同 reuse_key=5, Full → 应复用 GO（不销毁重建）
                var blob2 = new FrameBlob(OneNodeBlobV10(
                    id: 200, x: 30f, y: 40f, w: 5f, h: 5f,
                    sortKey: 0, payloadKind: 1, changeLevel: 2, reuseKey: 5));
                Assert.AreEqual(1, blob2.NodeCount, "blob2 NodeCount=1");
                Assert.AreEqual(200u, blob2.NodeId(0), "blob2 node_id=200");
                Assert.AreEqual(5u, blob2.ReuseKey(0), "blob2 reuse_key=5");
                pool.Sync(blob2, root.transform, mm, null, fallback);

                // 复用验证
                Assert.AreEqual(1, pool.Count, "帧2: pool.Count 仍=1（复用，非新建）");
                Assert.AreEqual(1, root.transform.childCount, "帧2: 仍 1 子 GO");
                var go2 = root.transform.GetChild(0).gameObject;
                Assert.AreSame(go1, go2, "reuse_key 不变 → 同一 GO 复用（ReferenceEquals）");

                // 通过反射验证 _poolByReuse[5] 的 LastNodeId==200
                var poolByReuseField = typeof(MirrorPool).GetField("_poolByReuse",
                    System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance);
                Assert.IsNotNull(poolByReuseField, "应能反射 _poolByReuse");
                var poolByReuse = (System.Collections.IDictionary)poolByReuseField.GetValue(pool);
                Assert.IsTrue(poolByReuse.Contains(5u), "_poolByReuse 应含 key=5");

                // RenderObj 是 internal sealed class，用反射读 LastNodeId
                var ro = poolByReuse[5u];
                var lastNodeIdField = ro.GetType().GetField("LastNodeId",
                    System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.Instance);
                Assert.IsNotNull(lastNodeIdField, "应能反射 LastNodeId");
                Assert.AreEqual(200u, (uint)lastNodeIdField.GetValue(ro),
                    "复用后 LastNodeId 应为 200（更新为新 node_id）");

                // position 应已更新到帧2的坐标 (30,40)
                Assert.AreEqual(new Vector3(30f, 40f, 0f), go2.transform.localPosition,
                    "帧2 GO position 更新为 (30,40)");
            }
            finally
            {
                pool.Clear();
                mm.Clear();
                Object.DestroyImmediate(root);
            }
        }
    }

    /// MirrorPool UV 线性映射测试：core 产 [0,1] UV → sprite 在 atlas 页内的子区 uvRect。
    /// 验 RemapMeshUvToSprite 把 [0,1] 全图 UV 正确映射到 sprite 的 atlas 子区。
    public class MirrorPoolUvRemapTests
    {
        /// 构造含 path 表条目的 v10 1 节点 Mesh blob。
        static byte[] OneNodeBlobWithPath(uint id, string path, uint pathIdx,
            float x, float y, float w, float h)
        {
            var b = new List<byte>();
            b.AddRange(System.BitConverter.GetBytes(0x4D4F4F4Cu)); // magic
            b.AddRange(System.BitConverter.GetBytes(10u));          // version
            b.AddRange(System.BitConverter.GetBytes(1u));           // node_count

            int colOff = 116;
            int[] offs = new int[20];
            int[] elemSize = { 4, 4, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 1, 4, 4, 4, 1, 80, 1, 4 };
            for (int i = 0; i < 20; i++) { offs[i] = colOff; colOff += elemSize[i]; }
            int arenaOff = colOff;

            // mesh arena: 4 verts, 6 idx, UV [0,1] 全图
            var arena = new List<byte>();
            arena.AddRange(System.BitConverter.GetBytes(4));  // vert_count
            arena.AddRange(System.BitConverter.GetBytes(6));  // idx_count
            AppV(arena, 0f, 0f); AppV(arena, w, 0f); AppV(arena, w, h); AppV(arena, 0f, h);
            AppV(arena, 0f, 0f); AppV(arena, 1f, 0f); AppV(arena, 1f, 1f); AppV(arena, 0f, 1f);
            for (int v = 0; v < 4; v++)
            {
                arena.AddRange(System.BitConverter.GetBytes(1f));
                arena.AddRange(System.BitConverter.GetBytes(1f));
                arena.AddRange(System.BitConverter.GetBytes(1f));
                arena.AddRange(System.BitConverter.GetBytes(1f));
            }
            arena.AddRange(System.BitConverter.GetBytes(0u));
            arena.AddRange(System.BitConverter.GetBytes(1u));
            arena.AddRange(System.BitConverter.GetBytes(2u));
            arena.AddRange(System.BitConverter.GetBytes(0u));
            arena.AddRange(System.BitConverter.GetBytes(2u));
            arena.AddRange(System.BitConverter.GetBytes(3u));
            int arenaLen = arena.Count;

            // 构建 path 表
            byte[] pathBytes = System.Text.Encoding.UTF8.GetBytes(path);
            int pathTableLen = 4 + 4 + pathBytes.Length; // path_count:u32 + path_len:u32 + bytes

            // arena headers
            foreach (var o in offs) b.AddRange(System.BitConverter.GetBytes(o));
            b.AddRange(System.BitConverter.GetBytes(arenaOff));
            b.AddRange(System.BitConverter.GetBytes(arenaLen));
            int clipOff = arenaOff + arenaLen;
            b.AddRange(System.BitConverter.GetBytes(clipOff));
            b.AddRange(System.BitConverter.GetBytes(4u)); // clip_count only
            int pathOff = clipOff + 4;
            b.AddRange(System.BitConverter.GetBytes(pathOff));
            b.AddRange(System.BitConverter.GetBytes((uint)pathTableLen));

            // column data
            b.AddRange(System.BitConverter.GetBytes(id));          // col 0: node_id
            b.AddRange(System.BitConverter.GetBytes(-1));          // col 1: parent_id
            b.Add(1);                                              // col 2: visible
            b.AddRange(System.BitConverter.GetBytes(1f));          // col 3: alpha
            b.AddRange(System.BitConverter.GetBytes(0u));          // col 4: sort_key
            b.AddRange(System.BitConverter.GetBytes(0u));          // col 5: mask_context
            b.AddRange(System.BitConverter.GetBytes(1f));          // col 6-11: identity 2x2 + translate
            b.AddRange(System.BitConverter.GetBytes(0f));
            b.AddRange(System.BitConverter.GetBytes(0f));
            b.AddRange(System.BitConverter.GetBytes(1f));
            b.AddRange(System.BitConverter.GetBytes(x));
            b.AddRange(System.BitConverter.GetBytes(y));
            b.Add(1);                                              // col 12: payload_kind=Mesh
            b.AddRange(System.BitConverter.GetBytes(0u));          // col 13: mesh_off
            b.AddRange(System.BitConverter.GetBytes((uint)arenaLen)); // col 14: mesh_len
            b.AddRange(System.BitConverter.GetBytes(pathIdx));     // col 15: path_idx
            b.Add(0);                                              // col 16: program=0 (img/Container)
            for (int j = 0; j < 20; j++) b.AddRange(System.BitConverter.GetBytes(0f));
            b.Add((byte)2);                                        // col 18: change_level=FULL
            b.AddRange(System.BitConverter.GetBytes(0u));          // col 19: reuse_key=0

            b.AddRange(arena);
            b.AddRange(System.BitConverter.GetBytes(0u));          // clip_count=0
            // path table
            b.AddRange(System.BitConverter.GetBytes(1u));          // path_count=1
            b.AddRange(System.BitConverter.GetBytes((uint)pathBytes.Length)); // path_len
            b.AddRange(pathBytes);                                 // path_bytes
            return b.ToArray();

            static void AppV(List<byte> a, float vx, float vy) {
                a.AddRange(System.BitConverter.GetBytes(vx));
                a.AddRange(System.BitConverter.GetBytes(vy));
            }
        }

        /// core 产 [0,1] 全图 UV → 线性映射到 sprite 的 atlas 子区 (uvRect)。
        /// 断言：映射后 UV 落在 uvRect 内。
        [Test]
        public void RemapMeshUvMapsCoreUnitUvIntoSpriteSubRect()
        {
            var root = new GameObject("root");
            var shader = Shader.Find("LoomGUI/Unlit");
            var mm = new MaterialManager(shader);
            var pool = new MirrorPool();

            // 构造 SpriteResolver：一个 atlas，一个 sprite "sprites/test" 在子区 (0.2, 0.3, 0.1, 0.15)
            // uvRect = (x=0.2, y=0.3, w=0.1, h=0.15) 即 UV 子区 [0.2,0.3]..[0.3,0.45]
            UnityEngine.Rect knownUvRect = new UnityEngine.Rect(0.2f, 0.3f, 0.1f, 0.15f);
            var tex = new Texture2D(64, 64);
            var atlas = new AtlasManifest();
            atlas.pages.Add("test.png");
            atlas.sprites["sprites/test"] = new SpriteEntry {
                page = 0,
                uv = new float[] { knownUvRect.xMin, knownUvRect.yMin,
                                   knownUvRect.xMax, knownUvRect.yMax },
                orig = new int[] { 32, 32 }
            };
            var sprites = new SpriteResolver();
            sprites.Init(new List<AtlasManifest> { atlas }, fileName => tex);

            try
            {
                var blob = new FrameBlob(OneNodeBlobWithPath(
                    id: 100, path: "sprites/test", pathIdx: 1u,
                    x: 10f, y: 20f, w: 32f, h: 32f));
                Assert.AreEqual(1, blob.NodeCount, "NodeCount=1");
                Assert.AreEqual(1u, blob.PathIdx(0), "path_idx=1");
                Assert.AreEqual("sprites/test", blob.ReadPath(1u), "path resolves");

                pool.Sync(blob, root.transform, mm, sprites, Texture2D.whiteTexture);

                Assert.AreEqual(1, root.transform.childCount, "1 child GO");
                var mf = root.transform.GetChild(0).GetComponent<MeshFilter>();
                Assert.IsNotNull(mf, "MeshFilter present");

                var uvs = new List<UnityEngine.Vector2>();
                mf.sharedMesh.GetUVs(0, uvs);
                Assert.AreEqual(4, uvs.Count, "4 verts");

                // 每个映射后 UV 必须落在 uvRect 子区内
                for (int i = 0; i < uvs.Count; i++)
                {
                    Assert.GreaterOrEqual(uvs[i].x, knownUvRect.xMin - 1e-5f,
                        $"vert[{i}].x >= {knownUvRect.xMin}");
                    Assert.LessOrEqual(uvs[i].x, knownUvRect.xMax + 1e-5f,
                        $"vert[{i}].x <= {knownUvRect.xMax}");
                    Assert.GreaterOrEqual(uvs[i].y, knownUvRect.yMin - 1e-5f,
                        $"vert[{i}].y >= {knownUvRect.yMin}");
                    Assert.LessOrEqual(uvs[i].y, knownUvRect.yMax + 1e-5f,
                        $"vert[{i}].y <= {knownUvRect.yMax}");
                }

                // 核心 [0,0] → (uvRect.x, uvRect.y)，[1,1] → (uvRect.xMax, uvRect.yMax)
                Assert.AreEqual(knownUvRect.xMin, uvs[0].x, 1e-5f, "tl.u");
                Assert.AreEqual(knownUvRect.yMin, uvs[0].y, 1e-5f, "tl.v");
                Assert.AreEqual(knownUvRect.xMax, uvs[2].x, 1e-5f, "br.u");
                Assert.AreEqual(knownUvRect.yMax, uvs[2].y, 1e-5f, "br.v");
            }
            finally
            {
                pool.Clear();
                mm.Clear();
                Object.DestroyImmediate(root);
                Object.DestroyImmediate(tex);
            }
        }
    }
}
