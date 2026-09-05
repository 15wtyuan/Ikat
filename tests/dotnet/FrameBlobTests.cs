using System;
using System.Collections.Generic;
using Xunit;

namespace Yio.Tests.Core
{
    public class FrameBlobTests
    {
        // 构造 v15 blob（镜像 yio_ffi_c/src/blob.rs::VERSION=15 + FrameBlob.cs）。
        // v15 = 列级增量：Skip 行出 SOA 进 skip 段、胖参数进 fat arena、+mount_id/fat_off 列（21 列）。
        // nodeCount 参数 = lean 行数（Header/Full）；skipCount = skip 段条目数（全零占位）。
        static byte[] BuildBlob(int nodeCount, byte[][] columnData, byte[] meshArena = null,
            byte[] clipTable = null, byte[] pathTable = null, byte[] fatArena = null, int skipCount = 0)
        {
            meshArena ??= [];
            clipTable ??= [];
            pathTable ??= [];
            fatArena ??= [];

            // 21 lean 列元素字节大小（序同 FrameBlob.cs ColOff 注释一一对应）。
            int[] elemSizes = { 8, 8, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 1, 4, 4, 4, 1, 1, 4, 8, 4 };
            int numCols = elemSizes.Length;
            // header = magic+version+node_count+skip_count(16) + numCols×col_offset + 4 arena ×(off,len)=32。
            int colOff = 16 + numCols * 4 + 32;
            var offs = new int[numCols];
            for (int i = 0; i < numCols; i++) { offs[i] = colOff; colOff += elemSizes[i] * nodeCount; }
            int meshArenaOff = colOff;
            int clipTableOff = meshArenaOff + meshArena.Length;
            int pathTableOff = clipTableOff + clipTable.Length;
            int fatArenaOff = pathTableOff + pathTable.Length;
            int skipOff = fatArenaOff + fatArena.Length;

            var b = new List<byte>();
            b.AddRange(BitConverter.GetBytes(0x314F4959u)); // magic
            b.AddRange(BitConverter.GetBytes(15u));          // version = 15
            b.AddRange(BitConverter.GetBytes((uint)(nodeCount + skipCount)));
            b.AddRange(BitConverter.GetBytes((uint)skipCount));
            foreach (var o in offs) b.AddRange(BitConverter.GetBytes(o));
            b.AddRange(BitConverter.GetBytes(meshArenaOff));
            b.AddRange(BitConverter.GetBytes(meshArena.Length));
            b.AddRange(BitConverter.GetBytes(clipTableOff));
            b.AddRange(BitConverter.GetBytes(clipTable.Length));
            b.AddRange(BitConverter.GetBytes(pathTableOff));
            b.AddRange(BitConverter.GetBytes(pathTable.Length));
            b.AddRange(BitConverter.GetBytes(fatArenaOff));
            b.AddRange(BitConverter.GetBytes(fatArena.Length));

            // column data: caller provides full nodeCount * elemSize bytes per column, or null for zeros.
            for (int c = 0; c < numCols; c++)
            {
                int expected = elemSizes[c] * nodeCount;
                var data = c < columnData.Length ? columnData[c] : null;
                if (data != null)
                    b.AddRange(data);
                else
                    b.AddRange(new byte[expected]);
            }

            b.AddRange(meshArena);
            b.AddRange(clipTable);
            b.AddRange(pathTable);
            b.AddRange(fatArena);
            b.AddRange(new byte[skipCount * 16]); // skip 段占位（全零 = 非 parked Skip 行）
            return b.ToArray();
        }

        static byte[] U32(uint v) => BitConverter.GetBytes(v);
        static byte[] U64(ulong v) => BitConverter.GetBytes(v);
        static byte[] I64(long v) => BitConverter.GetBytes(v);
        static byte[] F32(float v) => BitConverter.GetBytes(v);
        static byte[] U8(byte v) => [v];

        [Fact]
        public void IsValid_GoodBlob_ReturnsTrue()
        {
            var blob = new FrameBlob(BuildBlob(0, new byte[21][]));
            Assert.True(blob.IsValid);
        }

        [Fact]
        public void IsValid_BadMagic_ReturnsFalse()
        {
            var b = BuildBlob(0, new byte[21][]);
            b[0] = 0xFF;
            var blob = new FrameBlob(b);
            Assert.False(blob.IsValid);
        }

        [Fact]
        public void IsValid_BadVersion_ReturnsFalse()
        {
            var b = BuildBlob(0, new byte[21][]);
            BitConverter.GetBytes(99u).CopyTo(b, 4);
            var blob = new FrameBlob(b);
            Assert.False(blob.IsValid);
        }

        [Fact]
        public void NodeCount_ReturnsCorrectValue()
        {
            var blob = new FrameBlob(BuildBlob(3, new byte[21][]));
            Assert.Equal(3, blob.NodeCount);
            Assert.Equal(3, blob.LeanCount);
            Assert.Equal(0, blob.SkipCount);
        }

        [Fact]
        public void ColumnAccessors_ReadCorrectValues()
        {
            var cols = new byte[21][];
            cols[0] = U64(42);          // node_id
            cols[1] = I64(-1);          // parent_id
            cols[2] = U8(1);            // visible
            cols[3] = F32(0.5f);        // alpha
            cols[4] = U32(100);         // sort_key
            cols[5] = U32(7);           // mask_context
            cols[6] = F32(1f);          // m_a
            cols[7] = F32(0f);          // m_b
            cols[8] = F32(0f);          // m_c
            cols[9] = F32(1f);          // m_d
            cols[10] = F32(10f);        // m_tx
            cols[11] = F32(20f);        // m_ty
            cols[12] = U8(1);           // payload_kind
            cols[13] = U32(0);          // mesh_off
            cols[14] = U32(0);          // mesh_len
            cols[15] = U32(3);          // path_idx
            cols[16] = U8(2);           // program
            cols[17] = U8(2);           // change_level（v15 lean 行只 1/2）
            cols[18] = U32(5);          // reuse_key
            cols[19] = U64(0);          // mount_id

            var blob = new FrameBlob(BuildBlob(1, cols));

            Assert.Equal(42ul, blob.NodeId(0));
            Assert.Equal(-1L, blob.ParentId(0));
            Assert.True(blob.Visible(0));
            Assert.Equal(0.5f, blob.Alpha(0));
            Assert.Equal(100u, blob.SortKey(0));
            Assert.Equal(7u, blob.MaskContext(0));
            Assert.Equal(1f, blob.Ma(0));
            Assert.Equal(0f, blob.Mb(0));
            Assert.Equal(0f, blob.Mc(0));
            Assert.Equal(1f, blob.Md(0));
            Assert.Equal(10f, blob.Mtx(0));
            Assert.Equal(20f, blob.Mty(0));
            Assert.Equal((byte)1, blob.PayloadKind(0));
            Assert.Equal(3u, blob.PathIdx(0));
            Assert.Equal((byte)2, blob.Program(0));
            Assert.Equal((byte)2, blob.ChangeLevel(0));
            Assert.Equal(5u, blob.ReuseKey(0));
            Assert.Equal(0ul, blob.MountId(0));
        }

        [Fact]
        public void EffectBlock_ReadsCorrectValues()
        {
            // v15：effect 进 fat arena（mask bit1）。eb[0]=outline_width，eb[31]=blur_width。
            var eb = new byte[128];
            BitConverter.GetBytes(3f).CopyTo(eb, 0);        // eb[0]  outline_width = 3
            BitConverter.GetBytes(7f).CopyTo(eb, 31 * 4);   // eb[31] blur_width     = 7
            var fat = new List<byte>();
            fat.Add(0b0010);      // mask：仅 effect 块
            fat.AddRange(eb);

            var cols = new byte[21][];
            cols[20] = U32(1);    // fat_off = 1（1-based）

            var blob = new FrameBlob(BuildBlob(1, cols, fatArena: fat.ToArray()));
            float[] result = blob.EffectBlock(0);
            Assert.Equal(3f, result[0]);
            Assert.Equal(7f, result[31]);
            Assert.Equal(0f, result[1]);   // outline_color R 默认 0（未写）
        }

        [Fact]
        public void FatParams_AbsentRef_ReturnAllZero()
        {
            // v15 列级增量语义：全零胖块不写（fat_off=0）——读取侧回落全零数组。
            var blob = new FrameBlob(BuildBlob(1, new byte[21][]));
            Assert.All(blob.EffectBlock(0), v => Assert.Equal(0f, v));
            Assert.All(blob.ShadowParams(0), v => Assert.Equal(0f, v));
            Assert.All(blob.GradParams(0), v => Assert.Equal(0f, v));
            Assert.All(blob.ColorMatrix(0), v => Assert.Equal(0f, v));
        }

        [Fact]
        public void GradParams_ReadsCorrectValues()
        {
            // v15：grad 进 fat arena（mask bit3）。
            // gp[0]=kind gp[6..8]=center gp[8..10]=radii gp[10]=stop_count gp[12..17]=stop0。
            var gp = new byte[208];
            BitConverter.GetBytes(1f).CopyTo(gp, 0);           // kind = radial
            BitConverter.GetBytes(1574.4f).CopyTo(gp, 6 * 4);  // cx
            BitConverter.GetBytes(-129.6f).CopyTo(gp, 7 * 4);  // cy
            BitConverter.GetBytes(1100f).CopyTo(gp, 8 * 4);    // rx
            BitConverter.GetBytes(560f).CopyTo(gp, 9 * 4);     // ry
            BitConverter.GetBytes(2f).CopyTo(gp, 10 * 4);      // stop_count
            BitConverter.GetBytes(0.6f).CopyTo(gp, 16 * 4);    // stop1 pos = 0.6
            var fat = new List<byte>();
            fat.Add(0b1000);      // mask：仅 grad 块
            fat.AddRange(gp);

            var cols = new byte[21][];
            cols[20] = U32(1);

            var blob = new FrameBlob(BuildBlob(1, cols, fatArena: fat.ToArray()));
            Assert.True(blob.IsValid);
            float[] r = blob.GradParams(0);
            Assert.Equal(1f, r[0]);
            Assert.Equal(1574.4f, r[6]);
            Assert.Equal(-129.6f, r[7]);
            Assert.Equal(1100f, r[8]);
            Assert.Equal(560f, r[9]);
            Assert.Equal(2f, r[10]);
            Assert.Equal(0.6f, r[16]);
        }

        [Fact]
        public void FatArena_MultiBlockLayout_ReadsEach()
        {
            // fat entry 多块布局：mask=CM|SHADOW → [80B cm][24B shadow]，行内偏移须跳过前块。
            var cm = new byte[80];
            BitConverter.GetBytes(0.5f).CopyTo(cm, 0);
            var shadow = new byte[24];
            BitConverter.GetBytes(12.5f).CopyTo(shadow, 0);
            var fat = new List<byte>();
            fat.Add(0b0101);       // bit0=CM bit2=SHADOW
            fat.AddRange(cm);
            fat.AddRange(shadow);

            var cols = new byte[21][];
            cols[20] = U32(1);

            var blob = new FrameBlob(BuildBlob(1, cols, fatArena: fat.ToArray()));
            Assert.Equal(0.5f, blob.ColorMatrix(0)[0]);
            Assert.Equal(12.5f, blob.ShadowParams(0)[0]);
            // effect 块不在 mask → 全零。
            Assert.Equal(0f, blob.EffectBlock(0)[0]);
        }

        [Fact]
        public void SkipSegment_Accessors_ReadEntries()
        {
            // v15：Skip 行（+parked keepalive）在段末 skip 段，16B/条 {id u64, reuse u32, flags u8, pad}。
            var skipSeg = new List<byte>();
            skipSeg.AddRange(U64(11)); skipSeg.AddRange(U32(0)); skipSeg.Add(0); skipSeg.AddRange(new byte[3]);
            skipSeg.AddRange(U64(22)); skipSeg.AddRange(U32(77)); skipSeg.Add(0b10); skipSeg.AddRange(new byte[3]); // parked

            var b = BuildBlob(1, new byte[21][], skipCount: skipSeg.Count / 16);
            skipSeg.ToArray().CopyTo(b, b.Length - skipSeg.Count);

            var blob = new FrameBlob(b);
            Assert.Equal(3, blob.NodeCount);
            Assert.Equal(2, blob.SkipCount);
            Assert.Equal(1, blob.LeanCount);
            Assert.Equal(11ul, blob.SkipNodeId(0));
            Assert.Equal(0u, blob.SkipReuseKey(0));
            Assert.False(blob.SkipParked(0));
            Assert.Equal(22ul, blob.SkipNodeId(1));
            Assert.Equal(77u, blob.SkipReuseKey(1));
            Assert.True(blob.SkipParked(1));
        }

        [Fact]
        public void IsPureTranslation_Identity_ReturnsTrue()
        {
            var cols = new byte[21][];
            cols[6] = F32(1f); cols[7] = F32(0f); cols[8] = F32(0f); cols[9] = F32(1f);
            var blob = new FrameBlob(BuildBlob(1, cols));
            Assert.True(blob.IsPureTranslation(0));
        }

        [Fact]
        public void IsPureTranslation_Rotated_ReturnsFalse()
        {
            var cols = new byte[21][];
            cols[6] = F32(0.7f); cols[7] = F32(0.7f); cols[8] = F32(-0.7f); cols[9] = F32(0.7f);
            var blob = new FrameBlob(BuildBlob(1, cols));
            Assert.False(blob.IsPureTranslation(0));
        }

        [Fact]
        public void ReadPath_IndexZero_ReturnsNull()
        {
            var blob = new FrameBlob(BuildBlob(0, new byte[21][]));
            Assert.Null(blob.ReadPath(0));
        }

        [Fact]
        public void ReadPath_ValidIndex_ReturnsCorrectString()
        {
            var pathBytes = System.Text.Encoding.UTF8.GetBytes("res/icon.png");
            var pathTable = new List<byte>();
            pathTable.AddRange(U32(2));                      // path_count = 2
            pathTable.AddRange(U32((uint)pathBytes.Length)); // len
            pathTable.AddRange(pathBytes);                    // bytes
            pathTable.AddRange(U32(3));                       // len=3
            pathTable.AddRange(System.Text.Encoding.UTF8.GetBytes("a/b"));

            var blob = new FrameBlob(BuildBlob(0, new byte[21][], pathTable: pathTable.ToArray()));
            Assert.Equal(2, blob.PathCount);
            Assert.Equal("res/icon.png", blob.ReadPath(1));
            Assert.Equal("a/b", blob.ReadPath(2));
        }

        [Fact]
        public void ReadPath_OutOfRange_ReturnsNull()
        {
            var pathTable = new List<byte>();
            pathTable.AddRange(U32(1));
            pathTable.AddRange(U32(3));
            pathTable.AddRange(System.Text.Encoding.UTF8.GetBytes("abc"));
            var blob = new FrameBlob(BuildBlob(0, new byte[21][], pathTable: pathTable.ToArray()));
            Assert.Null(blob.ReadPath(5));
        }

        [Fact]
        public void ReadClipEntries_Found_ReturnsEntries()
        {
            // 多 entry 布局（#52）：92B entry = ctx | flags | inv_frame×6 | w,h |
            // radii×8 | circle×3 | poly_count | poly_off；poly_arena 紧随 entries。
            var clipTable = new List<byte>();
            clipTable.AddRange(U32(2));  // clip_count = 2
            // entry 0：ctx=5，rect + radii，identity frame。
            clipTable.AddRange(U32(5));            // ctx
            clipTable.AddRange(U32(0b011));        // flags: has_rect | has_radii
            clipTable.AddRange(F32(1f)); clipTable.AddRange(F32(0f));  // a b
            clipTable.AddRange(F32(0f)); clipTable.AddRange(F32(1f));  // c d
            clipTable.AddRange(F32(-10f)); clipTable.AddRange(F32(-20f));  // tx ty
            clipTable.AddRange(F32(100f)); clipTable.AddRange(F32(200f));  // w h
            clipTable.AddRange(F32(8f)); clipTable.AddRange(F32(8f));   // tl
            clipTable.AddRange(F32(4f)); clipTable.AddRange(F32(4f));   // tr
            clipTable.AddRange(F32(6f)); clipTable.AddRange(F32(6f));   // br
            clipTable.AddRange(F32(2f)); clipTable.AddRange(F32(2f));   // bl
            clipTable.AddRange(F32(0f)); clipTable.AddRange(F32(0f)); clipTable.AddRange(F32(0f));  // circle
            clipTable.AddRange(U32(0)); clipTable.AddRange(U32(0));     // poly
            // entry 1：ctx=8，shape=polygon（4 点，落 arena 偏移 0）。
            clipTable.AddRange(U32(8));            // ctx
            clipTable.AddRange(U32(0b100 | (1 << 8)));  // flags: has_shape + kind=1(polygon)
            clipTable.AddRange(F32(1f)); clipTable.AddRange(F32(0f));
            clipTable.AddRange(F32(0f)); clipTable.AddRange(F32(1f));
            clipTable.AddRange(F32(0f)); clipTable.AddRange(F32(0f));   // identity frame
            clipTable.AddRange(F32(0f)); clipTable.AddRange(F32(0f));   // w h（无 rect）
            for (int i = 0; i < 8; i++) clipTable.AddRange(F32(0f));    // radii 全零
            clipTable.AddRange(F32(0f)); clipTable.AddRange(F32(0f)); clipTable.AddRange(F32(0f));
            clipTable.AddRange(U32(4));            // poly_count = 4
            clipTable.AddRange(U32(0));            // poly_off = 0
            // poly_arena：菱形 4 点。
            clipTable.AddRange(F32(50f)); clipTable.AddRange(F32(0f));
            clipTable.AddRange(F32(100f)); clipTable.AddRange(F32(50f));
            clipTable.AddRange(F32(50f)); clipTable.AddRange(F32(100f));
            clipTable.AddRange(F32(0f)); clipTable.AddRange(F32(50f));

            var blob = new FrameBlob(BuildBlob(0, new byte[21][], clipTable: clipTable.ToArray()));
            Assert.Equal(2, blob.ClipCount);

            var e5 = blob.ReadClipEntries(5);
            Assert.Single(e5);
            Assert.True(e5[0].HasRect && e5[0].HasRadii && !e5[0].HasShape);
            Assert.Equal(-10f, e5[0].Tx);
            Assert.Equal(100f, e5[0].W);
            Assert.Equal(200f, e5[0].H);
            Assert.Equal(8f, e5[0].RadiiTlTr.x);
            Assert.Equal(2f, e5[0].RadiiBrBl.z);  // bl rx

            var e8 = blob.ReadClipEntries(8);
            Assert.Single(e8);
            Assert.True(e8[0].HasShape && !e8[0].HasRect);
            Assert.Equal(1, e8[0].ShapeKind);
            Assert.Equal(4, e8[0].Poly.Length);
            Assert.Equal(50f, e8[0].Poly[0].x);
            Assert.Equal(50f, e8[0].Poly[3].y);
        }

        [Fact]
        public void ReadClipEntries_NotFound_ReturnsEmpty()
        {
            var clipTable = new List<byte>();
            clipTable.AddRange(U32(1));
            clipTable.AddRange(U32(5)); clipTable.AddRange(U32(0b001));
            for (int i = 0; i < 22; i++) clipTable.AddRange(F32(0f));
            clipTable.AddRange(U32(0)); clipTable.AddRange(U32(0));

            var blob = new FrameBlob(BuildBlob(0, new byte[21][], clipTable: clipTable.ToArray()));
            Assert.Empty(blob.ReadClipEntries(99));
            Assert.Single(blob.ReadClipEntries(5));
        }

        [Fact]
        public void MultiNode_EachHasDistinctValues()
        {
            var cols = new byte[21][];
            cols[0] = U64(100); // node 0: id=100
            var b0 = new List<byte>(); b0.AddRange(cols[0]); b0.AddRange(U64(200)); // node 1: id=200
            cols[0] = b0.ToArray();

            cols[4] = U32(10);  // node 0: sort=10
            var b4 = new List<byte>(); b4.AddRange(cols[4]); b4.AddRange(U32(20)); // node 1: sort=20
            cols[4] = b4.ToArray();

            var blob = new FrameBlob(BuildBlob(2, cols));
            Assert.Equal(100ul, blob.NodeId(0));
            Assert.Equal(200ul, blob.NodeId(1));
            Assert.Equal(10u, blob.SortKey(0));
            Assert.Equal(20u, blob.SortKey(1));
        }
    }
}
