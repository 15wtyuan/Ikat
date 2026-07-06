using System.Collections.Generic;
using NUnit.Framework;
using UnityEngine;

namespace LoomGUI.Tests
{
    /// MirrorPool v9 reuse_key 按 reuse_key 复用 GO 的 EditMode 测试。
    /// 手搓 v9 1 节点 mesh blob（22 列含 reuse_key） → 验 slot 换绑 GO 复用。
    public class MirrorPoolReuseKeyTests
    {
        /// 构造一个 v9 1 节点 Mesh blob（22 列 SOA）。
        /// 与 OneMeshNodeBlob 的区别：v9 增加了 program/color_matrix/change_level/reuse_key 列。
        static byte[] OneNodeBlobV9(
            uint id, float x, float y, float w, float h, uint sortKey,
            byte payloadKind = 1, byte changeLevel = 2, uint reuseKey = 0)
        {
            var b = new List<byte>();

            // header: magic, version=9, node_count=1
            b.AddRange(System.BitConverter.GetBytes(0x4D4F4F4Cu));
            b.AddRange(System.BitConverter.GetBytes(9u));
            b.AddRange(System.BitConverter.GetBytes(1u));

            // header 总长 = 12 + 22*4 + 8*4 = 132。列 offset 从此起按 elemSize 递进。
            int colOff = 132;
            int[] offs = new int[22];
            int[] elemSize = { 4, 4, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 1, 4, 4, 4, 4, 4, 1, 80, 1, 4 };
            for (int i = 0; i < 22; i++) { offs[i] = colOff; colOff += elemSize[i]; }
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

            // 22 列 offset + mesh/text/clip/path 四 arena off+len
            foreach (var o in offs) b.AddRange(System.BitConverter.GetBytes(o));
            b.AddRange(System.BitConverter.GetBytes(arenaOff));                      // mesh_arena_off
            b.AddRange(System.BitConverter.GetBytes(arenaLen));                      // mesh_arena_len
            b.AddRange(System.BitConverter.GetBytes(arenaOff + arenaLen));            // text_arena_off
            b.AddRange(System.BitConverter.GetBytes(0u));                             // text_arena_len（空）
            int clipOff = arenaOff + arenaLen;
            b.AddRange(System.BitConverter.GetBytes(clipOff));                        // clip_table_off
            b.AddRange(System.BitConverter.GetBytes(4u));                             // clip_table_len（仅 clip_count）
            int pathOff = clipOff + 4;                                                // clip_count 后
            b.AddRange(System.BitConverter.GetBytes(pathOff));                        // path_table_off
            b.AddRange(System.BitConverter.GetBytes(4u));                             // path_table_len（仅 path_count）

            // 列数据 SOA（列优先，镜像 blob.rs/FrameBlob）
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
            b.AddRange(System.BitConverter.GetBytes(0u));        // col 15: text_off
            b.AddRange(System.BitConverter.GetBytes(0u));        // col 16: text_len
            b.AddRange(System.BitConverter.GetBytes(0u));        // col 17: path_idx
            b.Add(0);                                            // col 18: program（img/无图 Container）
            // col 19: color_matrix（[f32;20] = 80B，全零）
            for (int j = 0; j < 20; j++) b.AddRange(System.BitConverter.GetBytes(0f));
            b.Add(changeLevel);                                  // col 20: change_level
            b.AddRange(System.BitConverter.GetBytes(reuseKey));  // col 21: reuse_key  ← v9

            b.AddRange(arena);
            // text_arena 空，跳过。
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
                var fonts = new Dictionary<string, Font>();
                // 帧 1：node_id=100, reuse_key=5, Full → 建 GO
                var blob1 = new FrameBlob(OneNodeBlobV9(
                    id: 100, x: 10f, y: 20f, w: 5f, h: 5f,
                    sortKey: 0, payloadKind: 1, changeLevel: 2, reuseKey: 5));
                Assert.AreEqual(1, blob1.NodeCount, "blob1 NodeCount=1");
                Assert.AreEqual(5u, blob1.ReuseKey(0), "blob1 reuse_key=5");
                pool.Sync(blob1, root.transform, mm, null, fallback, fonts, null, fontVersion: 0);

                Assert.AreEqual(1, pool.Count, "帧1: pool.Count=1");
                Assert.AreEqual(1, root.transform.childCount, "帧1: 1 子 GO");
                var go1 = root.transform.GetChild(0).gameObject;

                // 帧 2：node_id=200, 同 reuse_key=5, Full → 应复用 GO（不销毁重建）
                var blob2 = new FrameBlob(OneNodeBlobV9(
                    id: 200, x: 30f, y: 40f, w: 5f, h: 5f,
                    sortKey: 0, payloadKind: 1, changeLevel: 2, reuseKey: 5));
                Assert.AreEqual(1, blob2.NodeCount, "blob2 NodeCount=1");
                Assert.AreEqual(200u, blob2.NodeId(0), "blob2 node_id=200");
                Assert.AreEqual(5u, blob2.ReuseKey(0), "blob2 reuse_key=5");
                pool.Sync(blob2, root.transform, mm, null, fallback, fonts, null, fontVersion: 0);

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

    /// MirrorPool text 节点 content 变化重建 mesh 的 EditMode 回归测试。
    /// 锁：text 节点 level==2 (Full) 时必须重建 mesh（与 mesh 分支对称）。
    /// 原 bug（偶现错乱）：text 分支 needRebuild 门控只看 font version，
    /// 已渲染 text 内容更新、字体没变 → 不重建 → 画面停在旧文字。
    public class MirrorPoolTextTests
    {
        const string DejaVuPath = "Assets/Fonts/DejaVuSans.ttf";

        /// 构造单节点 text blob（kind=2，v9 22 列 SOA + text_arena 段）。
        /// 镜像 OneNodeBlobV9（mesh）但 payload_kind=2、mesh_arena 空、text_arena 含 glyphs。
        static byte[] OneTextBlobV9(GlyphData[] glyphs, byte changeLevel = 2)
        {
            var b = new List<byte>();
            b.AddRange(System.BitConverter.GetBytes(0x4D4F4F4Cu)); // magic
            b.AddRange(System.BitConverter.GetBytes(9u));          // version=9
            b.AddRange(System.BitConverter.GetBytes(1u));          // node_count=1

            int colOff = 132;
            int[] offs = new int[22];
            int[] elemSize = { 4,4,1,4,4,4,4,4,4,4,4,4,1,4,4,4,4,4,1,80,1,4 };
            for (int i = 0; i < 22; i++) { offs[i] = colOff; colOff += elemSize[i]; }

            // text_arena 段：font_size:u32 | color:f32×4 | glyph_count:u32 | glyphs[count×{cp,px,py}]
            var textArena = new List<byte>();
            textArena.AddRange(System.BitConverter.GetBytes(24u));  // font_size
            textArena.AddRange(System.BitConverter.GetBytes(1f));   // r
            textArena.AddRange(System.BitConverter.GetBytes(1f));   // g
            textArena.AddRange(System.BitConverter.GetBytes(1f));   // b
            textArena.AddRange(System.BitConverter.GetBytes(1f));   // a
            textArena.AddRange(System.BitConverter.GetBytes((uint)glyphs.Length));
            foreach (var g in glyphs)
            {
                textArena.AddRange(System.BitConverter.GetBytes(g.Codepoint));
                textArena.AddRange(System.BitConverter.GetBytes(g.PenX));
                textArena.AddRange(System.BitConverter.GetBytes(g.PenY));
            }
            int textArenaLen = textArena.Count;
            int textArenaOff = colOff;   // mesh_arena 空（len=0），text 紧跟其后
            int clipOff = textArenaOff + textArenaLen;
            int pathOff = clipOff + 4;

            foreach (var o in offs) b.AddRange(System.BitConverter.GetBytes(o));
            b.AddRange(System.BitConverter.GetBytes(textArenaOff)); b.AddRange(System.BitConverter.GetBytes(0u));                   // mesh_arena 空
            b.AddRange(System.BitConverter.GetBytes(textArenaOff)); b.AddRange(System.BitConverter.GetBytes((uint)textArenaLen));   // text_arena
            b.AddRange(System.BitConverter.GetBytes(clipOff));      b.AddRange(System.BitConverter.GetBytes(4u));                   // clip_table（count=0）
            b.AddRange(System.BitConverter.GetBytes(pathOff));      b.AddRange(System.BitConverter.GetBytes(4u));                   // path_table（count=0）

            b.AddRange(System.BitConverter.GetBytes(7u));   // col0 node_id
            b.AddRange(System.BitConverter.GetBytes(-1));   // col1 parent_id
            b.Add(1);                                       // col2 visible
            b.AddRange(System.BitConverter.GetBytes(1f));   // col3 alpha
            b.AddRange(System.BitConverter.GetBytes(0u));   // col4 sort_key
            b.AddRange(System.BitConverter.GetBytes(0u));   // col5 mask_context
            b.AddRange(System.BitConverter.GetBytes(1f));   // col6 m_a
            b.AddRange(System.BitConverter.GetBytes(0f));   // col7 m_b
            b.AddRange(System.BitConverter.GetBytes(0f));   // col8 m_c
            b.AddRange(System.BitConverter.GetBytes(1f));   // col9 m_d
            b.AddRange(System.BitConverter.GetBytes(0f));   // col10 m_tx
            b.AddRange(System.BitConverter.GetBytes(0f));   // col11 m_ty
            b.Add(2);                                       // col12 payload_kind=2 (Text)
            b.AddRange(System.BitConverter.GetBytes(0u));   // col13 mesh_off
            b.AddRange(System.BitConverter.GetBytes(0u));   // col14 mesh_len
            b.AddRange(System.BitConverter.GetBytes(0u));   // col15 text_off（arena 内偏移）
            b.AddRange(System.BitConverter.GetBytes((uint)textArenaLen)); // col16 text_len
            b.AddRange(System.BitConverter.GetBytes(0u));   // col17 path_idx
            b.Add(1);                                       // col18 program=1 (Text)
            for (int j = 0; j < 20; j++) b.AddRange(System.BitConverter.GetBytes(0f)); // col19 color_matrix
            b.Add(changeLevel);                             // col20 change_level
            b.AddRange(System.BitConverter.GetBytes(0u));   // col21 reuse_key

            b.AddRange(textArena);
            b.AddRange(System.BitConverter.GetBytes(0u));   // clip_count=0
            b.AddRange(System.BitConverter.GetBytes(0u));   // path_count=0
            return b.ToArray();
        }

        /// text 节点 content 变（Full）→ 必须重建 mesh。
        /// 帧1 "A"（1 glyph, 4 verts）→ 帧2 "AB"（2 glyph, 8 verts）。
        /// 触发条件：两次 Sync 之间 FontVersion 不变（无 atlas rebuild）。
        /// 原 bug：text 分支 needRebuild=fontDirty||LastFontVersion!=FontVersion → font 没变=false
        /// → 不重建 → mesh2.vertexCount 仍 4（画面停在 'A'）。
        [Test]
        public void TextContentChangeOnFull_RebuildsMesh()
        {
#if UNITY_EDITOR
            var font = UnityEditor.AssetDatabase.LoadAssetAtPath<Font>(DejaVuPath);
            Assume.That(font, Is.Not.Null, $"DejaVu 字体应在 {DejaVuPath}");

            var root = new GameObject("root");
            var shader = Shader.Find("LoomGUI/Unlit");
            var mm = new MaterialManager(shader);
            var pool = new MirrorPool();
            var fallback = Texture2D.whiteTexture;
            var fonts = new Dictionary<string, Font> { ["DejaVu"] = font };

            try
            {
                // 帧1：1 字 'A'，Full → 建 GO + mesh 4 verts
                var blob1 = new FrameBlob(OneTextBlobV9(new[] { new GlyphData((uint)'A', 0f, 20f) }));
                pool.Sync(blob1, root.transform, mm, null, fallback, fonts, font, fontVersion: 0);
                Assume.That(root.transform.childCount, Is.EqualTo(1), "帧1 建 1 GO");
                var mesh1 = root.transform.GetChild(0).GetComponent<MeshFilter>().sharedMesh;
                Assume.That(mesh1.vertexCount, Is.EqualTo(4), "帧1: 'A' 1 glyph = 4 verts");

                // 关键：两次 Sync 间 fontVersion 不变（未触发 atlas rebuild）→ 原 bug 在此失效。
                // 帧2 content 变（A→AB），Rust 侧 payload_hash 全量采样 glyph → Full。
                var blob2 = new FrameBlob(OneTextBlobV9(new[] {
                    new GlyphData((uint)'A', 0f, 20f),
                    new GlyphData((uint)'B', 30f, 20f),
                }));
                pool.Sync(blob2, root.transform, mm, null, fallback, fonts, font, fontVersion: 0);

                var mesh2 = root.transform.GetChild(0).GetComponent<MeshFilter>().sharedMesh;
                Assert.AreEqual(8, mesh2.vertexCount,
                    "帧2 'AB' (Full) → 必须重建 mesh 为 8 verts；" +
                    "原 bug：needRebuild 只看 font version，font 没变 → 不重建 → 仍 4 verts（画面停在 'A'，偶现错乱）");
            }
            finally
            {
                pool.Clear();
                mm.Clear();
                Object.DestroyImmediate(root);
            }
#else
            Assert.Inconclusive("EditMode-only（AssetDatabase）");
#endif
        }

        /// font atlas rebuild 后，Skip text（content 没变、blob 无 text 段）必须用缓存 glyphs 重建 mesh。
        /// 这是本轮「上下颠倒」的根因路径：atlas rebuild → 所有 text UV 失效 → 但 Skip text 不进重建分支。
        /// 帧1 Full 填缓存 → 清 mesh（区分「重建」vs「保留旧 mesh」）→ OnRebuilt 模拟 rebuild →
        /// 帧2 Skip（text_len=0）Sync → 断言 mesh 被重建（vertexCount=4）。
        /// 若 vertexCount=0 → 漏重建（旧 UV 永久残留）；若抛 OOM → ReadText 读 text_len=0 垃圾（旧 #2' 副作用）。
        [Test]
        public void FontDirty_SkipText_RebuildsFromCachedGlyphs()
        {
#if UNITY_EDITOR
            var font = UnityEditor.AssetDatabase.LoadAssetAtPath<Font>(DejaVuPath);
            Assume.That(font, Is.Not.Null, $"DejaVu 字体应在 {DejaVuPath}");

            var root = new GameObject("root");
            var shader = Shader.Find("LoomGUI/Unlit");
            var mm = new MaterialManager(shader);
            var pool = new MirrorPool();
            var fallback = Texture2D.whiteTexture;
            var fonts = new Dictionary<string, Font> { ["DejaVu"] = font };

            try
            {
                // 帧1：Full 'A' → 建 GO + ReadText 缓存进 ro + BuildMesh（4 verts）
                var blob1 = new FrameBlob(OneTextBlobV9(new[] { new GlyphData((uint)'A', 0f, 20f) }));
                pool.Sync(blob1, root.transform, mm, null, fallback, fonts, font, fontVersion: 0);
                Assume.That(root.transform.childCount, Is.EqualTo(1), "帧1 建 1 GO");
                var mf = root.transform.GetChild(0).GetComponent<MeshFilter>();
                Assume.That(mf.sharedMesh.vertexCount, Is.EqualTo(4), "帧1: 'A' 1 glyph = 4 verts");

                // 清空 mesh——若帧2 没真重建，vertexCount 会保持 0（区分「重建」与「保留旧 mesh」）
                mf.sharedMesh.Clear();

                // 模拟 atlas rebuild：fontVersion 从 0 → 1（生产代码由 LoomStage.OnFontRebuilt 自增）。
                // 帧2：同 'A' 但 Skip（changeLevel=0，text_len=0，blob 无 text 段）。
                // fontDirty=true → Skip 提升为 Full → text_len==0 走缓存 → BuildMesh 重建取新 UV。
                var blob2 = new FrameBlob(OneTextBlobV9(new[] { new GlyphData((uint)'A', 0f, 20f) }, changeLevel: 0));
                pool.Sync(blob2, root.transform, mm, null, fallback, fonts, font, fontVersion: 1);

                Assert.AreEqual(4, mf.sharedMesh.vertexCount,
                    "fontDirty + Skip text（text_len=0）→ 必须用缓存 glyphs 重建 mesh（vertexCount=4）；" +
                    "若=0 则漏重建（旧 bug：UV 永久残留→上下颠倒）；若崩则 OOM（旧 #2' 副作用：ReadText 读 text_len=0 垃圾）");
            }
            finally
            {
                pool.Clear();
                mm.Clear();
                Object.DestroyImmediate(root);
                // 隔离：重置 MirrorPool 的 _lastFontVersion（反射），避免污染其它测试。
                // 生产代码无静态状态可重置（FontVersion 已是 per-instance）。
                var f = typeof(MirrorPool).GetField("_lastFontVersion",
                    System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance);
                if (f != null) f.SetValue(pool, -1);
            }
#else
            Assert.Inconclusive("EditMode-only（AssetDatabase）");
#endif
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
            stage.Tick(0.016f, renderRoot: null);
            Assert.Pass("LoomStage 构造 + 空 tick 成功（pure class，无 MonoBehaviour 依赖）");
        }
    }
}
