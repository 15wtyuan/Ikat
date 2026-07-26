//! 控件视觉子节点注入：instantiate 时给控件节点追加框架内部 `.loom-*` 子节点。
//!
//! 控件即容器模型——ProgressBar 注入 `.loom-fill`，Slider 注入 `track > fill` + thumb
//! （结构：slider → [track, thumb]，track → [fill]），Toggle/RadioButton 注入 `.loom-check`。
//! 这些子节点只携带保留 class（无 id_attr），绝不污染用户 id 命名空间（`Get<T>` 作用域查找
//! 不会误命中框架内部节点）。子节点是普通 Container（div），display 默认按 schema 铺底，
//! 与用户手写 `<div class="loom-fill">` 实例化结果一致。

use crate::scene::dynamic::{append_child, create_node};
use crate::scene::node::{NodeId, NodeKind, Scene};

const FILL: &str = "loom-fill";
const TRACK: &str = "loom-track";
const THUMB: &str = "loom-thumb";
const CHECK: &str = "loom-check";

/// 建一个携带单个框架保留 class 的 Container 子节点（div）。
///
/// 无 id_attr：框架内部视觉节点绝不能占用用户 id 命名空间（`Get<T>` 按作用域递归查找，
/// 若内部节点带 id 会误命中、与用户同名 id 冲突）。复用 `create_node`（div → Container，
/// 含 display schema 铺底 + slotmap insert + parallel-array resize），保证注入的子节点
/// 与用户手写 `<div class="loom-*">` 的实例化路径完全一致。
fn make_child(scene: &mut Scene, class: &str) -> NodeId {
    let id = create_node(scene, "div", "").expect("\"div\" is in the fence whitelist");
    scene.get_mut(id).unwrap().classes.push(class.to_string());
    id
}

/// 给控件节点注入框架内部视觉子节点。非控件 NodeKind 为 no-op。
///
/// 在 `create_node_from_template` 填完 `ControlTable` side table 后调用——只有
/// `control_init.is_some()` 的控件节点才进此路径，普通容器/叶子节点不受影响。
///
/// Slider 结构是分层的：slider → [track, thumb]（track 与 thumb 平级），track → [fill]。
/// 故先挂 track+thumb 到 slider，再把 fill 挂到 track 内部。其余控件是单层单子。
/// `append_child` 对全新构造的子节点（无 parent）必成功，`.expect` 仅防逻辑漂移。
pub fn inject_control_children(scene: &mut Scene, id: NodeId, kind: NodeKind) {
    match kind {
        NodeKind::ProgressBar => {
            let fill = make_child(scene, FILL);
            append_child(scene, id, fill).expect("fresh child has no parent");
        }
        NodeKind::Slider => {
            // slider → [track, thumb]；track → [fill]
            let track = make_child(scene, TRACK);
            let thumb = make_child(scene, THUMB);
            append_child(scene, id, track).expect("fresh child has no parent");
            append_child(scene, id, thumb).expect("fresh child has no parent");
            let fill = make_child(scene, FILL);
            append_child(scene, track, fill).expect("fresh child has no parent");
        }
        NodeKind::Toggle | NodeKind::RadioButton => {
            let check = make_child(scene, CHECK);
            append_child(scene, id, check).expect("fresh child has no parent");
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::dynamic::create_node_from_template;
    use crate::scene::node::{NodeKind, Scene};
    use crate::style::resolved::ResolvedStyle;

    /// 建一个指定 kind 的控件节点（无 control_init，仅验注入结构）。
    fn make_control(scene: &mut Scene, kind: NodeKind) -> NodeId {
        create_node_from_template(scene, kind, ResolvedStyle::default(), None)
    }

    #[test]
    fn progress_injects_fill_child() {
        let mut scene = Scene::default();
        let id = make_control(&mut scene, NodeKind::ProgressBar);
        inject_control_children(&mut scene, id, NodeKind::ProgressBar);
        let children = scene.get(id).unwrap().children.clone();
        assert_eq!(children.len(), 1, "ProgressBar gets exactly one fill child");
        let fill = scene.get(children[0]).unwrap();
        assert!(fill.classes.iter().any(|c| c == FILL));
        assert_eq!(fill.kind, NodeKind::Container);
    }

    #[test]
    fn slider_injects_track_fill_thumb() {
        let mut scene = Scene::default();
        let id = make_control(&mut scene, NodeKind::Slider);
        inject_control_children(&mut scene, id, NodeKind::Slider);
        let children = scene.get(id).unwrap().children.clone();
        assert_eq!(children.len(), 2, "Slider gets track + thumb as siblings");
        // children[0] = track
        let track = scene.get(children[0]).unwrap();
        assert!(track.classes.iter().any(|c| c == TRACK));
        assert_eq!(track.kind, NodeKind::Container);
        // children[1] = thumb
        let thumb = scene.get(children[1]).unwrap();
        assert!(thumb.classes.iter().any(|c| c == THUMB));
        assert_eq!(thumb.kind, NodeKind::Container);
        // track → [fill]
        let track_children = track.children.clone();
        assert_eq!(track_children.len(), 1, "track contains the fill");
        let fill = scene.get(track_children[0]).unwrap();
        assert!(fill.classes.iter().any(|c| c == FILL));
        assert_eq!(fill.kind, NodeKind::Container);
        // fill 的 parent 是 track，不是 slider
        assert_eq!(fill.parent, Some(children[0]));
    }

    #[test]
    fn toggle_injects_check() {
        let mut scene = Scene::default();
        let id = make_control(&mut scene, NodeKind::Toggle);
        inject_control_children(&mut scene, id, NodeKind::Toggle);
        let children = scene.get(id).unwrap().children.clone();
        assert_eq!(children.len(), 1);
        let check = scene.get(children[0]).unwrap();
        assert!(check.classes.iter().any(|c| c == CHECK));
        assert_eq!(check.kind, NodeKind::Container);
    }

    #[test]
    fn radio_injects_check() {
        let mut scene = Scene::default();
        let id = make_control(&mut scene, NodeKind::RadioButton);
        inject_control_children(&mut scene, id, NodeKind::RadioButton);
        let children = scene.get(id).unwrap().children.clone();
        assert_eq!(children.len(), 1);
        let check = scene.get(children[0]).unwrap();
        assert!(check.classes.iter().any(|c| c == CHECK));
        assert_eq!(check.kind, NodeKind::Container);
    }

    #[test]
    fn non_control_kinds_get_no_children() {
        // Container / Button / Image 不是控件 —— 注入是 no-op。
        let mut scene = Scene::default();
        let id = make_control(&mut scene, NodeKind::Container);
        inject_control_children(&mut scene, id, NodeKind::Container);
        assert!(scene.get(id).unwrap().children.is_empty());
    }

    #[test]
    fn injected_children_carry_no_id_attr() {
        // 框架内部子节点绝不能带 id（不污染用户 id 命名空间，防 Get<T> 误命中）。
        let mut scene = Scene::default();
        let id = make_control(&mut scene, NodeKind::ProgressBar);
        inject_control_children(&mut scene, id, NodeKind::ProgressBar);
        for &child in &scene.get(id).unwrap().children {
            assert!(
                scene.get(child).unwrap().id_attr.is_none(),
                "injected child must not carry an id"
            );
        }
    }
}
