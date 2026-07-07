using System;
using System.Collections.Generic;
using LoomGUI.Editor;
using Xunit;

namespace LoomGUI.Tests.Core
{
    public class PkgManifestReaderTests
    {
        [Fact]
        public void ReadAssetManifest_NullBuffer_Throws()
        {
            Assert.Throws<PkgManifestReader.PkgManifestException>(() =>
                PkgManifestReader.ReadAssetManifest(null));
        }

        [Fact]
        public void ReadAssetManifest_TooShort_Throws()
        {
            Assert.Throws<PkgManifestReader.PkgManifestException>(() =>
                PkgManifestReader.ReadAssetManifest(new byte[10]));
        }

        [Fact]
        public void ReadAssetManifest_BadMagic_Throws()
        {
            var b = new byte[20];
            Assert.Throws<PkgManifestReader.PkgManifestException>(() =>
                PkgManifestReader.ReadAssetManifest(b));
        }

        [Fact]
        public void ReadAssetManifest_WrongVersion_Throws()
        {
            var b = new byte[20];
            BitConverter.GetBytes(PkgManifestReader.PKG_MAGIC).CopyTo(b, 0);
            BitConverter.GetBytes(99u).CopyTo(b, 4);
            Assert.Throws<PkgManifestReader.PkgManifestException>(() =>
                PkgManifestReader.ReadAssetManifest(b));
        }

        [Fact]
        public void ReadAssetManifest_EmptyManifest_ReturnsEmptyList()
        {
            var b = new List<byte>();
            // header
            b.AddRange(BitConverter.GetBytes(PkgManifestReader.PKG_MAGIC));
            b.AddRange(BitConverter.GetBytes(PkgManifestReader.PKG_VERSION));
            b.AddRange(BitConverter.GetBytes(0u)); // flags
            b.AddRange(BitConverter.GetBytes(0u)); // component_count = 0
            b.AddRange(BitConverter.GetBytes(0u)); // string_count = 0
            // asset manifest: entry_count = 0
            b.AddRange(BitConverter.GetBytes(0u));

            var result = PkgManifestReader.ReadAssetManifest(b.ToArray());
            Assert.NotNull(result);
            Assert.Empty(result);
        }

        [Fact]
        public void ReadAssetManifest_WithEntries_ReturnsCorrectPaths()
        {
            // 构造一个最小 pkg.bin：空 scene（2 个组件各 0 节点）+ 2 个 manifest entry
            var b = new List<byte>();

            // header
            b.AddRange(BitConverter.GetBytes(PkgManifestReader.PKG_MAGIC));
            b.AddRange(BitConverter.GetBytes(PkgManifestReader.PKG_VERSION));
            b.AddRange(BitConverter.GetBytes(0u)); // flags
            b.AddRange(BitConverter.GetBytes(2u)); // component_count = 2
            b.AddRange(BitConverter.GetBytes(4u)); // string_count = 4

            // string table (4 strings)
            AddString(b, "compA");
            AddString(b, "compB");
            AddString(b, "res/a.png");
            AddString(b, "res/b.png");

            // component table: 2 × {name_idx(u16) + root_node_idx(u32) + node_count(u32) + dynamic_len(u32)}
            // comp 0: name=0, 0 nodes, 0 dynamic
            b.AddRange(BitConverter.GetBytes((ushort)0)); b.AddRange(BitConverter.GetBytes(0u));
            b.AddRange(BitConverter.GetBytes(0u)); b.AddRange(BitConverter.GetBytes(0u));
            // comp 1: name=1, 0 nodes, 0 dynamic
            b.AddRange(BitConverter.GetBytes((ushort)1)); b.AddRange(BitConverter.GetBytes(0u));
            b.AddRange(BitConverter.GetBytes(0u)); b.AddRange(BitConverter.GetBytes(0u));

            // asset manifest: 2 entries
            b.AddRange(BitConverter.GetBytes(2u));
            // entry 0: path_idx=2, w=100, h=200
            b.AddRange(BitConverter.GetBytes((ushort)2)); b.AddRange(BitConverter.GetBytes(100u)); b.AddRange(BitConverter.GetBytes(200u));
            // entry 1: path_idx=3, w=300, h=400
            b.AddRange(BitConverter.GetBytes((ushort)3)); b.AddRange(BitConverter.GetBytes(300u)); b.AddRange(BitConverter.GetBytes(400u));

            var result = PkgManifestReader.ReadAssetManifest(b.ToArray());
            Assert.Equal(2, result.Count);
            Assert.Equal("res/a.png", result[0].path);
            Assert.Equal(100u, result[0].w);
            Assert.Equal(200u, result[0].h);
            Assert.Equal("res/b.png", result[1].path);
            Assert.Equal(300u, result[1].w);
            Assert.Equal(400u, result[1].h);
        }

        static void AddString(List<byte> b, string s)
        {
            var utf8 = System.Text.Encoding.UTF8.GetBytes(s);
            b.AddRange(BitConverter.GetBytes((ushort)utf8.Length));
            b.AddRange(utf8);
        }
    }
}
