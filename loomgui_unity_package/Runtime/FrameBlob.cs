using System;
using System.Text;   // Encoding.UTF8 for ReadPath
using UnityEngine;

namespace LoomGUI
{
    /// 帧 blob 托管解析视图。解析 Rust build_blob 产出的 little-endian blob。
    ///
    /// 布局（镜像 loomgui_ffi_c/src/blob.rs，v10）：
    ///   header (116B): magic(u32 LE), version(u32)=10, node_count(u32),
    ///                 20× col_offset(u32, byte offset from blob start),
    ///                 mesh_arena_off(u32), mesh_arena_len(u32),
    ///                 clip_table_off(u32), clip_table_len(u32),
    ///                 path_table_off(u32), path_table_len(u32)
    ///   20 列 SOA（顺序见 ColOff 注释），随后 mesh_arena / clip_table / path_table 段。
    ///   v10：text_arena 已删（文本字形塌进 mesh_arena，核心自产 atlas），列 text_off/text_len 删除（22→20 列）。
    /// C# on Windows 是 little-endian，BitConverter 直读无需 byte swap。
    public readonly struct FrameBlob
    {
        public const uint Magic = 0x4D4F4F4C;
        /// blob 版本。magic+version 校验在 IsValid。
        /// v10：删 text_arena + text_off/text_len 列（22→20），文本字形塌进 mesh_arena。
        public const uint ExpectedVersion = 10;

        readonly byte[] _buf;

        public FrameBlob(byte[] buf) { _buf = buf; }

        /// magic==Magic && version==ExpectedVersion。MirrorPool.Sync 顶据此拒绝过期 blob。
        public bool IsValid => ReadU32(0) == Magic && ReadU32(4) == ExpectedVersion;
        public uint Version => ReadU32(4);
        public int NodeCount => (int)ReadU32(8);

        // 列 offset 在 header[12 .. 12+20*4)。顺序同 Rust columns：
        //   0=node_id(u32) 1=parent_id(i32,-1=none) 2=visible(u8) 3=alpha(f32)
        //   4=sort_key(u32) 5=mask_context(u32)
        //   6=m_a(f32) 7=m_b(f32) 8=m_c(f32) 9=m_d(f32) 10=m_tx(f32) 11=m_ty(f32)
        //   ↑ world matrix Affine2 6 列（m_a..m_ty）。
        //   12=payload_kind(u8, 1=Mesh；0 不产生——变更级别由 change_level 列表达)
        //   13=mesh_off(u32) 14=mesh_len(u32)
        //   15=path_idx(u32)  ← v7：path 表 1-based 索引，0=纯色无图
        //   16=program(u8, 0=img/无图 1=Text 2=Container+bg-image 3=filter无bg-image 4=filter+bg-image)
        //   17=color_matrix([f32;20], 80B)
        //   18=change_level(u8, 0=Skip 1=Header 2=Full)
        //   19=reuse_key(u32, 0=无复用 >0=slot 复用键)
        //   v10：删 text_off(u32)/text_len(u32) 列（原第 15-16 列），其后列统一前移 2。
        int ColOff(int idx) => (int)ReadU32(12 + idx * 4);

        // 三 arena header offset。20 列 col_offset 之后：mesh(2), clip(2), path(2) 各 off+len。
        //   v10：text_arena 已删，arena header 由 8 项缩为 6 项。
        int MeshArenaOff => (int)ReadU32(12 + 20 * 4);
        int MeshArenaLen => (int)ReadU32(12 + 20 * 4 + 4);
        int ClipTableOff => (int)ReadU32(12 + 20 * 4 + 2 * 4);
        int ClipTableLen => (int)ReadU32(12 + 20 * 4 + 2 * 4 + 4);
        int PathTableOff => (int)ReadU32(12 + 20 * 4 + 4 * 4);
        int PathTableLen => (int)ReadU32(12 + 20 * 4 + 4 * 4 + 4);

        public uint NodeId(int i) => ReadU32(ColOff(0) + i * 4);
        public int ParentId(int i) => (int)ReadU32(ColOff(1) + i * 4);
        public bool Visible(int i) => _buf[ColOff(2) + i] != 0;
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
        /// v10：path_idx 前移至第 15 列（原第 17 列，删 text_off/text_len 后前移 2）。
        /// Mesh→path 表 1-based 索引（0=纯色无图）。MirrorPool 读 path_idx → ReadPath(idx) 取 path → 查 Sprite。
        public uint PathIdx(int i) => ReadU32(ColOff(15) + i * 4);
        /// 节点 i 的 program（u8 列，ColOff(16) + i）。v10 前移至第 16 列（原第 18 列）。
        /// 0=img/无图 Container，1=Text（文本现走 mesh 路径，核心产 atlas），2=Container+bg-image，3=filter无bg-image，4=filter+bg-image。
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

        /// v10：change_level 前移至第 18 列（原第 20 列）。0=Skip 1=Header 2=Full。MirrorPool 三分支用。
        public byte ChangeLevel(int i) => _buf[ColOff(18) + i];
        /// v10：reuse_key 前移至第 19 列（原第 21 列）。0=无复用（按 node_id），>0=按 reuse_key 复用 GO。
        public uint ReuseKey(int i) => ReadU32(ColOff(19) + i * 4);

        /// 判断节点 i 是否为纯平移（identity 2×2 部分）—— epsilon 1e-6 对齐 Rust。
        public bool IsPureTranslation(int i) =>
            Math.Abs(Ma(i) - 1f) < 1e-6f && Math.Abs(Mb(i)) < 1e-6f
            && Math.Abs(Mc(i)) < 1e-6f && Math.Abs(Md(i) - 1f) < 1e-6f;

        // ===== v7 path string table（§5.2）：path_idx 列 1-based 索引此表。
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
        /// clip 表段布局：clip_count(u32) + entries[count × {ctx,x,y,w,h}]。
        /// clip_count(u32) 在 ClipTableOff 处；clip_table_len 含 clip_count 本身。
        public int ClipCount => ClipTableLen >= 4 ? (int)ReadU32(ClipTableOff) : 0;

        /// 读某 clip context 的 design rect（绝对，y-down）。entry 布局：ctx,x,y,w,h 各 4B（20B/entry）。
        /// mask_context==0 永不入表（无裁剪）；未找到 ctx → found=false（调用方跳过 SetClipBox）。
        /// 镜像 Rust blob.rs::read_clips。线性扫描（few entries，O(n) 足够）。
        public bool ClipRect(uint ctx, out float x, out float y, out float w, out float h)
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
                    return true;
                }
                p += 20;
            }
            x = y = w = h = 0f;
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
        float ReadF32(int o) => BitConverter.ToSingle(_buf, o);
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
