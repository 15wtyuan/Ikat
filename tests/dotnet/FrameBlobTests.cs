using System;
using System.Collections.Generic;
using Xunit;

namespace LoomGUI.Tests.Core
{
    public class FrameBlobTests
    {
        // 构造 v14 blob（镜像 loomgui_ffi_c/src/blob.rs::VERSION=14 + FrameBlob.cs）。
        // v14 = v13 + node_id/parent_id 列 4B→8B（#26 u64 拓宽）；列数不变（23）。
        static byte[] BuildBlob(int nodeCount, byte[][] columnData, byte[] meshArena = null, byte[] clipTable = null, byte[] pathTable = null)
        {
            meshArena ??= [];
            clipTable ??= [];
            pathTable ??= [];

            // 23 列元素字节大小（须与 FrameBlob.cs ColOff 注释一一对应）。末列 grad_params=208B。
            int[] elemSizes = { 8, 8, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 1, 4, 4, 4, 1, 80, 1, 4, 128, 24, 208 };
            int numCols = elemSizes.Length;
            // header = magic+version+node_count(12) + numCols×col_offset + 3 arena ×(off,len)=24 → 列数据起点。
            int colOff = 12 + numCols * 4 + 24;
            var offs = new int[numCols];
            for (int i = 0; i < numCols; i++) { offs[i] = colOff; colOff += elemSizes[i] * nodeCount; }
            int meshArenaOff = colOff;
            int clipTableOff = meshArenaOff + meshArena.Length;
            int pathTableOff = clipTableOff + clipTable.Length;

            var b = new List<byte>();
            b.AddRange(BitConverter.GetBytes(0x4D4F4F4Cu)); // magic
            b.AddRange(BitConverter.GetBytes(14u));          // version = 14
            b.AddRange(BitConverter.GetBytes((uint)nodeCount));
            foreach (var o in offs) b.AddRange(BitConverter.GetBytes(o));
            b.AddRange(BitConverter.GetBytes(meshArenaOff));
            b.AddRange(BitConverter.GetBytes(meshArena.Length));
            b.AddRange(BitConverter.GetBytes(clipTableOff));
            b.AddRange(BitConverter.GetBytes(clipTable.Length));
            b.AddRange(BitConverter.GetBytes(pathTableOff));
            b.AddRange(BitConverter.GetBytes(pathTable.Length));

            // column data: caller provides full nodeCount * elemSize bytes per column, or null for zeros.
            // 越过调用方数组长度的列（v13 新增 grad_params 等未更新的旧用例）补零。
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
            return b.ToArray();
        }

        static byte[] U32(uint v) => BitConverter.GetBytes(v);
        static byte[] U64(ulong v) => BitConverter.GetBytes(v);
        static byte[] I64(long v) => BitConverter.GetBytes(v);
        static byte[] I32(int v) => BitConverter.GetBytes(v);
        static byte[] F32(float v) => BitConverter.GetBytes(v);
        static byte[] U8(byte v) => [v];

        [Fact]
        public void IsValid_GoodBlob_ReturnsTrue()
        {
            var blob = new FrameBlob(BuildBlob(0, new byte[22][]));
            Assert.True(blob.IsValid);
        }

        [Fact]
        public void IsValid_BadMagic_ReturnsFalse()
        {
            var b = BuildBlob(0, new byte[22][]);
            b[0] = 0xFF;
            var blob = new FrameBlob(b);
            Assert.False(blob.IsValid);
        }

        [Fact]
        public void IsValid_BadVersion_ReturnsFalse()
        {
            var b = BuildBlob(0, new byte[22][]);
            BitConverter.GetBytes(99u).CopyTo(b, 4);
            var blob = new FrameBlob(b);
            Assert.False(blob.IsValid);
        }

        [Fact]
        public void NodeCount_ReturnsCorrectValue()
        {
            var blob = new FrameBlob(BuildBlob(3, new byte[22][]));
            Assert.Equal(3, blob.NodeCount);
        }

        [Fact]
        public void ColumnAccessors_ReadCorrectValues()
        {
            var cols = new byte[22][];
            cols[0] = U64(42);          // node_id（u64，#26）
            cols[1] = I64(-1);          // parent_id（i64）
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
            // cols[17] = 80 zero bytes  // color_matrix
            cols[18] = U8(2);           // change_level
            cols[19] = U32(5);          // reuse_key

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
        }

        [Fact]
        public void EffectBlock_ReadsCorrectValues()
        {
            // v11 新列 effect_block（第 21 列，index 20，[f32;32]=128B）。eb[0]=outline_width，eb[31]=blur_width。
            var eb = new byte[128];
            BitConverter.GetBytes(3f).CopyTo(eb, 0);        // eb[0]  outline_width = 3
            BitConverter.GetBytes(7f).CopyTo(eb, 31 * 4);   // eb[31] blur_width     = 7
            var cols = new byte[22][];
            cols[20] = eb;

            var blob = new FrameBlob(BuildBlob(1, cols));
            float[] result = blob.EffectBlock(0);
            Assert.Equal(3f, result[0]);
            Assert.Equal(7f, result[31]);
            Assert.Equal(0f, result[1]);   // outline_color R 默认 0（未写）
        }

        [Fact]
        public void GradParams_ReadsCorrectValues()
        {
            // v13 新列 grad_params（第 23 列，index 22，[f32;52]=208B）。
            // gp[0]=kind gp[6..8]=center gp[8..10]=radii gp[10]=stop_count gp[12..17]=stop0。
            var gp = new byte[208];
            BitConverter.GetBytes(1f).CopyTo(gp, 0);           // kind = radial
            BitConverter.GetBytes(1574.4f).CopyTo(gp, 6 * 4);  // cx
            BitConverter.GetBytes(-129.6f).CopyTo(gp, 7 * 4);  // cy
            BitConverter.GetBytes(1100f).CopyTo(gp, 8 * 4);    // rx
            BitConverter.GetBytes(560f).CopyTo(gp, 9 * 4);     // ry
            BitConverter.GetBytes(2f).CopyTo(gp, 10 * 4);      // stop_count
            BitConverter.GetBytes(0.6f).CopyTo(gp, 16 * 4);    // stop1 pos = 0.6
            var cols = new byte[23][];
            cols[22] = gp;

            var blob = new FrameBlob(BuildBlob(1, cols));
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
        public void IsPureTranslation_Identity_ReturnsTrue()
        {
            var cols = new byte[22][];
            cols[6] = F32(1f); cols[7] = F32(0f); cols[8] = F32(0f); cols[9] = F32(1f);
            var blob = new FrameBlob(BuildBlob(1, cols));
            Assert.True(blob.IsPureTranslation(0));
        }

        [Fact]
        public void IsPureTranslation_Rotated_ReturnsFalse()
        {
            var cols = new byte[22][];
            cols[6] = F32(0.7f); cols[7] = F32(0.7f); cols[8] = F32(-0.7f); cols[9] = F32(0.7f);
            var blob = new FrameBlob(BuildBlob(1, cols));
            Assert.False(blob.IsPureTranslation(0));
        }

        [Fact]
        public void ReadPath_IndexZero_ReturnsNull()
        {
            var blob = new FrameBlob(BuildBlob(0, new byte[22][]));
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

            var blob = new FrameBlob(BuildBlob(0, new byte[22][], pathTable: pathTable.ToArray()));
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
            var blob = new FrameBlob(BuildBlob(0, new byte[22][], pathTable: pathTable.ToArray()));
            Assert.Null(blob.ReadPath(5));
        }

        [Fact]
        public void ClipRect_Found_ReturnsCorrectRect()
        {
            var clipTable = new List<byte>();
            clipTable.AddRange(U32(2));  // clip_count = 2
            // 每条目 52B：ctx(4) + rect x/y/w/h(16) + 8 角半径 tl/tr/br/bl 各 xy(32)。
            clipTable.AddRange(U32(5));  // ctx=5
            clipTable.AddRange(F32(10f)); clipTable.AddRange(F32(20f));
            clipTable.AddRange(F32(100f)); clipTable.AddRange(F32(200f));
            clipTable.AddRange(F32(8f)); clipTable.AddRange(F32(8f));   // tl
            clipTable.AddRange(F32(4f)); clipTable.AddRange(F32(4f));   // tr
            clipTable.AddRange(F32(6f)); clipTable.AddRange(F32(6f));   // br
            clipTable.AddRange(F32(2f)); clipTable.AddRange(F32(2f));   // bl
            clipTable.AddRange(U32(8));  // ctx=8
            clipTable.AddRange(F32(1f)); clipTable.AddRange(F32(2f));
            clipTable.AddRange(F32(3f)); clipTable.AddRange(F32(4f));
            for (int i = 0; i < 8; i++) clipTable.AddRange(F32(0f));    // 全 0 半径

            var blob = new FrameBlob(BuildBlob(0, new byte[22][], clipTable: clipTable.ToArray()));
            Assert.Equal(2, blob.ClipCount);

            Assert.True(blob.ClipRect(5, out float x, out float y, out float w, out float h, out float r));
            Assert.Equal(10f, x);
            Assert.Equal(20f, y);
            Assert.Equal(100f, w);
            Assert.Equal(200f, h);
            // 统一半径 = min(各角 rx/ry) = min(8,4,6,2) = 2。
            Assert.Equal(2f, r);

            Assert.True(blob.ClipRect(8, out x, out y, out w, out h, out r));
            Assert.Equal(1f, x);
            Assert.Equal(2f, y);
            Assert.Equal(3f, w);
            Assert.Equal(4f, h);
        }

        [Fact]
        public void ClipRect_NotFound_ReturnsFalse()
        {
            var clipTable = new List<byte>();
            clipTable.AddRange(U32(1));
            clipTable.AddRange(U32(5)); clipTable.AddRange(F32(0f)); clipTable.AddRange(F32(0f));
            clipTable.AddRange(F32(0f)); clipTable.AddRange(F32(0f));

            var blob = new FrameBlob(BuildBlob(0, new byte[22][], clipTable: clipTable.ToArray()));
            Assert.False(blob.ClipRect(99, out _, out _, out _, out _, out _));
        }

        [Fact]
        public void MultiNode_EachHasDistinctValues()
        {
            var cols = new byte[22][];
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
