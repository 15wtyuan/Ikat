using System;
using System.Text;
using UnityEngine;

namespace Ikat
{
    /// 帧 blob 托管解析视图。解析 Rust build_blob 产出的 little-endian blob。
    ///
    /// 布局（镜像 ikat_ffi_c/src/blob.rs，v14）：
    ///   header (128B): magic(u32 LE), version(u32)=14, node_count(u32),
    ///                 23× col_offset(u32, byte offset from blob start),
    ///                 mesh_arena_off(u32), mesh_arena_len(u32),
    ///                 clip_table_off(u32), clip_table_len(u32),
    ///                 path_table_off(u32), path_table_len(u32)
    ///   23 列 SOA（顺序见 ColOff 注释），随后 mesh_arena / clip_table / path_table 段。
    ///   v10：text_arena 已删（文本字形塌进 mesh_arena，核心自产 atlas），列 text_off/text_len 删除（22→20 列）。
    ///   v11：加 effect_block 列（[f32;32]=128B，per-text-node SDF effect 参数），列数 20→21。
    ///   v12：加 shadow_params 列（[f32;6]=24B，box-shadow SDF 参数），列数 21→22。
    ///   v13：加 grad_params 列（[f32;52]=208B，背景渐变像素参数），列数 22→23。
    ///   v14：node_id/parent_id 列 4B→8B（NodeId u64 拓宽，#26），列数不变。
    /// C# on Windows 是 little-endian，BitConverter 直读无需 byte swap。
    public readonly struct FrameBlob
    {
        public const uint Magic = 0x4D4F4F4C;
        /// blob 版本。magic+version 校验在 IsValid。
        /// v10：删 text_arena + text_off/text_len 列（22→20），文本字形塌进 mesh_arena。
        /// v11：加 effect_block 列（SDF effect 参数，照 color_matrix 先例），列数 20→21。
        /// v12：加 shadow_params 列（box-shadow SDF 参数 [f32;6]，照 color_matrix/effect_block 先例），列数 21→22。
        /// v13：加 grad_params 列（渐变像素参数 [f32;52]，照 effect_block 先例），列数 22→23。
        /// v14：node_id/parent_id 列 u32→u64/i64（NodeId ABI 拓宽，#26）。
        public const uint ExpectedVersion = 14;

        readonly byte[] _buf;

        public FrameBlob(byte[] buf) { _buf = buf; }

        /// magic==Magic && version==ExpectedVersion。MirrorPool.Sync 顶据此拒绝过期 blob。
        public bool IsValid => ReadU32(0) == Magic && ReadU32(4) == ExpectedVersion;
        public uint Version => ReadU32(4);
        public int NodeCount => (int)ReadU32(8);

        // 列 offset 在 header[12 .. 12+23*4)。顺序同 Rust columns：
        //   0=node_id(u64) 1=parent_id(i64,-1=none) 2=visible(u8) 3=alpha(f32)
        //   4=sort_key(u32) 5=mask_context(u32)
        //   6=m_a(f32) 7=m_b(f32) 8=m_c(f32) 9=m_d(f32) 10=m_tx(f32) 11=m_ty(f32)
        //   ↑ world matrix Affine2 6 列（m_a..m_ty）。
        //   12=payload_kind(u8, 1=Mesh；0 不产生——变更级别由 change_level 列表达)
        //   13=mesh_off(u32) 14=mesh_len(u32)
        //   15=path_idx(u32)  ← v7：path 表 1-based 索引，0=纯色无图
        //   16=program(u8, 0=img/无图 1=Text 2=Container+bg-image 3=filter无bg-image 4=filter+bg-image 5=box-shadow blur)
        //   17=color_matrix([f32;20], 80B)
        //   18=change_level(u8, 0=Skip 1=Header 2=Full)
        //   19=reuse_key(u32, 0=无复用 >0=slot 复用键)
        //   20=effect_block([f32;32], 128B)  ← v11：SDF 文字效果参数（outline/underlay×3/glow/blur）
        //   21=shadow_params([f32;6], 24B)   ← v12：box-shadow SDF 参数（halfSize.xy,radius,σ,inset,_pad）
        //   22=grad_params([f32;52], 208B)   ← v13：背景渐变像素参数（program=6/7 门控读取）
        //   v10：删 text_off(u32)/text_len(u32) 列（原第 15-16 列），其后列统一前移 2。
        //   v11：加 effect_block 列（新第 20 列，不动 v10 前移结果）。
        //   v12：加 shadow_params 列（新第 21 列，列数 21→22，arena header 起点同步后移 4 字节）。
        //   v13：加 grad_params 列（新第 22 列，列数 22→23，arena header 起点同步后移 4 字节）。
        //   v14：node_id/parent_id 列 4B→8B（#26），列数不变，col_off 布局不变（列宽变）。
        int ColOff(int idx) => (int)ReadU32(12 + idx * 4);

        // 三 arena header offset。23 列 col_offset 之后：mesh(2), clip(2), path(2) 各 off+len。
        //   v10：text_arena 已删，arena header 由 8 项缩为 6 项。
        //   v11：col_offset 段扩到 21 项，arena header 起点 12+20*4 → 12+21*4（移后 4 字节）。
        //   v12：col_offset 段扩到 22 项，arena header 起点 12+21*4 → 12+22*4（再移后 4 字节）。
        //   v13：col_offset 段扩到 23 项，arena header 起点 12+22*4 → 12+23*4（再移后 4 字节）。
        int MeshArenaOff => (int)ReadU32(12 + 23 * 4);
        int MeshArenaLen => (int)ReadU32(12 + 23 * 4 + 4);
        int ClipTableOff => (int)ReadU32(12 + 23 * 4 + 2 * 4);
        int ClipTableLen => (int)ReadU32(12 + 23 * 4 + 2 * 4 + 4);
        int PathTableOff => (int)ReadU32(12 + 23 * 4 + 4 * 4);
        int PathTableLen => (int)ReadU32(12 + 23 * 4 + 4 * 4 + 4);

        /// v14：node_id 列 u64（NodeId ABI 拓宽，#26）。tag 字节（bits[63:56]）区分
        /// 真实节点（0）与渲染层合成节点（1..=15 跨页子页 / 16-17 scrollbar thumb /
        /// 32-35 TextField / 36-47 box-shadow 等）；MirrorPool 按完整 u64 keying。
        public ulong NodeId(int i) => ReadU64(ColOff(0) + i * 8);
        /// v14：parent_id 列 i64（-1 = 无父）。合成节点的 parent 仍是其 primary 的 NodeId。
        public long ParentId(int i) => (long)ReadU64(ColOff(1) + i * 8);
        /// visible 字节双用：bit0=本帧渲染，bit1=parked keepalive（留 GO 不渲染）。
        /// MirrorPool.Sync 用 Parked(i) 识别 keepalive 条目，留镜像对象、跳过渲染上传。
        public bool Visible(int i) => (_buf[ColOff(2) + i] & 0x01) != 0;
        public bool Parked(int i)  => (_buf[ColOff(2) + i] & 0x02) != 0;
        public float Alpha(int i) => ReadF32(ColOff(3) + i * 4);
        public uint SortKey(int i) => ReadU32(ColOff(4) + i * 4);
        public uint MaskContext(int i) => ReadU32(ColOff(5) + i * 4);
        // world matrix Affine2 6 列 (m_a..m_ty)。
        public float Ma(int i) => ReadF32(ColOff(6) + i * 4);
        public float Mb(int i) => ReadF32(ColOff(7) + i * 4);
        public float Mc(int i) => ReadF32(ColOff(8) + i * 4);
        public float Md(int i) => ReadF32(ColOff(9) + i * 4);
        public float Mtx(int i) => ReadF32(ColOff(10) + i * 4);
        public float Mty(int i) => ReadF32(ColOff(11) + i * 4);
        public byte PayloadKind(int i) => _buf[ColOff(12) + i];
        uint MeshOff(int i) => ReadU32(ColOff(13) + i * 4);
        uint MeshLen(int i) => ReadU32(ColOff(14) + i * 4);
        /// 诊断用：节点 i 的 mesh_len 原始列值（Skip/Header=0，Full>0）。判断 ReadMesh 是否有效。
        public uint ReadMeshLenRaw(int i) => MeshLen(i);
        /// v10：path_idx 前移至第 15 列（原第 17 列，删 text_off/text_len 后前移 2）。
        /// Mesh→path 表 1-based 索引（0=纯色无图）。MirrorPool 读 path_idx → ReadPath(idx) 取 path → 查 Sprite。
        public uint PathIdx(int i) => ReadU32(ColOff(15) + i * 4);
        /// 节点 i 的 program（u8 列，ColOff(16) + i）。v10 前移至第 16 列（原第 18 列）。
        /// 0=img/无图 Container，1=Text（文本现走 mesh 路径，核心产 atlas），2=Container+bg-image，3=filter无bg-image，4=filter+bg-image，5=box-shadow blur（SHADOW_BLUR），6=背景渐变（GRADIENT），7=渐变+filter（GRADIENT+COLOR_FILTER）。
        public byte Program(int i) => _buf[ColOff(16) + i];

        /// 节点 i 的 color_matrix（[f32;20]，ColOff(17) + i*80）。v10 前移至第 17 列（原第 19 列）。
        /// program=3/4 节点填矩阵，其余全零。
        /// 拆 5 个 Vector4 供 MPB SetVector：_CF0..3（矩阵行）+ _CFOff（offset）。
        public float[] ColorMatrix(int i) {
            int off = ColOff(17) + i * 80;
            float[] m = new float[20];
            for (int j = 0; j < 20; j++) {
                m[j] = BitConverter.ToSingle(_buf, off + j * 4);
            }
            return m;
        }

        /// v11：effect_block 列（第 21 列，index 20）。32 × f32 = 128B/节点。
        /// flatten 顺序（镜像 Rust EffectBlock::to_bytes）：
        ///   eb[0]=outline_width  eb[1..5]=outline_color(RGBA)
        ///   underlay[3] 各 7 f32：[offset_x, offset_y, softness, color(RGBA)]，起点 [5]/[12]/[19]
        ///   eb[26]=glow_power  eb[27..31]=glow_color(RGBA)  eb[31]=blur_width
        /// 非 text 节点 default 全 0 = 无 effect（MirrorPool 仅 program==1 时读此 → MPB）。
        public float[] EffectBlock(int i) {
            int off = ColOff(20) + i * 128;
            float[] eb = new float[32];
            for (int j = 0; j < 32; j++) {
                eb[j] = BitConverter.ToSingle(_buf, off + j * 4);
            }
            return eb;
        }

        /// v12：shadow_params 列（第 22 列，index 21）。6 × f32 = 24B/节点。
        /// box-shadow SDF 参数（halfSize.xy, radius, σ, inset, _pad）。非 shadow 节点 default 全零。
        /// MirrorPool 仅 program==5 时读此 → per-renderer MPB（_ShadowHalfSize/_ShadowRadius/
        /// _ShadowSigma/_ShadowInset），shader SHADOW_BLUR 变体消费。用 BitConverter.ToSingle
        /// （Unity Mono 无 BitConverter.SingleToUInt32Bits）。
        public float[] ShadowParams(int i) {
            int off = ColOff(21) + i * 24;
            return new float[6] {
                BitConverter.ToSingle(_buf, off),
                BitConverter.ToSingle(_buf, off + 4),
                BitConverter.ToSingle(_buf, off + 8),
                BitConverter.ToSingle(_buf, off + 12),
                BitConverter.ToSingle(_buf, off + 16),
                BitConverter.ToSingle(_buf, off + 20),
            };
        }

        /// v13：grad_params 列（第 23 列，index 22）。52 × f32 = 208B/节点。
        /// 背景渐变像素参数（镜像 Rust GradientParams::to_bytes）：
        ///   gp[0]=kind(0=linear,1=radial)  gp[1]=angle_deg
        ///   gp[2..4]=dir(xy)  gp[4]=t0  gp[5]=inv_span       （linear）
        ///   gp[6..8]=center(xy)  gp[8..10]=radii(xy)         （radial）
        ///   gp[10]=stop_count  gp[11]=reserved
        ///   gp[12..52]=stops[8] × {r,g,b,a,pos}
        /// 非渐变节点 default 全零。MirrorPool 仅 program==6/7 时读此 → MPB（_Grad* uniforms），
        /// shader GRADIENT 变体消费。
        public float[] GradParams(int i) {
            int off = ColOff(22) + i * 208;
            float[] gp = new float[52];
            for (int j = 0; j < 52; j++) {
                gp[j] = BitConverter.ToSingle(_buf, off + j * 4);
            }
            return gp;
        }

        /// v10：change_level 前移至第 18 列（原第 20 列）。0=Skip 1=Header 2=Full。MirrorPool 三分支用。
        public byte ChangeLevel(int i) => _buf[ColOff(18) + i];
        /// v10：reuse_key 前移至第 19 列（原第 21 列）。0=无复用（按 node_id），>0=按 reuse_key 复用 GO。
        public uint ReuseKey(int i) => ReadU32(ColOff(19) + i * 4);

        /// 判断节点 i 是否为纯平移（identity 2×2 部分）—— epsilon 1e-6 对齐 Rust。
        public bool IsPureTranslation(int i) =>
            Math.Abs(Ma(i) - 1f) < 1e-6f && Math.Abs(Mb(i)) < 1e-6f
            && Math.Abs(Mc(i)) < 1e-6f && Math.Abs(Md(i) - 1f) < 1e-6f;

        // layout: path_count:u32 后跟 count × {path_len:u32, path_bytes:u8[path_len]}（length-prefixed UTF-8）。
        // 镜像 Rust blob.rs::read_path / path_count。MirrorPool 读 path_idx → ReadPath(idx) 取 path 串。
        /// path string table 的 path_count（path table 首 4B）。无 image_path scene 恒为 0。
        public int PathCount => PathTableLen >= 4 ? (int)ReadU32(PathTableOff) : 0;

        /// 读 path string table 第 idx（1-based）条 path。
        /// idx=0 → null（纯色无图）；idx>0 → 读 path_table 内第 idx 条 length-prefixed UTF-8。
        /// 越界 / 损坏 → null（调用方 fallback，不崩）。
        public string ReadPath(uint idx)
        {
            if (idx == 0) return null;
            int count = PathCount;
            if (idx > count) return null;
            int p = PathTableOff + 4;   // 跳 path_count
            for (uint n = 1; n <= idx; n++)
            {
                int len = (int)ReadU32(p);
                p += 4;
                if (n == idx)
                {
                    return System.Text.Encoding.UTF8.GetString(_buf, p, len);
                }
                p += len;
            }
            return null;
        }

        /// clip 表 entry 数（context>0 入表）。无 mask scene 恒为 0。
        /// clip 表段布局：clip_count(u32) + entries[count × {ctx,x,y,w,h + 4×(rx,ry) 各 f32} = 52B/entry]。
        /// clip_count(u32) 在 ClipTableOff 处；clip_table_len 含 clip_count 本身。
        public int ClipCount => ClipTableLen >= 4 ? (int)ReadU32(ClipTableOff) : 0;

        /// 读某 clip context 的 design rect（绝对，y-down）+ 四角圆角半径。
        /// entry 布局：ctx,x,y,w,h 各 4B + radii 4×(rx,ry) 8×f32 = 52B/entry。
        /// mask_context==0 永不入表（无裁剪）；未找到 ctx → found=false（调用方跳过 SetClipBox）。
        /// radii 全零 → cornerRadius=0（调用方走 CLIPPED 直角变体）；非全零 → 走 CLIPPED_ROUNDED SDF。
        /// 镜像 Rust blob.rs clip 表序列化。线性扫描（few entries，O(n) 足够）。
        public bool ClipRect(uint ctx, out float x, out float y, out float w, out float h,
                             out float cornerRadius)
        {
            int count = ClipCount;
            int p = ClipTableOff + 4;   // 跳过 clip_count
            for (int i = 0; i < count; i++)
            {
                if (ReadU32(p) == ctx)
                {
                    x = ReadF32(p + 4);
                    y = ReadF32(p + 8);
                    w = ReadF32(p + 12);
                    h = ReadF32(p + 16);
                    // MVP 统一半径：取四角 (rx,ry) 的最小值（非均匀 SDF 留后续）。
                    // 四角半径在圆角矩形 SDF 中需统一 r——取 min 保证四角都不超目标半径，
                    // 视觉上小半径角精确、大半径角偏小（保守，不溢出 clip 边界）。
                    float tlx = ReadF32(p + 20), tly = ReadF32(p + 24);
                    float trx = ReadF32(p + 28), try_ = ReadF32(p + 32);
                    float brx = ReadF32(p + 36), bry = ReadF32(p + 40);
                    float blx = ReadF32(p + 44), bly = ReadF32(p + 48);
                    float minRx = Mathf.Min(Mathf.Min(tlx, trx), Mathf.Min(brx, blx));
                    float minRy = Mathf.Min(Mathf.Min(tly, try_), Mathf.Min(bry, bly));
                    cornerRadius = Mathf.Min(minRx, minRy);
                    return true;
                }
                p += 52; // 52B/entry（ctx+rect 20B + radii 32B）
            }
            x = y = w = h = 0f;
            cornerRadius = 0f;
            return false;
        }

        /// 读节点 i 的 mesh（仅 payload_kind==1 时调用）。v10：所有渲染节点（含 text）统一走 mesh_arena。
        /// mesh arena 段布局：vert_count(u32) idx_count(u32) verts[vc×2 f32] uvs[vc×2 f32]
        ///               colors[vc×4 f32] indices[idx_count u32]。
        /// 返回的 MeshSegment 持有 verts/uvs/colors/indices 数组的拷贝。
        public MeshSegment ReadMesh(int i)
        {
            int p = MeshArenaOff + (int)MeshOff(i);
            int vertCount = (int)ReadU32(p); p += 4;
            int idxCount = (int)ReadU32(p); p += 4;
            var seg = new MeshSegment(vertCount, idxCount);
            for (int v = 0; v < vertCount; v++)
            {
                seg.Verts[v] = new UnityEngine.Vector2(ReadF32(p), ReadF32(p + 4)); p += 8;
            }
            for (int v = 0; v < vertCount; v++)
            {
                seg.Uvs[v] = new UnityEngine.Vector2(ReadF32(p), ReadF32(p + 4)); p += 8;
            }
            for (int v = 0; v < vertCount; v++)
            {
                seg.Colors[v] = new UnityEngine.Color(ReadF32(p), ReadF32(p + 4), ReadF32(p + 8), ReadF32(p + 12)); p += 16;
            }
            for (int k = 0; k < idxCount; k++) { seg.Idx[k] = ReadU32(p); p += 4; }
            return seg;
        }

        uint ReadU32(int o) => BitConverter.ToUInt32(_buf, o);
        ulong ReadU64(int o) => BitConverter.ToUInt64(_buf, o);
        float ReadF32(int o) => BitConverter.ToSingle(_buf, o);

        // 诊断 dump 用：暴露读原语 + clip 表偏移（UnityIkatBackend.DumpBlobState 线性扫表）。
        public uint ReadU32Public(int o) => ReadU32(o);
        public float ReadF32Public(int o) => ReadF32(o);
        public int ClipTableOffPub => ClipTableOff;
    }

    /// ReadMesh 返回的 mesh 数据拷贝。verts/uvs/colors 长度 == vertCount，Idx 长度 == idxCount。
    public sealed class MeshSegment
    {
        public readonly UnityEngine.Vector2[] Verts;
        public readonly UnityEngine.Vector2[] Uvs;
        public readonly UnityEngine.Color[] Colors;
        public readonly uint[] Idx;

        public MeshSegment(int vertCount, int idxCount)
        {
            Verts = new UnityEngine.Vector2[vertCount];
            Uvs = new UnityEngine.Vector2[vertCount];
            Colors = new UnityEngine.Color[vertCount];
            Idx = new uint[idxCount];
        }
    }
}
