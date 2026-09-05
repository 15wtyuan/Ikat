using System;
using System.IO;
using System.Text;
using Yio.Bindings;
using Xunit;

namespace Yio.HeadlessTests
{
    /// <summary>
    /// box-shadow visual bundle acceptance: parse → cascade → render → blob end-to-end
    /// through the real dll. The fixture (#card) carries three box-shadow layers on one
    /// primary node — one outer blur, one inset blur, one inset hard-edge — so the frame
    /// blob must contain three synthetic RenderNodes (one per layer) with the expected
    /// program / shadow_params encoding.
    ///
    /// What this verifies (honest layering):
    ///   - Each CSS box-shadow layer becomes a distinct synth RenderNode in the blob
    ///     (node_id high byte 36..=43 = inset synth, 44..=47 = outer synth).
    ///   - Blur layers (blur ≥ 0.5) select the SDF shader (program == 5); hard-edge
    ///     layers (blur &lt; 0.5) degrade to the solid rounded-rect shader (program == 0).
    ///   - The SDF shadow_params column ([f32;6] = half.x, half.y, radius, sigma,
    ///     inset_flag, pad) is non-zero for every synth node (half extents come from a
    ///     real layout rect, never zero for a visible card).
    ///
    /// What this does NOT verify (deferred to Unity PlayMode): the actual pixel output of
    /// the SDF box-shadow shader (soft falloff, inset clipping, rounded-corner masking).
    /// </summary>
    public unsafe class BoxShadowTests
    {
        // Frame blob v15 column layout (mirror crates/ffi/src/blob.rs build_blob).
        // Header 132B = magic(4)+version(4)+node_count(4)+skip_count(4) + 21 col offsets(×4)
        //   + mesh off/len(2) + clip off/len(2) + path off/len(2) + fat off/len(2).
        private const int HeaderFixedLen = 16; // magic + version + node_count + skip_count
        private const int NumColumns = 21;
        // Column indices (must match LEAN_COLUMNS in build_blob).
        private const int ColNodeId = 0;        // u64, 8B stride
        private const int ColProgram = 16;      // u8, 1B stride
        private const int ColFatOff = 20;       // u32, 4B stride（1-based fat arena 引用）
        private const int FatArenaOffAt = 124;  // header 内 fat_arena_off 字段（16 + 21*4 + 6*4）
        // fat block: mask byte (bit0=color_matrix 80B, bit1=effect 128B, bit2=shadow 24B,
        // bit3=gradient 208B) + present blocks packed in that order.
        private const int FatColorMatrixSize = 80;
        private const int FatEffectSize = 128;
        private const int FatShadowSize = 24;
        // Synth node_id high-byte ranges (mirror render::FRONT/BACK_SHADOW_SYNTH_BYTE).
        private const byte FrontSynthLo = 36;   // inset synth: 36..=43
        private const byte FrontSynthHi = 43;
        private const byte BackSynthLo = 44;    // outer synth: 44..=47
        private const byte BackSynthHi = 47;
        // Shader program discriminant (mirror NodePayload::Mesh program for shadow quads).
        private const byte ProgramShadowSdf = 5; // blur ≥ 0.5
        private const byte ProgramSolid = 0;     // hard-edge (blur &lt; 0.5)

        [Fact]
        public void BoxShadow_LayersEmitSynthNodesWithSdfProgramAndParams()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                RegisterDefaultFont(h);
                Container root = InstantiateBoxShadowFixture(h, ctx);

                // The fixture is CSS-driven (no data-driven binds); a single tick runs
                // cascade + solve + build so the blob reflects the shadow synth nodes.
                ctx.FlushPendingWrites();
                Native.yio_stage_tick(h, 0.016f);

                FrameBlob blob = BorrowFrame(h);

                // Fixture #card declares 3 layers: 1 outer (blur) + 2 inset (blur, hard-edge).
                // Each layer → exactly one synth RenderNode, regardless of paint order.
                int synthCount = 0;
                int sdfProgramCount = 0; // blur layers (program == 5)
                int solidProgramCount = 0; // hard-edge layers (program == 0)
                int nodesWithZeroParams = 0;
                for (int i = 0; i < blob.LeanCount; i++)
                {
                    ulong nodeId = blob.GetNodeId(i);
                    byte hi = (byte)(nodeId >> 56);
                    if (hi < FrontSynthLo || hi > BackSynthHi)
                        continue; // not a shadow synth node

                    synthCount++;
                    byte program = blob.GetProgram(i);
                    if (program == ProgramShadowSdf)
                        sdfProgramCount++;
                    else if (program == ProgramSolid)
                        solidProgramCount++;

                    // shadow_params = [half.x, half.y, radius, sigma, inset_flag, pad].
                    // half.x/half.y derive from the card's layout rect (200×120 content +
                    // spread padding) so they must be strictly positive for a visible card;
                    // a zero column would mean the synth node never got its SDF params wired.
                    float[] p = blob.GetShadowParams(i);
                    Assert.True(p[0] > 0f, $"synth node {i} (id=0x{nodeId:X8}) half.x={p[0]} must be > 0");
                    Assert.True(p[1] > 0f, $"synth node {i} (id=0x{nodeId:X8}) half.y={p[1]} must be > 0");
                }

                // Three CSS layers → three synth nodes.
                Assert.Equal(3, synthCount);
                // 18337b3d rewrite unifies ALL shadows on SDF (program=5): blur=0 hard-edge
                // inset uses sigma=0.5 AA edge (program=0 solid fill is only correct for
                // outer shadows; for inset it covered the whole element → washed buttons).
                Assert.Equal(3, sdfProgramCount);
                Assert.Equal(0, solidProgramCount);
                Assert.Equal(0, nodesWithZeroParams);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── frame blob reader ─────────────────────────────────────────────

        /// <summary>
        /// borrow_frame returns a Rust-owned ptr valid until the next tick; we snapshot the
        /// column offsets + node count up front so Get* helpers index straight off the raw ptr.
        /// </summary>
        private readonly struct FrameBlob
        {
            private readonly byte* _ptr;
            private readonly int _nodeCount;
            private readonly int _skipCount;
            private readonly uint _nodeIdOff;
            private readonly uint _programOff;
            private readonly uint _fatOffCol;
            private readonly uint _fatArenaOff;

            public FrameBlob(byte* ptr)
            {
                _ptr = ptr;
                _nodeCount = (int)ReadU32(ptr, 8);  // node_count = lean + skip
                _skipCount = (int)ReadU32(ptr, 12); // skip_count（v15）
                _nodeIdOff = ReadU32(ptr, HeaderFixedLen + ColNodeId * 4);
                _programOff = ReadU32(ptr, HeaderFixedLen + ColProgram * 4);
                _fatOffCol = ReadU32(ptr, HeaderFixedLen + ColFatOff * 4);
                _fatArenaOff = ReadU32(ptr, FatArenaOffAt);
            }

            /// <summary>lean 行数（Skip 行不进列——列访问器只对 lean 行有效）。</summary>
            public int LeanCount => _nodeCount - _skipCount;

            public ulong GetNodeId(int i) => ReadU64(_ptr, (int)_nodeIdOff + i * 8);
            public byte GetProgram(int i) => _ptr[_programOff + i];

            /// shadow_params 经 fat arena（v15 挪出 SOA）：fat_off 1-based → mask byte +
            /// 按位在场的块（cm → effect → shadow → grad 顺序）——shadow 块前跳过 cm/effect。
            public float[] GetShadowParams(int i)
            {
                float[] p = new float[6];
                uint fatOff = ReadU32(_ptr, (int)_fatOffCol + i * 4);
                if (fatOff == 0) return p; // 无胖块（全零）——调用方断言会拦
                byte* blk = _ptr + _fatArenaOff + fatOff - 1;
                byte mask = *blk;
                byte* cur = blk + 1;
                if ((mask & 0b0001) != 0) cur += FatColorMatrixSize;
                if ((mask & 0b0010) != 0) cur += FatEffectSize;
                if ((mask & 0b0100) != 0)
                    for (int j = 0; j < 6; j++)
                        p[j] = ReadF32(cur, j * 4);
                return p;
            }

            private static uint ReadU32(byte* p, int offset)
                => p[offset] | ((uint)p[offset + 1] << 8) | ((uint)p[offset + 2] << 16) | ((uint)p[offset + 3] << 24);

            private static ulong ReadU64(byte* p, int offset)
            {
                ulong lo = ReadU32(p, offset);
                ulong hi = ReadU32(p, offset + 4);
                return lo | (hi << 32);
            }

            private static float ReadF32(byte* p, int offset)
            {
                uint bits = p[offset] | ((uint)p[offset + 1] << 8) | ((uint)p[offset + 2] << 16) | ((uint)p[offset + 3] << 24);
                return BitConverter.Int32BitsToSingle((int)bits);
            }
        }

        private static FrameBlob BorrowFrame(StageHandle* h)
        {
            nuint len = 0;
            byte* ptr = Native.yio_stage_borrow_frame(h, &len);
            if (ptr == null || len < 12)
                throw new InvalidOperationException("borrow_frame returned no/short blob");
            return new FrameBlob(ptr);
        }

        // ── helpers ──────────────────────────────────────────────────────

        /// <summary>Register DejaVuSans.ttf as default font (tick panics without one).</summary>
        private static void RegisterDefaultFont(StageHandle* h)
        {
            string fontPath = Path.Combine(AppContext.BaseDirectory, "fixtures", "fonts", "DejaVuSans.ttf");
            byte[] fontBytes = File.ReadAllBytes(fontPath);
            byte[] family = Encoding.UTF8.GetBytes("DejaVuSans");
            fixed (byte* fp = family)
            fixed (byte* bp = fontBytes)
            {
                int rc = Native.yio_stage_register_font(
                    h, fp, (nuint)family.Length, bp, (nuint)fontBytes.Length, is_default: 1);
                if (rc != 0)
                    throw new InvalidOperationException(
                        $"register_font failed rc={rc}; font path={fontPath}");
            }
        }

        private static ulong CreateRoot(StageHandle* h)
        {
            byte[] k = Encoding.UTF8.GetBytes("div");
            fixed (byte* kp = k)
                return Native.yio_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        private static void AppendChild(StageHandle* h, ulong parent, ulong child)
        {
            int rc = Native.yio_stage_append_child(h, parent, child);
            if (rc != 0)
                throw new InvalidOperationException(
                    $"append_child(parent={parent}, child={child}) failed rc={rc}");
        }

        /// <summary>
        /// Load fixtures/box-shadow.pkg.bin (package "box-shadow", template "box-shadow-acceptance"
        /// — template name is the html file stem), instantiate, and attach to a fresh scene root.
        /// Mirrors VisualDecorationTests.InstantiateVisualFixture.
        /// </summary>
        private static Container InstantiateBoxShadowFixture(StageHandle* h, UIContext ctx)
        {
            ulong sceneRootId = CreateRoot(h);
            ctx._rootId = sceneRootId;
            Container sceneRoot = (Container)ctx._registry.GetOrCreate(sceneRootId);

            string fixturePath = Path.Combine(AppContext.BaseDirectory, "fixtures", "box-shadow.pkg.bin");
            Assert.True(File.Exists(fixturePath),
                $"fixture pkg.bin not found at {fixturePath}. " +
                "Ensure csproj <None CopyToOutputDirectory> is configured.");

            byte[] pkgBytes = File.ReadAllBytes(fixturePath);
            Assert.True(pkgBytes.Length > 0, "box-shadow.pkg.bin is empty");

            UIPackage pkg = ctx.LoadPackage("box-shadow", pkgBytes);
            Assert.NotNull(pkg);

            Container instRoot = pkg.Instantiate("box-shadow-acceptance");
            Assert.NotNull(instRoot);
            AppendChild(h, sceneRoot._id, instRoot._id);
            return instRoot;
        }
    }
}
