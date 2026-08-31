using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
using Ikat.Bindings;
using UnityEngine;

namespace Ikat
{
    /// <summary>
    /// 资源宿主（C# 面）：字体表 / 包池 / 图尺寸表 / glyph atlas 的跨 Stage 共享层。
    /// 多个 <see cref="IkatStageDriver"/> 通过 <c>_useSharedHost</c> 挂同一实例——字体字节
    /// 一份、glyph atlas 一份（native 侧单实例 + 本类单份页纹理），per-Stage 固定成本
    /// 从 N 份降回 1 份。
    ///
    /// atlas 拉取必须宿主级单点：脏页 clear 是全局的，若每 driver 各拉各清，后拉的
    /// driver 会错过先清掉的页（其纹理永远缺新字形）。<see cref="SyncAtlas"/> 由各
    /// driver 的 SyncFrame 调用：首个调用者做真实拉取 + clear，其余看到零脏页只做
    /// 幂等注册；后挂的 driver 首帧经全量注册拿到所有已上传页。
    ///
    /// 字体注册守卫：<see cref="FontsRegistered"/> 置位后 driver 跳过重复注册——同名
    /// 重注册会换 font_id（native 侧代数失效钩触发全文本重测）且 atlas 按新 GlyphKey
    /// 重新光栅整套字形（N driver × N 套字形副本）。
    /// </summary>
    public sealed unsafe class IkatResourceHost : IDisposable
    {
        IntPtr _handle;
        readonly Dictionary<string, Texture2D> _atlasPages = new();

        /// <summary>宿主已注册过字体（driver 侧跳重复注册的守卫；宿主级 RegisterFont 置位）。</summary>
        public bool FontsRegistered { get; set; }

        /// <summary>
        /// 进程级共享宿主：<c>_useSharedHost</c> Driver 的默认挂接点（首个开启者懒建）。
        /// domain reload 清理由 Driver.ResetStatics 一并做（与 ikat_shutdown 同钩）。
        /// </summary>
        public static IkatResourceHost Shared { get; set; }

        public bool IsDisposed => _handle == IntPtr.Zero;

        /// <summary>native 宿主句柄（ikat_host_new）。disposed 后为 Zero。</summary>
        public IntPtr Handle => _handle;

        /// <exception cref="InvalidOperationException">ikat_host_new 返 null（分配失败）。</exception>
        public IkatResourceHost()
        {
            HostHandle* h = Native.ikat_host_new();
            if (h == null)
                throw new InvalidOperationException("ikat_host_new returned null");
            _handle = (IntPtr)h;
        }

        /// <summary>宿主级字体注册（多 Stage 挂接前统一注好；挂接后 stage 级 RegisterFont 等价落同一宿主）。</summary>
        public void RegisterFont(string family, byte[] bytes, bool isDefault)
        {
            if (_handle == IntPtr.Zero) return;
            byte[] fb = Encoding.UTF8.GetBytes(family ?? "");
            fixed (byte* fp = fb, bp = bytes)
            {
                Native.ikat_host_register_font(
                    (HostHandle*)_handle, fp, (nuint)fb.Length, bp, (nuint)(bytes?.Length ?? 0),
                    isDefault ? (byte)1 : (byte)0);
            }
            FontsRegistered = true;
        }

        /// <summary>宿主级字体回退链（语义同 stage 级；family 名以 \n 分隔）。</summary>
        public void SetFallbackFamilies(IEnumerable<string> families)
        {
            if (_handle == IntPtr.Zero) return;
            string text = families == null ? "" : string.Join("\n", families);
            byte[] tb = Encoding.UTF8.GetBytes(text);
            fixed (byte* tp = tb)
            {
                Native.ikat_host_set_fallback_families((HostHandle*)_handle, tp, (nuint)tb.Length);
            }
        }

        /// <summary>
        /// 宿主 glyph atlas 同步：拉脏页 → 上传/更新 R8 纹理（本类持有）→ 全量幂等注册进
        /// 该 driver 的 resolver。页是 append-only（native 侧不重排，旧字形 UV 永不变），
        /// 脏页只可能是新页或原页扩容——后者复用同一 Texture2D 重上传，不断材质引用。
        /// </summary>
        public void SyncAtlas(SpriteResolver resolver)
        {
            if (_handle == IntPtr.Zero || resolver == null) return;
            HostHandle* h = (HostHandle*)_handle;

            const int MAX_DIRTY = 16;
            uint* dirtyPtr = stackalloc uint[MAX_DIRTY];
            int n = (int)Native.ikat_host_font_atlas_dirty_pages(h, dirtyPtr, (nuint)MAX_DIRTY);
            if (n > 0)
            {
                if (n > MAX_DIRTY)
                {
                    Debug.LogWarning($"[IkatResourceHost] font atlas dirty pages ({n}) exceed MAX_DIRTY ({MAX_DIRTY}); skipping extras");
                    n = MAX_DIRTY;
                }
                for (int i = 0; i < n; i++)
                {
                    uint page = dirtyPtr[i];
                    uint w = 0, ph = 0;
                    int needed = (int)Native.ikat_host_font_atlas_page(h, page, &w, &ph, null, (nuint)0);
                    if (needed <= 0) continue;

                    byte[] buf = System.Buffers.ArrayPool<byte>.Shared.Rent(needed);
                    try
                    {
                        fixed (byte* pBuf = buf)
                        {
                            int got = (int)Native.ikat_host_font_atlas_page(h, page, &w, &ph, pBuf, (nuint)needed);
                            if (got != needed) continue;
                        }
                        string path = FontAtlasPath.Format(page);
                        // R8 必须 linear=true：distance 存 .r，sRGB 采样会压低 face 让字消失。
                        if (_atlasPages.TryGetValue(path, out var tex) && tex != null
                            && tex.width == (int)w && tex.height == (int)ph)
                        {
                            fixed (byte* p = buf) { tex.LoadRawTextureData((IntPtr)p, needed); }
                            tex.Apply(false, true);
                        }
                        else
                        {
                            if (tex != null) DestroyTex(tex);
                            var created = new Texture2D((int)w, (int)ph, TextureFormat.R8, false, true);
                            fixed (byte* p = buf) { created.LoadRawTextureData((IntPtr)p, needed); }
                            created.Apply(false, true);
                            _atlasPages[path] = created;
                        }
                    }
                    finally { System.Buffers.ArrayPool<byte>.Shared.Return(buf); }
                }
                Native.ikat_host_font_atlas_clear_dirty(h);
            }

            // 全量幂等注册：后挂 driver 首帧拿全；常驻帧是页数级别的字典写。
            foreach (var kv in _atlasPages)
                resolver.RegisterFontAtlasPage(kv.Key, kv.Value);
        }

        static void DestroyTex(Texture2D tex)
        {
            if (Application.isPlaying) UnityEngine.Object.Destroy(tex);
            else UnityEngine.Object.DestroyImmediate(tex);
        }

        /// <summary>
        /// 释放宿主句柄 + 页纹理。须在所有挂接 Stage 释放之后（Rc 语义：越序不悬垂，
        /// 资源随最后一个 Stage 引用释放）。
        /// </summary>
        public void Dispose()
        {
            if (_handle != IntPtr.Zero)
            {
                Native.ikat_host_free((HostHandle*)_handle);
                _handle = IntPtr.Zero;
            }
            foreach (var tex in _atlasPages.Values)
                if (tex != null) DestroyTex(tex);
            _atlasPages.Clear();
        }
    }
}
