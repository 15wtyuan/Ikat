//! Dump fence parse result for a showcase HTML: diagnostics + whether the
//! resolver actually applies <style>-declared display (proves whether fence
//! consumes <style> blocks or only inline style="...").
use loomgui_fence::{parse_template, IrNodeKind};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: dump_showcase <html>");
    let html = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("read {path}: {e}");
            std::process::exit(1);
        }
    };
    let r = parse_template(&html, &path);
    println!("### {path}");
    println!(
        "  nodes={}  diagnostics={}",
        r.tree.nodes.len(),
        r.diagnostics.len()
    );
    for d in &r.diagnostics {
        println!("  ! {:?}", d);
    }
    let (mut flex, mut block, mut elcount) = (0usize, 0usize, 0usize);
    for (i, n) in r.tree.nodes.iter().enumerate() {
        if let IrNodeKind::Element(_) = &n.kind {
            elcount += 1;
            let dm = format!("{:?}", r.styles[i].display_mode);
            if dm == "Flex" {
                flex += 1;
            } else if dm == "Block" {
                block += 1;
            }
        }
    }
    println!("  elements: {} (Flex={}, Block={})", elcount, flex, block);
}
