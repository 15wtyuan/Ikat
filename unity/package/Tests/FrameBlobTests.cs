using System.Collections.Generic;
using NUnit.Framework;

namespace LoomGUI.Tests
{
    /// FrameBlob Visible/Parked bit accessor 单元测试（v12 blob，22 列 SOA）。
    /// 焦点：active 条目 bit0=1 bit1=0；parked keepalive 条目 bit0=0 bit1=1。
    public class FrameBlobVisibleParkedTests
    {
        /// 构造 v12 blob（22 列）。active 条目 + parked keepalive 条目。
        /// col_visible 数组填每节点的 visible 字节值：
        ///   0b01 = active（bit0=可见）
        ///   0b10 = parked（bit1=keepalive）
        static byte[] BuildBlobV12(byte[] colVisible)
        {
            int nodeCount = colVisible.Length;
            var b = new List<byte>();

            // header: magic, version, node_count
            b.AddRange(System.BitConverter.GetBytes(0x4D4F4F4Cu));
            b.AddRange(System.BitConverter.GetBytes(12u));
            b.AddRange(System.BitConverter.GetBytes((uint)nodeCount));

            // v12: 22 列 stride（bytes per entry）：col 0..21
            int[] stride = { 4, 4, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 1, 4, 4, 4, 1, 80, 1, 4, 128, 24 };
            // 22 col offsets（SOA），header 总长 124
            const int headerLen = 124;
            int off = headerLen;
            for (int i = 0; i < 22; i++)
            {
                b.AddRange(System.BitConverter.GetBytes((uint)off));
                off += stride[i] * nodeCount;
            }
            // arena headers: mesh/clip/path 全空
            b.AddRange(System.BitConverter.GetBytes(off));           // mesh_arena_off
            b.AddRange(System.BitConverter.GetBytes(0u));            // mesh_arena_len
            b.AddRange(System.BitConverter.GetBytes(off + 4u));      // clip_table_off
            b.AddRange(System.BitConverter.GetBytes(4u));            // clip_table_len (clip_count=0)
            b.AddRange(System.BitConverter.GetBytes(off + 4u + 4u)); // path_table_off
            b.AddRange(System.BitConverter.GetBytes(4u));            // path_table_len (path_count=0)

            // col 0: node_id (per-entry index)
            for (int i = 0; i < nodeCount; i++)
                b.AddRange(System.BitConverter.GetBytes((uint)(i + 1)));
            // col 1: parent_id
            for (int i = 0; i < nodeCount; i++)
                b.AddRange(System.BitConverter.GetBytes(i < 2 ? 0 : -1)); // active=rooted, parked=no parent
            // col 2: visible byte
            for (int i = 0; i < nodeCount; i++)
                b.Add(colVisible[i]);
            // cols 3-21: fill zero (MirrorPool won't read these in parked path)
            int[] strideTail = { 4, 4, 4, 4, 4, 4, 4, 4, 4, 1, 4, 4, 4, 1, 80, 1, 4, 128, 24 };
            foreach (int s in strideTail)
                for (int i = 0; i < nodeCount; i++)
                    for (int j = 0; j < s; j++)
                        b.Add(0);

            // clip_table: clip_count=0
            b.AddRange(System.BitConverter.GetBytes(0u));
            // path_table: path_count=0
            b.AddRange(System.BitConverter.GetBytes(0u));

            return b.ToArray();
        }

        [Test]
        public void ParkedBit_RoundTrips()
        {
            // 3 active (0b01) + 2 parked keepalive (0b10)
            var blob = new FrameBlob(BuildBlobV12(new byte[] {
                0b01, 0b01, 0b01,  // active
                0b10, 0b10         // parked
            }));

            Assert.That(blob.IsValid, Is.True, "v12 blob IsValid");
            Assert.That(blob.NodeCount, Is.EqualTo(5), "3 active + 2 parked = 5");

            for (int i = 0; i < 3; i++)
            {
                Assert.That(blob.Visible(i), Is.True,  $"active[{i}].Visible=true");
                Assert.That(blob.Parked(i),  Is.False, $"active[{i}].Parked=false");
            }

            for (int i = 3; i < 5; i++)
            {
                Assert.That(blob.Visible(i), Is.False, $"parked[{i}].Visible=false");
                Assert.That(blob.Parked(i),  Is.True,  $"parked[{i}].Parked=true");
            }
        }

        /// 纯 active blob（无 parked）：Visible 全 true，Parked 全 false。
        [Test]
        public void AllActive_VisibleAllTrue_ParkedAllFalse()
        {
            var blob = new FrameBlob(BuildBlobV12(new byte[] { 0b01, 0b01, 0b01, 0b01 }));
            Assert.That(blob.NodeCount, Is.EqualTo(4));

            for (int i = 0; i < 4; i++)
            {
                Assert.That(blob.Visible(i), Is.True);
                Assert.That(blob.Parked(i), Is.False);
            }
        }

        /// 全零 visible 字节（gone 条目）：Visible=false, Parked=false。
        [Test]
        public void ZeroVisibleByte_IsNotVisibleAndNotParked()
        {
            var blob = new FrameBlob(BuildBlobV12(new byte[] { 0x00 }));
            Assert.That(blob.NodeCount, Is.EqualTo(1));
            Assert.That(blob.Visible(0), Is.False);
            Assert.That(blob.Parked(0), Is.False);
        }
    }
}
