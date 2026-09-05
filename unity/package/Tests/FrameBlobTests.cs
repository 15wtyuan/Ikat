using System.Collections.Generic;
using NUnit.Framework;

namespace Yio.Tests
{
    /// FrameBlob v15 reader 单元测试（合成 blob：132B header + 21 lean 列 + skip 段）。
    /// 焦点：lean 行 Visible bit0；skip 条目 SkipParked bit1 / SkipNodeId / SkipReuseKey
    /// 双段解耦（lean 列只对前 LeanCount 行有效，skip 段独立计数）。
    public class FrameBlobVisibleParkedTests
    {
        /// 构造 v15 blob。colVisible = lean 行的 visible 字节（bit0 = 可见）；
        /// skip 段 = 每条 (node_id, reuse_key, flags)——flags bit1 = parked keepalive。
        static byte[] BuildBlobV15(byte[] colVisible, (ulong id, uint rk, byte flags)[] skips)
        {
            int lean = colVisible.Length;
            var b = new List<byte>();

            // header 132B：magic(4)+version(4)+node_count(4)+skip_count(4) + 21 col offsets(×4)
            // + mesh off/len + clip off/len + path off/len + fat off/len（8×4）。
            const int numCols = 21;
            // lean 列 stride（顺序 = blob.rs LEAN_COLUMNS；合计 84B/行）：
            // node_id8 parent8 visible1 alpha4 sort4 mask4 ma4 mb4 mc4 md4 mtx4 mty4
            // kind1 mesh_off4 mesh_len4 path_idx4 program1 change1 reuse4 mount8 fat4。
            int[] stride = { 8, 8, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 1, 4, 4, 4, 1, 1, 4, 8, 4 };
            int headerLen = 16 + numCols * 4 + 8 * 4;
            Assert.That(headerLen, Is.EqualTo(132), "v15 header 恒 132B（防布局漂移的锚）");

            b.AddRange(System.BitConverter.GetBytes(0x4D4F4F4Du)); // magic
            b.AddRange(System.BitConverter.GetBytes(15u));          // version
            b.AddRange(System.BitConverter.GetBytes((uint)(lean + skips.Length))); // node_count = lean + skip
            b.AddRange(System.BitConverter.GetBytes((uint)skips.Length));          // skip_count

            int off = headerLen;
            for (int i = 0; i < numCols; i++)
            {
                b.AddRange(System.BitConverter.GetBytes((uint)off));
                off += stride[i] * lean;
            }
            // 四 arena 顺序：mesh（空）→ clip（count=0）→ path（count=0）→ fat（空）。
            int meshOff = off, clipOff = off, pathOff = off + 4, fatOff = off + 8;
            b.AddRange(System.BitConverter.GetBytes((uint)meshOff));
            b.AddRange(System.BitConverter.GetBytes(0u));
            b.AddRange(System.BitConverter.GetBytes((uint)clipOff));
            b.AddRange(System.BitConverter.GetBytes(4u));
            b.AddRange(System.BitConverter.GetBytes((uint)pathOff));
            b.AddRange(System.BitConverter.GetBytes(4u));
            b.AddRange(System.BitConverter.GetBytes((uint)fatOff));
            b.AddRange(System.BitConverter.GetBytes(0u));

            // lean 列内容：node_id = i+1；visible = 入参；其余全零（reader 只测这两列）。
            for (int i = 0; i < lean; i++)
                b.AddRange(System.BitConverter.GetBytes((uint)(i + 1)));   // col0 node_id 低 4B
            for (int i = 0; i < lean; i++)
                b.AddRange(System.BitConverter.GetBytes(0u));               // col0 高 4B
            for (int i = 0; i < lean; i++)
                b.AddRange(System.BitConverter.GetBytes(0u));               // col1 parent_id 低 4B
            for (int i = 0; i < lean; i++)
                b.AddRange(System.BitConverter.GetBytes(0u));               // col1 高 4B
            for (int i = 0; i < lean; i++)
                b.Add(colVisible[i]);                                       // col2 visible
            int tailBytes = 84 - 8 - 8 - 1;                                 // 余 18 列全零
            for (int i = 0; i < tailBytes * lean; i++)
                b.Add(0);

            // clip 表（count=0）+ path 表（count=0）。
            b.AddRange(System.BitConverter.GetBytes(0u));
            b.AddRange(System.BitConverter.GetBytes(0u));

            // skip 段：每条 16B = node_id u64 + reuse_key u32 + flags u8 + pad3。
            foreach (var (id, rk, flags) in skips)
            {
                b.AddRange(System.BitConverter.GetBytes(id));
                b.AddRange(System.BitConverter.GetBytes(rk));
                b.Add(flags);
                b.Add(0); b.Add(0); b.Add(0);
            }
            return b.ToArray();
        }

        [Test]
        public void VisibleBit_And_SkipSegment_RoundTrip()
        {
            // 2 可见 lean 行 + 1 隐藏 lean 行；skip 段 1 parked + 1 普通 keepalive。
            var blob = new FrameBlob(BuildBlobV15(
                new byte[] { 0b1, 0b1, 0b0 },
                new[] { (0x7777_0001_0000_0002ul, 5u, (byte)0b10), (0x7777_0001_0000_0003ul, 0u, (byte)0) }));

            Assert.That(blob.IsValid, Is.True, "v15 blob IsValid");
            Assert.That(blob.NodeCount, Is.EqualTo(5), "node_count = lean + skip");
            Assert.That(blob.LeanCount, Is.EqualTo(3), "LeanCount 剥离 skip 段");
            Assert.That(blob.SkipCount, Is.EqualTo(2));

            Assert.That(blob.Visible(0), Is.True);
            Assert.That(blob.Visible(1), Is.True);
            Assert.That(blob.Visible(2), Is.False, "隐藏行（世界锚点出屏）visible=0");

            Assert.That(blob.SkipNodeId(0), Is.EqualTo(0x7777_0001_0000_0002ul));
            Assert.That(blob.SkipReuseKey(0), Is.EqualTo(5u));
            Assert.That(blob.SkipParked(0), Is.True, "skip flags bit1 = parked keepalive");
            Assert.That(blob.SkipParked(1), Is.False, "普通 keepalive 条目 bit1=0");
            Assert.That(blob.SkipNodeId(1), Is.EqualTo(0x7777_0001_0000_0003ul));
        }

        /// 旧 Parked(i) 访问器在 v15 恒 false（parked 语义移居 SkipParked）——锁死兼容语义。
        [Test]
        public void ParkedLegacyAccessor_IsAlwaysFalseInV15()
        {
            var blob = new FrameBlob(BuildBlobV15(
                new byte[] { 0b1 }, new[] { (9ul, 3u, (byte)0b10) }));
            Assert.That(blob.Parked(0), Is.False);
        }

        [Test]
        public void WrongVersion_IsInvalid()
        {
            var raw = BuildBlobV15(new byte[] { 0b1 }, new (ulong, uint, byte)[0]);
            raw[4] = 14; // 旧版本号
            Assert.That(new FrameBlob(raw).IsValid, Is.False, "版本不匹配必须拒收");
        }
    }
}
