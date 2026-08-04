use super::*;

/// 辅助：构造一个最小 TemplateNode（默认值）。
fn tn(kind: NodeKind) -> TemplateNode {
    TemplateNode {
        kind,
        style: ResolvedStyle::default(),
        parent_idx: None,
        classes: vec![],
        id_attr: None,
        draggable: false,
        tabindex: None,
        content: None,
        src: None,
        control_init: None,
        role: None,
        data_slot: None,
        aria_controls: None,
    }
}

/// 辅助：空 DynamicRuleTable 的稳定引用（避免临时生命周期）。
fn empty_rules() -> DynamicRuleTable {
    DynamicRuleTable { rules: vec![] }
}

/// 测试 StringTable 溢出 panic：intern 函数在 string 数达到 u16 上限时 panic（不产坏包）。
/// 通过写入大量唯一 content 的 Text 节点填充 StringTable。
#[test]
#[should_panic(expected = "string table overflow")]
fn write_package_panics_when_string_table_exhausted() {
    // StringTable 用 u16 索引 + NULL_IDX(0xFFFF) 哨兵，最多 65535 个不同串。
    // 超过则索引撞 NULL_IDX（读回空串）/ 回绕到 0（撞首串）——原为静默数据损坏。
    // write_package 须在打包期就 panic，不产坏包。
    let mut nodes = vec![tn(NodeKind::Container)];
    for i in 0..65536u32 {
        nodes.push(TemplateNode {
            kind: NodeKind::TextNode,
            content: Some(i.to_string()),
            src: None,
            style: ResolvedStyle::default(),
            parent_idx: Some(0),
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            control_init: None,
            role: None,
            data_slot: None,
            aria_controls: None,
        });
    }
    let rules = empty_rules();
    let input = PackageInput {
        components: vec![("c", nodes.as_slice(), &rules)],
    };
    let _ = write_package(&input);
}

#[test]
fn write_read_multi_component_roundtrip() {
    // 两组件：comp1 = root(parent=None) + child；comp2 单节点
    let mut tn_root = tn(NodeKind::Container);
    tn_root.id_attr = Some("r".into());
    let mut tn_child = tn(NodeKind::TextNode);
    tn_child.content = Some("hi".into());
    tn_child.parent_idx = Some(0);
    let comp1_nodes = vec![tn_root, tn_child];
    let comp2_nodes = vec![tn(NodeKind::Container)];
    let rules = empty_rules();
    let input = PackageInput {
        components: vec![
            ("comp1", comp1_nodes.as_slice(), &rules),
            ("comp2", comp2_nodes.as_slice(), &rules),
        ],
    };
    let bytes = write_package(&input);
    let pkg = read_package(&bytes).expect("read ok");
    assert_eq!(pkg.components.len(), 2);
    assert_eq!(pkg.components["comp1"].nodes.len(), 2);
    assert!(
        pkg.components["comp1"].nodes[1].parent_idx == Some(0),
        "child parent=root"
    );
}

#[test]
fn old_version_pkg_rejected() {
    // 手构 version < MIN_VERSION 的 header -> read_package 报 TooOld（v19 弃载，MIN=20）
    let mut old = vec![];
    old.extend_from_slice(&PKG_MAGIC.to_le_bytes());
    old.extend_from_slice(&(MIN_VERSION - 1).to_le_bytes()); // v19 低于 MIN_VERSION=20
    assert!(matches!(read_package(&old), Err(PkgError::TooOld(_))));
}

/// v19 pkg（pre-TextField-split schema）一刀切拒载：MIN=MAX=20，无迁移器。
/// 验证 bump 后旧 fixture/pkg.bin 不会半读半坏（schema 已不兼容）。
#[test]
fn v19_pkg_rejected_after_schema_drop() {
    let mut v19 = vec![];
    v19.extend_from_slice(&PKG_MAGIC.to_le_bytes());
    v19.extend_from_slice(&19u32.to_le_bytes()); // v19 = MIN_VERSION - 1
    let err = read_package(&v19);
    assert!(
        matches!(err, Err(PkgError::TooOld(19))),
        "v19 pkg must be rejected as TooOld after v20 bump, got {err:?}"
    );
}

#[test]
fn read_rejects_bad_magic() {
    let mut bad = vec![0u8; 20];
    bad[0..4].copy_from_slice(&0x4D4F4F4Cu32.to_le_bytes());
    assert!(matches!(read_package(&bad), Err(PkgError::BadMagic)));
}

#[test]
fn read_rejects_too_new_version() {
    let nodes = [tn(NodeKind::Container)];
    let rules = empty_rules();
    let input = PackageInput {
        components: vec![("c", &nodes, &rules)],
    };
    let mut bytes = write_package(&input);
    bytes[4..8].copy_from_slice(&(MAX_VERSION + 1).to_le_bytes());
    assert!(matches!(read_package(&bytes), Err(PkgError::TooNew(_))));
}

#[test]
fn header_is_20_bytes_no_root_size() {
    // header 20B（magic+version+flags+component_count+string_count），不含 root_w/root_h。
    let nodes = [tn(NodeKind::Container)];
    let rules = empty_rules();
    let input = PackageInput {
        components: vec![("c", &nodes, &rules)],
    };
    let bytes = write_package(&input);
    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    assert_eq!(magic, PKG_MAGIC);
    let ver = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    assert_eq!(ver, PKG_FORMAT_VERSION);
    let comp_count = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
    assert_eq!(comp_count, 1);
    let sc = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
    assert_eq!(sc, 1, "string_count 应在 offset 16（20B header）");
}

#[test]
fn multi_component_parent_idx_is_component_local() {
    // 两个组件各 root + child：child parent_idx 应解析为各自组件内 0（局部），
    // 验证 write 全局化 + read 减 base 转局部。
    let mut root_a = tn(NodeKind::Container);
    root_a.id_attr = Some("a".into());
    let mut child_a = tn(NodeKind::TextNode);
    child_a.content = Some("ca".into());
    child_a.parent_idx = Some(0);
    let mut root_b = tn(NodeKind::Container);
    root_b.id_attr = Some("b".into());
    let mut child_b = tn(NodeKind::TextNode);
    child_b.content = Some("cb".into());
    child_b.parent_idx = Some(0);
    let comp_a = [root_a, child_a];
    let comp_b = [root_b, child_b];
    let rules = empty_rules();
    let input = PackageInput {
        components: vec![("a", &comp_a, &rules), ("b", &comp_b, &rules)],
    };
    let pkg = read_package(&write_package(&input)).unwrap();
    assert_eq!(pkg.components["a"].nodes[1].parent_idx, Some(0));
    assert_eq!(pkg.components["b"].nodes[1].parent_idx, Some(0));
    assert_eq!(pkg.components["a"].nodes[0].parent_idx, None);
    assert_eq!(pkg.components["b"].nodes[0].parent_idx, None);
}

#[test]
fn all_node_kinds_roundtrip() {
    let mut img = tn(NodeKind::Image);
    img.src = Some("icons/a.png".into());
    img.parent_idx = Some(0);
    let mut txt = tn(NodeKind::TextNode);
    txt.content = Some("hello".into());
    txt.parent_idx = Some(0);
    let nodes = [tn(NodeKind::Container), tn(NodeKind::Button), img, txt];
    let rules = empty_rules();
    let input = PackageInput {
        components: vec![("c", &nodes, &rules)],
    };
    let pkg = read_package(&write_package(&input)).unwrap();
    let ns = &pkg.components["c"].nodes;
    assert!(matches!(ns[0].kind, NodeKind::Container));
    assert!(matches!(ns[1].kind, NodeKind::Button));
    assert!(matches!(ns[2].kind, NodeKind::Image) && ns[2].src.as_deref() == Some("icons/a.png"));
    assert!(matches!(ns[3].kind, NodeKind::TextNode) && ns[3].content.as_deref() == Some("hello"));
}

// RichText retired in Spec-2 — rich-run pkg round-trip tests removed.
#[test]
fn classes_id_attr_draggable_tabindex_roundtrip() {
    let mut root = tn(NodeKind::Container);
    root.classes = vec!["a".into(), "b".into()];
    root.id_attr = Some("x".into());
    let mut btn = tn(NodeKind::Button);
    btn.parent_idx = Some(0);
    btn.draggable = true;
    btn.tabindex = Some(3);
    let nodes = [root, btn];
    let rules = empty_rules();
    let input = PackageInput {
        components: vec![("c", &nodes, &rules)],
    };
    let pkg = read_package(&write_package(&input)).unwrap();
    let ns = &pkg.components["c"].nodes;
    assert_eq!(ns[0].classes, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(ns[0].id_attr.as_deref(), Some("x"));
    assert!(!ns[0].draggable);
    assert_eq!(ns[0].tabindex, None);
    assert!(ns[1].draggable, "btn draggable=true round-trip");
    assert_eq!(ns[1].tabindex, Some(3));
}

// Controller schema (ControllerEntry/controllers/data_controller) dropped in v19
// — data_controller / ControllerSection round-trip tests removed (no longer serialized).

#[test]
fn style_blob_roundtrips_baked_resolved_style() {
    let mut n = tn(NodeKind::Container);
    n.style.background_color = Some([1.0, 0.0, 0.0, 1.0]);
    let nodes = [n];
    let rules = empty_rules();
    let input = PackageInput {
        components: vec![("c", &nodes, &rules)],
    };
    let pkg = read_package(&write_package(&input)).unwrap();
    let n2 = &pkg.components["c"].nodes[0];
    assert_eq!(n2.style.background_color, Some([1.0, 0.0, 0.0, 1.0]));
}

#[test]
fn stringtable_dedups_across_components() {
    // 两组件共用同 content "dup" -> StringTable 去重（string_count=3: "dup","c1","c2"）
    let mut n1 = tn(NodeKind::TextNode);
    n1.content = Some("dup".into());
    n1.id_attr = Some("c1".into());
    let mut n2 = tn(NodeKind::TextNode);
    n2.content = Some("dup".into());
    n2.id_attr = Some("c2".into());
    let c1_nodes = [n1];
    let c2_nodes = [n2];
    let rules = empty_rules();
    let input = PackageInput {
        components: vec![("c1", &c1_nodes, &rules), ("c2", &c2_nodes, &rules)],
    };
    let bytes = write_package(&input);
    let sc = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
    assert_eq!(sc, 3, "重复 content 应跨组件去重");
    let pkg = read_package(&bytes).unwrap();
    assert!(
        matches!(pkg.components["c1"].nodes[0].kind, NodeKind::TextNode)
            && pkg.components["c1"].nodes[0].content.as_deref() == Some("dup")
    );
    assert!(
        matches!(pkg.components["c2"].nodes[0].kind, NodeKind::TextNode)
            && pkg.components["c2"].nodes[0].content.as_deref() == Some("dup")
    );
}

#[test]
fn empty_package_roundtrips() {
    let rules = empty_rules();
    let input = PackageInput { components: vec![] };
    let _ = &rules; // 占位保持 lifetime 分析简单
    let pkg = read_package(&write_package(&input)).unwrap();
    assert_eq!(pkg.components.len(), 0);
    assert_eq!(pkg.name, "");
}

// —— 防御 malformed ComponentTable 测试（review fix）——

/// 辅助：计算 ComponentTable 段在 pkg bytes 中的起始 offset。
/// 布局：Header(20B) + StringTable(每串 u16 len + bytes)。返回 ComponentTable 首字节 offset。
fn comp_table_offset(bytes: &[u8]) -> usize {
    assert!(bytes.len() >= 20);
    let string_count = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let mut off = 20usize;
    for _ in 0..string_count {
        let len = u16::from_le_bytes(bytes[off..off + 2].try_into().unwrap()) as usize;
        off += 2 + len;
    }
    off
}

/// 辅助：构造一个 2 组件 pkg（comp_a: root+child，comp_b: root），返回 bytes。
/// 用于 patch 出 malformed 输入测 read_package 防御。
fn two_comp_pkg_bytes() -> Vec<u8> {
    let mut root_a = tn(NodeKind::Container);
    root_a.id_attr = Some("a".into());
    let mut child_a = tn(NodeKind::TextNode);
    child_a.content = Some("ca".into());
    child_a.parent_idx = Some(0);
    let comp_a = [root_a, child_a];
    let comp_b = [tn(NodeKind::Container)];
    let rules = empty_rules();
    let input = PackageInput {
        components: vec![("a", &comp_a, &rules), ("b", &comp_b, &rules)],
    };
    write_package(&input)
}

/// Important 1：malformed ComponentTable 的 root_node_idx/node_count 越界 → Truncated（不 panic）。
/// 构造 comp_a 声称 root_node_idx=2, node_count=2，全 NodeBlock 有 3 节点 → slice [2..4] end=4 > 3 越界。
/// （node_count 不改成 10：那样 total_nodes=11 跑 NodeBlock 循环时先 Truncated 在 node 读，
///  到不了 comp slice 检查。root=2/count=2 让 total_nodes=3 与实际匹配，专门触发 slice 边界。）
#[test]
fn read_rejects_oob_component_slice() {
    let mut bytes = two_comp_pkg_bytes();
    let ct_off = comp_table_offset(&bytes);
    // ComponentTable 条目 0：name_idx(2) + root_node_idx(4) + node_count(4) + dynamic_len(4)
    // 篡改 root_node_idx=2（comp_a 原 0），node_count=2（原 2，不变）→ end=4 > 3
    bytes[ct_off + 2..ct_off + 6].copy_from_slice(&2u32.to_le_bytes());
    // node_count 保持 2（原值），total_nodes = 2 + 1 = 3 == 实际 NodeBlock
    let err = read_package(&bytes).expect_err("oob slice should error");
    assert!(
        matches!(err, PkgError::Truncated("comp_node_slice")),
        "expected Truncated(\"comp_node_slice\"), got {err:?}"
    );
}

/// Important 2：malformed NodeBlock 的 parent_idx 全局值 < 组件 base → Truncated（不静默 reparent）。
/// 构造 comp_b（base=2）的 root 节点 parent_idx=0（< 2，跨组件指向 comp_a）→ cross_comp_parent。
#[test]
fn read_rejects_cross_component_parent() {
    let bytes = two_comp_pkg_bytes();
    // 找 comp_b 的 root 节点在 NodeBlock 中的 parent_idx 字段位置。
    // 布局（v19）：ComponentTable(2 条目 × 14B = 28B) + NodeBlock（无 RichRunsArena 段）。
    let ct_off = comp_table_offset(&bytes);
    let nodeblock_off = ct_off + 2 * 14;
    // 节点布局（v28）：parent_idx(4) + kind(1) + style_len(4) + style_blob + text_idx(2) + src_idx(2)
    //   + class_count(2) + class_idx[] + id_idx(2) + flags(1) + tabindex(4)
    //   固定部分 = 22B + style_blob_len + 2*class_count（v19 删 dc_idx 2B，v18 的 24B 减 2B）。
    //   v24 加 control_init_len(4) + control_init_blob（None = 1B），故固定部分 +5B。
    //   v28 加 role_idx(2) + data_slot_idx(2) 于 control_init_blob 后，固定部分再 +4B。
    //   v29 加 aria_controls_idx(2)（TabList），固定部分再 +2B。
    let style_len_0 = u32::from_le_bytes(
        bytes[nodeblock_off + 5..nodeblock_off + 9]
            .try_into()
            .unwrap(),
    ) as usize;
    // class_count 偏移 = node_start + 9 + style_len + 4（跳过 text_idx + src_idx）
    let class_count_0 = u16::from_le_bytes(
        bytes[nodeblock_off + 9 + style_len_0 + 4..nodeblock_off + 11 + style_len_0 + 4]
            .try_into()
            .unwrap(),
    ) as usize;
    let node0_size = 22 + style_len_0 + 2 * class_count_0 + 5 + 6;
    let node1_off = nodeblock_off + node0_size;
    let style_len_1 =
        u32::from_le_bytes(bytes[node1_off + 5..node1_off + 9].try_into().unwrap()) as usize;
    let class_count_1 = u16::from_le_bytes(
        bytes[node1_off + 9 + style_len_1 + 4..node1_off + 11 + style_len_1 + 4]
            .try_into()
            .unwrap(),
    ) as usize;
    let node1_size = 22 + style_len_1 + 2 * class_count_1 + 5 + 6;
    let node2_off = nodeblock_off + node0_size + node1_size;
    // 篡改节点 2（comp_b root）的 parent_idx 从 -1 → 0（< base=2，跨组件）
    let mut patched = bytes.clone();
    patched[node2_off..node2_off + 4].copy_from_slice(&0i32.to_le_bytes());
    let err = read_package(&patched).expect_err("cross-comp parent should error");
    assert!(
        matches!(err, PkgError::Truncated("cross_comp_parent")),
        "expected Truncated(\"cross_comp_parent\"), got {err:?}"
    );
}

/// Important 3：两个 ComponentTable 条目指向同一 name_idx（同名组件）→ DupComponent（不静默覆盖）。
#[test]
fn read_rejects_duplicate_component_name() {
    let mut bytes = two_comp_pkg_bytes();
    let ct_off = comp_table_offset(&bytes);
    // ComponentTable 条目 1（comp_b）的 name_idx 改为条目 0 的 name_idx → 同名
    let name_idx_0 = u16::from_le_bytes(bytes[ct_off..ct_off + 2].try_into().unwrap());
    bytes[ct_off + 14..ct_off + 16].copy_from_slice(&name_idx_0.to_le_bytes());
    let err = read_package(&bytes).expect_err("dup component name should error");
    assert!(
        matches!(err, PkgError::DupComponent(_)),
        "expected DupComponent(_), got {err:?}"
    );
}

/// Important 4：NodeBlock 的 kind_tag 字节不在 `NodeKind::from_u8` 判别值范围（≥21）→ BadKind
/// （不静默塌成 Container）。v17 的 KIND_* 5 常量方案 read 侧用 wildcard fallback 把未知字节
/// 全塌成 Container（kind collapse）；v18 起 kind_tag = NodeKind 判别值，from_u8 对越界值
/// 返 None → BadKind。本测试同时正向验证改的是 kind_tag 字节（改成 Button 须读回 Button），
/// 避免"改错字节却因别的原因碰巧报错"的假阳性。
#[test]
fn read_rejects_unknown_kind_tag() {
    // 单组件单节点（Container, kind_tag=0），便于直接定位 kind_tag 字节。
    let nodes = [tn(NodeKind::Container)];
    let rules = empty_rules();
    let input = PackageInput {
        components: vec![("c", &nodes, &rules)],
    };
    let bytes = write_package(&input);

    // NodeBlock 紧跟 ComponentTable（每条目 14B）。单组件 → NodeBlock 起点 = ct_off + 14。
    let nodeblock_off = comp_table_offset(&bytes) + 14;
    // 节点内布局：parent_idx(4) + kind_tag(1) + ... → kind_tag 在 node_start + 4。
    let kind_tag_off = nodeblock_off + 4;

    // 正向 sanity：把 kind_tag 从 0(Container) 改成 3(Button)，read 应回 Button。
    // 确保改的是 kind_tag 字节；否则下面的 BadKind 断言会"对错原因通过"。
    let mut patched_valid = bytes.clone();
    patched_valid[kind_tag_off] = NodeKind::Button as u8;
    let pkg = read_package(&patched_valid).expect("valid kind_tag must still read");
    assert_eq!(
        pkg.components["c"].nodes[0].kind,
        NodeKind::Button,
        "kind_tag offset sanity: patching to Button must read back Button"
    );

    // 21 = from_u8 的首个 None 分支（Template=20 是最后合法判别值）。
    let mut patched_bad = bytes.clone();
    patched_bad[kind_tag_off] = 21;
    let err = read_package(&patched_bad).expect_err("unknown kind_tag must error");
    assert!(
        matches!(err, PkgError::BadKind(21)),
        "expected BadKind(21), got {err:?}"
    );

    // 0xFF = 远超判别值范围，同样必须 BadKind。防 from_u8 回归（如 off-by-one 把 25 误返 Some）。
    let mut patched_ff = bytes.clone();
    patched_ff[kind_tag_off] = 0xFF;
    let err = read_package(&patched_ff).expect_err("0xFF kind_tag must error");
    assert!(
        matches!(err, PkgError::BadKind(0xFF)),
        "expected BadKind(0xFF), got {err:?}"
    );
}

/// Minor 4：write_package 对 nodes[0].parent_idx=Some 的输入触发 debug_assert（spec 约定 nodes[0]=组件根）。
/// write 输入由打包器控制，违反即打包器 bug；用 debug_assert（release 无代价）。
/// 测试用 #[should_panic] 验证 debug 构建下触发。
#[test]
#[should_panic(expected = "nodes[0] must be root")]
fn write_rejects_non_root_nodes_zero() {
    let mut root = tn(NodeKind::Container);
    root.parent_idx = Some(0); // 违反：nodes[0] 必须是组件根（parent=None）
    let nodes = [root];
    let rules = empty_rules();
    let input = PackageInput {
        components: vec![("c", &nodes, &rules)],
    };
    let _ = write_package(&input);
}

/// TemplateNode content/src 字段经 write_package → read_package 往返稳定。
/// 这是真实持久化路径（pkg.bin 不是 serde，是手动编码）。
#[test]
fn template_node_content_src_roundtrip_via_pkg() {
    let mut text = tn(NodeKind::TextNode);
    text.content = Some("hello world".into());
    let img = TemplateNode {
        kind: NodeKind::Image,
        style: ResolvedStyle::default(),
        parent_idx: Some(0),
        classes: vec![],
        id_attr: None,
        draggable: false,
        tabindex: None,
        content: None,
        src: Some("icon.png".into()),
        control_init: None,
        role: None,
        data_slot: None,
        aria_controls: None,
    };
    let nodes = [text, img];
    let rules = empty_rules();
    let input = PackageInput {
        components: vec![("c", &nodes, &rules)],
    };
    let buf = write_package(&input);
    let pkg = read_package(&buf).unwrap();
    let read_nodes = &pkg.components["c"].nodes;
    assert_eq!(read_nodes[0].kind, NodeKind::TextNode);
    assert_eq!(
        read_nodes[0].content.as_deref(),
        Some("hello world"),
        "TextNode content must survive pkg roundtrip"
    );
    assert_eq!(read_nodes[1].kind, NodeKind::Image);
    assert_eq!(
        read_nodes[1].src.as_deref(),
        Some("icon.png"),
        "Image src must survive pkg roundtrip"
    );
}

/// v18: 20 个非 Container/Button/Image 的 NodeKind 变体经 write_package → read_package
/// 往返后 kind 不塌成 Container。v17 的 KIND_* 5 常量方案只覆盖 4 种 NodeKind，其余 wildcard
/// fallback 塌成 Container；v18 用 NodeKind 判别值保真全 23 变体（Container/Button/Image 的
/// content/src 路径由 template_node_content_src_roundtrip_via_pkg 覆盖，此处不重复）。
#[test]
fn v18_nontrivial_nodekinds_roundtrip() {
    let all_kinds = [
        NodeKind::TextNode,
        NodeKind::TextElement,
        NodeKind::TextField,
        NodeKind::NumberField,
        NodeKind::Slider,
        NodeKind::Toggle,
        NodeKind::RadioButton,
        NodeKind::TextArea,
        NodeKind::Dropdown,
        NodeKind::OptionItem,
        NodeKind::ProgressBar,
        NodeKind::ListView,
        NodeKind::ListItem,
        NodeKind::Slot,
        NodeKind::CustomElement,
    ];
    for &k in &all_kinds {
        let one = TemplateNode {
            kind: k,
            style: ResolvedStyle::default(),
            parent_idx: None,
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            content: None,
            src: None,
            control_init: None,
            role: None,
            data_slot: None,
            aria_controls: None,
        };
        let empty_rules = DynamicRuleTable { rules: vec![] };
        let input = PackageInput {
            components: vec![("c", std::slice::from_ref(&one), &empty_rules)],
        };
        let bytes = write_package(&input);
        let pkg = read_package(&bytes).unwrap();
        let comp = pkg.components.get("c").unwrap();
        assert_eq!(
            comp.nodes[0].kind, k,
            "kind {k:?} collapsed after roundtrip"
        );
    }
}

/// v24: ControlInit 经 bincode serialize/deserialize 往返保真（pkg.bin 里 control_init
/// 字段就是 Option<ControlInit> 的 bincode blob）。锁定序列化布局稳定性，防后续重构
/// 悄悄改 variant 载荷形态破坏 pkg.bin 兼容。
#[test]
fn pkg_v24_control_init_roundtrip() {
    let init = Some(ControlInit::Progress {
        value: 70.0,
        max: 100.0,
        indeterminate: false,
    });
    let bytes = bincode::serialize(&init).expect("Option<ControlInit> serializable");
    let back: Option<ControlInit> =
        bincode::deserialize(&bytes).expect("Option<ControlInit> deserializable");
    assert_eq!(back, init);
}

/// v24: control_init 经完整 pkg.bin 路径（write_package → read_package）往返保真。
/// 这是真实持久化路径（pkg.bin 手动编码，非 serde），验证 control_init_blob 段写读一致。
#[test]
fn pkg_v24_control_init_roundtrip_via_pkg() {
    let mut node = tn(NodeKind::ProgressBar);
    node.control_init = Some(ControlInit::Progress {
        value: 70.0,
        max: 100.0,
        indeterminate: false,
    });
    let nodes = [node];
    let rules = empty_rules();
    let input = PackageInput {
        components: vec![("c", &nodes, &rules)],
    };
    let pkg = read_package(&write_package(&input)).expect("roundtrip read ok");
    let back = &pkg.components["c"].nodes[0];
    assert_eq!(
        back.control_init,
        Some(ControlInit::Progress {
            value: 70.0,
            max: 100.0,
            indeterminate: false,
        })
    );
}

/// v24: version=23 的 pkg 加载报 TooOld（一刀切升，MIN=MAX=24，无迁移器）。
/// 验证 bump 后旧 v23 fixture 不会半读半坏（control_init 字段缺失致 bincode 错位）。
#[test]
fn pkg_v24_rejects_v23() {
    let mut bad = vec![];
    bad.extend_from_slice(&PKG_MAGIC.to_le_bytes());
    bad.extend_from_slice(&23u32.to_le_bytes()); // v23 < MIN_VERSION=24
    let err = read_package(&bad);
    assert!(
        matches!(err, Err(PkgError::TooOld(23))),
        "v23 pkg must be rejected as TooOld after v24 bump, got {err:?}"
    );
}

// ── v25: TextField/TextArea ──────────────────────────────────────────

/// v25: EditInit 经 bincode serialize/deserialize 往返保真。EditInit 是 TextField 和 TextArea
/// 共用的载荷结构，锁定其序列化布局稳定性，防后续重构破坏 pkg.bin 兼容。
#[test]
fn pkg_v25_edit_init_roundtrip() {
    let init = ControlInit::TextField(EditInit {
        value: "hi".into(),
        placeholder: "name".into(),
        max_length: 20,
        readonly: false,
    });
    let bytes = bincode::serialize(&init).expect("ControlInit::TextField serializable");
    let back: ControlInit =
        bincode::deserialize(&bytes).expect("ControlInit::TextField deserializable");
    assert_eq!(init, back);

    // TextArea 同样路径
    let ta = ControlInit::TextArea(EditInit {
        value: "line1\nline2".into(),
        placeholder: "enter text".into(),
        max_length: 0,
        readonly: true,
    });
    let bytes = bincode::serialize(&ta).expect("ControlInit::TextArea serializable");
    let back: ControlInit = bincode::deserialize(&bytes).expect("deserializable");
    assert_eq!(ta, back);
}

/// v25: version=24 的 pkg 加载报 TooOld（一刀切升，MIN=MAX=25，无迁移器）。
/// ControlInit 新增 TextField/TextArea 变体改变 bincode layout，旧 v24 fixture 不能
/// 半读半坏。
#[test]
fn pkg_v25_rejects_v24() {
    let mut bad = vec![];
    bad.extend_from_slice(&PKG_MAGIC.to_le_bytes());
    bad.extend_from_slice(&24u32.to_le_bytes()); // v24 < MIN_VERSION=25
    let err = read_package(&bad);
    assert!(
        matches!(err, Err(PkgError::TooOld(24))),
        "v24 pkg must be rejected as TooOld after v25 bump, got {err:?}"
    );
}

// ── v26: Dropdown / NumberField ─────────────────────────────────────

/// v26: ControlInit::Dropdown 经 bincode serialize/deserialize 往返保真。
/// 锁定序列化布局稳定性，防后续重构破坏 pkg.bin 兼容。
#[test]
fn pkg_v26_dropdown_init_roundtrip() {
    let init = ControlInit::Dropdown { selected_index: 1 };
    let bytes = bincode::serialize(&init).expect("ControlInit::Dropdown serializable");
    let back: ControlInit =
        bincode::deserialize(&bytes).expect("ControlInit::Dropdown deserializable");
    assert_eq!(init, back);
}

/// v26: ControlInit::NumberField 经 bincode serialize/deserialize 往返保真。
#[test]
fn pkg_v26_number_field_init_roundtrip() {
    let init = ControlInit::NumberField {
        edit: EditInit {
            value: "50".into(),
            placeholder: "0".into(),
            max_length: 10,
            readonly: false,
        },
        min: 0.0,
        max: 100.0,
        step: 1.0,
    };
    let bytes = bincode::serialize(&init).expect("ControlInit::NumberField serializable");
    let back: ControlInit =
        bincode::deserialize(&bytes).expect("ControlInit::NumberField deserializable");
    assert_eq!(init, back);
}

/// v26: Dropdown control_init 经完整 pkg.bin 路径往返保真。
#[test]
fn pkg_v26_dropdown_init_via_pkg() {
    let mut node = tn(NodeKind::Dropdown);
    node.control_init = Some(ControlInit::Dropdown { selected_index: 1 });
    let nodes = [node];
    let rules = empty_rules();
    let input = PackageInput {
        components: vec![("c", &nodes, &rules)],
    };
    let pkg = read_package(&write_package(&input)).expect("roundtrip read ok");
    let back = &pkg.components["c"].nodes[0];
    assert_eq!(
        back.control_init,
        Some(ControlInit::Dropdown { selected_index: 1 })
    );
}

/// v26: NumberField control_init 经完整 pkg.bin 路径往返保真。
#[test]
fn pkg_v26_number_field_init_via_pkg() {
    let mut node = tn(NodeKind::NumberField);
    node.control_init = Some(ControlInit::NumberField {
        edit: EditInit {
            value: "50".into(),
            placeholder: "0".into(),
            max_length: 10,
            readonly: false,
        },
        min: 0.0,
        max: 100.0,
        step: 1.0,
    });
    let nodes = [node];
    let rules = empty_rules();
    let input = PackageInput {
        components: vec![("c", &nodes, &rules)],
    };
    let pkg = read_package(&write_package(&input)).expect("roundtrip read ok");
    let back = &pkg.components["c"].nodes[0];
    assert_eq!(
        back.control_init,
        Some(ControlInit::NumberField {
            edit: EditInit {
                value: "50".into(),
                placeholder: "0".into(),
                max_length: 10,
                readonly: false,
            },
            min: 0.0,
            max: 100.0,
            step: 1.0,
        })
    );
}

/// v27: version=26 的 pkg 加载报 TooOld（一刀切升，MIN=MAX=27，无迁移器）。
/// NodeKind 新增 Template 变体 + template 子树进 pkg，旧 v26 fixture 不能半读半坏。
#[test]
fn pkg_v27_rejects_v26() {
    let mut bad = vec![];
    bad.extend_from_slice(&PKG_MAGIC.to_le_bytes());
    bad.extend_from_slice(&26u32.to_le_bytes()); // v26 < MIN_VERSION=27
    let err = read_package(&bad);
    assert!(
        matches!(err, Err(PkgError::TooOld(26))),
        "v26 pkg must be rejected as TooOld after v27 bump, got {err:?}"
    );
}

// ── v29: aria_controls（TabList tab→panel 跨树关联）────────────────

/// v29: role/data-slot/aria_controls 三个 StringTable interning 字符串列经完整 pkg.bin
/// 路径（write_package → read_package）往返保真。aria_controls 是 TabList 地基：
/// 打包期提取 HTML aria-controls 属性，runtime instantiate 拷进 RoleInfo.aria_controls，
/// sync_control_visuals 据此 find_node_by_id 解析 panel。
#[test]
fn pkg_v29_roundtrip_with_aria_controls() {
    assert_eq!(
        PKG_FORMAT_VERSION, 29,
        "pkg format version must be 29 after aria_controls bump"
    );
    let mut node = tn(NodeKind::Container);
    node.role = Some("tab".into());
    node.data_slot = Some("thumb".into());
    node.aria_controls = Some("panel-audio".into());
    let nodes = [node];
    let rules = empty_rules();
    let input = PackageInput {
        components: vec![("c", &nodes, &rules)],
    };
    let pkg = read_package(&write_package(&input)).expect("roundtrip read ok");
    let back = &pkg.components["c"].nodes[0];
    assert_eq!(back.role.as_deref(), Some("tab"));
    assert_eq!(back.data_slot.as_deref(), Some("thumb"));
    assert_eq!(back.aria_controls.as_deref(), Some("panel-audio"));
}

/// v29: 三个字符串列均缺省（None）也须往返保真（NULL_IDX 哨兵路径，多数节点走此分支）。
#[test]
fn pkg_v29_roundtrip_without_strings_defaults_none() {
    let node = tn(NodeKind::Container); // role/data_slot/aria_controls 均默认 None
    let nodes = [node];
    let rules = empty_rules();
    let input = PackageInput {
        components: vec![("c", &nodes, &rules)],
    };
    let pkg = read_package(&write_package(&input)).expect("roundtrip read ok");
    let back = &pkg.components["c"].nodes[0];
    assert!(back.role.is_none(), "no role attr → None after roundtrip");
    assert!(
        back.data_slot.is_none(),
        "no data-slot attr → None after roundtrip"
    );
    assert!(
        back.aria_controls.is_none(),
        "no aria-controls attr → None after roundtrip"
    );
}

/// v29: version=28 的 pkg 加载报 TooOld（一刀切升，MIN=MAX=29，无迁移器）。
/// TemplateNode 新增 aria_controls_idx 列改变 NodeBlock 布局，旧 v28 fixture 不能半读半坏
/// （aria_controls_idx 缺失致后续读错位）。
#[test]
fn pkg_v29_rejects_v28() {
    let mut bad = vec![];
    bad.extend_from_slice(&PKG_MAGIC.to_le_bytes());
    bad.extend_from_slice(&28u32.to_le_bytes()); // v28 < MIN_VERSION=29
    let err = read_package(&bad);
    assert!(
        matches!(err, Err(PkgError::TooOld(28))),
        "v28 pkg must be rejected as TooOld after v29 bump, got {err:?}"
    );
}
