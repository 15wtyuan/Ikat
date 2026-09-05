//! 验证：读 showcase.pkg.bin 回读，打印组件名。
//! 用法：cargo run -p yio_core --example verify_showcase_pkg -- <path-to-pkg.bin>
use std::env;
use yio_core::asset::read_package;

fn main() {
    let path = env::args()
        .nth(1)
        .expect("usage: verify_showcase_pkg <pkg.bin>");
    let bytes = std::fs::read(&path).expect("read pkg.bin");
    let pkg = read_package(&bytes).expect("read_package");
    println!("package name: {:?}", pkg.name);
    println!("component count: {}", pkg.components.len());
    let mut names: Vec<&String> = pkg.components.keys().collect();
    names.sort();
    for n in &names {
        let c = &pkg.components[*n];
        println!(
            "  - {:<20} nodes={:<4} dyn_rules={}",
            c.name,
            c.nodes.len(),
            c.dynamic_rules.rules.len()
        );
    }
    // 校验根节点 parent_idx=None
    let mut bad = 0;
    for n in &names {
        let c = &pkg.components[*n];
        if c.nodes.first().and_then(|n| n.parent_idx).is_some() {
            println!("ERROR: component {} root has parent_idx != None", n);
            bad += 1;
        }
    }
    if bad == 0 {
        println!("OK: all component roots have parent_idx=None");
    }
}
