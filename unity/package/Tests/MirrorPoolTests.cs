using System.Collections.Generic;
using NUnit.Framework;
using UnityEngine;

namespace LoomGUI.Tests
{
    /// MirrorPool reuse_key 按 reuse_key 复用 GO 的 EditMode 测试。
    /// 手搓 v14 1 节点 mesh blob（23 列含 reuse_key） → 验 slot 换绑 GO 复用。
    public class MirrorPoolReuseKeyTests
    {
        /// 构造一个 v14 1 节点 Mesh blob（23 列 SOA）。
        /// v14 = v13 + node_id/parent_id 列 4B→8B（#26 u64 拓宽）；header 128B。
        /// 线性矩阵参数（ma..md）供 bounds 补偿测试复用，默认 identity。
        internal static byte[] OneNodeBlobV14(
            uint id, float x, float y, float w, float h, uint sortKey,
            byte payloadKind = 1, byte changeLevel = 2, uint reuseKey = 0,
            float ma = 1f, float mb = 0f, float mc = 0f, float md = 1f)
        {
            var b = new List<byte>();

            // header: magic, version=14, node_count=1
            b.AddRange(System.BitConverter.GetBytes(0x4D4F4F4Cu));
            b.AddRange(System.BitConverter.GetBytes(14u));
            b.AddRange(System.BitConverter.GetBytes(1u));

            // header 总长 = 12 + 23*4 + 6*4 = 128。列 offset 从此起按 elemSize 递进。
            int colOff = 128;
            int[] offs = new int[23];
            // v14: 23 cols — node_id(8) parent_id(8) visible(1) alpha(4) sort_key(4) mask_context(4)
            //   m_a..m_ty(6×4) payload_kind(1) mesh_off(4) mesh_len(4) path_idx(4)
            //   program(1) color_matrix(80) change_level(1) reuse_key(4)
            //   effect_block(128) shadow_params(24) grad_params(208)
            int[] elemSize = { 8, 8, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 1, 4, 4, 4, 1, 80, 1, 4, 128, 24, 208 };
            for (int i = 0; i < 23; i++) { offs[i] = colOff; colOff += elemSize[i]; }
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

            // 23 列 offset + mesh/clip/path 三 arena off+len
            foreach (var o in offs) b.AddRange(System.BitConverter.GetBytes(o));
            b.AddRange(System.BitConverter.GetBytes(arenaOff));                      // mesh_arena_off
            b.AddRange(System.BitConverter.GetBytes(arenaLen));                      // mesh_arena_len
            int clipOff = arenaOff + arenaLen;
            b.AddRange(System.BitConverter.GetBytes(clipOff));                        // clip_table_off
            b.AddRange(System.BitConverter.GetBytes(4u));                             // clip_table_len（仅 clip_count）
            int pathOff = clipOff + 4;                                                // clip_count 后
            b.AddRange(System.BitConverter.GetBytes(pathOff));                        // path_table_off
            b.AddRange(System.BitConverter.GetBytes(4u));                             // path_table_len（仅 path_count）

            // 列数据 SOA（列优先，镜像 blob.rs v14 / FrameBlob）
            b.AddRange(System.BitConverter.GetBytes((ulong)id)); // col 0: node_id（v14 u64）
            b.AddRange(System.BitConverter.GetBytes(-1L));       // col 1: parent_id（v14 i64）
            b.Add(1);                                            // col 2: visible
            b.AddRange(System.BitConverter.GetBytes(1f));        // col 3: alpha
            b.AddRange(System.BitConverter.GetBytes(sortKey));   // col 4: sort_key
            b.AddRange(System.BitConverter.GetBytes(0u));        // col 5: mask_context
            b.AddRange(System.BitConverter.GetBytes(ma));        // col 6: m_a
            b.AddRange(System.BitConverter.GetBytes(mb));        // col 7: m_b
            b.AddRange(System.BitConverter.GetBytes(mc));        // col 8: m_c
            b.AddRange(System.BitConverter.GetBytes(md));        // col 9: m_d
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
            for (int j = 0; j < 128; j++) b.Add((byte)0);        // col 20: effect_block 全零
            for (int j = 0; j < 24; j++) b.Add((byte)0);         // col 21: shadow_params 全零
            for (int j = 0; j < 208; j++) b.Add((byte)0);        // col 22: grad_params 全零

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
                var blob1 = new FrameBlob(OneNodeBlobV14(
                    id: 100, x: 10f, y: 20f, w: 5f, h: 5f,
                    sortKey: 0, payloadKind: 1, changeLevel: 2, reuseKey: 5));
                Assert.AreEqual(1, blob1.NodeCount, "blob1 NodeCount=1");
                Assert.AreEqual(5u, blob1.ReuseKey(0), "blob1 reuse_key=5");
                pool.Sync(blob1, root.transform, mm, null, fallback);

                Assert.AreEqual(1, pool.Count, "帧1: pool.Count=1");
                Assert.AreEqual(1, root.transform.childCount, "帧1: 1 子 GO");
                var go1 = root.transform.GetChild(0).gameObject;

                // 帧 2：node_id=200, 同 reuse_key=5, Full → 应复用 GO（不销毁重建）
                var blob2 = new FrameBlob(OneNodeBlobV14(
                    id: 200, x: 30f, y: 40f, w: 5f, h: 5f,
                    sortKey: 0, payloadKind: 1, changeLevel: 2, reuseKey: 5));
                Assert.AreEqual(1, blob2.NodeCount, "blob2 NodeCount=1");
                Assert.AreEqual(200ul, blob2.NodeId(0), "blob2 node_id=200");
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
                Assert.IsTrue(poolByReuse.Contains(5ul), "_poolByReuse 应含 key=5");

                // RenderObj 是 internal sealed class，用反射读 LastNodeId（v14 起为 ulong）
                var ro = poolByReuse[5ul];
                var lastNodeIdField = ro.GetType().GetField("LastNodeId",
                    System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.Instance);
                Assert.IsNotNull(lastNodeIdField, "应能反射 LastNodeId");
                Assert.AreEqual(200ul, (ulong)lastNodeIdField.GetValue(ro),
                    "复用后 LastNodeId 应为 200（更新为新 node_id）");

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

    /// #66 剔除 bounds 补偿的幂等性回归：Header 帧（只更 header，不重建 mesh）不得在
    /// 已补偿的 bounds 上再乘一次线性矩阵——修前读 Mesh.bounds 顶替原始 AABB，
    /// 滚动中的旋转/缩放节点逐帧叠加（scale<1 几何级缩小 = #66 消失 bug 复发；
    /// 90° 非正方形交替轴交换；45° 无界膨胀）。修后从 RenderObj.RawMeshBounds 缓存底重算。
    public class MirrorPoolBoundsCompensationTests
    {
        /// 单场景：FULL 帧建立补偿 bounds，随后两个 HEADER 帧（改 m_tx 模拟滚动，
        /// 线性矩阵不变）。断言三个帧的 Mesh.bounds 全等，且首帧值 == AABB(L·原始) 数学期望。
        static void RunScenario(float ma, float mb, float mc, float md,
                                float w, float h,
                                Vector2 expectCenter, Vector2 expectExtents)
        {
            var root = new GameObject("root");
            var shader = Shader.Find("LoomGUI/Unlit");
            var mm = new MaterialManager(shader);
            var pool = new MirrorPool();
            var fallback = Texture2D.whiteTexture;

            try
            {
                // 帧 1：FULL（上传 mesh + RecalculateBounds + 首次补偿）
                pool.Sync(new FrameBlob(MirrorPoolReuseKeyTests.OneNodeBlobV14(
                    id: 100, x: 10f, y: 20f, w: w, h: h, sortKey: 0,
                    payloadKind: 1, changeLevel: 2, reuseKey: 0,
                    ma: ma, mb: mb, mc: mc, md: md)),
                    root.transform, mm, null, fallback);
                var mesh = root.transform.GetChild(0).GetComponent<MeshFilter>().sharedMesh;
                var b1 = mesh.bounds;
                Assert.AreEqual(expectCenter.x, b1.center.x, 1e-4f, "帧1 center.x == AABB(L·B)");
                Assert.AreEqual(expectCenter.y, b1.center.y, 1e-4f, "帧1 center.y == AABB(L·B)");
                Assert.AreEqual(expectExtents.x, b1.extents.x, 1e-4f, "帧1 extents.x == AABB(L·B)");
                Assert.AreEqual(expectExtents.y, b1.extents.y, 1e-4f, "帧1 extents.y == AABB(L·B)");

                // 帧 2、3：HEADER（change_level=1，mesh 不重建，仅平移变化模拟滚动）
                for (int frame = 2; frame <= 3; frame++)
                {
                    pool.Sync(new FrameBlob(MirrorPoolReuseKeyTests.OneNodeBlobV14(
                        id: 100, x: 10f + frame * 7f, y: 20f, w: w, h: h, sortKey: 0,
                        payloadKind: 1, changeLevel: 1, reuseKey: 0,
                        ma: ma, mb: mb, mc: mc, md: md)),
                        root.transform, mm, null, fallback);
                    var bn = root.transform.GetChild(0).GetComponent<MeshFilter>().sharedMesh.bounds;
                    Assert.AreEqual(b1.center.x, bn.center.x, 1e-5f,
                        $"帧{frame} center.x 不得漂移（Header 帧重复补偿 = 叠加）");
                    Assert.AreEqual(b1.center.y, bn.center.y, 1e-5f,
                        $"帧{frame} center.y 不得漂移");
                    Assert.AreEqual(b1.extents.x, bn.extents.x, 1e-5f,
                        $"帧{frame} extents.x 不得漂移（scale<1 会几何级缩小）");
                    Assert.AreEqual(b1.extents.y, bn.extents.y, 1e-5f,
                        $"帧{frame} extents.y 不得漂移");
                }
            }
            finally
            {
                pool.Clear();
                mm.Clear();
                Object.DestroyImmediate(root);
            }
        }

        [Test]
        public void HeaderFramesDoNotReapplyScaleCompensation()
        {
            // quad 8×4（raw center(4,2) extents(4,2)）× scale 0.5 → 视觉 [0,4]×[0,2]：
            // 补偿后 center(2,1) extents(2,1)。修前每 Header 帧再缩一半（2→1→0.5…）。
            RunScenario(0.5f, 0f, 0f, 0.5f, 8f, 4f, new Vector2(2f, 1f), new Vector2(2f, 1f));
        }

        [Test]
        public void HeaderFramesDoNotAxisSwapRotationCompensation()
        {
            // quad 2×8 旋转 90°（L: rx=-y, ry=x）→ 视觉 x∈[-8,0], y∈[0,2]：
            // 补偿后 center(-4,1) extents(4,1)。修前 Header 帧把 bounds 转回 2×8 形状（轴交换）。
            RunScenario(0f, 1f, -1f, 0f, 2f, 8f, new Vector2(-4f, 1f), new Vector2(4f, 1f));
        }
    }

    /// MirrorPool UV 线性映射测试：core 产 [0,1] UV → sprite 在 atlas 页内的子区 uvRect。
    /// 验 RemapMeshUvToSprite 把 [0,1] 全图 UV 正确映射到 sprite 的 atlas 子区。
    public class MirrorPoolUvRemapTests
    {
        /// 构造含 path 表条目的 v14 1 节点 Mesh blob。
        static byte[] OneNodeBlobWithPath(uint id, string path, uint pathIdx,
            float x, float y, float w, float h)
        {
            var b = new List<byte>();
            b.AddRange(System.BitConverter.GetBytes(0x4D4F4F4Cu)); // magic
            b.AddRange(System.BitConverter.GetBytes(14u));          // version
            b.AddRange(System.BitConverter.GetBytes(1u));           // node_count

            int colOff = 128;
            int[] offs = new int[23];
            int[] elemSize = { 8, 8, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 1, 4, 4, 4, 1, 80, 1, 4, 128, 24, 208 };
            for (int i = 0; i < 23; i++) { offs[i] = colOff; colOff += elemSize[i]; }
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
            b.AddRange(System.BitConverter.GetBytes((ulong)id)); // col 0: node_id（v14 u64）
            b.AddRange(System.BitConverter.GetBytes(-1L));       // col 1: parent_id（v14 i64）
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
            for (int j = 0; j < 128; j++) b.Add((byte)0);          // col 20: effect_block 全零
            for (int j = 0; j < 24; j++) b.Add((byte)0);           // col 21: shadow_params 全零
            for (int j = 0; j < 208; j++) b.Add((byte)0);          // col 22: grad_params 全零

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

    /// MirrorPool parked keepalive lifecycle 测试。
    /// 验 parked→active 过渡：parked 保留 GO 并 SetActive(false)、reactivate 恢复、
    /// lazy（无历史 GO 不创建）、稳态零 churn。
    /// 需要 Unity Editor（EditMode tests），构造 v14 blob 驱动 MirrorPool.Sync。
    public class MirrorPoolParkedLifecycleTests
    {
        /// 构造 v14 blob，支持 active + parked 混合条目。
        /// 每条目 (visByte, nodeId, reuseKey)。
        ///   visByte 0x01 = active → 自动设 changeLevel=2, payloadKind=1, 附加 quad mesh。
        ///   visByte 0x02 = parked → 自动设 changeLevel=0, payloadKind=0, 无 mesh。
        ///   其他列填零/identity。
        static byte[] BuildV14Blob(params (byte visByte, uint nodeId, uint reuseKey)[] entries)
        {
            int N = entries.Length;
            var b = new List<byte>();

            // header: magic, version=14, node_count
            b.AddRange(System.BitConverter.GetBytes(0x4D4F4F4Cu));
            b.AddRange(System.BitConverter.GetBytes(14u));
            b.AddRange(System.BitConverter.GetBytes((uint)N));

            // v14: 23 col strides (bytes per entry)
            int[] stride = { 8, 8, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 1, 4, 4, 4, 1, 80, 1, 4, 128, 24, 208 };

            // col offsets (SOA): header 128B then each col = prev + stride[prev] * N
            const int headerLen = 128;
            int off = headerLen;
            for (int i = 0; i < 23; i++)
            {
                b.AddRange(System.BitConverter.GetBytes((uint)off));
                off += stride[i] * N;
            }
            // arena headers: mesh/clip/path — will fill after building arenas
            int colEnd = off; // end of SOA data, start of arenas
            // reserve arena header slots (6 × u32 = 24B), fill after
            int arenaHeaderPos = b.Count;
            for (int k = 0; k < 6; k++) b.AddRange(System.BitConverter.GetBytes(0u));

            // Build column data + mesh arena
            var meshArena = new List<byte>();
            byte[] meshOffVals = new byte[N * 4];  // col 13: mesh_off per entry (write after arena)
            byte[] meshLenVals = new byte[N * 4];  // col 14: mesh_len per entry

            // Pre-compute mesh offsets: only for active entries (visByte & 1)
            for (int i = 0; i < N; i++)
            {
                bool active = (entries[i].visByte & 0x01) != 0;
                if (active)
                {
                    uint mOff = (uint)meshArena.Count;
                    System.Buffer.BlockCopy(System.BitConverter.GetBytes(mOff), 0, meshOffVals, i * 4, 4);
                    // simple quad: 10×10 at origin
                    meshArena.AddRange(System.BitConverter.GetBytes(4u));  // vert_count
                    meshArena.AddRange(System.BitConverter.GetBytes(6u));  // idx_count
                    // verts (4 × vec2)
                    AppendV2(meshArena, 0f, 0f); AppendV2(meshArena, 10f, 0f);
                    AppendV2(meshArena, 10f, 10f); AppendV2(meshArena, 0f, 10f);
                    // uvs (4 × vec2, [0,1] full-image)
                    AppendV2(meshArena, 0f, 0f); AppendV2(meshArena, 1f, 0f);
                    AppendV2(meshArena, 1f, 1f); AppendV2(meshArena, 0f, 1f);
                    // colors (4 × RGBA, white)
                    for (int v = 0; v < 4; v++)
                    {
                        meshArena.AddRange(System.BitConverter.GetBytes(1f));
                        meshArena.AddRange(System.BitConverter.GetBytes(1f));
                        meshArena.AddRange(System.BitConverter.GetBytes(1f));
                        meshArena.AddRange(System.BitConverter.GetBytes(1f));
                    }
                    // indices (6 × u32, two triangles)
                    meshArena.AddRange(System.BitConverter.GetBytes(0u));
                    meshArena.AddRange(System.BitConverter.GetBytes(1u));
                    meshArena.AddRange(System.BitConverter.GetBytes(2u));
                    meshArena.AddRange(System.BitConverter.GetBytes(0u));
                    meshArena.AddRange(System.BitConverter.GetBytes(2u));
                    meshArena.AddRange(System.BitConverter.GetBytes(3u));
                    uint mLen = (uint)(meshArena.Count - (int)mOff);
                    System.Buffer.BlockCopy(System.BitConverter.GetBytes(mLen), 0, meshLenVals, i * 4, 4);
                }
                // parked entries: mesh_off=0, mesh_len=0 (stays zero-initialized)
            }

            // Write SOA columns
            // col 0: node_id（v14 u64）
            for (int i = 0; i < N; i++)
                b.AddRange(System.BitConverter.GetBytes((ulong)entries[i].nodeId));
            // col 1: parent_id（v14 i64, -1 = none）
            for (int i = 0; i < N; i++)
                b.AddRange(System.BitConverter.GetBytes(-1L));
            // col 2: visible byte
            for (int i = 0; i < N; i++)
                b.Add(entries[i].visByte);
            // col 3: alpha (1.0)
            for (int i = 0; i < N; i++)
                b.AddRange(System.BitConverter.GetBytes(1f));
            // col 4: sort_key (0)
            for (int i = 0; i < N; i++)
                b.AddRange(System.BitConverter.GetBytes(0u));
            // col 5: mask_context (0)
            for (int i = 0; i < N; i++)
                b.AddRange(System.BitConverter.GetBytes(0u));
            // col 6-11: identity 2×2 + (0,0) translate
            for (int i = 0; i < N; i++) b.AddRange(System.BitConverter.GetBytes(1f));  // m_a
            for (int i = 0; i < N; i++) b.AddRange(System.BitConverter.GetBytes(0f));  // m_b
            for (int i = 0; i < N; i++) b.AddRange(System.BitConverter.GetBytes(0f));  // m_c
            for (int i = 0; i < N; i++) b.AddRange(System.BitConverter.GetBytes(1f));  // m_d
            for (int i = 0; i < N; i++) b.AddRange(System.BitConverter.GetBytes(0f));  // m_tx
            for (int i = 0; i < N; i++) b.AddRange(System.BitConverter.GetBytes(0f));  // m_ty
            // col 12: payload_kind (1 for active, 0 for parked)
            for (int i = 0; i < N; i++)
                b.Add((byte)((entries[i].visByte & 0x01) != 0 ? 1 : 0));
            // col 13: mesh_off
            b.AddRange(meshOffVals);
            // col 14: mesh_len
            b.AddRange(meshLenVals);
            // col 15: path_idx (0)
            for (int i = 0; i < N; i++)
                b.AddRange(System.BitConverter.GetBytes(0u));
            // col 16: program (0)
            for (int i = 0; i < N; i++) b.Add((byte)0);
            // col 17: color_matrix (80B zeros per entry)
            for (int i = 0; i < N; i++)
                for (int j = 0; j < 80; j++) b.Add((byte)0);
            // col 18: change_level (2=Full for active, 0=Skip for parked)
            for (int i = 0; i < N; i++)
                b.Add((byte)((entries[i].visByte & 0x01) != 0 ? 2 : 0));
            // col 19: reuse_key
            for (int i = 0; i < N; i++)
                b.AddRange(System.BitConverter.GetBytes(entries[i].reuseKey));
            // col 20: effect_block (128B zeros per entry)
            for (int i = 0; i < N; i++)
                for (int j = 0; j < 128; j++) b.Add((byte)0);
            // col 21: shadow_params (24B zeros per entry)
            for (int i = 0; i < N; i++)
                for (int j = 0; j < 24; j++) b.Add((byte)0);
            // col 22: grad_params (208B zeros per entry)
            for (int i = 0; i < N; i++)
                for (int j = 0; j < 208; j++) b.Add((byte)0);

            // Now fill arena headers: mesh_arena at colEnd
            int meshArenaStart = colEnd;
            int meshArenaEnd = meshArenaStart + meshArena.Count;
            // clip_table: just clip_count=0 (4B)
            int clipStart = meshArenaEnd;
            int clipLen = 4;
            // path_table: just path_count=0 (4B)
            int pathStart = clipStart + clipLen;
            int pathLen = 4;

            // Write arena headers into reserved slot (arenaHeaderPos)
            byte[] arenaHeaderBytes = new byte[24];
            System.Buffer.BlockCopy(System.BitConverter.GetBytes((uint)meshArenaStart), 0, arenaHeaderBytes, 0, 4);
            System.Buffer.BlockCopy(System.BitConverter.GetBytes((uint)meshArena.Count), 0, arenaHeaderBytes, 4, 4);
            System.Buffer.BlockCopy(System.BitConverter.GetBytes((uint)clipStart), 0, arenaHeaderBytes, 8, 4);
            System.Buffer.BlockCopy(System.BitConverter.GetBytes((uint)clipLen), 0, arenaHeaderBytes, 12, 4);
            System.Buffer.BlockCopy(System.BitConverter.GetBytes((uint)pathStart), 0, arenaHeaderBytes, 16, 4);
            System.Buffer.BlockCopy(System.BitConverter.GetBytes((uint)pathLen), 0, arenaHeaderBytes, 20, 4);
            // Overwrite reserved arena header bytes
            for (int k = 0; k < 24; k++) b[arenaHeaderPos + k] = arenaHeaderBytes[k];

            // Append arena data
            b.AddRange(meshArena);
            b.AddRange(System.BitConverter.GetBytes(0u));  // clip_count=0
            b.AddRange(System.BitConverter.GetBytes(0u));  // path_count=0

            return b.ToArray();

            static void AppendV2(List<byte> a, float x, float y)
            {
                a.AddRange(System.BitConverter.GetBytes(x));
                a.AddRange(System.BitConverter.GetBytes(y));
            }
        }

        [Test]
        public void ParkedKeepalive_KeepsGo_Inactive()
        {
            var root = new GameObject("root");
            var shader = Shader.Find("LoomGUI/Unlit");
            var mm = new MaterialManager(shader);
            var pool = new MirrorPool();
            var fallback = Texture2D.whiteTexture;

            try
            {
                // Frame 1: active entry creates GO
                var blob1 = new FrameBlob(BuildV14Blob(
                    (0x01, nodeId: 100, reuseKey: 5)));
                Assert.That(blob1.IsValid, Is.True, "v14 blob valid");
                pool.Sync(blob1, root.transform, mm, null, fallback);
                Assert.That(pool.Count, Is.EqualTo(1), "frame1: GO created");

                // Get the GO via reflection
                var poolByReuseField = typeof(MirrorPool).GetField("_poolByReuse",
                    System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance);
                var poolByReuse = (System.Collections.IDictionary)poolByReuseField.GetValue(pool);
                var ro = poolByReuse[5ul];
                var goField = ro.GetType().GetField("Go",
                    System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.Instance);
                var go = (GameObject)goField.GetValue(ro);
                Assert.That(go.activeSelf, Is.True, "frame1: GO active");

                // Frame 2: same slot now parked
                var blob2 = new FrameBlob(BuildV14Blob(
                    (0x02, nodeId: 100, reuseKey: 5)));
                Assert.That(blob2.IsValid, Is.True);
                Assert.That(blob2.Parked(0), Is.True, "blob2.Parked=true");
                Assert.That(blob2.Visible(0), Is.False, "blob2.Visible=false");
                pool.Sync(blob2, root.transform, mm, null, fallback);

                Assert.That(pool.Count, Is.EqualTo(1), "frame2: GO kept, not destroyed");
                Assert.That(go.activeSelf, Is.False, "frame2: GO SetActive(false)");
            }
            finally
            {
                pool.Clear();
                mm.Clear();
                Object.DestroyImmediate(root);
            }
        }

        [Test]
        public void Reactivate_SetsActive_AfterParked()
        {
            var root = new GameObject("root");
            var shader = Shader.Find("LoomGUI/Unlit");
            var mm = new MaterialManager(shader);
            var pool = new MirrorPool();
            var fallback = Texture2D.whiteTexture;

            try
            {
                // Frame 1: active → creates GO
                pool.Sync(new FrameBlob(BuildV14Blob(
                    (0x01, nodeId: 100, reuseKey: 5))),
                    root.transform, mm, null, fallback);
                Assert.That(pool.Count, Is.EqualTo(1));

                // Get GO reference
                var poolByReuseField = typeof(MirrorPool).GetField("_poolByReuse",
                    System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance);
                var poolByReuse = (System.Collections.IDictionary)poolByReuseField.GetValue(pool);
                var ro = poolByReuse[5ul];
                var goField = ro.GetType().GetField("Go",
                    System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.Instance);
                var go = (GameObject)goField.GetValue(ro);

                // Frame 2: parked → GO kept, inactive
                pool.Sync(new FrameBlob(BuildV14Blob(
                    (0x02, nodeId: 100, reuseKey: 5))),
                    root.transform, mm, null, fallback);
                Assert.That(pool.Count, Is.EqualTo(1), "parked: GO kept");
                Assert.That(go.activeSelf, Is.False, "parked: GO inactive");

                // Frame 3: reactivated → GO SetActive(true) again
                pool.Sync(new FrameBlob(BuildV14Blob(
                    (0x01, nodeId: 200, reuseKey: 5))),
                    root.transform, mm, null, fallback);
                Assert.That(pool.Count, Is.EqualTo(1), "reactivate: GO still kept");
                Assert.That(go.activeSelf, Is.True, "reactivate: GO active again");
            }
            finally
            {
                pool.Clear();
                mm.Clear();
                Object.DestroyImmediate(root);
            }
        }

        [Test]
        public void ParkedNoPriorGo_DoesNotCreate()
        {
            var root = new GameObject("root");
            var shader = Shader.Find("LoomGUI/Unlit");
            var mm = new MaterialManager(shader);
            var pool = new MirrorPool();
            var fallback = Texture2D.whiteTexture;

            try
            {
                // Parked blob with no prior GO
                var blob = new FrameBlob(BuildV14Blob(
                    (0x02, nodeId: 100, reuseKey: 5)));
                Assert.That(blob.IsValid, Is.True);
                pool.Sync(blob, root.transform, mm, null, fallback);

                Assert.That(pool.Count, Is.EqualTo(0), "lazy: no GO created for parked-only entry");
                Assert.That(root.transform.childCount, Is.EqualTo(0), "no child GO");
            }
            finally
            {
                pool.Clear();
                mm.Clear();
                Object.DestroyImmediate(root);
            }
        }

        [Test]
        public void SteadyState_ZeroChurn()
        {
            var root = new GameObject("root");
            var shader = Shader.Find("LoomGUI/Unlit");
            var mm = new MaterialManager(shader);
            var pool = new MirrorPool();
            var fallback = Texture2D.whiteTexture;

            try
            {
                // Frame 1: active → creates GO
                var blobActive = new FrameBlob(BuildV14Blob(
                    (0x01, nodeId: 100, reuseKey: 5)));
                pool.Sync(blobActive, root.transform, mm, null, fallback);
                Assert.That(pool.Count, Is.EqualTo(1), "frame1: GO created");
                var go1 = root.transform.GetChild(0).gameObject;

                // Frame 2: same active entry, change_level=2 again (steady state)
                pool.Sync(blobActive, root.transform, mm, null, fallback);

                Assert.That(pool.Count, Is.EqualTo(1), "frame2: still 1 GO");
                Assert.That(root.transform.childCount, Is.EqualTo(1), "frame2: still 1 child");
                var go2 = root.transform.GetChild(0).gameObject;
                Assert.That(ReferenceEquals(go1, go2), Is.True, "same GO reused, not recreated");
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
