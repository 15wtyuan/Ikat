using System;

namespace Ikat
{
    /// 屏幕→设计坐标变换 与 裁剪框推导——纯数学，零 Unity/FFI 依赖。
    public static class CoordMath
    {
        /// screen→design 逆映射。公式与 IkatStageDriver.ConfigureTransforms 前向一致：
        ///   sf = min(areaW/rootW, areaH/rootH)
        ///   offX = areaX + (areaW - rootW*sf)*0.5
        ///   offYTop = areaY + areaH
        ///   dx = (screenX - offX) / sf
        ///   dy = (offYTop - screenY) / sf
        public static (float dx, float dy) ScreenToDesign(
            float screenX, float screenY,
            float screenW, float screenH,
            float rootW, float rootH,
            float areaX, float areaY, float areaW, float areaH,
            bool useSafeArea)
        {
            float sw = screenW > 0 ? screenW : 1f;
            float sh = screenH > 0 ? screenH : 1f;
            float ax = useSafeArea ? areaX : 0f;
            float ay = useSafeArea ? areaY : 0f;
            float aw = useSafeArea ? areaW : sw;
            float ah = useSafeArea ? areaH : sh;
            if (aw <= 0f || ah <= 0f) { aw = sw; ah = sh; ax = 0f; ay = 0f; }
            float rw = rootW > 0 ? rootW : 1f;
            float rh = rootH > 0 ? rootH : 1f;
            float sf = Math.Min(aw / rw, ah / rh);
            if (sf <= 0f) sf = 1f;
            float offX = ax + (aw - rw * sf) * 0.5f;
            float offYTop = ay + ah;
            float dx = (screenX - offX) / sf;
            float dy = (offYTop - screenY) / sf;
            return (dx, dy);
        }

        /// 由两 world-space 角点算 _ClipBox（shader 裁剪常量）。safe-blank=(−2,−2,0,0) 防除零。
        /// 返回 (clipX, clipY, clipZ, clipW) 对应 _ClipBox (xy = −center/half, zw = 1/half)。
        public static (float x, float y, float z, float w) ComputeClipBox(
            float tlX, float tlY, float brX, float brY)
        {
            float cx = (tlX + brX) * 0.5f;
            float cy = (tlY + brY) * 0.5f;
            float hw = Math.Abs(brX - tlX) * 0.5f;
            float hh = Math.Abs(brY - tlY) * 0.5f;
            if (hw == 0f || hh == 0f)
                return (-2f, -2f, 0f, 0f);   // safe-blank
            return (-cx / hw, -cy / hh, 1f / hw, 1f / hh);
        }
    }
}
