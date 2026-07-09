using System;
using System.Collections.Generic;
using Xunit;

namespace LoomGUI.Tests.Core
{
    public class FrameBlobTests
    {
        static byte[] V10Header(int nodeCount, byte[][] columnData, byte[] meshArena = null, byte[] clipTable = null, byte[] pathTable = null)
        {
            meshArena ??= [];
            clipTable ??= [];
            pathTable ??= [];

            int[] elemSizes = { 4, 4, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 1, 4, 4, 4, 1, 80, 1, 4 };
            int colOff = 116;
            var offs = new int[20];
            for (int i = 0; i < 20; i++) { offs[i] = colOff; colOff += elemSizes[i] * nodeCount; }
            int meshArenaOff = colOff;
            int clipTableOff = meshArenaOff + meshArena.Length;
            int pathTableOff = clipTableOff + clipTable.Length;

            var b = new List<byte>();
            b.AddRange(BitConverter.GetBytes(0x4D4F4F4Cu));
            b.AddRange(BitConverter.GetBytes(10u));
            b.AddRange(BitConverter.GetBytes((uint)nodeCount));
            foreach (var o in offs) b.AddRange(BitConverter.GetBytes(o));
            b.AddRange(BitConverter.GetBytes(meshArenaOff));
            b.AddRange(BitConverter.GetBytes(meshArena.Length));
            b.AddRange(BitConverter.GetBytes(clipTableOff));
            b.AddRange(BitConverter.GetBytes(clipTable.Length));
            b.AddRange(BitConverter.GetBytes(pathTableOff));
            b.AddRange(BitConverter.GetBytes(pathTable.Length));

            // column data: caller provides full nodeCount * elemSize bytes per column, or null for zeros
            for (int c = 0; c < 20; c++)
            {
                int expected = elemSizes[c] * nodeCount;
                var data = columnData[c];
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
        static byte[] I32(int v) => BitConverter.GetBytes(v);
        static byte[] F32(float v) => BitConverter.GetBytes(v);
        static byte[] U8(byte v) => [v];

        [Fact]
        public void IsValid_GoodBlob_ReturnsTrue()
        {
            var blob = new FrameBlob(V10Header(0, new byte[20][]));
            Assert.True(blob.IsValid);
        }

        [Fact]
        public void IsValid_BadMagic_ReturnsFalse()
        {
            var b = V10Header(0, new byte[20][]);
            b[0] = 0xFF;
            var blob = new FrameBlob(b);
            Assert.False(blob.IsValid);
        }

        [Fact]
        public void IsValid_BadVersion_ReturnsFalse()
        {
            var b = V10Header(0, new byte[20][]);
            BitConverter.GetBytes(99u).CopyTo(b, 4);
            var blob = new FrameBlob(b);
            Assert.False(blob.IsValid);
        }

        [Fact]
        public void NodeCount_ReturnsCorrectValue()
        {
            var blob = new FrameBlob(V10Header(3, new byte[20][]));
            Assert.Equal(3, blob.NodeCount);
        }

        [Fact]
        public void ColumnAccessors_ReadCorrectValues()
        {
            var cols = new byte[20][];
            cols[0] = U32(42);          // node_id
            cols[1] = I32(-1);          // parent_id
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

            var blob = new FrameBlob(V10Header(1, cols));

            Assert.Equal(42u, blob.NodeId(0));
            Assert.Equal(-1, blob.ParentId(0));
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
        public void IsPureTranslation_Identity_ReturnsTrue()
        {
            var cols = new byte[20][];
            cols[6] = F32(1f); cols[7] = F32(0f); cols[8] = F32(0f); cols[9] = F32(1f);
            var blob = new FrameBlob(V10Header(1, cols));
            Assert.True(blob.IsPureTranslation(0));
        }

        [Fact]
        public void IsPureTranslation_Rotated_ReturnsFalse()
        {
            var cols = new byte[20][];
            cols[6] = F32(0.7f); cols[7] = F32(0.7f); cols[8] = F32(-0.7f); cols[9] = F32(0.7f);
            var blob = new FrameBlob(V10Header(1, cols));
            Assert.False(blob.IsPureTranslation(0));
        }

        [Fact]
        public void ReadPath_IndexZero_ReturnsNull()
        {
            var blob = new FrameBlob(V10Header(0, new byte[20][]));
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

            var blob = new FrameBlob(V10Header(0, new byte[20][], pathTable: pathTable.ToArray()));
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
            var blob = new FrameBlob(V10Header(0, new byte[20][], pathTable: pathTable.ToArray()));
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

            var blob = new FrameBlob(V10Header(0, new byte[20][], clipTable: clipTable.ToArray()));
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

            var blob = new FrameBlob(V10Header(0, new byte[20][], clipTable: clipTable.ToArray()));
            Assert.False(blob.ClipRect(99, out _, out _, out _, out _, out _));
        }

        [Fact]
        public void MultiNode_EachHasDistinctValues()
        {
            var cols = new byte[20][];
            cols[0] = U32(100); // node 0: id=100
            var b0 = new List<byte>(); b0.AddRange(cols[0]); b0.AddRange(U32(200)); // node 1: id=200
            cols[0] = b0.ToArray();

            cols[4] = U32(10);  // node 0: sort=10
            var b4 = new List<byte>(); b4.AddRange(cols[4]); b4.AddRange(U32(20)); // node 1: sort=20
            cols[4] = b4.ToArray();

            var blob = new FrameBlob(V10Header(2, cols));
            Assert.Equal(100u, blob.NodeId(0));
            Assert.Equal(200u, blob.NodeId(1));
            Assert.Equal(10u, blob.SortKey(0));
            Assert.Equal(20u, blob.SortKey(1));
        }
    }
}
