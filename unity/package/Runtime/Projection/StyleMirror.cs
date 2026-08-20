// StyleMirror：NodeStyle 写入属性的稀疏镜像 + 帧末攒批 flush seam。
//
// 投影层契约（docs/design/projection-layer.md §2.3 + §3.2）：
//   - 只存 setter 写过的属性（稀疏 dict，CSS prop name → typed value）。
//   - getter 查 mirror：有 → 返 typed 值；无 → 返 Unset 哨兵（Length.Unset / LoomColor.Unset /
//     enum 的 Unset=0 变体；Thickness/float 无 Unset 概念，getter 走 default）。
//   - setter 写 Unset 哨兵 → 视为撤销该属性（移除 key + unset_inline_override FFI），不写 Set。
//
// 攒批 flush（Task 9，§3.2 升级版）：setter 只标脏（_dirty=true + 注册到 NodeRegistry dirty 集），
// 不立即调 set_inline_override；帧末（LoomHost.Step 的 flush seam，或 UIContext.FlushPendingWrites）
// 一次性遍历 dirty 集 调 FlushInline。core 侧 inline_override 是累加语义（set 只 OR bit /
// 覆盖值，不清其他），故帧末重建整个镜像拼 CSS 串送一次安全。
//
// Unset 路径：unset_inline_override 是清 bit 操作（与 set 的 OR 互补），仍立即调——
// 清 bit 必须及时否则下帧 rematch 仍命中旧 inline。同时标脏让帧末 FlushInline 重同步剩余 _set。
//
// **严禁 set_style**（写 base_style，污染设计期基线）。只走 set_inline_override / unset_inline_override
// （写便签层，下帧 rematch 应用，优先级 > 动态规则 > base_style）。

using System.Collections.Generic;
using System.Text;
using LoomGUI.Bindings;

namespace LoomGUI
{
    /// <summary>
    /// 投影层内部：NodeStyle 写入属性的稀疏镜像 + FFI flush seam。
    /// 每个 NodeStyle 持一个；不缓存计算值（计算值走 Geometry，C4）。
    /// </summary>
    internal sealed unsafe class StyleMirror
    {
        readonly Node _owner;
        // CSS prop name → typed value（Length/LoomColor/Thickness/float/enum）。未在 dict 的属性 = 未写过。
        readonly Dictionary<string, object> _set = new();

        // 攒批 dirty 标志：Set/Unset 置 true；FlushInline 末尾置 false。
        // 配合 NodeRegistry dirty 集（帧末集中 flush，避免每 setter 一次 FFI）。
        internal bool _dirty;

        internal StyleMirror(Node owner) => _owner = owner;

        /// <summary>是否有未 flush 的写入（测试可观察 + registry flush 判据）。</summary>
        internal bool IsDirty => _dirty;

        /// <summary>该 prop 是否被 setter 写过（无则 getter 返 Unset 哨兵）。</summary>
        internal bool IsSet(string prop) => _set.ContainsKey(prop);

        /// <summary>取 typed 值；未写过返 null（调用方据此回落 Unset）。</summary>
        internal T? Get<T>(string prop) where T : struct
            => _set.TryGetValue(prop, out var v) ? (T)v : (T?)null;

        /// <summary>
        /// 写 prop = value。value 是 Unset 哨兵时改走 <see cref="Unset"/>（撤销）。
        /// 攒批版：只写 mirror + 标脏，不立即 flush——帧末（UIContext.FlushPendingWrites）集中过桥。
        /// </summary>
        internal void Set(string prop, object value)
        {
            if (IsUnsetSentinel(value))
            {
                Unset(prop);
                return;
            }
            _set[prop] = value;
            MarkDirty();
        }

        /// <summary>
        /// 撤销 prop：移除镜像 key + 立即调 unset_inline_override 清 core 侧 bit（即使镜像无此 key，
        /// 也 flush——用户可能之前 flush 过，core 侧 bit 仍置）+ 标脏（帧末 FlushInline 重同步剩余 _set）。
        /// </summary>
        internal void Unset(string prop)
        {
            _set.Remove(prop);
            FlushUnset(prop);
            MarkDirty();
        }

        // 标脏 + 注册到 registry dirty 集（帧末集中 flush）。
        void MarkDirty()
        {
            _dirty = true;
            _owner._ctx._registry.MarkStyleDirty(_owner);
        }

        // ── Unset 哨兵检测 ──────────────────────────────────────────────
        // Length/LoomColor/enum 各自有 Unset 哨兵（值类型，无 null）。Thickness/float 无 Unset 概念
        // （ Thickness 是裸四值结构；Opacity 是裸 float）—— setter 写啥就存啥，不走撤销路径。
        // enum 的 Unsent 变体恒为 0（frozen enum 全部以 Unset=0 开头），Convert.ToInt32 兜底判 0。
        static bool IsUnsetSentinel(object v) => v switch
        {
            Length l  => l.Unit == LengthUnit.Unset,
            LoomColor c   => c.IsUnset,
            System.Enum e => System.Convert.ToInt32(e) == 0,
            _ => false,
        };

        // ── Flush seam ─────────────────────────────────────────────────
        // 攒批版：setter 只标脏不调本组方法；帧末（NodeRegistry.FlushDirtyStyles）集中调 FlushInline。
        // FlushUnset 仍由 Unset 立即调（清 bit 必须及时，否则下帧 rematch 命中旧 inline）。

        /// <summary>
        /// 把整个镜像拼成 CSS 申明串（prop:val;prop:val;）调 set_inline_override FFI。
        /// 累加语义：core 侧只 OR bit / 覆盖值，不清其他；重复 flush 同 dict 安全。
        /// 帧末由 NodeRegistry.FlushDirtyStyles 遍历 dirty 集调本方法。末尾清 _dirty。
        /// </summary>
        internal void FlushInline()
        {
            _dirty = false;
            if (_set.Count == 0) return;
            var sb = new StringBuilder();
            foreach (var kv in _set)
            {
                var css = CssValueConvert.ToCss(kv.Value);
                if (css == null) continue;   // Unset 哨兵（理论不会到这——Set 已拦截），防御性跳
                sb.Append(kv.Key).Append(':').Append(css).Append(';');
            }
            if (sb.Length == 0) return;
            CallSetInlineOverride(sb.ToString());
        }

        /// <summary>调 unset_inline_override FFI 清 core 侧该 prop 的 bit。</summary>
        internal void FlushUnset(string prop) => CallUnsetInlineOverride(prop);

        // ── FFI 转调（ptr+len，A6 编码）─────────────────────────────────
        // UTF-8 编码 + fixed 钉住 + ptr+len。失败静默（rc!=0 仅发生于 null stage / 节点不 live /
        // 非 UTF-8——前两者 ThrowIfDisposed 已拦截，UTF-8 编码不会非 UTF-8；防御性不抛，
        // 与同 assembly 其他 FFI 转调一致）。

        void CallSetInlineOverride(string css)
        {
            StageHandle* h = (StageHandle*)_owner._ctx._stage.ToPointer();
            byte[] bytes = Encoding.UTF8.GetBytes(css);
            fixed (byte* p = bytes)
                Native.loomgui_stage_set_inline_override(h, _owner._id, p, (nuint)bytes.Length);
        }

        void CallUnsetInlineOverride(string prop)
        {
            StageHandle* h = (StageHandle*)_owner._ctx._stage.ToPointer();
            byte[] bytes = Encoding.UTF8.GetBytes(prop);
            fixed (byte* p = bytes)
                Native.loomgui_stage_unset_inline_override(h, _owner._id, p, (nuint)bytes.Length);
        }
    }
}
