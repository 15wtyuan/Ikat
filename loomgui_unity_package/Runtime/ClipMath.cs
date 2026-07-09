using UnityEngine;

namespace LoomGUI
{
    /// _ClipBox 推导。
    ///
    /// 给 design-space clip rect（绝对，y-down）+ 根 Stage transform：把两角经
    /// root.TransformPoint 转到 world（root scale=(sf,-sf,sf) y-down→y-up），取 world
    /// center/half，按公式 `_ClipBox = (-cx/hw, -cy/hh, 1/hw, 1/hh)` 算。
    /// 半宽/高为 0（嵌套 disjoint→空集）→ safe-blank (-2,-2,0,0)：clipPos 恒 (-2,-2)，
    /// max(abs)=2>1 → step(2,1)=0 → 全 discard（防除零）。
    ///
    /// shader 端（LoomGUI-Unlit.shader CLIPPED variant）：
    ///   clipPos = TransformObjectToWorld(pos).xy * _ClipBox.zw + _ClipBox.xy
    ///   col.a *= step(max(abs(clipPos)), 1)
    /// 代入：clipPos = (worldPos.x/hw - cx/hw, worldPos.y/hh - cy/hh) = (worldPos - center)/half。
    /// 区域内 → |clipPos|<=1（保留），外 → >1（discard）。
    public static class ClipMath
    {
        /// safe-blank：half=0 时返回，clipPos 恒在外 → 全裁。
        public static readonly Vector4 SafeBlank = new Vector4(-2f, -2f, 0f, 0f);

        /// 由 design rect + 根 transform 算 _ClipBox（world 空间 center/half）。
        /// designX/Y/W/H 是绝对 design 坐标（layout 已算绝对）。
        public static Vector4 ComputeClipBox(Transform root,
            float designX, float designY, float designW, float designH)
        {
            // 两角 design → world。root.TransformPoint 统一处理 scale(1,-1,1)+pos。
            Vector3 wTL = root.TransformPoint(new Vector3(designX, designY, 0f));
            Vector3 wBR = root.TransformPoint(new Vector3(designX + designW, designY + designH, 0f));

            float cx = (wTL.x + wBR.x) * 0.5f;
            float cy = (wTL.y + wBR.y) * 0.5f;
            float hw = Mathf.Abs(wBR.x - wTL.x) * 0.5f;
            float hh = Mathf.Abs(wBR.y - wTL.y) * 0.5f;

            if (hw == 0f || hh == 0f) return SafeBlank;
            return new Vector4(-cx / hw, -cy / hh, 1f / hw, 1f / hh);
        }

        /// 把 design-space 圆角半径归一化到 shader clipPos 空间（|x|,|y|<=1 在内）。
        ///
        /// shader SDF 在 clipPos 归一化空间计算（clipPos = worldPos * _ClipBox.zw + _ClipBox.xy，
        /// 区域内 |clipPos|<=1）。design 半径 r_design 须除以 half_size 转归一化：
        ///   r_norm = r_design / half_size。
        /// 非方形 rect（hw≠hh）下 SDF 的 q=abs(clipPos)-1+r 在两轴归一化不同——取 min(hw,hh)
        /// 归一化让 SDF 圆角保持圆形（非椭圆），视觉最接近 CSS border-radius。
        /// 与 ComputeClipBox 同根 transform 重新算 half_size（_ClipBox.zw = 1/hw, 1/hh 可用，
        /// 但 hw/hh 经 root scale 后是 world 单位；design 半径也是 design 单位，须同经
        /// TransformPoint 转 world 再除——此处重算避免 _ClipBox 是 SafeBlank 时除零）。
        public static float NormalizeCornerRadius(Transform root,
            float designX, float designY, float designW, float designH, float designRadius)
        {
            if (designRadius <= 0f) return 0f;
            Vector3 wTL = root.TransformPoint(new Vector3(designX, designY, 0f));
            Vector3 wBR = root.TransformPoint(new Vector3(designX + designW, designY + designH, 0f));
            float hw = Mathf.Abs(wBR.x - wTL.x) * 0.5f;
            float hh = Mathf.Abs(wBR.y - wTL.y) * 0.5f;
            float minHalf = Mathf.Min(hw, hh);
            if (minHalf <= 0f) return 0f;
            // design→world 经 root scale（sf）；半径同 scale，故用 world half 归一化。
            return designRadius / minHalf;
        }
    }
}
