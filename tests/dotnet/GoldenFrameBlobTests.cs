using System;
using System.IO;
using LoomGUI;
using Xunit;

namespace LoomGUI.Tests.Core
{
    /// <summary>
    /// golden 帧 blob 跨语言对拍：Rust 产器（crates/ffi golden_tests.rs 固定场景）落盘的
    /// 真实 blob，用 C# <see cref="FrameBlob"/> 镜像逐列断言语义——锁「Rust 写的字节 ↔
    /// C# 镜像布局」一致。此前 C# 侧零断言：magic+version 防整体漂移，防不住列语义错位
    /// （列序对调仍 v13 通过）。
    ///
    /// 断言分两层：全列结构断言（每节点每列值域/相互一致性）+ 场景已知量断言（golden
    /// 场景刻意覆盖 text/img/gradient/shadow/opacity/transform/clip——对应列必出现非默认值）。
    /// golden 再生成：`LOOMGUI_UPDATE_GOLDEN=1 cargo test -p loomgui_ffi_c --lib golden`。
    /// </summary>
    public class GoldenFrameBlobTests
    {
        static byte[] LoadGolden(string name) =>
            File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "golden", name));

        static bool Finite(float f) => !float.IsNaN(f) && !float.IsInfinity(f);

        [Fact]
        public void EveryColumnIsSane_AndSceneFeaturesPresent()
        {
            var blob = new FrameBlob(LoadGolden("frame-blob.bin"));
            Assert.True(blob.IsValid,
                $"golden blob 版本 {blob.Version} 与 C# ExpectedVersion 不符——Rust 侧 blob 布局变了：" +
                "再生成 golden（LOOMGUI_UPDATE_GOLDEN=1 cargo test -p loomgui_ffi_c --lib golden）并同步 FrameBlob.cs 列注释");
            Assert.Equal(FrameBlob.ExpectedVersion, blob.Version);

            int n = blob.NodeCount;
            Assert.True(n >= 10, $"golden 场景应有 ≥10 节点（root+8 子+clip 内层），实际 {n}");

            bool sawImage = false, sawText = false, sawGradient = false, sawShadow = false,
                 sawAlphaBelowOne = false, sawNonPureTranslation = false, sawFullMesh = false;

            for (int i = 0; i < n; i++)
            {
                // node_id：slotmap 句柄（1 基 idx），非 0/非全 F。
                Assert.NotEqual(0ul, blob.NodeId(i));
                Assert.NotEqual(ulong.MaxValue, blob.NodeId(i));   // #26 u64 INVALID
                if (blob.ParentId(i) != -1)
                {
                    Assert.NotEqual(0, blob.ParentId(i));
                    Assert.NotEqual(long.MinValue, blob.ParentId(i));   // #26 i64
                }
                // visible 字节：bit0 渲染 / bit1 parked keepalive，至少一位置位。
                Assert.True(blob.Visible(i) || blob.Parked(i), $"node {i} visible 字节全 0");

                float alpha = blob.Alpha(i);
                Assert.InRange(alpha, 0f, 1f);
                if (alpha < 1f) sawAlphaBelowOne = true;

                // world 矩阵 6 列全有限；场景含带缩放 transform → 至少一节点非纯平移。
                Assert.True(Finite(blob.Ma(i)) && Finite(blob.Mb(i)) && Finite(blob.Mc(i))
                    && Finite(blob.Md(i)) && Finite(blob.Mtx(i)) && Finite(blob.Mty(i)),
                    $"node {i} world 矩阵列非有限值（列错位典型症状）");
                if (!blob.IsPureTranslation(i)) sawNonPureTranslation = true;

                // v10 起所有渲染节点统一 mesh 路径：payload_kind 恒 1。
                Assert.Equal(1, blob.PayloadKind(i));
                Assert.InRange(blob.Program(i), 0, 7);
                Assert.InRange(blob.ChangeLevel(i), 0, 2);

                // Full 节点必有 mesh 且 arena 可解析出非空 mesh。
                if (blob.ChangeLevel(i) == 2 && blob.ReadMeshLenRaw(i) > 0)
                {
                    var seg = blob.ReadMesh(i);
                    Assert.True(seg.Verts.Length >= 3, $"node {i} Full 但 mesh 顶点 < 3");
                    Assert.True(seg.Idx.Length >= 3, $"node {i} Full 但 mesh 索引 < 3");
                    sawFullMesh = true;
                }

                // path_idx 1-based 索引 path 表，0=纯色无图。
                uint pathIdx = blob.PathIdx(i);
                Assert.InRange(pathIdx, 0u, (uint)blob.PathCount);
                if (pathIdx > 0) sawImage = true;
                if (blob.Program(i) == 1 && blob.ReadMeshLenRaw(i) > 0) sawText = true;

                // 渐变节点（program 6/7）：grad_params 非默认且结构合法。
                if (blob.Program(i) == 6 || blob.Program(i) == 7)
                {
                    float[] gp = blob.GradParams(i);
                    Assert.True(gp[0] == 0f || gp[0] == 1f, "grad kind ∈ {0=linear,1=radial}");
                    Assert.InRange(gp[10], 1f, 8f); // stop_count ≤ 8（FFI 定长 8 槽）
                    Assert.Equal(0f, gp[11]);       // reserved 恒 0
                    for (int s = 0; s < (int)gp[10]; s++)
                    {
                        for (int c = 0; c < 4; c++)
                            Assert.InRange(gp[12 + s * 5 + c], 0f, 1f); // stop RGBA
                        Assert.InRange(gp[12 + s * 5 + 4], 0f, 1f);     // stop pos
                    }
                    sawGradient = true;
                }

                // box-shadow blur 节点（program 5）：shadow_params halfSize > 0。
                if (blob.Program(i) == 5)
                {
                    float[] sp = blob.ShadowParams(i);
                    Assert.True(sp[0] > 0f && sp[1] > 0f, "shadow halfSize.xy > 0");
                    Assert.True(sp[3] >= 0f, "shadow σ ≥ 0");
                    sawShadow = true;
                }

                foreach (float f in blob.ColorMatrix(i)) Assert.True(Finite(f));
                foreach (float f in blob.EffectBlock(i)) Assert.True(Finite(f));
            }

            // 场景已知量：golden 场景刻意构造的特征必须真实出现在对应列。
            Assert.True(sawFullMesh, "应有 Full 变更级节点带 mesh");
            Assert.True(sawText, "文本节点（program=1 + mesh）缺席");
            Assert.True(sawImage, "图片节点（path_idx>0）缺席");
            Assert.True(blob.PathCount >= 2, "path 表应含字体图集页 + 图片路径");
            bool sawStarPath = false, sawFontAtlas = false;
            for (uint p = 1; p <= (uint)blob.PathCount; p++)
            {
                string path = blob.ReadPath(p);
                Assert.False(string.IsNullOrEmpty(path), $"path 表第 {p} 条空串");
                if (path == "icons/star.png") sawStarPath = true;
                if (path.StartsWith("loomgui://font-atlas/", StringComparison.Ordinal)) sawFontAtlas = true;
            }
            Assert.True(sawStarPath, "图片 path（icons/star.png）缺席");
            Assert.True(sawFontAtlas, "文本字形字体图集 path 缺席");
            Assert.True(sawGradient, "渐变节点（program=6/7）缺席");
            Assert.True(sawShadow, "box-shadow blur 节点（program=5）缺席");
            Assert.True(sawAlphaBelowOne, "opacity<1 节点缺席（alpha 列未覆盖）");
            Assert.True(sawNonPureTranslation, "带缩放 transform 节点缺席（矩阵列未覆盖）");
            Assert.True(blob.ClipCount >= 1, "overflow:hidden 裁剪节点缺席（clip 表未覆盖）");
        }

        [Fact]
        public void ClipTableEntry_DecodesDesignRect()
        {
            var blob = new FrameBlob(LoadGolden("frame-blob.bin"));
            Assert.True(blob.IsValid);
            // golden 场景的 clipper：500×60 overflow:hidden —— 找到任一 clip entry，
            // 断言 rect 数值落在场景尺寸内且 w/h 与声明宽度一致量级（52B entry 布局锁）。
            bool found = false;
            for (int i = 0; i < blob.NodeCount && !found; i++)
            {
                uint ctx = blob.MaskContext(i);
                if (ctx == 0) continue;
                if (blob.ClipRect(ctx, out float x, out float y, out float w, out float h, out float r))
                {
                    Assert.True(Finite(x) && Finite(y) && Finite(w) && Finite(h) && Finite(r));
                    Assert.InRange(x, 0f, 800f);
                    Assert.InRange(y, 0f, 600f);
                    Assert.InRange(w, 0f, 800f);
                    Assert.InRange(h, 0f, 600f);
                    found = true;
                }
            }
            Assert.True(found, "应存在 mask_context>0 且可从 clip 表解码 rect 的节点");
        }
    }
}
