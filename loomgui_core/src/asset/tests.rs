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
        }
    }

    /// 辅助：空 DynamicRuleTable 的稳定引用（避免临时生命周期）。
    fn empty_rules() -> DynamicRuleTable {
        DynamicRuleTable { rules: vec![] }
    }

    #[test]
    fn write_read_multi_component_roundtrip() {
        // 两组件：comp1 = root(parent=None) + child；comp2 单节点
        let mut tn_root = tn(NodeKind::Container);
        tn_root.id_attr = Some("r".into());
        let mut tn_child = tn(NodeKind::Text {
            content: "hi".into(),
        });
        tn_child.parent_idx = Some(0);
        let comp1_nodes = vec![tn_root, tn_child];
        let comp2_nodes = vec![tn(NodeKind::Container)];
        let rules = empty_rules();
        let manifest = [AssetEntry {
            path: "icons/skin.png".into(),
            w: 64,
            h: 32,
        }];
        let input = PackageInput {
            components: vec![
                ("comp1", comp1_nodes.as_slice(), &rules),
                ("comp2", comp2_nodes.as_slice(), &rules),
            ],
            asset_manifest: &manifest,
        };
        let bytes = write_package(&input);
        let pkg = read_package(&bytes).expect("read ok");
        assert_eq!(pkg.components.len(), 2);
        assert_eq!(pkg.components["comp1"].nodes.len(), 2);
        assert!(
            pkg.components["comp1"].nodes[1].parent_idx == Some(0),
            "child parent=root"
        );
        assert_eq!(
            pkg.asset_manifest,
            vec![AssetEntry {
                path: "icons/skin.png".into(),
                w: 64,
                h: 32
            }]
        );
    }

    #[test]
    fn old_version_pkg_rejected() {
        // 手构 version < MIN_VERSION 的 header -> read_package 报 TooOld
        let mut old = vec![];
        old.extend_from_slice(&PKG_MAGIC.to_le_bytes());
        old.extend_from_slice(&(MIN_VERSION - 1).to_le_bytes()); // version 低于 MIN_VERSION
        assert!(matches!(read_package(&old), Err(PkgError::TooOld(_))));
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
            asset_manifest: &[],
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
            asset_manifest: &[],
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
        let mut child_a = tn(NodeKind::Text {
            content: "ca".into(),
        });
        child_a.parent_idx = Some(0);
        let mut root_b = tn(NodeKind::Container);
        root_b.id_attr = Some("b".into());
        let mut child_b = tn(NodeKind::Text {
            content: "cb".into(),
        });
        child_b.parent_idx = Some(0);
        let comp_a = [root_a, child_a];
        let comp_b = [root_b, child_b];
        let rules = empty_rules();
        let input = PackageInput {
            components: vec![("a", &comp_a, &rules), ("b", &comp_b, &rules)],
            asset_manifest: &[],
        };
        let pkg = read_package(&write_package(&input)).unwrap();
        assert_eq!(pkg.components["a"].nodes[1].parent_idx, Some(0));
        assert_eq!(pkg.components["b"].nodes[1].parent_idx, Some(0));
        assert_eq!(pkg.components["a"].nodes[0].parent_idx, None);
        assert_eq!(pkg.components["b"].nodes[0].parent_idx, None);
    }

    #[test]
    fn all_node_kinds_roundtrip() {
        let mut img = tn(NodeKind::Image {
            src: "icons/a.png".into(),
        });
        img.parent_idx = Some(0);
        let mut txt = tn(NodeKind::Text {
            content: "hello".into(),
        });
        txt.parent_idx = Some(0);
        let nodes = [tn(NodeKind::Container), tn(NodeKind::Button), img, txt];
        let rules = empty_rules();
        let manifest = [AssetEntry {
            path: "icons/a.png".into(),
            w: 0,
            h: 0,
        }];
        let input = PackageInput {
            components: vec![("c", &nodes, &rules)],
            asset_manifest: &manifest,
        };
        let pkg = read_package(&write_package(&input)).unwrap();
        let ns = &pkg.components["c"].nodes;
        assert!(matches!(ns[0].kind, NodeKind::Container));
        assert!(matches!(ns[1].kind, NodeKind::Button));
        assert!(matches!(&ns[2].kind, NodeKind::Image { src } if src == "icons/a.png"));
        assert!(matches!(&ns[3].kind, NodeKind::Text { content } if content == "hello"));
        assert_eq!(
            pkg.asset_manifest,
            vec![AssetEntry {
                path: "icons/a.png".into(),
                w: 0,
                h: 0
            }]
        );
    }

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
            asset_manifest: &[],
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

    #[test]
    fn style_blob_roundtrips_baked_resolved_style() {
        let mut n = tn(NodeKind::Container);
        n.style.background_color = Some([1.0, 0.0, 0.0, 1.0]);
        let nodes = [n];
        let rules = empty_rules();
        let input = PackageInput {
            components: vec![("c", &nodes, &rules)],
            asset_manifest: &[],
        };
        let pkg = read_package(&write_package(&input)).unwrap();
        let n2 = &pkg.components["c"].nodes[0];
        assert_eq!(n2.style.background_color, Some([1.0, 0.0, 0.0, 1.0]));
    }

    #[test]
    fn per_component_dynamic_rules_roundtrip() {
        use crate::parse::css::Declaration;
        use crate::parse::selector::parse_selector;
        use crate::style::dynamic::{DynamicRule, DynamicRuleTable};
        let rules_a = DynamicRuleTable {
            rules: vec![DynamicRule {
                selector: parse_selector(".a:hover").unwrap(),
                declarations: vec![Declaration {
                    prop: "background-color".into(),
                    value: "#f00".into(),
                }],
            }],
        };
        let rules_b = DynamicRuleTable {
            rules: vec![DynamicRule {
                selector: parse_selector(".b:active").unwrap(),
                declarations: vec![Declaration {
                    prop: "color".into(),
                    value: "#00f".into(),
                }],
            }],
        };
        let na = [tn(NodeKind::Container)];
        let nb = [tn(NodeKind::Container)];
        let input = PackageInput {
            components: vec![("a", &na, &rules_a), ("b", &nb, &rules_b)],
            asset_manifest: &[],
        };
        let pkg = read_package(&write_package(&input)).unwrap();
        assert_eq!(pkg.components["a"].dynamic_rules.rules.len(), 1);
        assert!(pkg.components["a"].dynamic_rules.rules[0].selector.compound[0].pseudo_hover);
        assert_eq!(pkg.components["b"].dynamic_rules.rules.len(), 1);
        assert!(pkg.components["b"].dynamic_rules.rules[0].selector.compound[0].pseudo_active);
    }

    #[test]
    fn stringtable_dedups_across_components() {
        // 两组件共用同 content "dup" -> StringTable 去重（string_count=3: "dup","c1","c2"）
        let mut n1 = tn(NodeKind::Text {
            content: "dup".into(),
        });
        n1.id_attr = Some("c1".into());
        let mut n2 = tn(NodeKind::Text {
            content: "dup".into(),
        });
        n2.id_attr = Some("c2".into());
        let c1_nodes = [n1];
        let c2_nodes = [n2];
        let rules = empty_rules();
        let input = PackageInput {
            components: vec![("c1", &c1_nodes, &rules), ("c2", &c2_nodes, &rules)],
            asset_manifest: &[],
        };
        let bytes = write_package(&input);
        let sc = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
        assert_eq!(sc, 3, "重复 content 应跨组件去重");
        let pkg = read_package(&bytes).unwrap();
        assert!(
            matches!(&pkg.components["c1"].nodes[0].kind, NodeKind::Text { content } if content == "dup")
        );
        assert!(
            matches!(&pkg.components["c2"].nodes[0].kind, NodeKind::Text { content } if content == "dup")
        );
    }

    #[test]
    fn empty_package_roundtrips() {
        let rules = empty_rules();
        let input = PackageInput {
            components: vec![],
            asset_manifest: &[],
        };
        let _ = &rules; // 占位保持 lifetime 分析简单
        let pkg = read_package(&write_package(&input)).unwrap();
        assert_eq!(pkg.components.len(), 0);
        assert!(pkg.asset_manifest.is_empty());
        assert_eq!(pkg.name, "");
    }

    #[test]
    fn asset_manifest_multiple_paths_roundtrip() {
        let nodes = [
            tn(NodeKind::Image {
                src: "a/x.png".into(),
            }),
            tn(NodeKind::Image {
                src: "b/y.png".into(),
            }),
        ];
        let rules = empty_rules();
        let manifest = [
            AssetEntry {
                path: "a/x.png".into(),
                w: 40,
                h: 20,
            },
            AssetEntry {
                path: "b/y.png".into(),
                w: 128,
                h: 128,
            },
        ];
        let input = PackageInput {
            components: vec![("c", &nodes, &rules)],
            asset_manifest: &manifest,
        };
        let pkg = read_package(&write_package(&input)).unwrap();
        assert_eq!(
            pkg.asset_manifest,
            vec![
                AssetEntry {
                    path: "a/x.png".into(),
                    w: 40,
                    h: 20
                },
                AssetEntry {
                    path: "b/y.png".into(),
                    w: 128,
                    h: 128
                },
            ]
        );
    }

    /// 图尺寸非对称（w≠h）通过 roundtrip 保留——measure 三档 + 九宫格 UV 依赖真实尺寸。
    /// 40×20 图 → manifest 存 w=40 h=20（非 0/0 兜底）。0/0 仍合法（非 PNG / 读失败 fallback）。
    #[test]
    fn asset_manifest_preserves_non_square_dims() {
        let nodes = [tn(NodeKind::Image {
            src: "wide.png".into(),
        })];
        let rules = empty_rules();
        let manifest = [AssetEntry {
            path: "wide.png".into(),
            w: 40,
            h: 20,
        }];
        let input = PackageInput {
            components: vec![("c", &nodes, &rules)],
            asset_manifest: &manifest,
        };
        let pkg = read_package(&write_package(&input)).unwrap();
        assert_eq!(pkg.asset_manifest.len(), 1);
        let e = &pkg.asset_manifest[0];
        assert_eq!(e.path, "wide.png");
        assert_eq!(e.w, 40, "w 保留 40（非 0 兜底）");
        assert_eq!(e.h, 20, "h 保留 20（非 0 兜底）");
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
        let mut child_a = tn(NodeKind::Text {
            content: "ca".into(),
        });
        child_a.parent_idx = Some(0);
        let comp_a = [root_a, child_a];
        let comp_b = [tn(NodeKind::Container)];
        let rules = empty_rules();
        let input = PackageInput {
            components: vec![("a", &comp_a, &rules), ("b", &comp_b, &rules)],
            asset_manifest: &[],
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
        // NodeBlock 紧跟 ComponentTable（2 条目 × 14B = 28B）。
        let ct_off = comp_table_offset(&bytes);
        let nodeblock_off = ct_off + 2 * 14; // 2 组件条目
                                             // 节点布局：parent_idx(4) + kind(1) + style_len(4) + style_blob + text_idx(2) + src_idx(2)
                                             //   + class_count(2) + class_idx[] + id_idx(2) + flags(1) + tabindex(4)
                                             //   固定部分 = 22B + style_blob_len + 2*class_count。所有节点用默认 style → style_len 相同。
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
        let node0_size = 22 + style_len_0 + 2 * class_count_0;
        let node1_off = nodeblock_off + node0_size;
        let style_len_1 =
            u32::from_le_bytes(bytes[node1_off + 5..node1_off + 9].try_into().unwrap()) as usize;
        let class_count_1 = u16::from_le_bytes(
            bytes[node1_off + 9 + style_len_1 + 4..node1_off + 11 + style_len_1 + 4]
                .try_into()
                .unwrap(),
        ) as usize;
        let node1_size = 22 + style_len_1 + 2 * class_count_1;
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
            asset_manifest: &[],
        };
        let _ = write_package(&input);
    }

    // —— path 归一化 + CSS 归一化 ——

    #[test]
    fn normalize_path_strips_res_prefix() {
        assert_eq!(
            normalize_path("res/icons/skin.png", "res"),
            Some("icons/skin.png".into())
        );
        assert_eq!(
            normalize_path("./res/icons/skin.png", "res"),
            Some("icons/skin.png".into())
        );
        assert_eq!(
            normalize_path("res\\icons\\skin.png", "res"),
            Some("icons/skin.png".into()),
            "Win 反斜杠"
        );
    }

    #[test]
    fn normalize_path_custom_res_dir() {
        assert_eq!(
            normalize_path("assets/icons/skin.png", "assets"),
            Some("icons/skin.png".into())
        );
    }

    #[test]
    fn normalize_path_outside_res_returns_none() {
        // 不在 res 目录下 → None（打包期 warning，不入 manifest）
        assert_eq!(normalize_path("other/foo.png", "res"), None);
    }

    #[test]
    fn normalize_path_rejects_false_segment_match() {
        // "pres/x" 含子串 "res/" 但 res 不是路径段 → None（边界检查）
        assert_eq!(
            normalize_path("pres/icons/skin.png", "res"),
            None,
            "pres/ 不是 res/ 段"
        );
        // "ares/x" 同理
        assert_eq!(
            normalize_path("ares/icons/skin.png", "res"),
            None,
            "ares/ 不是 res/ 段"
        );
    }

    #[test]
    fn normalize_path_leading_slash_res() {
        // "/res/x" — 前缀前是串首（/ 后即 res 段）→ Some
        assert_eq!(
            normalize_path("/res/icons/skin.png", "res"),
            Some("icons/skin.png".into())
        );
    }

    #[test]
    fn normalize_path_empty_after_strip() {
        // "res/" 剥前缀后空 → None（没有有效 path）
        assert_eq!(normalize_path("res/", "res"), None);
        assert_eq!(normalize_path("res", "res"), None, "res 无尾斜杠不构成段");
    }

    #[test]
    fn extract_component_css_merges_style_and_link() {
        // HTML 含 <style> + <link> → 合并成一个 stylesheet 串
        // 行内 style="" 由 resolve_styles 直接 bake，不进本函数产物。
        use std::io::Write;
        let tmp = std::env::temp_dir().join(format!("loomgui_t2_css_{}.css", std::process::id()));
        {
            let mut f = std::fs::File::create(&tmp).unwrap();
            f.write_all(b".b { color: blue; }").unwrap();
        }
        let href = tmp.to_string_lossy().replace('\\', "/");
        let html = format!(
            r#"<style>.a {{ color: red; }}</style><div><link rel="stylesheet" href="{href}"></div>"#
        );
        let merged = extract_component_css(&html, tmp.parent().unwrap());
        assert!(
            merged.contains(".a"),
            "merged 必含 <style> 内联规则 .a: {merged}"
        );
        assert!(
            merged.contains(".b"),
            "merged 必含 <link> 引用文件规则 .b: {merged}"
        );
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn extract_component_css_no_style_returns_empty() {
        // 无 <style>/<link> → 空串
        let html = r#"<div class="x"><span>hi</span></div>"#;
        let merged = extract_component_css(html, std::path::Path::new("."));
        assert!(merged.is_empty(), "无 CSS 应返回空串，got: {merged}");
    }

    #[test]
    fn extract_component_css_missing_link_file_skipped() {
        // <link href> 指向不存在文件 → 跳过该 link（不 panic，<style> 仍抽）
        let html = r#"<style>.a { color: red; }</style><link rel="stylesheet" href="nope.css">"#;
        let merged = extract_component_css(html, std::path::Path::new("/nonexistent/dir"));
        assert!(merged.contains(".a"), "<style> 内联仍抽出: {merged}");
        assert!(!merged.contains("nope"), "缺失文件不进合并串: {merged}");
    }

    #[test]
    fn extract_component_css_ignores_non_stylesheet_link() {
        // <link rel="icon"> 非 stylesheet → 不抽
        let html = r#"<link rel="icon" href="favicon.ico"><style>.a { color: red; }</style>"#;
        let merged = extract_component_css(html, std::path::Path::new("."));
        assert!(merged.contains(".a"));
        assert!(
            !merged.contains("favicon"),
            "非 stylesheet 的 link 不抽: {merged}"
        );
    }
