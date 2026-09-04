using System;
using System.Text;
using UnityEngine;

namespace Ikat
{
    /// 帧 blob 托管解析视图。解析 Rust build_blob 产出的 little-endian blob。
    ///
    /// 布局（镜像 ikat_ffi_c/src/blob.rs，v15 列级增量）：
    ///   header (132B): magic(u32 LE), version(u32)=15, node_count(u32),
    ///                   skip_count(u32),
    ///                   21× col_offset(u32, byte offset from blob start),
    ///                   mesh_arena off/len, clip_table off/len, path_table off/len,
    ///                   fat_arena off/len（4 pair×u32）
    ///   lean 21 列 SOA（Skip 之外的全部行，84B/行）——顺序：
    ///     0=node_id(u64) 1=parent_id(i64,-1=none) 2=visible(u8) 3=alpha(f32)
    ///     4=sort_key(u32) 5=mask_context(u32) 6..11=m_a..m_ty(f32×6)
    ///     12=payload_kind(u8) 13=mesh_off(u32) 14=mesh_len(u32) 15=path_idx(u32)
    ///     16=program(u8) 17=change_level(u8, 1=Header 2=Full) 18=reuse_key(u32)
    ///     19=mount_id(u64, 0=screen——world-space 子树锚标记) 20=fat_off(u32, 0=无)
    ///   fat arena：fat_off（1-based）指 entry = {mask u8, [color_matrix 80B],
    ///     [effect_block 128B], [shadow_params 24B], [grad_params 208B]}（mask 位命中才有块）。
    ///   skip 段（fat arena 末尾起）：skip_count × {node_id u64, reuse_key u32, flags u8, pad 3}
    ///     ——Skip 行与 parked keepalive（flags bit1）。16B/条。
    /// v15 语义：Skip 行不进 SOA（后端只清 stale）；胖参数全零不写（省 440B/行）。
    /// C# on Windows 是 little-endian，BitConverter 直读无需 byte swap。
    public readonly struct FrameBlob
    {
        public const uint Magic = 0x4D4F4F4C;
        /// blob 版本。magic+version 校验在 IsValid。
        /// v15：列级增量——Skip 段 + fat arena + mount_id 列；v14 及以前的胖定宽列删除。
        public const uint ExpectedVersion = 15;

        const int ColCount = 21;
        const int SkipEntrySize = 16;

        readonly byte[] _buf;

        public FrameBlob(byte[] buf) { _buf = buf; }

        /// magic==Magic && version==ExpectedVersion。MirrorPool.Sync 顶据此拒绝过期 blob。
        public bool IsValid => ReadU32(0) == Magic && ReadU32(4) == ExpectedVersion;
        public uint Version => ReadU32(4);
        /// 总条目数 = LeanCount + SkipCount（header）。
        public int NodeCount => (int)ReadU32(8);
        /// skip 段条目数（Skip 行 + parked keepalive）。
        public int SkipCount => (int)ReadU32(12);
        /// lean 行数（Header/Full 行；= NodeCount - SkipCount）。
        public int LeanCount => NodeCount - SkipCount;

        int ColOff(int idx) => (int)ReadU32(16 + idx * 4);

        // 四 arena pair 起点（21 列 col_offset 之后 = header byte 100）。
        int MeshArenaOff => (int)ReadU32(16 + ColCount * 4);
        int MeshArenaLen => (int)ReadU32(16 + ColCount * 4 + 4);
        int ClipTableOff => (int)ReadU32(16 + ColCount * 4 + 2 * 4);
        int ClipTableLen => (int)ReadU32(16 + ColCount * 4 + 2 * 4 + 4);
        int PathTableOff => (int)ReadU32(16 + ColCount * 4 + 4 * 4);
        int PathTableLen => (int)ReadU32(16 + ColCount * 4 + 4 * 4 + 4);
        int FatArenaOff => (int)ReadU32(16 + ColCount * 4 + 6 * 4);
        int FatArenaLen => (int)ReadU32(16 + ColCount * 4 + 6 * 4 + 4);

        // —— skip 段（v15：Skip 行 + parked keepalive，16B/条）——
        /// skip 段第 s 条的 node_id。
        public ulong SkipNodeId(int s) => ReadU64(SkipOff(s));
        /// skip 段第 s 条的 reuse_key（0 = 按 node_id 池化）。
        public uint SkipReuseKey(int s) => ReadU32(SkipOff(s) + 8);
        /// skip 段第 s 条 flags：bit1=parked keepalive（留 GO 不渲染）。
        public bool SkipParked(int s) => (_buf[SkipOff(s) + 12] & 0x02) != 0;
        int SkipOff(int s) => FatArenaOff + FatArenaLen + s * SkipEntrySize;

        // —— lean 行访问器（i ∈ [0, LeanCount)；Skip 行不在此，别越权读）——
        /// lean 行 i 的 node_id（u64）。tag 字节（bits[63:56]）区分真实节点（0）与
        /// 渲染层合成节点（1..=15 跨页子页 / 16-17 scrollbar thumb / 32-35 TextField /
        /// 36-47 box-shadow 等）；MirrorPool 按完整 u64 keying。
        public ulong NodeId(int i) => ReadU64(ColOff(0) + i * 8);
        /// lean 行 i 的 parent_id（i64，-1 = 无父）。合成节点的 parent 仍是其 primary 的 NodeId。
        public long ParentId(int i) => (long)ReadU64(ColOff(1) + i * 8);
        /// visible 字节 bit0：本帧渲染。lean 行恒置位（不渲染的节点经 prune 剪除，
        /// 不产条目）；skip 段行不渲染。
        public bool Visible(int i) => (_buf[ColOff(2) + i] & 0x01) != 0;
        /// lean 行 i 是否 parked keepalive——v15 起恒 false（parked 在 skip 段，
        /// 用 <see cref="SkipParked"/>）。保留签名防旧调用点编译断。
        public bool Parked(int i) => false;
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
        /// 诊断用：节点 i 的 mesh_len 原始列值（Header=0，Full>0）。判断 ReadMesh 是否有效。
        public uint ReadMeshLenRaw(int i) => MeshLen(i);
        /// Mesh→path 表 1-based 索引（0=纯色无图）。MirrorPool 读 path_idx → ReadPath(idx)
        /// 取 path → 查 Sprite。
        public uint PathIdx(int i) => ReadU32(ColOff(15) + i * 4);
        /// 节点 i 的 program（u8 列）。
        /// 0=img/无图 Container，1=Text，2=Container+bg-image，3=filter无bg-image，
        /// 4=filter+bg-image，5=box-shadow blur，6=背景渐变，7=渐变+filter。
        public byte Program(int i) => _buf[ColOff(16) + i];
        /// lean 行 i 的 change_level（1=Header 2=Full；Skip 行在 skip 段不占 lean 位）。
        public byte ChangeLevel(int i) => _buf[ColOff(17) + i];
        /// lean 行 i 的 reuse_key（0=无复用按 node_id，>0=按 reuse_key 复用 GO）。
        public uint ReuseKey(int i) => ReadU32(ColOff(18) + i * 4);
        /// v15：world-space 子树锚标记（0=screen）。C8 接线用——MirrorPool 按它路由 SetParent。
        public ulong MountId(int i) => ReadU64(ColOff(19) + i * 8);

        /// 判断节点 i 是否为纯平移（identity 2×2 部分）—— epsilon 1e-6 对齐 Rust。
        public bool IsPureTranslation(int i) =>
            Math.Abs(Ma(i) - 1f) < 1e-6f && Math.Abs(Mb(i)) < 1e-6f
            && Math.Abs(Mc(i)) < 1e-6f && Math.Abs(Md(i) - 1f) < 1e-6f;

        // —— fat arena（v15：胖参数块按需存在；全零块不写）——
        // mask 位：bit0=color_matrix(80B) bit1=effect_block(128B)
        //          bit2=shadow_params(24B) bit3=grad_params(208B)
        const byte FatCm = 0b0001, FatEffect = 0b0010, FatShadow = 0b0100, FatGrad = 0b1000;

        /// lean 行 i 的 fat 引用（0=无胖块）。
        uint FatOff(int i) => ReadU32(ColOff(20) + i * 4);
        int FatArenaEnd => FatArenaOff + FatArenaLen;

        /// fat entry 内某块的 blob 偏移；无该块 → -1。
        int FatBlockOff(int i, byte bit)
        {
            uint off = FatOff(i);
            if (off == 0) return -1;
            int p = FatArenaOff + (int)off; // 跳过 mask 字节后的首块位置起点
            byte mask = _buf[p - 1];
            if ((mask & bit) == 0) return -1;
            if ((mask & FatCm) != 0) { if (bit == FatCm) return p; p += 80; }
            if ((mask & FatEffect) != 0) { if (bit == FatEffect) return p; p += 128; }
            if ((mask & FatShadow) != 0) { if (bit == FatShadow) return p; p += 24; }
            return p; // grad（唯一剩余块）
        }

        /// 节点 i 的 color_matrix（[f32;20]）。无胖块 = 全零。
        /// 拆 5 个 Vector4 供 MPB SetVector：_CF0..3（矩阵行）+ _CFOff（offset）。
        public float[] ColorMatrix(int i)
        {
            var m = new float[20];
            int off = FatBlockOff(i, FatCm);
            if (off < 0) return m;
            for (int j = 0; j < 20; j++)
                m[j] = BitConverter.ToSingle(_buf, off + j * 4);
            return m;
        }

        /// effect_block（32 × f32 = 128B）。flatten 顺序（镜像 Rust EffectBlock::to_bytes）：
        ///   eb[0]=outline_width  eb[1..5]=outline_color(RGBA)
        ///   underlay[3] 各 7 f32：[offset_x, offset_y, softness, color(RGBA)]，起点 [5]/[12]/[19]
        ///   eb[26]=glow_power  eb[27..31]=glow_color(RGBA)  eb[31]=blur_width
        /// 无胖块 = 全 0 = 无 effect（MirrorPool 仅 program==1 时读此 → MPB）。
        public float[] EffectBlock(int i)
        {
            var eb = new float[32];
            int off = FatBlockOff(i, FatEffect);
            if (off < 0) return eb;
            for (int j = 0; j < 32; j++)
                eb[j] = BitConverter.ToSingle(_buf, off + j * 4);
            return eb;
        }

        /// shadow_params（6 × f32 = 24B）。box-shadow SDF 参数
        /// （halfSize.xy, radius, σ, inset, _pad）。无胖块 = 全零。
        /// MirrorPool 仅 program==5 时读此 → per-renderer MPB。
        public float[] ShadowParams(int i)
        {
            var sp = new float[6];
            int off = FatBlockOff(i, FatShadow);
            if (off < 0) return sp;
            for (int j = 0; j < 6; j++)
                sp[j] = BitConverter.ToSingle(_buf, off + j * 4);
            return sp;
        }

        /// grad_params（52 × f32 = 208B）。背景渐变像素参数（镜像 Rust GradientParams::to_bytes）：
        ///   gp[0]=kind(0=linear,1=radial)  gp[1]=angle_deg
        ///   gp[2..4]=dir(xy)  gp[4]=t0  gp[5]=inv_span       （linear）
        ///   gp[6..8]=center(xy)  gp[8..10]=radii(xy)         （radial）
        ///   gp[10]=stop_count  gp[11]=reserved
        ///   gp[12..52]=stops[8] × {r,g,b,a,pos}
        /// 无胖块 = 全零。MirrorPool 仅 program==6/7 时读此 → MPB（_Grad* uniforms）。
        public float[] GradParams(int i)
        {
            var gp = new float[52];
            int off = FatBlockOff(i, FatGrad);
            if (off < 0) return gp;
            for (int j = 0; j < 52; j++)
                gp[j] = BitConverter.ToSingle(_buf, off + j * 4);
            return gp;
        }

        // layout: path_count:u32 后跟 count × {path_len:u32, path_bytes:u8[path_len]}（length-prefixed UTF-8）。
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
        /// clip 表段布局（多 entry，#52）：clip_count(u32) + entries[count × 92B] + poly_arena。
        /// entry：ctx u32 | flags u32 | inv_frame 6×f32 | rect w,h | radii 8×f32 |
        /// circle 3×f32 | poly_count u32 | poly_off u32（arena 内字节偏移）。
        public int ClipCount => ClipTableLen >= 4 ? (int)ReadU32(ClipTableOff) : 0;

        /// 读某 clip context 的全部链 entry（多 entry 交集语义：该 ctx 的有效裁剪 =
        /// 链上全部 entry 逐条测试全过）。mask_context==0 永不入表；空表 = 无该 ctx
        /// entry（调用方跳过 clip uniform 设置）。inv_frame 是 clipper 世界（design
        /// 空间）矩阵逆——fragment design 坐标经它映回 clipper box-local
        /// （(0,0) = border box 左上）再测形状。
        public System.Collections.Generic.List<ClipEntryView> ReadClipEntries(uint ctx)
        {
            var list = new System.Collections.Generic.List<ClipEntryView>();
            int count = ClipCount;
            int entriesEnd = ClipTableOff + 4 + count * 92;
            int p = ClipTableOff + 4;
            for (int i = 0; i < count; i++)
            {
                if (ReadU32(p) == ctx)
                {
                    var e = new ClipEntryView();
                    uint flags = ReadU32(p + 4);
                    e.HasRect = (flags & 0b1) != 0;
                    e.HasRadii = (flags & 0b10) != 0;
                    e.HasShape = (flags & 0b100) != 0;
                    e.ShapeKind = (int)((flags >> 8) & 0xFF);   // 0=circle 1=polygon
                    // inv_frame：a b c d tx ty（core Affine2 六元组）。
                    e.A = ReadF32(p + 8); e.B = ReadF32(p + 12);
                    e.C = ReadF32(p + 16); e.D = ReadF32(p + 20);
                    e.Tx = ReadF32(p + 24); e.Ty = ReadF32(p + 28);
                    e.W = ReadF32(p + 32); e.H = ReadF32(p + 36);
                    // radii 序 [TL, TR, BR, BL] 各 (rx, ry)。
                    e.RadiiTlTr = new UnityEngine.Vector4(
                        ReadF32(p + 40), ReadF32(p + 44), ReadF32(p + 48), ReadF32(p + 52));
                    e.RadiiBrBl = new UnityEngine.Vector4(
                        ReadF32(p + 56), ReadF32(p + 60), ReadF32(p + 64), ReadF32(p + 68));
                    e.CircleCx = ReadF32(p + 72);
                    e.CircleCy = ReadF32(p + 76);
                    e.CircleR = ReadF32(p + 80);
                    int polyCount = (int)ReadU32(p + 84);
                    int polyOff = (int)ReadU32(p + 88);
                    if (polyCount > 0)
                    {
                        e.Poly = new UnityEngine.Vector2[polyCount];
                        int q = entriesEnd + polyOff;
                        for (int k = 0; k < polyCount; k++)
                        {
                            e.Poly[k] = new UnityEngine.Vector2(ReadF32(q), ReadF32(q + 4));
                            q += 8;
                        }
                    }
                    else
                    {
                        e.Poly = System.Array.Empty<UnityEngine.Vector2>();
                    }
                    list.Add(e);
                }
                p += 92;
            }
            return list;
        }

        /// 读节点 i 的 mesh（仅 payload_kind==1 时调用）。所有渲染节点（含 text）统一走 mesh_arena。
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

        /// clip 表中出现的全部 context id（去重）——MirrorPool 每帧为**每个** ctx 刷新
        /// clip 链数组（不依赖任何 lean 行被处理；idle 全 Skip 帧也刷新）。
        public System.Collections.Generic.HashSet<uint> ClipContextIds()
        {
            var set = new System.Collections.Generic.HashSet<uint>();
            int count = ClipCount;
            int p = ClipTableOff + 4;
            for (int i = 0; i < count; i++)
            {
                set.Add(ReadU32(p));
                p += 92;
            }
            return set;
        }

        // 诊断 dump 用：暴露读原语 + clip 表偏移（UnityIkatBackend.DumpBlobState 线性扫表）。
        public uint ReadU32Public(int o) => ReadU32(o);
        public float ReadF32Public(int o) => ReadF32(o);
        public int ClipTableOffPub => ClipTableOff;
    }

    /// ReadClipEntries 返回的单条 clip entry（多 entry 链中一条，#52）。
    /// 几何在 clipper box-local 坐标（(0,0) = border box 左上，design px y-down）；
    /// (A,B,C,D,Tx,Ty) 是 clipper design 世界矩阵逆（core Affine2 六元组：
    /// x' = A·x + C·y + Tx，y' = B·x + D·y + Ty）。HasRect 与 HasShape 独立——
    /// 同元素 overflow:hidden + clip-path 两条测试都过（web 交集原义）。
    public sealed class ClipEntryView
    {
        public bool HasRect;
        public bool HasRadii;
        public bool HasShape;
        /// 0 = circle，1 = polygon（HasShape=false 时无意义）。
        public int ShapeKind;
        public float A, B, C, D, Tx, Ty;
        public float W, H;
        /// radii：(tl_rx, tl_ry, tr_rx, tr_ry) / (br_rx, br_ry, bl_rx, bl_ry)。HasRadii=false 全零。
        public UnityEngine.Vector4 RadiiTlTr, RadiiBrBl;
        public float CircleCx, CircleCy, CircleR;
        /// polygon 顶点（box-local）；非 polygon 为空数组。
        public UnityEngine.Vector2[] Poly = System.Array.Empty<UnityEngine.Vector2>();
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
