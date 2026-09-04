using System.Collections.Generic;
using UnityEngine;

namespace Ikat
{
    /// DrawState 缓存。
    /// key = (program, texture, mask_context, matrix)。同 key 复用 Material 实例。
    /// tint×alpha 走顶点色（不在 key 里）；clip 链 uniform 数组进 mask_context 专属
    /// Material（多 entry 交集语义，#52：rect/圆角/circle/polygon 按 entry kind 分派，
    /// 单 CLIPPED 变体——不再有独立圆角变体）。
    public sealed class MaterialManager
    {
        const int MaxEntries = 4;       // 与 core render::MAX_CLIP_CHAIN 同值（shader 数组定长）
        const int PolyVec4PerEntry = 8; // 每 entry polygon 点槽：16 点 × 2 float = 8 float4

        readonly Shader _shader;
        readonly Dictionary<Key, Material> _cache = new();
        // clip 链数组代数计数：每次 SetClipEntries 递增并写 _ClipGen（shader CBUFFER
        // 哑字段，逻辑不用）——值恒变，防 SRP batcher 对「数组同长重写」的材质数据
        // 缓存不失效（var 换形滞后取证的保险）。
        static int _clipGen;

        public int ClipGen => _clipGen;
        // per-ctx clip 链数组（新建 Material 时 Get 会带上；SetClipEntries 同步刷新已缓存实例）。
        readonly Dictionary<uint, Vector4[]> _clipFrame0ByCtx = new();
        readonly Dictionary<uint, Vector4[]> _clipFrame1ByCtx = new();
        readonly Dictionary<uint, Vector4[]> _clipRectByCtx = new();
        readonly Dictionary<uint, Vector4[]> _clipRadii0ByCtx = new();
        readonly Dictionary<uint, Vector4[]> _clipRadii1ByCtx = new();
        readonly Dictionary<uint, Vector4[]> _clipCircleByCtx = new();
        readonly Dictionary<uint, Vector4[]> _clipPolyByCtx = new();
        readonly Dictionary<uint, float> _clipCountByCtx = new();

        public MaterialManager(Shader shader) { _shader = shader; }

        public Material Get(int program, Texture texture, uint maskContext, bool matrixFlag)
        {
            var key = new Key(program, texture, maskContext, matrixFlag);
            if (!_cache.TryGetValue(key, out var mat))
            {
                mat = new Material(_shader);
                mat.mainTexture = texture;
                mat.SetFloat("_SrcFactor", 5f);   // SrcAlpha
                mat.SetFloat("_DstFactor", 10f);  // OneMinusSrcAlpha
                if (maskContext > 0u)
                {
                    // ctx>0 → CLIPPED 变体（多 entry 数组：rect/圆角 SDF/circle/polygon
                    // 按 entry kind 分派，见 Ikat-Unlit.shader clip 段）。
                    // mask_context 进 key，每 ctx 独立 Material 实例。
                    mat.EnableKeyword("CLIPPED");
                    // 首帧路径：MirrorPool 先 SetClipEntries 再 Get；新建 Material 时从
                    // dict 读数组。后续帧材质已缓存，SetClipEntries 直接刷新实例。
                    if (_clipFrame0ByCtx.TryGetValue(maskContext, out var f0))
                        ApplyClipArrays(mat, maskContext);
                }
                if (matrixFlag) mat.EnableKeyword("OBJECT_MATRIX");
                if (program == 1) mat.EnableKeyword("ALPHA_MASK");   // text: font atlas 是 alpha-mask（rgb 黑，glyph 在 alpha）
                if (program == 2) mat.EnableKeyword("BG_COMPOSITE"); // Container+bg-image: CSS 合成（图透明区显 bg-color）
                if (program == 3) mat.EnableKeyword("COLOR_FILTER"); // filter + tex*vcol base（Image+filter / Container+filter 无 bg-image）
                if (program == 4) { mat.EnableKeyword("COLOR_FILTER"); mat.EnableKeyword("BG_COMPOSITE"); } // filter + bg-image base（Container+bg-image+filter，双 keyword）
                if (program == 5) mat.EnableKeyword("SHADOW_BLUR"); // box-shadow blur：纹理无关圆角矩形 SDF（shader 自含，不采 _MainTex）
                if (program == 6) mat.EnableKeyword("GRADIENT"); // 背景渐变：per-fragment stops/radial（uv=box 局部坐标，不采 _MainTex）
                if (program == 7) { mat.EnableKeyword("GRADIENT"); mat.EnableKeyword("COLOR_FILTER"); } // 渐变 + filter（渐变基色再过色彩矩阵）
                _cache[key] = mat;
            }
            return mat;
        }

        /// 注册某 mask_context 的 clip 链（多 entry）。把 ClipEntryView 列表转 shader
        /// 数组布局（frame0 = (A, C, Tx, kind)，frame1 = (B, D, Ty, hasRect)，
        /// poly 两点一 float4）后写 dict + 刷新该 ctx 已缓存 Material 实例。
        /// 调用顺序对 MirrorPool 不构成约束（同旧 SetClipBox 双路覆盖语义）。
        public void SetClipEntries(uint maskContext, List<ClipEntryView> entries)
        {
            int n = Mathf.Min(entries.Count, MaxEntries);
            var f0 = new Vector4[MaxEntries];
            var f1 = new Vector4[MaxEntries];
            var rect = new Vector4[MaxEntries];
            var r0 = new Vector4[MaxEntries];
            var r1 = new Vector4[MaxEntries];
            var circ = new Vector4[MaxEntries];
            var poly = new Vector4[MaxEntries * PolyVec4PerEntry];
            for (int e = 0; e < n; e++)
            {
                var en = entries[e];
                // 双独立 kind（HasRect 与 HasShape 可同 entry 并存——同元素
                // overflow:hidden + clip-path 两条测试都过，web 交集原义）：
                // frame0.w = shapeKind（0 无 / 1 circle / 2 polygon），
                // frame1.w = rectKind（0 无 / 1 直角 / 2 圆角）。
                float shapeKind = !en.HasShape ? 0f : (en.ShapeKind == 1 ? 2f : 1f);
                float rectKind = !en.HasRect ? 0f : (en.HasRadii ? 2f : 1f);
                f0[e] = new Vector4(en.A, en.C, en.Tx, shapeKind);
                f1[e] = new Vector4(en.B, en.D, en.Ty, rectKind);
                rect[e] = new Vector4(en.W, en.H, en.Poly.Length, 0f);
                r0[e] = en.RadiiTlTr;
                r1[e] = en.RadiiBrBl;
                circ[e] = new Vector4(en.CircleCx, en.CircleCy, en.CircleR, 0f);
                // polygon 点：两点一 float4（x1,y1,x2,y2），entry 槽基址 = e × 8。
                int slot = e * PolyVec4PerEntry;
                for (int k = 0; k + 1 < en.Poly.Length && k < PolyVec4PerEntry * 2; k += 2)
                {
                    var p1 = en.Poly[k];
                    var p2 = en.Poly[k + 1];
                    poly[slot + k / 2] = new Vector4(p1.x, p1.y, p2.x, p2.y);
                }
                // 奇数点数：末点重复进 float4 尾槽（core 限 3..=16 点，crossing 判定
                // 用 poly_count 截断，重复点不参与）。
                if ((en.Poly.Length & 1) == 1 && en.Poly.Length <= PolyVec4PerEntry * 2)
                {
                    var last = en.Poly[en.Poly.Length - 1];
                    poly[slot + (en.Poly.Length - 1) / 2] = new Vector4(last.x, last.y, last.x, last.y);
                }
            }
            _clipFrame0ByCtx[maskContext] = f0;
            _clipFrame1ByCtx[maskContext] = f1;
            _clipRectByCtx[maskContext] = rect;
            _clipRadii0ByCtx[maskContext] = r0;
            _clipRadii1ByCtx[maskContext] = r1;
            _clipCircleByCtx[maskContext] = circ;
            _clipPolyByCtx[maskContext] = poly;
            _clipCountByCtx[maskContext] = n;
            _clipGen++;
            foreach (var kv in _cache)
                if (kv.Key.Ctx == maskContext) ApplyClipArrays(kv.Value, maskContext);
        }

        void ApplyClipArrays(Material mat, uint ctx)
        {
            if (!_clipFrame0ByCtx.TryGetValue(ctx, out var f0)) return;
            mat.SetVectorArray("_ClipFrame0", f0);
            mat.SetVectorArray("_ClipFrame1", _clipFrame1ByCtx[ctx]);
            mat.SetVectorArray("_ClipRect", _clipRectByCtx[ctx]);
            mat.SetVectorArray("_ClipRadii0", _clipRadii0ByCtx[ctx]);
            mat.SetVectorArray("_ClipRadii1", _clipRadii1ByCtx[ctx]);
            mat.SetVectorArray("_ClipCircle", _clipCircleByCtx[ctx]);
            mat.SetVectorArray("_ClipPoly", _clipPolyByCtx[ctx]);
            mat.SetFloat("_ClipCount", _clipCountByCtx[ctx]);
            mat.SetFloat("_ClipGen", _clipGen);
        }

        public void Clear()
        {
            foreach (var kv in _cache)
            {
                if (Application.isPlaying) Object.Destroy(kv.Value);
                else Object.DestroyImmediate(kv.Value);   // [ExecuteAlways] 编辑器预览走 Edit mode
            }
            _cache.Clear();
            _clipFrame0ByCtx.Clear();
            _clipFrame1ByCtx.Clear();
            _clipRectByCtx.Clear();
            _clipRadii0ByCtx.Clear();
            _clipRadii1ByCtx.Clear();
            _clipCircleByCtx.Clear();
            _clipPolyByCtx.Clear();
            _clipCountByCtx.Clear();
        }

        // key 持 Texture 引用（Unity 对象同一性），避开 Unity 6.5 废弃的 GetInstanceID/GetEntityId/EntityId。
        // 材质与纹理同生命周期，缓存随纹理存活正确。
        readonly struct Key
        {
            readonly int _program;
            readonly Texture _tex;
            readonly uint _ctx;
            readonly bool _matrix;
            public Key(int p, Texture t, uint c, bool m) { _program = p; _tex = t; _ctx = c; _matrix = m; }
            public uint Ctx => _ctx;   // SetClipEntries 按 ctx 反查已缓存 material。
            public override int GetHashCode() => System.HashCode.Combine(_program, _tex, (int)_ctx, _matrix);
            public override bool Equals(object o) => o is Key k
                && k._program == _program
                && k._tex == _tex
                && k._ctx == _ctx
                && k._matrix == _matrix;
        }
    }
}
