//! `ikat verify`：Unity batchmode 导入冒烟（#99）。
//!
//! 发版前的本地自动化验收面：build 重打产物（直落 `<unity_root>/Assets/...`）
//! → 拉起 Unity batchmode 跑包内 `Ikat.Editor.IkatVerifySmoke.Run`（Refresh +
//! 逐文件正向加载）→ 解析报告。把「产物可导入」从人肉开 Unity 变成一条命令。
//!
//! 分层边界（与 unity-smoke CI 分工，不重复）：verify = 本地导入冒烟（发版门，
//! 编辑器 license 走本机已激活的 Hub 安装）；CI 的 EditMode/PlayMode 测试归
//! `.github/workflows/unity-smoke.yml`（等 UNITY_LICENSE secret 启用）。
//!
//! 退出码：0 = 全部产物可导入；1 = Unity 报导入失败（数据性，诊断含逐资产
//! FAIL 行）；2 = 工具性失败（无 unity_root 的本地模式 / 找不到编辑器 / 超时 /
//! executeMethod 未运行）。编辑器查找三层：`--unity-editor` 显式参数 → 读
//! `ProjectVersion.txt` 匹配 Unity Hub 标准安装目录 → 都没有 exit 2 教用法。
//! 编辑器绝对路径**不进** `.ikat/config.json`——那文件入库共享，机器相关路径
//! 属于命令行参数层。

use crate::build;
use crate::config;
use crate::diag::{self, BuildFailure, PackDiagnostic};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// batchmode 看门狗：首次导入大工程（Library 冷）也可能慢，给足 15 分钟；
/// 超时按工具性失败（编辑器可能卡在交互对话框）。
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(900);

/// verify 成功产物：给 human/json 输出的摘要材料。
#[derive(Debug)]
pub struct VerifyOutcome {
    pub assets_checked: usize,
    pub editor: PathBuf,
    pub log: PathBuf,
    /// 重打阶段携带的围栏 warning（与 build 成功输出同口径）。
    pub warnings: Vec<PackDiagnostic>,
}

/// 冒烟主流程：build → 拉起 Unity batchmode → 解析报告。
pub fn run(ui: &Path, unity_editor: Option<&Path>) -> Result<VerifyOutcome, BuildFailure> {
    // unity 模式是硬前置：本地输出模式没有可冒烟的 Unity 工程。
    let Some(unity_root) = config::resolve_output_base(ui)? else {
        return Err(BuildFailure::config(
            "verify 需要产物直落 Unity 工程的工作区形态（.ikat/config.json 带 \
             unity_root）；本地输出模式没有可冒烟的 Unity 工程。用 GUI 打包器重建\
             工作区绑定 Unity 工程，或 `ikat init --unity-root <path>`",
        ));
    };
    // Assets 前置检查先于重打：落点不对时白打一轮。
    let ws = crate::workspace::load_workspace(ui).map_err(BuildFailure::config)?;
    let out_dir = ws.output_dir.replace('\\', "/");
    if !out_dir.starts_with("Assets/") {
        return Err(BuildFailure::config(format!(
            "verify 要求 output_dir 位于 Unity 工程的 Assets/ 下（当前 `{out_dir}`）——\
             Assets 外的文件 Unity 不会导入，冒烟无对象"
        )));
    }
    // 冒烟的必须是「本批源码的产物」——陈货过了不算过，无条件重打。
    let report = build::build(ui)?;

    let editor = find_unity_editor(&unity_root, unity_editor).map_err(BuildFailure::config)?;

    let tmp = std::env::temp_dir().join(format!("ikat-verify-{}", std::process::id()));
    std::fs::create_dir_all(&tmp)
        .map_err(|e| BuildFailure::config(format!("create {}: {e}", tmp.display())))?;
    let report_path = tmp.join("report.txt");
    let log_path = tmp.join("unity.log");

    eprintln!(
        "verify: launching Unity batchmode on {} (editor {})…",
        unity_root.display(),
        editor.display()
    );
    let mut child = Command::new(&editor)
        .arg("-batchmode")
        .arg("-quit")
        .arg("-nographics")
        .arg("-projectPath")
        .arg(&unity_root)
        .arg("-executeMethod")
        .arg("Ikat.Editor.IkatVerifySmoke.Run")
        .arg("-logFile")
        .arg(&log_path)
        .arg(format!("-ikatVerifyDir={out_dir}"))
        .arg(format!("-ikatVerifyReport={}", report_path.display()))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| BuildFailure::config(format!("launch Unity {}: {e}", editor.display())))?;

    let start = Instant::now();
    let status = loop {
        match child
            .try_wait()
            .map_err(|e| BuildFailure::config(format!("wait Unity: {e}")))?
        {
            Some(s) => break s,
            None if start.elapsed() >= DEFAULT_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(BuildFailure::config(format!(
                    "Unity batchmode 超时（{}s）已终止——编辑器可能卡在交互对话框；\
                     重试或查日志 {}",
                    DEFAULT_TIMEOUT.as_secs(),
                    log_path.display()
                )));
            }
            None => std::thread::sleep(Duration::from_millis(500)),
        }
    };

    let text = std::fs::read_to_string(&report_path).map_err(|_| {
        BuildFailure::config(format!(
            "Unity 退出码 {} 但没写冒烟报告（executeMethod 未运行：包未装/编译失败/\
             参数丢失）。日志: {}",
            status.code().unwrap_or(-1),
            log_path.display()
        ))
    })?;
    let parsed = parse_report(&text);
    let _ = std::fs::remove_dir_all(&tmp);

    if let Some(unity_code) = status.code() {
        // C# 侧契约：0=全过 / 1=有导入失败（FAIL 行）/ 2=参数缺失。其余码 =
        // Unity 自身崩/许可问题，工具性失败并指日志。
        if unity_code != 0 && unity_code != 1 {
            return Err(BuildFailure::config(format!(
                "Unity 进程异常退出（码 {unity_code}）——编辑器/许可问题，查日志 {}",
                log_path.display()
            )));
        }
    }

    if !parsed.failures.is_empty() {
        let diagnostics = parsed
            .failures
            .iter()
            .map(|(path, reason)| {
                PackDiagnostic::synthetic_error(
                    diag::code::UNITY_IMPORT_FAILED,
                    "verify",
                    path,
                    format!("Unity 导入失败：{reason}"),
                )
            })
            .collect();
        return Err(BuildFailure::validation(
            format!(
                "{} of {} asset(s) failed Unity import (log: {})",
                parsed.failures.len(),
                parsed.ok_count + parsed.failures.len(),
                log_path.display()
            ),
            diagnostics,
        ));
    }

    Ok(VerifyOutcome {
        assets_checked: parsed.ok_count,
        editor,
        log: log_path,
        warnings: report.warnings,
    })
}

/// 报告解析结果（纯函数，单测锁契约）。
struct ParsedReport {
    ok_count: usize,
    /// (资产路径, 原因)——FAIL 行拆解。
    failures: Vec<(String, String)>,
}

/// 解析 C# 侧报告行：`OK <path>` / `FAIL <path>: <reason>`（资产路径相对工程
/// 根、正斜杠，无盘符冒号——首个 `: ` 拆路径与原因）。
fn parse_report(text: &str) -> ParsedReport {
    let mut ok_count = 0;
    let mut failures = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("OK ") {
            let _ = rest;
            ok_count += 1;
        } else if let Some(rest) = line.strip_prefix("FAIL ") {
            match rest.split_once(": ") {
                Some((path, reason)) => failures.push((path.to_string(), reason.to_string())),
                None => failures.push((rest.to_string(), "unparsed failure".to_string())),
            }
        }
    }
    ParsedReport { ok_count, failures }
}

/// 从 `ProjectSettings/ProjectVersion.txt` 抽编辑器版本（`m_EditorVersion:
/// 6000.5.0f1`）。纯函数，单测锁契约。
fn parse_project_version(text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("m_EditorVersion:") {
            let v = rest.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Hub 标准安装目录下该版本的编辑器可执行文件候选（Win/mac/Linux 三平台形态）。
fn hub_editor_candidates(version: &str) -> Vec<PathBuf> {
    if cfg!(windows) {
        vec![PathBuf::from(format!(
            r"C:\Program Files\Unity\Hub\Editor\{version}\Editor\Unity.exe"
        ))]
    } else if cfg!(target_os = "macos") {
        vec![PathBuf::from(format!(
            "/Applications/Unity/Hub/Editor/{version}/Unity.app/Contents/MacOS/Unity"
        ))]
    } else {
        let home = std::env::var("HOME").unwrap_or_default();
        vec![PathBuf::from(format!(
            "{home}/Unity/Hub/Editor/{version}/Editor/Unity"
        ))]
    }
}

/// 三层编辑器查找：显式参数（存在性校验）→ ProjectVersion.txt 匹配 Hub 标准
/// 目录 → Err 带修复指引。
fn find_unity_editor(unity_root: &Path, explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        if p.is_file() {
            return Ok(p.to_path_buf());
        }
        return Err(format!("--unity-editor 指定的文件不存在: {}", p.display()));
    }
    let pv_path = unity_root
        .join("ProjectSettings")
        .join("ProjectVersion.txt");
    let text = std::fs::read_to_string(&pv_path)
        .map_err(|e| format!("read {}（不是 Unity 工程？）: {e}", pv_path.display()))?;
    let version = parse_project_version(&text)
        .ok_or_else(|| format!("{} 里没有 m_EditorVersion 行", pv_path.display()))?;
    for cand in hub_editor_candidates(&version) {
        if cand.is_file() {
            return Ok(cand);
        }
    }
    Err(format!(
        "未在 Unity Hub 标准位置找到编辑器 {version}。安装该版本，或显式传 \
         `--unity-editor <Unity 可执行文件路径>`"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_version_parse() {
        assert_eq!(
            parse_project_version("m_EditorVersion: 6000.5.0f1\nm_EditorVersionWithRevision: x"),
            Some("6000.5.0f1".to_string())
        );
        assert_eq!(parse_project_version("garbage"), None);
        assert_eq!(parse_project_version("m_EditorVersion:"), None);
    }

    #[test]
    fn report_parse_ok_and_fail_lines() {
        let p = parse_report("OK Assets/Bundles/ui/game.pkg.bin\nOK Assets/Bundles/atlas/a.png\nFAIL Assets/Bundles/fonts/x.ttf: import produced no loadable asset\n");
        assert_eq!(p.ok_count, 2);
        assert_eq!(p.failures.len(), 1);
        assert_eq!(p.failures[0].0, "Assets/Bundles/fonts/x.ttf");
        assert_eq!(p.failures[0].1, "import produced no loadable asset");
    }

    #[test]
    fn report_parse_fail_without_reason() {
        let p = parse_report("FAIL Assets/Bundles\n");
        assert_eq!(p.failures[0].0, "Assets/Bundles");
        assert_eq!(p.failures[0].1, "unparsed failure");
    }

    /// verify 的 unity 模式硬前置：本地模式工作区 → 工具性失败（exit 2 语义）。
    #[test]
    fn local_mode_workspace_rejected() {
        let tmp = std::env::temp_dir().join("ikat_verify_localmode_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join(crate::workspace::WORKSPACE_FILE), "{}").unwrap();
        let err = run(&tmp, None).unwrap_err();
        assert_eq!(err.exit_code, 2, "本地模式应工具性失败: {err}");
        assert!(err.message.contains("unity_root"), "{}", err.message);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// output_dir 不在 Assets/ 下 → 工具性失败（无导入对象）。构造仅 config +
    /// workspace.json 的最小工程（build 前的快速失败路径——config 校验先于重打，
    /// 本用例不触发 build：让 config 的 unity_root 指向存在目录即可走到
    /// load_workspace 之后）。
    #[test]
    fn editor_discovery_explicit_missing_errors() {
        let err = find_unity_editor(
            Path::new("nowhere"),
            Some(Path::new("E:/no/such/unity.exe")),
        )
        .unwrap_err();
        assert!(err.contains("--unity-editor"), "{err}");
    }
}
