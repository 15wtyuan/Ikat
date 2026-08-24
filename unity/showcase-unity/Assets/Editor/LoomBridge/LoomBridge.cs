using LoomGUI;            // LoomStageDriver / LoomHost
using UnityEngine;

namespace Showcase.LoomBridge
{
    /// <summary>
    /// dev 调试桥 helper：被 unity-cli-loop 的 <c>execute-dynamic-code</c> 调用，
    /// 编排包内诊断方法（<see cref="LoomHost.DumpSceneJson"/> /
    /// <see cref="LoomStageDriver.DumpMirrorPoolState"/>）。
    /// PlayMode-only（EditMode 无活 LoomStageDriver）。
    /// </summary>
    public static class LoomBridge
    {
        static LoomStageDriver FindDriver()
            => Object.FindAnyObjectByType<LoomStageDriver>();

        /// <summary>
        /// 整树 JSON：node_id/parent/tag/id/classes/kind/layout{x,y,w,h}/world_matrix/anim/visible。
        /// 无活 driver（非 PlayMode）返提示串。
        /// </summary>
        public static string DumpScene()
        {
            var driver = FindDriver();
            if (driver == null || driver.Host == null) return "no active LoomStageDriver (PlayMode?)";
            return driver.Host.DumpSceneJson();
        }

        /// <summary>
        /// MirrorPool 状态文本：active/parked GO slot + reuse_key（虚拟列表/pooled-slot 调试）。
        /// 无活 driver 返提示串。
        /// </summary>
        public static string DumpMirrorPool()
        {
            var driver = FindDriver();
            if (driver == null) return "no active LoomStageDriver (PlayMode?)";
            return driver.DumpMirrorPoolState();
        }
    }
}
