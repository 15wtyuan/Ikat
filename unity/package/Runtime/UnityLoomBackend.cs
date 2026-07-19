using System;
using System.Buffers;                // ArrayPool<byte> for _frameBuf（搬自 LoomStage.Tick）
using System.Collections.Generic;    // List<AtlasManifest> for InitSprites
using System.Runtime.InteropServices;
using LoomGUI.Bindings;
using UnityEngine;

namespace LoomGUI
{
    /// <summary>
    /// Unity 引擎后端实现：持 MirrorPool / MaterialManager / NativeHostManager / SpriteResolver /
    /// LoomInputCollector（零改复用，引用从 LoomStage 搬来——LoomStage 在 P2.6 退役前与之共存）。
    /// <see cref="LoomHost"/> 通过 <see cref="LoomBackend"/> 契约驱动：每帧先 <see cref="CollectInput"/>
    /// 再 <see cref="SyncFrame"/>（borrow_frame 已由 LoomHost 完成，ptr+len 传入避免二次 borrow）。
    ///
    /// NativeHost（GameObject 绑定 3D 模型，<see cref="NativeHostManager"/>）是 Unity 专属，
    /// 不进 <see cref="LoomBackend"/> 通用契约，作额外属性 <see cref="NativeHost"/> 暴露给 LoomHost。
    /// </summary>
    public sealed unsafe class UnityLoomBackend : LoomBackend
    {
        readonly MirrorPool _pool = new();
        MaterialManager _mm;                 // ctgfx 程序化 material 缓存（Shader Find 后注入）
        readonly NativeHostManager _nhm = new();
        internal SpriteResolver _sprites = new();  // LoomHost InitSprites/SetImageSizes 资源注册用
        LoomInputCollector _inputCollector;  // Driver Awake 注入（指针/键盘/滚轮采集）
        Transform _renderRoot;               // MirrorPool 镜像 GO + NativeHost container 挂此 root
        byte[] _frameBuf;                    // ArrayPool 租用（搬自 LoomStage.Tick 的复用语义）

        /// <param name="mm">由 Driver 构造并注入（Shader.Find("LoomGUI/Unlit") 后建）。</param>
        public UnityLoomBackend(MaterialManager mm) { _mm = mm; }

        /// <summary>
        /// Driver Awake 注入：渲染根（MirrorPool / NativeHost 镜像 GO 挂此 root）+ 输入采集器。
        /// 必须在第一次 <see cref="SyncFrame"/> 前调——SyncFrame 读 _renderRoot，null 时跳过镜像。
        /// NativeHostManager.Init 也在此调用方（Driver）建 container；本 backend 不重复建。
        /// </summary>
        public void SetRuntimeRoot(Transform root, LoomInputCollector input)
        {
            _renderRoot = root;
            _inputCollector = input;
        }

        /// <summary>
        /// NativeHost 绑定点（Unity 专属，不进 LoomBackend 通用契约）。
        /// internal——<see cref="NativeHostManager"/> 自身是 internal sealed，LoomHost 同程序集可见。
        /// </summary>
        internal NativeHostManager NativeHost => _nhm;

        /// <summary>
        /// SpriteResolver 初始化：传入所有 atlas manifest + 页纹理懒加载委托。
        /// Driver.Awake 后调：ParseAtlas 解析每个 atlas.json → <see cref="AtlasManifest"/>，传入此方法。
        /// loadPage(pageFileName) 按需加载页 PNG（Driver 决定走 Resources/AB/Addressables）。
        /// loadPage=null 则 GetSprite 全 miss（调用方 fallback）。
        ///
        /// Unity 特定资源 IO（Texture2D）——不进 <see cref="LoomHost"/> 引擎无关层。
        /// 搬自 LoomStage.cs:131-134（_sprites.Init 转调）。
        /// </summary>
        public void InitSprites(List<AtlasManifest> atlases, Func<string, Texture2D> loadPage)
        {
            _sprites?.Init(atlases, loadPage);
        }

        // ── LoomBackend 契约 ──

        /// <summary>
        /// 采集 Unity 输入（指针/键盘/滚轮）→ set_input 系 FFI（引擎中立，由 LoomInputCollector 内部调）。
        /// DesignSize / UseSafeArea 由 LoomInputCollector 实例携带（Driver Awake 注入）。
        /// </summary>
        public override void CollectInput(IntPtr stage)
        {
            if (stage == IntPtr.Zero || _inputCollector == null) return;
            _inputCollector.Collect(stage, _inputCollector.DesignSize, _inputCollector.UseSafeArea);
            _inputCollector.CollectKeys(stage);
            LoomInputCollector.CollectWheel(stage, _inputCollector);
        }

        /// <summary>
        /// 消费 borrow_frame blob → ArrayPool 复制 → <see cref="SyncFontAtlas"/>（脏页上传）+
        /// <see cref="MirrorPool.Sync"/>（RenderNode 镜像）+ <see cref="NativeHostManager.Sync"/>（3D 模型绑定）。
        /// 与 LoomStage.Tick 的 borrow→Sync 段对齐，逻辑零改（_stage 换 h 参数）。
        /// </summary>
        public override void SyncFrame(IntPtr stage, IntPtr framePtr, int frameLen)
        {
            if (framePtr == IntPtr.Zero || frameLen <= 0 || _renderRoot == null) return;
            StageHandle* h = (StageHandle*)stage.ToPointer();

            // frame buffer（ArrayPool 复用——搬自 LoomStage.Tick:198-203）。Rent 返 ≥len，只 copy/解析 len 字节。
            if (_frameBuf == null || _frameBuf.Length < frameLen)
            {
                if (_frameBuf != null) ArrayPool<byte>.Shared.Return(_frameBuf);
                _frameBuf = ArrayPool<byte>.Shared.Rent(frameLen);
            }
            Marshal.Copy(framePtr, _frameBuf, 0, frameLen);
            var blob = new FrameBlob(_frameBuf);

            SyncFontAtlas(h);
            // v10：不再传字体表（核心自产 atlas，后端不再光栅化文本）。
            _pool.Sync(blob, _renderRoot, _mm, _sprites, Texture2D.whiteTexture);
            _nhm.Sync(h);
        }

        // ── SyncFontAtlas（搬自 LoomStage.SyncFontAtlas，零改；_stage → h 参数）──

        /// <summary>
        /// 拉取核心字体 atlas 脏页 → 上传 R8 Texture2D → Sprite 包装 → 注册进 SpriteResolver。
        /// SyncFrame 内 <see cref="MirrorPool.Sync"/> 前调——本帧渲染节点包含 text Mesh image_path，
        /// 先注册 atlas Sprite 使 text 节点的 image_path 命中 GetSprite 缓存。
        ///
        /// 双调法取页数据：先探 buf_len=0 返所需字节数 → 分配 buf → 再调填 w/h/bytes。
        /// Atlas 页面通常是 512×512=256KB，每页用独立 ArrayPool 缓冲区（不挤 _frameBuf）。
        /// v1.6 单字体路径固定 f0（默认字体 font_id=0）；多字体 T8 再扩 key。
        /// </summary>
        unsafe void SyncFontAtlas(StageHandle* h)
        {
            // 探脏页（通常 ≤8 页；v1.6 单字体极少超 16）。
            const int MAX_DIRTY = 16;
            uint* dirtyPtr = stackalloc uint[MAX_DIRTY];
            int n = (int)Native.loomgui_stage_font_atlas_dirty_pages(h, dirtyPtr, (nuint)MAX_DIRTY);
            if (n <= 0) return;
            if (n > MAX_DIRTY)
            {
                Debug.LogWarning($"[UnityLoomBackend] font atlas dirty pages ({n}) exceed MAX_DIRTY ({MAX_DIRTY}); skipping extras");
                n = MAX_DIRTY;
            }

            for (int i = 0; i < n; i++)
            {
                uint page = dirtyPtr[i];
                // 原 LoomStage 代码用 w/h 局部；此处外层参数已名 h（StageHandle*），重名局 page height 为 ph 避冲突。
                uint w = 0, ph = 0;
                // 探所需字节数（buf_len=0, out_buf=null → 返 needed 不写 w/h/pixels）。
                int needed = (int)Native.loomgui_stage_font_atlas_page(h, page, &w, &ph, null, (nuint)0);
                if (needed <= 0) continue;

                byte[] buf = ArrayPool<byte>.Shared.Rent(needed);
                try
                {
                    fixed (byte* pBuf = buf)
                    {
                        int got = (int)Native.loomgui_stage_font_atlas_page(h, page, &w, &ph, pBuf, (nuint)needed);
                        if (got != needed) continue;
                    }
                    // R8 必须用 linear=true：distance 存在 .r，默认 sRGB 采样会被硬件 sRGB→Linear 解码
                    // 把 d 压低（inside 0.59→0.30）→ faceAlpha 算成 0 → 字消失。linear=true 直读 raw byte。
                    var tex = new Texture2D((int)w, (int)ph, TextureFormat.R8, false, true);
                    fixed (byte* p = buf) { tex.LoadRawTextureData((IntPtr)p, needed); }
                    tex.Apply(false, true);
                    // atlas 是 Stage 级单一共享实例（所有字体字形混在同一 page），路径只以 page 为键——
                    // 不含 font_id（font_id 只作 GlyphKey 区分字形槽位，不进 path）。与 render 侧
                    // build_text_mesh 合成的 loomgui://font-atlas/p{n} 对齐。
                    string path = FontAtlasPath.Format(page);
                    _sprites.RegisterFontAtlasPage(path, tex);
                }
                finally { ArrayPool<byte>.Shared.Return(buf); }
            }
            Native.loomgui_stage_font_atlas_clear_dirty(h);
        }
    }
}
