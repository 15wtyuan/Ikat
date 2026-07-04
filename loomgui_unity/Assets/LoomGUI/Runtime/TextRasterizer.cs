using System.Collections.Generic;
using System.Text;
using UnityEngine;

namespace LoomGUI
{
    /// Text 光栅器：把 Rust（text_arena）给的 glyph 笔位 + Unity 动态字体 atlas 的
    /// glyph 像素 box/UV 烤成 glyph quad mesh。**Rust 是布局权威，Unity 是纯光栅器**——
    /// 笔位用 blob 的 (pen_x, pen_y)（Rust ttf 真 advance），**不**用 Unity `CharacterInfo.advance`；
    /// 行高用 blob 的 pen_y（= line.baseline 绝对 y），**不**用 Unity `fontSize*1.25`（跨平台根）。
    ///
    /// atlas rebuild 监听：动态字体 atlas 异步 rebuild 时 glyph UV 变。`Font.textureRebuilt`
    /// 是静态事件；本类持静态 `s_fontVersion`，`OnRebuilt` 自增。MirrorPool.Sync 比对版本号，
    /// 不等则强制所有 text 节点下帧重光栅。
    public static class TextRasterizer
    {
        // atlas rebuild 版本号。MirrorPool 记上次消费的版本；不等则 dirty 所有 text 节点。
        static int s_fontVersion;

        // TEMP DIAG（坑 115 续）：禁 kern 验收仍重叠，打印实际渲染 pen_x + Unity quad 取证。
        // 限 PlayMode 前 10 个 text 节点。取证后删。
        static int s_diagCount;

        /// 当前 font atlas 版本。MirrorPool.Sync 据此判断是否需强制重光栅。
        public static int FontVersion => s_fontVersion;

        /// `Font.textureRebuilt` 静态事件回调（LoomStage.Awake 注册）。任何字体 atlas rebuild
        /// 触发自增 → 下帧 Sync 检测到版本变 → 重 RequestCharactersInTexture + 重取 UV。
        public static void OnRebuilt(Font font) => s_fontVersion++;

        /// Domain reload 保护：清静态版本号。SubsystemRegistration 调。
        public static void ResetStatic() => s_fontVersion = 0;

        /// 把 text_arena 给的 glyphs 烤成 glyph quad mesh（每 glyph 一个 quad：BL,TL,TR,BR）。
        /// 顶点色 = color（不烤 node opacity——node opacity 走 _Alpha uniform，不进顶点色）。
        /// texture = font atlas（caller 设 material）。program=1（text，与 Image 共用 LoomGUI/Unlit）由 caller 在 mm.Get 时指定。
        ///
        /// quad 数学（y-down GO-local，pen 已 GO-local（layout-rect 相对；节点绝对位在 local_x/local_y，pen 是相对节点原点的偏移，勿与 local_x/local_y 叠加），**不 re-base**）：
        ///   quad_left   = pen_x + info.minX
        ///   quad_right  = pen_x + info.maxX
        ///   quad_top    = pen_y − info.maxY   （maxY 在基线上方 → y-down 减）
        ///   quad_bottom = pen_y − info.minY   （minY 在基线下方 → y-down 减，得正值更大的下边）
        /// 顶点序 BL(pl,pb), TL(pl,pt), TR(pr,pt), BR(pr,pb)。
        /// UV：info.uvBottomLeft/TopLeft/TopRight/BottomRight 同序。索引每 quad 0,1,2,0,2,3（累加 vert 基）。
        public static MeshSegment BuildMesh(Font font, int fontSize, Color color, GlyphData[] glyphs)
        {
            // 预扫：GetCharacterInfo 失败的 glyph 跳过（不在 atlas / 字体无此字）。先数有效 glyph 算容量。
            // 同时收集 codepoint 串 RequestCharactersInTexture 填 atlas（必先调，否则 GetCharacterInfo 恒 false）。
            var tinted = color;

            // 1. 收集 codepoints → RequestCharactersInTexture 填 atlas。
            var sb = new StringBuilder(glyphs.Length);
            // BMP 外 codepoint (>0xFFFF) 暂不支持（char 截断会取错字）；跳过。
            for (int i = 0; i < glyphs.Length; i++)
            {
                uint cp = glyphs[i].Codepoint;
                if (cp <= 0xFFFF) sb.Append((char)cp);
            }
            font.RequestCharactersInTexture(sb.ToString(), fontSize, FontStyle.Normal);

            // 2. 每 glyph → quad（先填 List，最后拷进 MeshSegment 数组）。
            var verts = new List<Vector2>(glyphs.Length * 4);
            var uvs = new List<Vector2>(glyphs.Length * 4);
            var cols = new List<Color>(glyphs.Length * 4);
            var idx = new List<int>(glyphs.Length * 6);
            int vi = 0;
            for (int i = 0; i < glyphs.Length; i++)
            {
                uint cp = glyphs[i].Codepoint;
                if (cp > 0xFFFF) continue;  // BMP 外跳过
                if (!font.GetCharacterInfo((char)cp, out var info, fontSize, FontStyle.Normal)) continue;

                float pl = glyphs[i].PenX + info.minX;
                float pr = glyphs[i].PenX + info.maxX;
                float pt = glyphs[i].PenY - info.maxY;   // maxY 基线上方 → y-down 减
                float pb = glyphs[i].PenY - info.minY;   // minY 基线下方

                // BL, TL, TR, BR。
                verts.Add(new Vector2(pl, pb));
                verts.Add(new Vector2(pl, pt));
                verts.Add(new Vector2(pr, pt));
                verts.Add(new Vector2(pr, pb));

                uvs.Add(info.uvBottomLeft);
                uvs.Add(info.uvTopLeft);
                uvs.Add(info.uvTopRight);
                uvs.Add(info.uvBottomRight);

                cols.Add(tinted); cols.Add(tinted); cols.Add(tinted); cols.Add(tinted);

                idx.Add(vi); idx.Add(vi + 1); idx.Add(vi + 2);
                idx.Add(vi); idx.Add(vi + 2); idx.Add(vi + 3);
                vi += 4;
            }

            // 3. 拷进 MeshSegment（数组）。不做 buffer 池化（压测若 GC 抖动再做）。
            var seg = new MeshSegment(verts.Count, idx.Count);
            for (int v = 0; v < verts.Count; v++)
            {
                seg.Verts[v] = verts[v];
                seg.Uvs[v] = uvs[v];
                seg.Colors[v] = cols[v];
            }
            for (int k = 0; k < idx.Count; k++) seg.Idx[k] = (uint)idx[k];

            // TEMP DIAG（坑 115 续）：打印实际渲染的 pen_x + Unity CharacterInfo quad。
            //   pen_x 不含 kern（V≈adv_A=16.4）→ .dll 生效 + 禁 kern 生效。
            //   pen_x 含 kern（V≈15）→ stale .dll 或禁 kern 未生效。
            //   quad 相邻重叠（ov>0）→ Unity quad 宽于 advance 是真正的重叠来源。
            if (Application.isPlaying && s_diagCount < 10 && glyphs.Length > 1)
            {
                s_diagCount++;
                var diag = new StringBuilder();
                diag.AppendLine($"[TEXTDIAG] #{s_diagCount} n={glyphs.Length} size={fontSize}");
                float prevRight = float.NegativeInfinity;
                for (int i = 0; i < glyphs.Length && i < 12; i++)
                {
                    uint cp = glyphs[i].Codepoint;
                    if (cp > 0xFFFF) continue;
                    if (!font.GetCharacterInfo((char)cp, out var ci, fontSize, FontStyle.Normal)) continue;
                    float pl = glyphs[i].PenX + ci.minX;
                    float pr = glyphs[i].PenX + ci.maxX;
                    float ov = prevRight - pl;   // >0 = 与前字 quad 重叠
                    diag.AppendLine($"  cp='{(char)cp}' penX={glyphs[i].PenX,7:F2} adv={ci.advance} quad=[{pl,7:F1},{pr,7:F1}] ov={ov,5:F1}");
                    prevRight = pr;
                }
                Debug.Log(diag.ToString());
            }
            return seg;
        }
    }
}
