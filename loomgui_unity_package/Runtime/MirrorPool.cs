using System.Collections.Generic;
using UnityEngine;

namespace LoomGUI
{
    /// 渲染树 → GameObject 镜像 diff。每帧 O(n)：标 stale → 遍历命中清 stale/更新 → 余销毁。
    /// flatten：所有 GO 挂 root；纯平移节点 localPosition=(Mtx,Mty) 绝对 design；非纯平移节点
    /// GO transform=identity + _ObjectMatrix uniform；sortingOrder=sort_key。
    /// parent_id 仍在 blob 列但渲染不用（事件系统再用）。
    /// Mesh 顶点已由 Rust re-base 到节点本地空间，此处按 (x,y,0) 上传。
    /// change_level 三分支：0=SKIP(保留GO) 1=HEADER(只更header,不重建mesh) 2=FULL(重建mesh)。
    /// v10：所有渲染节点统一走 mesh 路径（text 核心自产 atlas 产 Mesh，kind 只有 1=Mesh）。
    sealed class RenderObj
    {
        public GameObject Go;
        public MeshFilter Mf;
        public MeshRenderer Mr;
        public Mesh Mesh;
        public bool Stale;
        public uint LastNodeId;       // 复用 GO 时校验

        // buffer 复用（500 节点静态压测 GC 缓解）：每 RenderObj 持可复用 List，
        // UploadMesh 每帧 Clear+fill 后用 Mesh.SetVertices(List) 等 overload 上传——
        // List<T>.Clear() 保留 Capacity，故 warm-up 后零 per-frame 数组 alloc。
        public readonly List<Vector3> VList = new();
        public readonly List<Vector2> UvList = new();
        public readonly List<Color> CList = new();
        public readonly List<int> IList = new();
        // cached MaterialPropertyBlock for per-renderer uniforms (_ObjM, _CF, _Alpha).
        // Lazy-init; consolidated into single SetPropertyBlock per frame.
        public MaterialPropertyBlock Mpb;
    }

    public sealed class MirrorPool
    {
        // 双 dict keying。reuse_key>0 的 slot 节点按 reuse_key 复用 GO
        // （slot 换绑 item 时 NodeId 变但 reuse_key 不变 → GO 不销毁重建）；
        // reuse_key=0 的普通节点按 node_id keying（v1 行为不变）。
        readonly Dictionary<uint, RenderObj> _poolByNodeId = new();
        readonly Dictionary<uint, RenderObj> _poolByReuse = new();
        // 每 ctx 每帧首次算一次 _ClipBox 并 SetClipBox。
        // Sync 开头清空；clip 表 entry 少（few ctx），每帧开销可忽略。
        readonly HashSet<uint> _clipsAppliedThisFrame = new();
        // 本帧启 CLIPPED_ROUNDED 的 ctx 集合（同 ctx 后续节点复用 rounded 标志保 material key 一致）。
        readonly HashSet<uint> _roundedCtxsThisFrame = new();

        /// 当前镜像中的 GO 数量（两 dict 之和）。测试/调试用。
        public int Count => _poolByNodeId.Count + _poolByReuse.Count;

        /// <summary>
        /// 镜像 diff：遍历 blob 节点 → 建/更新/复用 GO。
        /// v10：所有节点统一走 mesh 路径（核心自绘字体产 Mesh，text 不再单分一道）。
        /// </summary>
        public void Sync(FrameBlob blob, Transform root, MaterialManager mm,
                         SpriteResolver sprites, Texture fallback)
        {
            // 防御：陈旧/非当前 blob 直接早退（magic+version 校验）。不做清理——上一帧的 GO
            // 维持不动比误销毁更安全；调用方应自检 IsValid 再 Sync。
            if (!blob.IsValid) return;

            // ① 全标 stale（两个 dict）
            foreach (var kv in _poolByNodeId) kv.Value.Stale = true;
            foreach (var kv in _poolByReuse) kv.Value.Stale = true;
            // 本帧 clip 应用集清空（per-ctx-per-frame 一次性算 _ClipBox）。
            _clipsAppliedThisFrame.Clear();
            _roundedCtxsThisFrame.Clear();

            // ② 遍历节点：v8 三分支 SKIP / HEADER / FULL；v9 双 dict keying
            int n = blob.NodeCount;
            for (int i = 0; i < n; i++)
            {
                if (!blob.Visible(i)) continue;
                byte kind = blob.PayloadKind(i);
                byte level = blob.ChangeLevel(i);   // 0=Skip 1=Header 2=Full
                uint id = blob.NodeId(i);
                uint reuseKey = blob.ReuseKey(i);    // 虚拟列表
                uint poolKey = reuseKey != 0 ? reuseKey : id;
                Dictionary<uint, RenderObj> pool = reuseKey != 0 ? _poolByReuse : _poolByNodeId;

                // SKIP：本帧无变化，保留上帧 GO，清 stale。
                if (level == 0)
                {
                    if (pool.TryGetValue(poolKey, out var ro0)) ro0.Stale = false;
                    continue;
                }
                // v10：kind 只有 1=Mesh（文本核心自产 atlas 走同路径）。
                if (kind != 1) continue;

                // 解决图资源（path_idx → path → SpriteResolver.GetSprite）。
                // 文本节点的 font-atlas path（loomgui://font-atlas/...）由 RegisterFontAtlasPage 注册
                // 进 SpriteResolver，GetSprite 命中返 SpriteLookup——tex 回落 fallback whiteTexture，
                // 由 SyncFontAtlas 换贴真实 atlas。
                SpriteLookup look = default; Texture tex = fallback;
                uint pathIdx = blob.PathIdx(i);
                if (pathIdx != 0 && sprites != null)
                {
                    string path = blob.ReadPath(pathIdx);
                    if (!string.IsNullOrEmpty(path))
                    {
                        look = sprites.GetSprite(path);
                        if (look.found) tex = look.tex;
                    }
                }

                // 确保 RenderObj 存在；新建 GO 无 mesh → 强制 FULL（无视 blob 的 HEADER）
                if (!pool.TryGetValue(poolKey, out var ro))
                {
                    ro = NewRenderObj(root);
                    pool[poolKey] = ro;
                    level = 2; // 强制 FULL
                }
                ro.LastNodeId = id; // 新建 + 复用均更新（slot 换绑时 node_id 变）
                ro.Stale = false;

                UpdateHeader(ro, blob, i, root, mm, kind, look, tex);
                if (level == 2) UploadMeshOrText(ro, blob, i, look);
            }

            // ③ 余 stale 销毁（两个 dict）
            var dead1 = new List<uint>();
            foreach (var kv in _poolByNodeId) if (kv.Value.Stale) dead1.Add(kv.Key);
            foreach (var id in dead1) { TearDown(_poolByNodeId[id]); _poolByNodeId.Remove(id); }
            var dead2 = new List<uint>();
            foreach (var kv in _poolByReuse) if (kv.Value.Stale) dead2.Add(kv.Key);
            foreach (var id in dead2) { TearDown(_poolByReuse[id]); _poolByReuse.Remove(id); }
        }

        /// 更新 GO header（position/rotation/scale + sortingOrder + clip + material + per-renderer uniforms）。
        /// 无论 HEADER 还是 FULL 路径均调用；仅 SKIP 跳过。
        /// v10：不再有 kind==2 text 单独材质路径——所有节点统一走 program+tex 选材。
        void UpdateHeader(RenderObj ro, FrameBlob blob, int i, Transform root,
                          MaterialManager mm, byte kind, SpriteLookup look, Texture tex)
        {
            // flatten：所有节点挂 root。
            // pure 和非 pure 统一 GO localPosition=(Mtx,Mty)（world translate 进 GO transform）。
            // 非纯平移的 scale/rotate 进 _ObjectMatrix（无 translate）。这样 renderer.bounds = GO.worldTransform ×
            // Mesh.bounds 自动 world（culling 正确），不需 mutate Mesh.bounds 做 translate hack。
            ro.Go.transform.SetParent(root, false);
            bool pure = blob.IsPureTranslation(i);
            ro.Go.transform.localPosition = new Vector3(blob.Mtx(i), blob.Mty(i), 0f);
            ro.Go.transform.localRotation = Quaternion.identity;
            ro.Go.transform.localScale = Vector3.one;

            ro.Mr.sortingOrder = (int)blob.SortKey(i);

            uint maskCtx = blob.MaskContext(i);

            // mc>0 节点本帧首次见 → 读 clip 表 design rect + 圆角半径，转 world，算 _ClipBox，
            // SetClipBox + SetCornerRadius 到该 ctx 的 per-context Material。
            // 必须在 mm.Get 之前调，使新建 Material 时即带 box + radius；同 ctx 后续节点跳过（HashSet 去重）。
            bool rounded = false;
            if (maskCtx > 0u && _clipsAppliedThisFrame.Add(maskCtx))
            {
                if (blob.ClipRect(maskCtx, out float dx, out float dy, out float dw, out float dh,
                                  out float cornerRadius))
                {
                    Vector4 clipBox = ClipMath.ComputeClipBox(root, dx, dy, dw, dh);
                    mm.SetClipBox(maskCtx, clipBox);
                    if (cornerRadius > 0f)
                    {
                        // 归一化半径（design px → shader clipPos 空间）+ 启 CLIPPED_ROUNDED 变体。
                        float normR = ClipMath.NormalizeCornerRadius(dw, dh, cornerRadius);
                        mm.SetCornerRadius(maskCtx, normR);
                        rounded = true;
                        _roundedCtxsThisFrame.Add(maskCtx);
                    }
                }
                // ClipRect miss（表里无该 ctx）→ 不 SetClipBox；material 仍按 mc 建（CLIPPED variant
                // + 默认 _ClipBox=0,0,1,1 → 全保留，clip 无效但不崩；正常 flow 表必含所有 mc>0 ctx）。
            }
            else if (maskCtx > 0u)
            {
                // 同 ctx 已在本帧应用过——从 dict 读回 rounded 标志，保证 Get 的 material key 一致。
                rounded = _roundedCtxsThisFrame.Contains(maskCtx);
            }

            // 材质：按 program+texture 选（包括 text 节点，program=1，tex 由 SyncFontAtlas 注入 font atlas）。
            Material mat = mm.Get((int)blob.Program(i), tex, maskCtx, !pure, rounded);
            if (mat != null) ro.Mr.sharedMaterial = mat;

            // 合并 per-renderer uniform（MPB 一次 SetPropertyBlock，避免 _ObjM/_CF/_Alpha 互相覆盖）。
            // _ObjM：非纯平移时传 scale/rotate 矩阵（纯平移 = shader 默认 identity，不设）。
            // _CF：ColorFilter（program 3/4）传 5 Vector；其他不设。
            // _Alpha：每帧无条件设（alpha 剥离顶点色）。
            float alpha = blob.Alpha(i);
            bool hasFilter = blob.Program(i) == 3 || blob.Program(i) == 4;

            ro.Mpb ??= new MaterialPropertyBlock();
            ro.Mr.GetPropertyBlock(ro.Mpb);
            if (!pure)
            {
                // _ObjectMatrix 只 scale/rotate（translate 进 GO localPosition，renderer.bounds 自动 world）。
                var objM = Matrix4x4.identity;
                objM[0, 0] = blob.Ma(i); objM[0, 1] = blob.Mc(i);
                objM[1, 0] = blob.Mb(i); objM[1, 1] = blob.Md(i);
                ro.Mpb.SetVector("_ObjM0", objM.GetRow(0));
                ro.Mpb.SetVector("_ObjM1", objM.GetRow(1));
                ro.Mpb.SetVector("_ObjM2", objM.GetRow(2));
                ro.Mpb.SetVector("_ObjM3", objM.GetRow(3));
            }
            // ColorFilter（program=3=filter 无图 / 4=filter+bg-image 双 keyword）：
            // 矩阵 20 float 拆 5 Vector MPB SetVector。
            if (hasFilter)
            {
                float[] cf = blob.ColorMatrix(i);
                ro.Mpb.SetVector("_CF0", new Vector4(cf[0],  cf[1],  cf[2],  cf[3]));
                ro.Mpb.SetVector("_CF1", new Vector4(cf[5],  cf[6],  cf[7],  cf[8]));
                ro.Mpb.SetVector("_CF2", new Vector4(cf[10], cf[11], cf[12], cf[13]));
                ro.Mpb.SetVector("_CF3", new Vector4(cf[15], cf[16], cf[17], cf[18]));
                ro.Mpb.SetVector("_CFOff", new Vector4(cf[4], cf[9], cf[14], cf[19]));
            }
            ro.Mpb.SetFloat("_Alpha", alpha);
            ro.Mr.SetPropertyBlock(ro.Mpb);
        }

        /// 上传 mesh / 重建 mesh（仅 FULL 路径调用）。
        /// v10：不再有 kind==2 text 分支——所有节点统一走 mesh 上传（核心自产 atlas，word mesh 已含字形 UV）。
        static void UploadMeshOrText(RenderObj ro, FrameBlob blob, int i,
                                     SpriteLookup look)
        {
            // mesh 上传（顶点已 re-base 到本地）。
            var seg = blob.ReadMesh(i);
            UploadMesh(ro, seg);
            ro.Mesh.RecalculateBounds();
            // path → SpriteResolver.GetSprite → look。look.found=false（path_idx=0 纯色 / 查不到）则跳过重映射，
            // mesh 沿用 blob 全图 UV [0,1] + fallback whiteTexture。
            // look.found → RemapMeshUvToSprite 把全图 UV 重映射到 sprite 在 atlas 的子区（用 look.uvRect）。
            if (look.found)
                RemapMeshUvToSprite(ro, look.uvRect);
        }

        static RenderObj NewRenderObj(Transform root)
        {
            var go = new GameObject("loom_node");
            // ExecuteAlways 下镜像 GO 是运行时派生产物，标 DontSaveInEditor 防被存进场景
            // （否则 EditMode Sync 产出的 GO 会 dirty 场景、Play/Stop 与 domain reload 累积残留）。
            go.hideFlags = HideFlags.DontSaveInEditor;
            go.transform.SetParent(root, false);
            go.layer = root.gameObject.layer;  // LoomUI
            var mf = go.AddComponent<MeshFilter>();
            var mr = go.AddComponent<MeshRenderer>();
            var mesh = new Mesh { indexFormat = UnityEngine.Rendering.IndexFormat.UInt32 };
            mesh.hideFlags = HideFlags.DontSaveInEditor;  // Mesh 是独立 Object，也别存盘
            mesh.MarkDynamic();
            mf.sharedMesh = mesh;
            return new RenderObj { Go = go, Mf = mf, Mr = mr, Mesh = mesh };
        }

        /// buffer 复用：从 MeshSegment 填 ro 持有的可复用 List，再走 SetVertices(List) 等 overload。
        /// List<T>.Clear() 保留 Capacity → warm-up 后每帧零数组 alloc。
        /// 注意：SetVertices(List) 要求 list 长度 == 顶点数；Clear()+Add 精确填到 Verts.Length 即满足。
        static void UploadMesh(RenderObj ro, MeshSegment seg)
        {
            int vc = seg.Verts.Length;
            // Clear 保留 capacity，再填（避免每帧 new List / new 数组）。
            var v = ro.VList; v.Clear();
            var uv = ro.UvList; uv.Clear();
            var c = ro.CList; c.Clear();
            var idx = ro.IList; idx.Clear();
            // 预扩一次（首次或更大 mesh 时）；后续 Clear 不收缩，零 alloc。
            if (v.Capacity < vc) { v.Capacity = vc; uv.Capacity = vc; c.Capacity = vc; }
            int ic = seg.Idx.Length;
            if (idx.Capacity < ic) idx.Capacity = ic;

            for (int i = 0; i < vc; i++)
            {
                v.Add(new Vector3(seg.Verts[i].x, seg.Verts[i].y, 0f));
                uv.Add(seg.Uvs[i]);
                c.Add(seg.Colors[i]);
            }
            for (int i = 0; i < ic; i++) idx.Add((int)seg.Idx[i]);

            ro.Mesh.Clear();                 // Unity 要求 SetVertices 前清空，否则顶点数变更报错
            ro.Mesh.SetVertices(v);
            ro.Mesh.SetUVs(0, uv);
            ro.Mesh.SetColors(c);
            ro.Mesh.SetTriangles(idx, 0);
        }

        /// 把 mesh UV（core 产 [0,1] 全图 UV）线性映射到 sprite 在 atlas 页内的子区。
        /// uvRect 是打包器算好的 atlas 子区——Unity Rect (x=u0, y=v0, w=u1-u0, h=v1-v0)。
        /// 不内缩半纹素：padding 下 bilinear 边缘 fringe 几乎不可见；开 atlas enableAlphaDilation（边缘像素复制进 padding）防 bleed。
        static void RemapMeshUvToSprite(RenderObj ro, Rect uvRect)
        {
            // blob UV v 已翻转（TL.v=1），线性映射进子区保持翻转。
            // 直接改 ro.UvList（UploadMesh 已填），不复 alloc 新 List。
            var uvs = ro.UvList;
            for (int i = 0; i < uvs.Count; i++)
                uvs[i] = new Vector2(
                    uvRect.x + uvs[i].x * uvRect.width,
                    uvRect.y + uvs[i].y * uvRect.height);
            ro.Mesh.SetUVs(0, uvs);
        }

        public void Clear()
        {
            foreach (var kv in _poolByNodeId) TearDown(kv.Value);
            _poolByNodeId.Clear();
            foreach (var kv in _poolByReuse) TearDown(kv.Value);
            _poolByReuse.Clear();
        }

        // Edit-mode-safe 销毁：Driver 可能挂 [ExecuteAlways]，Sync/Clear 会在 Edit mode 跑；
        // Object.Destroy 在 Edit mode 非法（须 DestroyImmediate）。
        static void TearDown(RenderObj ro)
        {
            DestroyObj(ro.Mesh);   // new Mesh() 是独立 UnityEngine.Object，须显式销毁，否则泄漏
            DestroyObj(ro.Go);
        }

        static void DestroyObj(Object o)
        {
            if (o == null) return;
            if (Application.isPlaying) Object.Destroy(o);
            else Object.DestroyImmediate(o);
        }
    }
}
