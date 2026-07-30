namespace LoomGUI
{
    /// <summary>
    /// 投影层内部：Rust <c>get_node_kind</c> FFI 返回的 <c>NodeKind</c> 判别值（u8）。
    /// NodeFactory（C2）据本 enum 把 raw byte 映射到 typed Node 子类（Container/Button/...）。
    ///
    /// 判别值必须与 <c>crates/core/src/scene/node.rs</c> 的 <c>#[repr(u8)] NodeKind</c>
    /// 变体声明顺序一一对应。显式赋值（非靠隐式顺序）——防 Rust 后续插变体时 C# 隐式错位
    /// （静默 ABI bug：byte 值不再对应同名变体）。Rust 侧由 <c>from_u8</c> 的编译期穷尽
    /// guard 保证变体只追加到末尾、既有判别值稳定。
    ///
    /// 本类型是 internal：公共 API 表面是 typed Node 子类，不是 NodeKind enum。
    /// </summary>
    internal enum NodeKind : byte
    {
        Container     = 0,
        TextNode      = 1,
        TextElement   = 2,
        Button        = 3,
        Image         = 4,
        TextField     = 5,
        NumberField   = 6,
        Slider        = 7,
        Toggle        = 8,
        RadioButton   = 9,
        TextArea      = 10,
        Dropdown      = 11,
        OptionItem    = 12,
        ProgressBar   = 13,
        ListView      = 14,
        ListItem      = 15,
        Slot          = 16,
        CustomElement = 17,
    }
}
