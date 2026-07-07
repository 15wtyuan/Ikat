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

    /// LoomStage 纯 C# 类构造测试（B2）。
    /// 锁：LoomStage 不再是 MonoBehaviour——new 构造可用、无 Unity 生命周期依赖；
    /// 无字体注册时 Tick(null) 不崩（stage 句柄建好，borrow_frame 返空帧 → 跳渲染 → 事件派发空过）。
    public class LoomStagePureClassTests
    {
        /// LoomStage 是纯 class（非 Component）：new 构造 + Tick(null) 不崩。
        [Test]
        public void LoomStage_ConstructsAsPureClass_WithoutMonoBehaviour()
        {
            using var stage = new LoomStage(new Vector2(1080, 1920));
            Assert.IsFalse(stage is UnityEngine.Component,
                "LoomStage must be a pure class, not a MonoBehaviour/Component");
            // 无字体注册 + 无 renderRoot → tick 走空帧路径不崩（stage 句柄非空即 tick）。
            stage.Tick(0.016f);
            Assert.Pass("LoomStage 构造 + 空 tick 成功（pure class，无 MonoBehaviour 依赖）");
        }
    }
}
