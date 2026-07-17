// StyleMirror：NodeStyle 写入属性的稀疏镜像 + FlushInline seam。
//
// 投影层契约（docs/design/projection-layer.md §2.3 + §3.2）：
//   - 只存 setter 写过的属性（稀疏 dict，CSS prop name → typed value）。
//   - getter 查 mirror：有 → 返 typed 值；无 → 返 Unset 哨兵（Length.Unset / Color.Unset /
//     enum 的 Unset=0 变体；Thickness/float 无 Unset 概念，getter 走 default）。
//   - setter 写 Unset 哨兵 → 视为撤销该属性（移除 key + unset_inline_override FFI），不写 Set。
//
// FlushInline seam（§3.2 即时过桥版）：每次 setter 立即把整个镜像 flush 成 CSS 串调
// set_inline_override FFI（A6 ptr+len）。core 侧 inline_override 是累加语义（set 只 OR bit /
// 覆盖值，不清其他），故每次 flush 整个镜像安全。
//
// ponytail: 即时过桥够用（NodeStyle 写入是低频 UI 事件路径，非每帧热路径）。升级到攒批版只改
// 本文件：setter 标脏不立即 flush，帧末（UIContext.EndFrame 或显式 Flush）一次性 flush。
// 公共签名零改动——setter 还是写 mirror，只是 FlushInline 调用时机从立即改帧末。seam 两边共用。
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
        // CSS prop name → typed value（Length/Color/Thickness/float/enum）。未在 dict 的属性 = 未写过。
        readonly Dictionary<string, object> _set = new();

        internal StyleMirror(Node owner) => _owner = owner;

        /// <summary>该 prop 是否被 setter 写过（无则 getter 返 Unset 哨兵）。</summary>
        internal bool IsSet(string prop) => _set.ContainsKey(prop);

        /// <summary>取 typed 值；未写过返 null（调用方据此回落 Unset）。</summary>
        internal T? Get<T>(string prop) where T : struct
            => _set.TryGetValue(prop, out var v) ? (T)v : (T?)null;

        /// <summary>
        /// 写 prop = value。value 是 Unset 哨兵时改走 <see cref="Unset"/>（撤销）。
        /// 写后立即 flush 整个镜像到 core（ponytail: 攒批版改延迟 flush）。
        /// </summary>
        internal void Set(string prop, object value)
        {
            if (IsUnsetSentinel(value))
            {
                Unset(prop);
                return;
            }
            _set[prop] = value;
            FlushInline();
        }

        /// <summary>
        /// 撤销 prop：移除镜像 key + 调 unset_inline_override 清 core 侧 bit（即使镜像无此 key，
        /// 也 flush——用户可能之前 flush 过，core 侧 bit 仍置）。
        /// </summary>
        internal void Unset(string prop)
        {
            _set.Remove(prop);
            FlushUnset(prop);
        }

        // ── Unset 哨兵检测 ──────────────────────────────────────────────
        // Length/Color/enum 各自有 Unset 哨兵（值类型，无 null）。Thickness/float 无 Unset 概念
        // （ Thickness 是裸四值结构；Opacity 是裸 float）—— setter 写啥就存啥，不走撤销路径。
        // enum 的 Unsent 变体恒为 0（frozen enum 全部以 Unset=0 开头），Convert.ToInt32 兜底判 0。
        static bool IsUnsetSentinel(object v) => v switch
        {
            Length l  => l.Unit == LengthUnit.Unset,
            Color c   => c.IsUnset,
            System.Enum e => System.Convert.ToInt32(e) == 0,
            _ => false,
        };

        // ── Flush seam ─────────────────────────────────────────────────
        // ponytail: 即时过桥——每次 setter 触发一次 FFI。攒批版改本组两方法为标脏 + 帧末批量 flush。
        // 公共签名不变（NodeStyle setter 还是同步返），只延迟过桥时机。

        /// <summary>
        /// 把整个镜像拼成 CSS 申明串（prop:val;prop:val;）调 set_inline_override FFI。
        /// 累加语义：core 侧只 OR bit / 覆盖值，不清其他；重复 flush 同 dict 安全。
        /// </summary>
        internal void FlushInline()
        {
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
