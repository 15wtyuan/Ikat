// NodeFactory：Rust NodeKind(u8) → typed C# Node 子类的唯一入口。
//
// 投影层机制（docs/design/projection-layer.md §2.3）：Rust 核心是带 kind 判别的 enum + side
// table，不做 OOP；C# 投影层用 typed 子类（Container/Button/Slider/...）给业务程序员稳定 API 表面。
// NodeFactory 据 loomgui_stage_get_node_kind 返的 byte，switch 到对应 C# 子类构造。
//
// 全 25 NodeKind 变体都需 arm（对照 Projection/NodeKind.cs）。当前 C# 公共类型集尚未覆盖全部
// Rust kind——LineBreak/OptionItem/Slot/CustomElement 4 个暂无专用 C# 子类，回落 Container
// （结构上都是容器型节点，Container 是它们最近的具体基类）。专用子类待后续 task 引入时补 arm。
//
// 兜底 arm：未知 byte → Container + 不 crash。围栏闭合保证理论上不达（pkg.bin 只装合法 kind_tag，
// kind_from_tag 只接受围栏白名单），防御性兜底防 FFI 异常 byte 崩整树。

using LoomGUI.Bindings;
using System;

namespace LoomGUI
{
    /// <summary>
    /// 投影层内部：NodeKind byte → typed C# Node 子类的工厂。所有 Node 实例的构造经此唯一入口。
    /// </summary>
    internal static unsafe class NodeFactory
    {
        /// <summary>
        /// 经 FFI 查 NodeKind，switch 到对应 C# 子类。同一 id 重复调返等价新实例
        /// （调用方 NodeRegistry 兜缓存，不直接重复调本方法）。
        ///
        /// FFI 失败（节点不存在 / 句柄无效）→ 抛 InvalidOperationException：投影层契约不允许
        /// 给无效 NodeId 造 wrapper（与公共 API 的 UIContractException 互补——内部不变量违例）。
        /// </summary>
        internal static Node CreateTyped(UIContext ctx, uint id)
        {
            StageHandle* h = (StageHandle*)ctx._stage.ToPointer();

            // get_node_kind：return-code + out byte（lib.rs:864）。0=ok 且 *out=u8 判别值；
            // 非 0=节点不 live 或 out null。不用 ->u8 + 0 哨兵是因为 Container=0 会撞「不存在」。
            byte kind = 0xFF;
            int rc = Native.loomgui_stage_get_node_kind(h, id, &kind);
            if (rc != 0)
                throw new InvalidOperationException(
                    $"get_node_kind(node_id={id}) failed (rc={rc}); node not live or stage handle invalid");

            return (NodeKind)kind switch
            {
                // ── 基础容器类（Container 派生）──
                NodeKind.Container     => new Container(ctx, id),
                NodeKind.TextElement   => new TextElement(ctx, id),
                NodeKind.ListItem      => new ListItem(ctx, id),
                NodeKind.Button        => new Button(ctx, id),
                NodeKind.ListView      => new ListView(ctx, id),

                // ── 叶子：内容/绘制 ──
                NodeKind.TextNode      => new TextNode(ctx, id),
                NodeKind.Image         => new Image(ctx, id),

                // ── 控件（叶子：私有内部结构）──
                NodeKind.TextField     => new TextField(ctx, id),
                NodeKind.PasswordField => new PasswordField(ctx, id),
                NodeKind.SearchField   => new SearchField(ctx, id),
                NodeKind.NumberField   => new NumberField(ctx, id),
                NodeKind.Slider        => new Slider(ctx, id),
                NodeKind.Toggle        => new Toggle(ctx, id),
                NodeKind.RadioButton   => new RadioButton(ctx, id),
                NodeKind.TextArea      => new TextArea(ctx, id),
                NodeKind.Dropdown      => new Dropdown(ctx, id),
                NodeKind.ProgressBar   => new ProgressBar(ctx, id),

                // ── Rust 侧变体尚无专用 C# 子类：回落 Container。
                // 结构上都是容器型节点（下拉选项 / 模板插槽 / 自定义标签），
                // Container 是它们最近的具体基类。专用 C# 子类待后续 task 引入时补 arm 替换。
                NodeKind.OptionItem     => new Container(ctx, id),
                NodeKind.Slot           => new Container(ctx, id),
                NodeKind.CustomElement  => new Container(ctx, id),

                // ── 兜底：围栏闭合理论不达，防 FFI 异常 byte 崩整树。──
                // Rust 侧 NodeKind #[repr(u8)] 20 变体（kind_as_u8_is_discriminant 锁），
                // 越界 byte 只能来自 ABI 漂移或内存损坏——这种情况下造 Container 不 crash，
                // 让上层逻辑继续运行（用户看到的是错类型而非进程崩溃，更易诊断）。
                _ => new Container(ctx, id),
            };
        }
    }
}
