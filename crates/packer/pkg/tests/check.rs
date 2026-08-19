//! check() 契约：零写入、不要求 output_dir、诊断 collect-all、与 build 同代码路径。

use loomgui_pkg::build::analyze;
use loomgui_pkg::diag::Severity;
use std::path::{Path, PathBuf};

/// 目录树快照：每文件 (path, len, mtime)。check 零写入 = 前后快照一致。
fn snapshot(root: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    fn walk(dir: &Path, out: &mut Vec<(PathBuf, u64, std::time::SystemTime)>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(&p, out);
            } else if let Ok(md) = entry.metadata() {
                out.push((p, md.len(), md.modified().unwrap_or(std::time::UNIX_EPOCH)));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out.sort();
    out
}

fn make_workspace(tmp: &Path, html: &str, output_dir: &str) {
    let pkg_dir = tmp.join("ui/showcase");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(pkg_dir.join("home.html"), html).unwrap();
    let json = format!(
        r#"{{"version":1,"output_dir":"{output_dir}","packages":[{{"name":"showcase","dirs":["ui/showcase"],"html":[]}}],"atlases":[],"fonts":[]}}"#
    );
    std::fs::write(tmp.join("loom.workspace.json"), json).unwrap();
}

/// check 零写入：跑 analyze 后工作区文件（内容 + mtime）不变。
#[test]
fn check_writes_nothing() {
    let tmp = std::env::temp_dir().join("loom_check_zero_write_test");
    let _ = std::fs::remove_dir_all(&tmp);
    make_workspace(
        &tmp,
        r#"<div style="border-width:2px;border-color:#ff0000"></div>"#,
        "output",
    );

    let before = snapshot(&tmp);
    let outcome = analyze(&tmp).expect("warning-only workspace analyzes clean");
    let after = snapshot(&tmp);
    assert_eq!(before, after, "check 不得写任何文件");
    assert!(
        !tmp.join("output").exists(),
        "check 不得创建输出目录（有 warning 也不行）"
    );
    // warning 照常暴露（与 build 成功路径同口径）。
    assert!(
        outcome.packages.iter().any(|(_, pr)| pr
            .warnings
            .iter()
            .any(|w| w.code == "FenceBorderWithoutStyle")),
        "check 也要暴露 warning"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// check 不要求 output_dir：空值也能校验（它零写入，不关心落点；build 才要求非空）。
#[test]
fn check_does_not_require_output_dir() {
    let tmp = std::env::temp_dir().join("loom_check_no_output_test");
    let _ = std::fs::remove_dir_all(&tmp);
    make_workspace(&tmp, r#"<div>hi</div>"#, "");

    let outcome = analyze(&tmp).expect("empty output_dir must not block check");
    assert_eq!(outcome.packages.len(), 1);
    let _ = std::fs::remove_dir_all(&tmp);
}

/// check 与 build 同一诊断：围栏错误 collect-all 报全（含字体缺失合成码——
/// check 也须报资源问题，不只是围栏）。
#[test]
fn check_collects_fence_and_resource_errors() {
    let tmp = std::env::temp_dir().join("loom_check_collect_test");
    let _ = std::fs::remove_dir_all(&tmp);
    // 两个组件各含围栏错误 + 一个缺失字体：三条 error 诊断一次给全。
    make_workspace(&tmp, r#"<p>not in fence</p>"#, "output");
    std::fs::write(
        tmp.join("ui/showcase/page2.html"),
        r#"<div role="nope"></div>"#,
    )
    .unwrap();
    let json = r#"{"version":1,"output_dir":"output","packages":[{"name":"showcase","dirs":["ui/showcase"],"html":[]}],"atlases":[],"fonts":[{"family":"Ghost","file":"fonts/ghost.ttf","default":true,"fallback":false}]}"#;
    std::fs::write(tmp.join("loom.workspace.json"), json).unwrap();

    let err = match analyze(&tmp) {
        Err(e) => e,
        Ok(_) => panic!("mixed errors must fail"),
    };
    assert_eq!(err.exit_code, 1);
    let errors: Vec<_> = err
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(errors.len(), 3, "fence×2 + font×1 全报: {err:?}");
    assert!(errors.iter().any(|d| d.file == "ui/showcase/home.html"));
    assert!(errors.iter().any(|d| d.file == "ui/showcase/page2.html"));
    assert!(
        errors
            .iter()
            .any(|d| d.code == "FontFileMissing" && d.file == "fonts/ghost.ttf"),
        "check 也要报字体缺失"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// 覆盖缺失（HTML 引用不在任何 atlas）在 check 阶段报出（build 失败前的最早反馈点）。
#[test]
fn check_reports_uncovered_sprite_reference() {
    let tmp = std::env::temp_dir().join("loom_check_coverage_test");
    let _ = std::fs::remove_dir_all(&tmp);
    make_workspace(
        &tmp,
        r#"<div><img src="ghost.png" style="display:block"></div>"#,
        "output",
    );

    let err = match analyze(&tmp) {
        Err(e) => e,
        Ok(_) => panic!("uncovered sprite must fail"),
    };
    assert_eq!(err.exit_code, 1);
    assert!(
        err.diagnostics
            .iter()
            .any(|d| d.code == "SpriteMissingFromAtlas"),
        "覆盖缺失须以合成诊断码暴露: {:?}",
        err.diagnostics
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
