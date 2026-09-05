using System;
using System.Text;
using Yio.Bindings;
using UnityEngine;

namespace Yio
{
    /// <summary>
    /// Pick 命中链调试探针——「点击没反应 / 悬浮失效」的第一现场工具。
    /// 静态 dump（F8 类）拿不到悬浮瞬间的命中状态；本探针实时 Pick 指针位置，打印
    /// 命中节点 → 根 的祖先链，每层带 HTML id / class / C# 类型 / opacity / touchable /
    /// world rect。「播完即隐形」的演出层偷命中时链顶即凶手：opacity=0 但 touchable=True
    /// （opacity:0 参与命中是浏览器标准语义，runtime 对齐是对的——该修的是演出层常开）。
    /// </summary>
    public static unsafe class YioDebugProbe
    {
        /// <summary>
        /// 返回 (x, y)（design 坐标，左上原点）处的 Pick 命中链描述文本。未命中返回 miss 行。
        /// Pick 用上帧 world_transforms（结构变更帧的新节点 1 帧延迟命中）。逐帧调用会刷屏——
        /// 调用方按顶层命中变化去重后再打日志（<see cref="YioStageDriver"/> 的 F9 探针已去重）。
        /// </summary>
        public static string DescribePickChain(UIContext ctx, float x, float y)
        {
            Node hit = ctx.Pick(new YioVector2(x, y));
            if (hit == null)
                return $"[Yio pick probe] miss at ({x:F0},{y:F0}) — no touchable node under pointer";

            var sb = new StringBuilder(256);
            sb.AppendLine($"[Yio pick probe] hit at ({x:F0},{y:F0})");
            int level = 0;
            for (Node n = hit; n != null; n = n.Parent, level++)
            {
                YioRect wr = n.Geometry.WorldRect;
                sb.AppendLine(
                    $"  L{level} {n.GetType().Name,-12} id=\"{n.Id}\" class=\"{ClassesOf(n)}\" " +
                    $"opacity={OpacityOf(n):0.###} touchable={n.Touchable} " +
                    $"rect=({wr.X:F0},{wr.Y:F0} {wr.Width:F0}x{wr.Height:F0})");
                if (level >= 32)
                {
                    sb.AppendLine("  ... (chain truncated at 32 levels)");
                    break;
                }
            }
            return sb.ToString().TrimEnd();
        }

        /// computed opacity（rematch 后真值，与渲染/命中同源）。NodeStyle.Opacity 是
        /// 写镜像（只反映 C# 侧 setter 写过的值），读运行时真值须走 get_node_opacity FFI。
        static float OpacityOf(Node n)
        {
            StageHandle* h = (StageHandle*)n.Context._stage.ToPointer();
            float v = 1f;
            int rc = Native.yio_stage_get_node_opacity(h, n._id, &v);
            return rc == 0 ? v : 1f;
        }

        /// 节点 class 全量（空格 join）。ClassList 公共面是 Contains/Add 族，无枚举——
        /// 直读 get_node_classes FFI（双调法：stack 探 256，不够按所需堆分配重调）。
        static string ClassesOf(Node n)
        {
            StageHandle* h = (StageHandle*)n.Context._stage.ToPointer();
            nuint needed = 0;
            Span<byte> stackBuf = stackalloc byte[256];
            fixed (byte* sbp = stackBuf)
            {
                int rc = Native.yio_stage_get_node_classes(h, n._id, sbp, (nuint)stackBuf.Length, &needed);
                if (rc == 0) return Encoding.UTF8.GetString(stackBuf.Slice(0, (int)needed));
                if (rc != -2) return "?";
            }
            byte[] heapBuf = new byte[(int)needed];
            fixed (byte* hbp = heapBuf)
            {
                nuint written = 0;
                int rc = Native.yio_stage_get_node_classes(h, n._id, hbp, (nuint)heapBuf.Length, &written);
                if (rc != 0) return "?";
                return Encoding.UTF8.GetString(heapBuf, 0, (int)written);
            }
        }
    }
}
