using System.Collections.Generic;
using UnityEngine;

namespace Ikat
{
    /// DrawState 缓存。
    /// key = (program, texture, mask_context, rounded)。同 key 复用 Material 实例。
    /// tint×alpha 走顶点色（不在 key 里）；clip_box + corner_radius 进 mask_context 专属 Material 的 uniform。
    /// 圆角 clip（cornerRadius>0）与直角 clip（cornerRadius==0）是互斥变体——CLIPPED_ROUNDED
    /// 走 SDF，CLIPPED 走 AABB step，两者都是 clip 实现不叠加。rounded 进 key 让两者各持独立 Material。
    public sealed class MaterialManager
    {
        readonly Shader _shader;
        readonly Dictionary<Key, Material> _cache = new();
        readonly Dictionary<uint, Vector4> _clipBoxByCtx = new();
        // per-ctx 归一化圆角半径（shader _CornerRadius.x）。0=直角（CLIPPED），>0=圆角（CLIPPED_ROUNDED）。
        readonly Dictionary<uint, float> _cornerRadiusByCtx = new();

        public MaterialManager(Shader shader) { _shader = shader; }

        public Material Get(int program, Texture texture, uint maskContext, bool matrixFlag, bool rounded)
        {
            var key = new Key(program, texture, maskContext, matrixFlag, rounded);
            if (!_cache.TryGetValue(key, out var mat))
            {
                mat = new Material(_shader);
                mat.mainTexture = texture;
                mat.SetFloat("_SrcFactor", 5f);   // SrcAlpha
                mat.SetFloat("_DstFactor", 10f);  // OneMinusSrcAlpha
                if (maskContext > 0u)
                {
                    // ctx>0 → clip 变体。rounded=true 启 CLIPPED_ROUNDED（SDF），否则 CLIPPED（AABB step）。
                    // mask_context 进 key，每 ctx 独立 Material 实例，keyword 设该实例。
                    if (rounded) mat.EnableKeyword("CLIPPED_ROUNDED");
                    else mat.EnableKeyword("CLIPPED");
                    // 首帧路径：MirrorPool 先 SetClipBox/SetCornerRadius 再 Get；
                    // 新建 Material 时从此 dict 读。后续帧材质已缓存，Set 的 SetVector 分支刷新。
                    if (_clipBoxByCtx.TryGetValue(maskContext, out var cb))
                        mat.SetVector("_ClipBox", cb);
                    if (rounded && _cornerRadiusByCtx.TryGetValue(maskContext, out var cr))
                        mat.SetFloat("_CornerRadius", cr);
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

        /// 注册某 mask_context 的 _ClipBox。先写 _clipBoxByCtx（新建 Material 时 Get 会带上），
        /// 再把已缓存 Material 实例的 _ClipBox 同步刷新（每 ctx 一实例）。
        /// 两路都覆盖：SetClipBox 既可在 Get 前（首帧：box 进 dict，Get 建材质时读取）也可在 Get 后
        /// （后续帧：材质已存，直接 SetVector 刷新）。故调用顺序对 MirrorPool 不构成约束。
        public void SetClipBox(uint maskContext, Vector4 clipBox)
        {
            _clipBoxByCtx[maskContext] = clipBox;
            foreach (var kv in _cache)
                if (kv.Key.Ctx == maskContext) kv.Value.SetVector("_ClipBox", clipBox);
        }

        /// 注册某 mask_context 的归一化圆角半径（shader _CornerRadius）。
        /// radius>0 → CLIPPED_ROUNDED 变体（SDF）；radius==0 → CLIPPED 变体（AABB）。
        /// 调用方（MirrorPool）按 ClipRect 读出的 cornerRadius 是否 >0 决定调不调此方法；
        /// 不调 = 保持 0 = 直角。同 SetClipBox 双路覆盖（dict + 已缓存实例）。
        public void SetCornerRadius(uint maskContext, float normalizedRadius)
        {
            _cornerRadiusByCtx[maskContext] = normalizedRadius;
            foreach (var kv in _cache)
                if (kv.Key.Ctx == maskContext && kv.Key.Rounded)
                    kv.Value.SetFloat("_CornerRadius", normalizedRadius);
        }

        public void Clear()
        {
            foreach (var kv in _cache)
            {
                if (Application.isPlaying) Object.Destroy(kv.Value);
                else Object.DestroyImmediate(kv.Value);   // [ExecuteAlways] 编辑器预览走 Edit mode
            }
            _cache.Clear();
            _clipBoxByCtx.Clear();
            _cornerRadiusByCtx.Clear();
        }

        // key 持 Texture 引用（Unity 对象同一性），避开 Unity 6.5 废弃的 GetInstanceID/GetEntityId/EntityId。
        // 材质与纹理同生命周期，缓存随纹理存活正确。
        readonly struct Key
        {
            readonly int _program;
            readonly Texture _tex;
            readonly uint _ctx;
            readonly bool _matrix;
            readonly bool _rounded;   // CLIPPED_ROUNDED vs CLIPPED（圆角 clip 与直角 clip 互斥变体）
            public Key(int p, Texture t, uint c, bool m, bool r) { _program = p; _tex = t; _ctx = c; _matrix = m; _rounded = r; }
            public uint Ctx => _ctx;   // SetClipBox/SetCornerRadius 按 ctx 反查已缓存 material。
            public bool Rounded => _rounded;
            public override int GetHashCode() => System.HashCode.Combine(_program, _tex, (int)_ctx, _matrix, _rounded);
            public override bool Equals(object o) => o is Key k
                && k._program == _program
                && k._tex == _tex
                && k._ctx == _ctx
                && k._matrix == _matrix
                && k._rounded == _rounded;
        }
    }
}
