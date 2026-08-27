using System;
using System.IO;
using System.Runtime.InteropServices;
using Ikat.Bindings;
using Xunit;

namespace Ikat.HeadlessTests
{
    /// <summary>
    /// 跨语言 ABI 镜像锁（golden 对拍 + 尺寸断言）：
    ///
    /// ① 事件流 golden——Rust 产器（crates/ffi golden_tests.rs）落盘的真实 EventRecord[]
    /// 字节，按 C# RawEventRecord 位布局解析断言。此前位复用编码（#26 u64 后 node_id@0(8) /
    /// type@8 / click_count@9 / touch_id@12 / x@16 / y@20）纯靠双侧注释对齐，magic+version 防不住
    /// 字段错位。golden 再生成：IKATGUI_UPDATE_GOLDEN=1 cargo test -p ikat_ffi_c --lib golden。
    ///
    /// ② 手补 #[repr(C)] struct 镜像的尺寸锁——csbindgen 不为 use-imported 的 struct 生成
    /// stub，PointerEvent/KeyEvent/WheelEvent 是手写镜像（Rust 侧 abi_tests 锁 Rust 端
    /// size_of，C# 端此前零断言；一端改布局另一端不知 = 静默错读）。
    /// </summary>
    public unsafe class GoldenEventsAndAbiLayoutTests
    {
        static byte[] LoadGolden(string name) =>
            File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "golden", name));

        /// <summary>
        /// golden 事件流 = 指针 Down+Up @(150,120) 的最终 tick 事件：Up + Click（Click 的
        /// click_count=1、坐标、命中 node_id 全进字节）。字段偏移错位 → 解析值立即露馅。
        /// </summary>
        [Fact]
        public void EventsGolden_MatchesRawEventRecordBitLayout()
        {
            byte[] bytes = LoadGolden("events.bin");
            const int REC = 32; // RawEventRecord 32B（#26 node_id u64；下方尺寸锁同断言）
            Assert.True(bytes.Length >= REC && bytes.Length % REC == 0,
                $"events golden 须为非空 32B 记录数组，实际 {bytes.Length}B");
            var recs = MemoryMarshal.Cast<byte, RawEventRecord>(bytes.AsSpan()).ToArray();
            Assert.True(recs.Length >= 2, "Down+Up 应至少产 Up+Click 两条事件");

            Assert.Equal((byte)EventType.Up, recs[0].eventType);
            Assert.Equal((byte)EventType.Click, recs[1].eventType);
            Assert.Equal(1, recs[1].clickCount);   // Down+Up 同点 = 单击
            for (int i = 0; i < recs.Length; i++)
            {
                Assert.NotEqual(ulong.MaxValue, recs[i].nodeId);
                Assert.Equal(-1, recs[i].touchId); // 鼠标主指
                Assert.Equal(150f, recs[i].x, 3);  // 发送坐标透传
                Assert.Equal(120f, recs[i].y, 3);
            }
            // 两条事件命中同一节点（Down/Up 同点）。
            Assert.Equal(recs[0].nodeId, recs[1].nodeId);
            // 非 DragMove 事件的增量槽恒 0（#63：dx/dy 仅 DragMove 有意义）。
            for (int i = 0; i < recs.Length; i++)
            {
                Assert.Equal(0f, recs[i].dx, 3);
                Assert.Equal(0f, recs[i].dy, 3);
            }
        }

        /// <summary>
        /// 手补镜像 struct 尺寸 = Rust repr(C) size_of（abi_tests 锁的 Rust 端值）。
        /// 一端加字段/改 pad → 双侧尺寸失配即红。
        /// </summary>
        [Fact]
        public void HandMirroredStructSizes_MatchRustAbi()
        {
            Assert.Equal(16, sizeof(PointerEvent)); // kind+button+pad[2]+touch_id+x+y
            Assert.Equal(8, sizeof(KeyEvent));      // key_code+modifiers+is_down+pad[2]
            Assert.Equal(16, sizeof(WheelEvent));   // x+y+delta_x+delta_y
            Assert.Equal(32, sizeof(RawEventRecord)); // node_id(u64)+type+count+pad[2]+touch_id+x+y+dx+dy（#26）
        }
    }
}
