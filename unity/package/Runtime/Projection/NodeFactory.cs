// NodeFactory：Rust NodeKind(u8) → typed C# Node 子类的唯一入口。
//
// 投影层机制：Rust 核心是带 kind 判别的 enum + side
// table，不做 OOP；C# 投影层用 typed 子类（Container/Button/Slider/...）给业务程序员稳定 API 表面。
// NodeFactory 据 loomgui_stage_get_node_kind 返的 byte，switch 到对应 C# 子类构造。
//
// 全 20 公共 NodeKind 变体都需 arm（对照 Projection/NodeKind.cs）。当前 C# 公共类型集已覆盖全部
// Rust 公共 kind——OptionItem/Slot/CustomElement/TabList/Tab 五容器型变体经本 factory 派发到专用
// 子类（继承 Container）；Rust 侧另有 Template=18（ListView item 蓝图），属内部 pkg 构造不进
// 公共类型树（见 NodeKindTests.VariantCountMatchesRust），若被遍历 materialize 走下方 catch-all 回退
// Container；仅 LineBreak 在 Rust 侧尚未实装（kind_from_tag 不产）。
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
        internal static Node CreateTyped(UIContext ctx, ulong id)
        {
            StageHandle* h = (StageHandle*)ctx._stage.ToPointer();

            // get_node_kind：return-code + out byte。0=ok 且 *out=u8 判别值；
            // 非 0=节点不 live 或 out null。不用 ->u8 + 0 哨兵是因为 Container=0 会撞「不存在」。
            byte kind = 0xFF;
            int rc = Native.loomgui_stage_get_node_kind(h, id, &kind);
            if (rc != 0)
                throw new InvalidOperationException(
                    $"get_node_kind(node_id={id}) failed (rc={rc}); node not live or stage handle invalid");

            return (NodeKind)kind switch
            {
                NodeKind.Container     => new Container(ctx, id),
                NodeKind.TextElement   => new TextElement(ctx, id),
                NodeKind.ListItem      => new ListItem(ctx, id),
                NodeKind.Button        => new Button(ctx, id),
                NodeKind.ListView      => new ListView(ctx, id),

                NodeKind.TextNode      => new TextNode(ctx, id),
                NodeKind.Image         => new Image(ctx, id),

                NodeKind.TextField     => new TextField(ctx, id),
                NodeKind.NumberField   => new NumberField(ctx, id),
                NodeKind.Slider        => new Slider(ctx, id),
                NodeKind.Toggle        => new Toggle(ctx, id),
                NodeKind.RadioButton   => new RadioButton(ctx, id),
                NodeKind.TextArea      => new TextArea(ctx, id),
                NodeKind.Dropdown      => new Dropdown(ctx, id),
                NodeKind.ProgressBar   => new ProgressBar(ctx, id),

                // OptionItem = <option>、Slot = <slot>、CustomElement = 自定义标签。都是容器型节点
                // （继承 Container），但 NodeFactory 派发到专用子类让业务 Get<OptionItem>() 命中。
                NodeKind.OptionItem     => new OptionItem(ctx, id),
                NodeKind.Slot           => new Slot(ctx, id),
                NodeKind.CustomElement  => new CustomElement(ctx, id),

                // TabList = <div role=tablist>（持 tab 子，selected_index 由打包期 aria-selected 烘焙 +
                // 运行时 setter 改写）；Tab = <button role=tab>（aria-selected 从父 TabList.selected_index 派生）。
                NodeKind.TabList        => new TabList(ctx, id),
                NodeKind.Tab            => new Tab(ctx, id),

                // Rust 侧 NodeKind #[repr(u8)] 共 21 个判别值（kind_as_u8_is_discriminant 锁：0..17 +
                // Template=18 + TabList=19 + Tab=20）；其中 Template=18 是合法 byte（ListView 蓝图，
                // display:none，经 get_node_kind 可返回），但它不进公共类型树，命中本臂回退 Container
                // （可查询、不 crash）——与越界 byte 同处理
                // 不会造成危害（Template 节点本就不该被业务代码当 typed Node 取）。其余越界 byte 只能来自
                // ABI 漂移或内存损坏，造 Container 不 crash 让上层逻辑继续运行（错类型比进程崩溃更易诊断）。
                _ => new Container(ctx, id),
            };
        }
    }
}
