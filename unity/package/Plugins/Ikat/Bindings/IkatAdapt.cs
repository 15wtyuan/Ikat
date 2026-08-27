using System.Runtime.InteropServices;

namespace Ikat.Bindings
{
    /// 手补 C# 镜像（csbindgen 不为 use-imported 的 Rust #[repr(C)] struct 生成 stub
    /// ——只扫描 lib.rs 内 #[no_mangle] fn 签名，不追 use 路径；同 IkatKeyEvent.cs 模式）。
    /// AdaptResult 被 ikat_compute_adaptation 签名引用但无 stub → 手动补。
    /// #[repr(C)] 5 × f32 = 20B，字段序须与 Rust ikat_core::adapt::AdaptResult 一致。
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    internal unsafe partial struct AdaptResult
    {
        public float scale;
        public float root_w;
        public float root_h;
        public float offset_x;
        public float offset_y;
    }

    /// 适配模式常量（Rust AdaptMode u32 ABI：只增不改）。
    /// 字符串形态（ikat.workspace.json / ikat.runtime.json 的 match_mode）：
    /// letterbox | fit-width | fit-height。
    /// </summary>
    internal static class IkatAdaptMode
    {
        public const uint Letterbox = 0;
        public const uint FitWidth = 1;
        public const uint FitHeight = 2;

        /// manifest 字符串 → u32。null/未知 → null（调用方落 Letterbox 默认）。
        public static uint? FromString(string s) => s switch
        {
            "letterbox" => Letterbox,
            "fit-width" => FitWidth,
            "fit-height" => FitHeight,
            _ => null,
        };
    }
}
