using System;

namespace LoomGUI
{
    /// <summary>
    /// 引擎后端契约（新后端只需实现「消费 RenderNode + 输入注入 + 资源加载」）。
    /// 引擎无关抽象基类——零 UnityEngine（放 Runtime/Host/，随 LoomGUI.Runtime asmdef 编译）。
    /// Unity 实现 <see cref="UnityLoomBackend"/>；Godot-C# 未来实现 GodotLoomBackend。
    /// <see cref="LoomHost"/> 持具体实现并按每帧管线驱动。
    /// </summary>
    public abstract class LoomBackend
    {
        /// <summary>
        /// 采集引擎输入（Unity: InputCollector）+ 调 set_input 系 FFI。
        /// set_input FFI 引擎中立（payload 是 PointerEvent / KeyEvent / WheelEvent struct，
        /// 不是引擎类型），故 backend 采集后直接调 FFI 不破坏引擎无关性——省一次 host↔backend 输入搬运。
        /// </summary>
        /// <param name="stage">Stage handle（<see cref="LoomHost"/> 持有的 native StageHandle*，以 IntPtr 透传）。</param>
        public abstract void CollectInput(IntPtr stage);

        /// <summary>
        /// 消费 borrow_frame 拿到的 frame blob 做镜像渲染——本方法不调 borrow FFI
        /// （<see cref="LoomHost"/> 已 borrow，把 stage + ptr + len 传进来，避免二次 borrow）。
        /// Unity: SyncFontAtlas（脏页上传）+ MirrorPool.Sync（RenderNode 镜像）+ NativeHostManager.Sync（3D 模型绑定）。
        /// </summary>
        /// <param name="stage">Stage handle（拉 atlas FFI / NativeHostManager.Sync 都需要）。</param>
        /// <param name="framePtr">borrow_frame 返回的 RenderNode blob 起始指针。</param>
        /// <param name="frameLen">blob 字节数。</param>
        public abstract void SyncFrame(IntPtr stage, IntPtr framePtr, int frameLen);
    }
}
