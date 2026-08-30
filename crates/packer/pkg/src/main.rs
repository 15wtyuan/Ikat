//! ikat CLI：Ikat UI 工作区的校验 / 打包 / 初始化 / workspace 编排。
//!
//! 用法（详情见 --help）：
//!   ikat check [<dir>] [--format human|json]
//!   ikat build [<dir>] [--format human|json]
//!   ikat verify [<dir>] [--unity-editor <path>] [--format human|json]
//!   ikat init <dir> [--ui <dir>] [--agent claude|agents]... [--unity-root <path>] [--output <dir>] [--force]
//!   ikat new <name>
//!   ikat list pkg|atlas|font [--format json]
//!   ikat show <pkg> [--format json]
//!   ikat font add <file> --family <f> [--default] [--fallback]
//!   ikat font remove <family>
//!   ikat font default <family>
//!   ikat atlas add <dir> [--name <n>] [--max-size <n>] [--padding <n>] [--standalone]
//!   ikat scaffold [--agent claude|agents]...
//!   ikat version [--format json]
//!
//! 输出约定：human 模式（默认）诊断/进度走 stderr；`--format json` 时 stdout 输出单个
//! JSON 文档（数据），进度走 stderr。退出码：0 干净 · 1 有 Error 级诊断 / 写命令冲突 ·
//! 2 用法/配置/io 错。目录解析统一走 config 发现：会话根（含 `.ikat/config.json`）、
//! ui 目录（含 `ikat.workspace.json`）、或 ui 的直接子目录都可直接作为 `<dir>` / cwd。

use ikat_pkg::diag::BuildFailure;
use ikat_pkg::report::{CommandOutput, VersionInfo};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Clone, Copy, PartialEq)]
enum Format {
    Human,
    Json,
}

enum ListKind {
    Pkg,
    Atlas,
    Font,
}

enum Cmd {
    Check {
        root: PathBuf,
        format: Format,
    },
    Build {
        root: PathBuf,
        format: Format,
    },
    /// Unity batchmode 导入冒烟（发版验收）：build 重打 → 拉起 Unity 导入产物
    /// → 逐文件正向加载报告。仅 unity 模式工作区（config 带 unity_root）。
    Verify {
        root: PathBuf,
        format: Format,
        unity_editor: Option<PathBuf>,
    },
    Init {
        root: PathBuf,
        ui: Option<PathBuf>,
        agents: Vec<String>,
        unity_root: Option<PathBuf>,
        output: String,
        force: bool,
    },
    New {
        name: String,
    },
    List {
        kind: ListKind,
        format: Format,
    },
    Show {
        pkg: String,
        format: Format,
    },
    FontAdd {
        file: PathBuf,
        family: Option<String>,
        default: bool,
        fallback: bool,
    },
    FontRemove {
        family: String,
    },
    FontDefault {
        family: String,
    },
    AtlasAdd {
        dir: String,
        name: Option<String>,
        max_size: u32,
        padding: u32,
        standalone: bool,
    },
    Design {
        size: Option<(f32, f32)>,
        match_mode: Option<String>,
        clear: bool,
    },
    /// 刷新 agent 脚手架（已有工作区的安全入口——不碰 workspace.json / .ikat / 源文件；
    /// 与 init --force 不同，后者会重写 workspace.json 骨架）。
    Scaffold {
        agents: Vec<String>,
    },
    /// 本地预览工作台（长驻 HTTP server，给人类浏览器看；stdout 单 JSON 含 URL）。
    /// `--stop` 停已跑的实例（先验 token 归属再杀）。
    Preview {
        root: PathBuf,
        port: Option<u16>,
        idle_timeout: Option<u64>,
        stop: bool,
    },
    Version {
        format: Format,
    },
}

fn usage() -> ! {
    let bin = bin_name();
    eprintln!("ikat — Ikat UI workspace CLI");
    eprintln!();
    eprintln!("usage:");
    eprintln!(
        "  {bin} check [<dir>] [--format human|json]     validate (fence/registry/assets), writes nothing"
    );
    eprintln!(
        "  {bin} build [<dir>] [--format human|json]     check + write artifacts to output_dir"
    );
    eprintln!("  {bin} verify [<dir>] [--unity-editor <p>] [--format human|json]");
    eprintln!(
        "                                            build + Unity batchmode import smoke (release"
    );
    eprintln!(
        "                                            gate; unity-root workspaces only; editor found"
    );
    eprintln!(
        "                                            via ProjectVersion + Unity Hub, or pass a path)"
    );
    eprintln!(
        "  {bin} init <dir> [--ui <dir>] [--agent <kind>]... [--unity-root <path>] [--output <dir>] [--force]"
    );
    eprintln!(
        "                                            scaffold skills + config + CLI at <dir>/.ikat/,"
    );
    eprintln!(
        "                                            workspace.json under --ui (default: same dir)"
    );
    eprintln!(
        "  {bin} new <name>                            create ui/<name>/main.html + register package"
    );
    eprintln!("  {bin} list pkg|atlas|font [--format json]   summary of workspace entities");
    eprintln!("  {bin} show <pkg> [--format json]            package detail (pages + components)");
    eprintln!("  {bin} font add <file> --family <f> [--default] [--fallback]");
    eprintln!(
        "  {bin} font remove <family>                deregister (source file stays in fonts/)"
    );
    eprintln!("  {bin} font default <family>               set as the single default family");
    eprintln!("  {bin} design [WxH] [--match letterbox|fit-width|fit-height] [--clear]");
    eprintln!(
        "  {bin} atlas add <dir> [--name <n>] [--max-size <n>] [--padding <n>] [--standalone]"
    );
    eprintln!(
        "  {bin} scaffold [--agent <kind>]...           refresh agent docs + skills only (safe for existing workspaces)"
    );
    eprintln!("  {bin} preview [<dir>] [--port <n>] [--idle-timeout <s>] [--stop]");
    eprintln!(
        "                                            local preview workbench for humans (long-running;"
    );
    eprintln!(
        "                                            prints one JSON with url; stable port per workspace;"
    );
    eprintln!("                                            auto-exits when idle, default 4h)");
    eprintln!("  {bin} version [--format json]");
    eprintln!();
    eprintln!("exit codes: 0 clean (warnings allowed) · 1 errors / command conflict · 2 usage/config failure");
    eprintln!();
    eprintln!(
        "`<dir>` (and the cwd for the remaining commands) may be the session root (has .ikat/),"
    );
    eprintln!(
        "the ui workspace itself (has ikat.workspace.json), or a direct child of the ui workspace."
    );
    std::process::exit(2);
}

fn bin_name() -> String {
    std::env::args()
        .next()
        .and_then(|a| {
            std::path::Path::new(&a)
                .file_name()
                .and_then(|n| n.to_str())
                .map(String::from)
        })
        .unwrap_or_else(|| "ikat".to_string())
}

/// 手写参数扫描（仓库零 clap 约定）：位置参数 + `--flag value` / `--bool`。
struct ArgScan<'a> {
    rest: &'a [String],
    i: usize,
}

impl<'a> ArgScan<'a> {
    fn new(rest: &'a [String]) -> Self {
        Self { rest, i: 0 }
    }
    /// 下一个非 flag 位置参数（消费式；跳过已知 flag 及其值）。
    fn positional(&mut self) -> Option<String> {
        while self.i < self.rest.len() {
            let a = &self.rest[self.i];
            if a == "--format"
                || a == "--agent"
                || a == "--ui"
                || a == "--unity-root"
                || a == "--output"
                || a == "--family"
                || a == "--name"
                || a == "--max-size"
                || a == "--padding"
                || a == "--match"
            {
                self.i += 2;
                continue;
            }
            if a.starts_with("--") {
                self.i += 1;
                continue;
            }
            self.i += 1;
            return Some(a.clone());
        }
        None
    }
    fn flag_value(&self, name: &str) -> Option<String> {
        let mut i = 0;
        while i < self.rest.len() {
            if self.rest[i] == name {
                return self.rest.get(i + 1).cloned();
            }
            i += 1;
        }
        None
    }
    fn values_of(&self, name: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i + 1 < self.rest.len() {
            if self.rest[i] == name {
                out.push(self.rest[i + 1].clone());
                i += 2;
            } else {
                i += 1;
            }
        }
        out
    }
    fn has(&self, name: &str) -> bool {
        self.rest.iter().any(|a| a == name)
    }
}

fn parse_format(rest: &[String]) -> Option<Format> {
    match ArgScan::new(rest).flag_value("--format").as_deref() {
        None | Some("human") => Some(Format::Human),
        Some("json") => Some(Format::Json),
        Some(_) => None,
    }
}

fn parse_cmd(args: &[String]) -> Option<Cmd> {
    let sub = args.first()?.as_str();
    let rest = &args[1..];
    let mut scan = ArgScan::new(rest);
    match sub {
        "check" | "build" | "verify" => {
            // <dir> 可选：缺省当前目录（AI 在工作区内裸跑 `ikat check` 的主形态）。
            let root = match scan.positional() {
                Some(d) => PathBuf::from(d),
                None => std::env::current_dir().ok()?,
            };
            let format = parse_format(rest)?;
            match sub {
                "check" => Some(Cmd::Check { root, format }),
                "build" => Some(Cmd::Build { root, format }),
                _ => Some(Cmd::Verify {
                    root,
                    format,
                    unity_editor: scan.flag_value("--unity-editor").map(PathBuf::from),
                }),
            }
        }
        "init" => Some(Cmd::Init {
            root: PathBuf::from(scan.positional()?),
            ui: scan.flag_value("--ui").map(PathBuf::from),
            agents: scan.values_of("--agent"),
            unity_root: scan.flag_value("--unity-root").map(PathBuf::from),
            output: scan.flag_value("--output").unwrap_or_else(|| "dist".into()),
            force: scan.has("--force"),
        }),
        "new" => Some(Cmd::New {
            name: scan.positional()?,
        }),
        "list" => {
            let kind = match scan.positional()?.as_str() {
                "pkg" | "package" | "packages" => ListKind::Pkg,
                "atlas" | "atlases" => ListKind::Atlas,
                "font" | "fonts" => ListKind::Font,
                _ => return None,
            };
            Some(Cmd::List {
                kind,
                format: parse_format(rest)?,
            })
        }
        "show" => Some(Cmd::Show {
            pkg: scan.positional()?,
            format: parse_format(rest)?,
        }),
        "design" => {
            // design [WxH] [--match letterbox|fit-width|fit-height] [--clear]
            let mut size = None;
            let mut match_mode = None;
            let mut clear = false;
            if let Some(tok) = scan.positional() {
                size = Some(parse_design_size(&tok)?);
            }
            if let Some(m) = scan.flag_value("--match") {
                match_mode = Some(m);
            }
            if scan.has("--clear") {
                clear = true;
            }
            if size.is_none() && match_mode.is_none() && !clear {
                return None;
            }
            Some(Cmd::Design {
                size,
                match_mode,
                clear,
            })
        }
        "font" => {
            // font add <file> --family <f> [--default] [--fallback]
            // font remove <family> / font default <family>
            match scan.positional()?.as_str() {
                "add" => Some(Cmd::FontAdd {
                    file: PathBuf::from(scan.positional()?),
                    family: scan.flag_value("--family"),
                    default: scan.has("--default"),
                    fallback: scan.has("--fallback"),
                }),
                "remove" => Some(Cmd::FontRemove {
                    family: scan.positional()?.to_string(),
                }),
                "default" => Some(Cmd::FontDefault {
                    family: scan.positional()?.to_string(),
                }),
                _ => None,
            }
        }
        "atlas" => {
            // atlas add <dir> [--name] [--max-size] [--padding] [--standalone]
            if scan.positional()?.as_str() != "add" {
                return None;
            }
            Some(Cmd::AtlasAdd {
                dir: scan.positional()?,
                name: scan.flag_value("--name"),
                max_size: scan
                    .flag_value("--max-size")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(2048),
                padding: scan
                    .flag_value("--padding")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(4),
                standalone: scan.has("--standalone"),
            })
        }
        "scaffold" => {
            let agents = scan.values_of("--agent");
            Some(Cmd::Scaffold {
                // 空 = 未显式给 --agent：刷新时按在场 agent 目录自动探测（见
                // run_scaffold），避免 claude 工作区被默认值漏刷。
                agents,
            })
        }
        "preview" => Some(Cmd::Preview {
            root: match scan.positional() {
                Some(d) => PathBuf::from(d),
                None => std::env::current_dir().ok()?,
            },
            port: scan.flag_value("--port").and_then(|v| v.parse().ok()),
            idle_timeout: scan
                .flag_value("--idle-timeout")
                .and_then(|v| v.parse().ok()),
            stop: scan.has("--stop"),
        }),
        "version" => Some(Cmd::Version {
            format: parse_format(rest)?,
        }),
        _ => None,
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = parse_cmd(&args) else {
        usage();
    };
    match cmd {
        Cmd::Check { root, format } => run_check(&root, format),
        Cmd::Build { root, format } => run_build(&root, format),
        Cmd::Verify {
            root,
            format,
            unity_editor,
        } => run_verify(&root, format, unity_editor),
        Cmd::Init {
            root,
            ui,
            agents,
            unity_root,
            output,
            force,
        } => run_init(root, ui, agents, unity_root, output, force),
        Cmd::New { name } => run_new(&name),
        Cmd::List { kind, format } => run_list(kind, format),
        Cmd::Show { pkg, format } => run_show(&pkg, format),
        Cmd::FontAdd {
            file,
            family,
            default,
            fallback,
        } => run_font_add(file, family, default, fallback),
        Cmd::FontRemove { family } => run_font_remove(family),
        Cmd::FontDefault { family } => run_font_default(family),
        Cmd::AtlasAdd {
            dir,
            name,
            max_size,
            padding,
            standalone,
        } => run_atlas_add(dir, name, max_size, padding, standalone),
        Cmd::Design {
            size,
            match_mode,
            clear,
        } => run_design(size, match_mode, clear),
        Cmd::Scaffold { agents } => run_scaffold(agents),
        Cmd::Preview {
            root,
            port,
            idle_timeout,
            stop,
        } => run_preview(&root, port, idle_timeout, stop),
        Cmd::Version { format } => {
            let v = VersionInfo::current();
            match format {
                Format::Human => println!("{}", v.render_human()),
                Format::Json => println!("{}", serde_json::to_string(&v).unwrap()),
            }
            ExitCode::SUCCESS
        }
    }
}

fn run_check(root: &std::path::Path, format: Format) -> ExitCode {
    let ui = match locate_arg(root) {
        Ok(p) => p,
        Err(f) => return failure_exit("check", &f, format),
    };
    match ikat_pkg::build::analyze(&ui) {
        Ok(outcome) => {
            // 全量 warning：registry 侧 + 各包侧（与 build 成功时的 BuildReport 同口径）。
            let mut warnings = outcome.warnings;
            for (_, pr) in &outcome.packages {
                warnings.extend(pr.warnings.iter().cloned());
            }
            // 工作区生成物 stale（scaffold 版本戳落后于本 CLI）——文档/工具没送到
            // 会静默过时，check 是唯一必经门，在此提醒刷新。
            if let Some(msg) = ikat_pkg::scaffold::stale_stamp_warning(&ui) {
                warnings.push(ikat_pkg::diag::PackDiagnostic::synthetic_warning(
                    "StaleScaffold",
                    "workspace",
                    ".ikat/scaffold.version",
                    msg,
                ));
            }
            // CLI ↔ Unity 包版本漂移（双向）：exe 与 Unity 插件跨边界配对，漂移
            // 原先只能靠 skill 仪式发现（AI 会忘），check 是唯一必经门。
            if let Some(msg) = ikat_pkg::config::version_drift_warning(&ui) {
                warnings.push(ikat_pkg::diag::PackDiagnostic::synthetic_warning(
                    ikat_pkg::diag::code::IKAT_VERSION_DRIFT,
                    "workspace",
                    "Packages/packages-lock.json",
                    msg,
                ));
            }
            match format {
                Format::Human => {
                    for w in &warnings {
                        eprintln!("{}", w.render());
                    }
                    eprintln!("OK: 0 errors, {} warning(s)", warnings.len());
                }
                Format::Json => {
                    println!("{}", CommandOutput::check_ok(warnings).to_json());
                }
            }
            ExitCode::SUCCESS
        }
        Err(f) => failure_exit("check", &f, format),
    }
}

fn run_build(root: &std::path::Path, format: Format) -> ExitCode {
    let ui = match locate_arg(root) {
        Ok(p) => p,
        Err(f) => return failure_exit("build", &f, format),
    };
    match ikat_pkg::build::build(&ui) {
        Ok(report) => {
            match format {
                Format::Human => {
                    // 围栏一致性 warning 打到 stderr：合法但预览≠运行时的
                    // 不一致，不阻断打包，但作者须看到以补全声明。
                    for w in &report.warnings {
                        eprintln!("{}", w.render());
                    }
                    for line in &report.log {
                        eprintln!("{line}");
                    }
                    eprintln!(
                        "OK: {} package(s), {} atlas(es), {} font(s){}",
                        report.packages.len(),
                        report.atlases.len(),
                        report.fonts.len(),
                        if report.warnings.is_empty() {
                            String::new()
                        } else {
                            format!(", {} warning(s)", report.warnings.len())
                        },
                    );
                }
                Format::Json => {
                    println!("{}", CommandOutput::build_ok(report).to_json());
                }
            }
            ExitCode::SUCCESS
        }
        Err(f) => failure_exit("build", &f, format),
    }
}

fn run_verify(root: &std::path::Path, format: Format, unity_editor: Option<PathBuf>) -> ExitCode {
    let ui = match locate_arg(root) {
        Ok(p) => p,
        Err(f) => return failure_exit("verify", &f, format),
    };
    match ikat_pkg::verify::run(&ui, unity_editor.as_deref()) {
        Ok(outcome) => match format {
            Format::Human => {
                for w in &outcome.warnings {
                    eprintln!("{}", w.render());
                }
                eprintln!(
                    "OK: {} asset(s) imported clean via {} (log: {})",
                    outcome.assets_checked,
                    outcome.editor.display(),
                    outcome.log.display()
                );
                ExitCode::SUCCESS
            }
            Format::Json => {
                println!(
                    "{}",
                    CommandOutput::verify_ok(
                        ikat_pkg::report::VerifySummary {
                            assets_checked: outcome.assets_checked,
                            editor: outcome.editor.display().to_string(),
                            log: outcome.log.display().to_string(),
                        },
                        outcome.warnings,
                    )
                    .to_json()
                );
                ExitCode::SUCCESS
            }
        },
        Err(f) => failure_exit("verify", &f, format),
    }
}

fn run_init(
    root: PathBuf,
    ui: Option<PathBuf>,
    agents: Vec<String>,
    unity_root: Option<PathBuf>,
    output: String,
    force: bool,
) -> ExitCode {
    match ikat_pkg::init::init(
        &root,
        ikat_pkg::init::InitOptions {
            agents,
            ui_dir: ui,
            unity_root,
            output_dir: output,
            force,
            cli_source: ikat_pkg::init::CliSource::CurrentExe,
        },
    ) {
        Ok(out) => {
            // cargo new 风格的后续步骤提示（stderr——stdout 保持数据纯净）。
            let split = out.ui != out.root;
            eprintln!(
                "initialized Ikat workspace (session root: {}, ui workspace: {})",
                out.root.display(),
                out.ui.display()
            );
            eprintln!();
            eprintln!("next steps:");
            if split {
                eprintln!(
                    "  1. open ONE agent session at {} — skills + .ikat/ are wired there",
                    out.root.display()
                );
            } else {
                eprintln!(
                    "  1. open an agent session at {} — skills + .ikat/ are wired there",
                    out.root.display()
                );
            }
            eprintln!("  2. ikat new <package>          — create the first UI package");
            eprintln!("  3. put fonts somewhere and:    ikat font add <file> --family <name>");
            eprintln!("  4. put PNGs under a dir and:   ikat atlas add <dir>");
            eprintln!("  5. ikat check                  — iterate until clean, then ikat build");
            if !out.unity_root_written {
                eprintln!();
                eprintln!("note: no --unity-root given — build outputs stay local (output_dir).");
                eprintln!("      re-run from the GUI packer (Ikat > Open Packer) to bind a Unity project.");
            }
            if !out.cli_copied {
                eprintln!();
                eprintln!("warning: could not copy the CLI into .ikat/ (exe busy?); the workspace");
                eprintln!(
                    "         still works via a ikat on PATH or the GitHub Release download."
                );
            }
            ExitCode::SUCCESS
        }
        Err(f) => failure_exit("init", &f, Format::Human),
    }
}

/// 参数目录 → ui 工作区（config 发现：会话根 / ui 本体 / ui 直接子目录）。
fn locate_arg(root: &std::path::Path) -> Result<PathBuf, BuildFailure> {
    Ok(ikat_pkg::config::locate(root)?.ui)
}

/// cwd → 工作区定位（new/list/show/font add/atlas add 的根解析；scaffold 另取 root）。
fn locate_cwd() -> Result<ikat_pkg::config::Located, BuildFailure> {
    let cwd =
        std::env::current_dir().map_err(|e| BuildFailure::config(format!("current dir: {e}")))?;
    ikat_pkg::config::locate(&cwd)
}

fn run_new(name: &str) -> ExitCode {
    let root = match locate_cwd() {
        Ok(l) => l.ui,
        Err(f) => return failure_exit("new", &f, Format::Human),
    };
    match ikat_pkg::workspace_cmd::new_pkg(&root, name) {
        Ok(s) => {
            println!("{}", serde_json::to_string(&s).unwrap());
            eprintln!(
                "created ui/{name}/main.html (package `{}`, registered); run `ikat check` next",
                s.name
            );
            ExitCode::SUCCESS
        }
        Err(f) => failure_exit("new", &f, Format::Human),
    }
}

fn run_list(kind: ListKind, format: Format) -> ExitCode {
    let root = match locate_cwd() {
        Ok(l) => l.ui,
        Err(f) => return failure_exit("list", &f, format),
    };
    let res: Result<serde_json::Value, BuildFailure> = match kind {
        ListKind::Pkg => list_to_json(ikat_pkg::workspace_cmd::list_pkgs(&root), "packages"),
        ListKind::Atlas => list_to_json(ikat_pkg::workspace_cmd::list_atlases(&root), "atlases"),
        ListKind::Font => list_to_json(ikat_pkg::workspace_cmd::list_fonts(&root), "fonts"),
    };
    match res {
        Ok(json) => {
            match format {
                Format::Human => print_human_list(&json),
                Format::Json => println!("{}", serde_json::to_string(&json).unwrap()),
            }
            ExitCode::SUCCESS
        }
        Err(f) => failure_exit("list", &f, format),
    }
}

fn list_to_json<T: serde::Serialize>(
    res: Result<Vec<T>, BuildFailure>,
    key: &str,
) -> Result<serde_json::Value, BuildFailure> {
    let items = res?;
    Ok(serde_json::json!({
        "command": "list",
        "format_version": ikat_pkg::report::FORMAT_VERSION,
        "success": true,
        key: items,
    }))
}

fn print_human_list(json: &serde_json::Value) {
    let (key, head) = if json.get("packages").is_some() {
        ("packages", "package")
    } else if json.get("atlases").is_some() {
        ("atlases", "atlas")
    } else {
        ("fonts", "font")
    };
    let items = json[key].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        eprintln!("no {head}s registered");
        return;
    }
    for it in &items {
        let o = it.as_object().unwrap();
        // 实体主键：pkg/atlas 用 name，font 用 family。
        let (name_key, skip_key) = if key == "fonts" {
            ("family", "family")
        } else {
            ("name", "name")
        };
        let name = o.get(name_key).and_then(|v| v.as_str()).unwrap_or("?");
        let mut parts = vec![name.to_string()];
        for (k, v) in o {
            if k == skip_key {
                continue;
            }
            let vv = match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Array(a) => a
                    .iter()
                    .filter_map(|x| x.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                _ => String::new(),
            };
            parts.push(format!("{k}={vv}"));
        }
        println!("{}", parts.join("  "));
    }
}

/// `ikat preview`：长驻服务（stdout 单 JSON 含 URL）或 `--stop` 停实例。
/// 注意 stop/start 都要会话根（server-info 落盘 `.ikat/preview.json`），
/// 不走 locate_arg（那只给 ui）。
fn run_preview(
    root: &std::path::Path,
    port: Option<u16>,
    idle_timeout: Option<u64>,
    stop: bool,
) -> ExitCode {
    let located = match ikat_pkg::config::locate(root) {
        Ok(l) => l,
        Err(f) => return failure_exit("preview", &f, Format::Human),
    };
    let res = if stop {
        ikat_pkg::preview::stop(&located)
    } else {
        ikat_pkg::preview::start(
            &located,
            &ikat_pkg::preview::PreviewOpts { port, idle_timeout },
        )
    };
    match res {
        Ok(code) => std::process::ExitCode::from(code as u8),
        Err(msg) => failure_exit(
            "preview",
            &ikat_pkg::diag::BuildFailure::config(msg),
            Format::Human,
        ),
    }
}

fn run_scaffold(agents: Vec<String>) -> ExitCode {
    // skills + .ikat CLI 自拷贝 + 版本戳，全会话根（.ikat/config.json 所在——分离
    // 形态下 ≠ ui 目录）。生成物白名单式刷新，config/workspace.json/源文件不动。
    let root = match locate_cwd() {
        Ok(l) => l.root,
        Err(f) => return failure_exit("scaffold", &f, Format::Human),
    };
    let agents = if agents.is_empty() {
        ikat_pkg::scaffold::detect_agents(&root)
    } else {
        agents
    };
    match ikat_pkg::scaffold::refresh_workspace(&root, &agents) {
        Ok(out) => {
            eprintln!(
                "refreshed workspace generated artifacts at {} (skills for {}; cli {})",
                root.display(),
                out.agents.join(", "),
                if out.cli_updated {
                    "updated"
                } else {
                    "already current"
                },
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("scaffold failed: {e}");
            ExitCode::from(2)
        }
    }
}

fn run_show(pkg: &str, format: Format) -> ExitCode {
    let root = match locate_cwd() {
        Ok(l) => l.ui,
        Err(f) => return failure_exit("show", &f, format),
    };
    match ikat_pkg::workspace_cmd::show_pkg(&root, pkg) {
        Ok(d) => {
            match format {
                Format::Human => {
                    println!("package `{}`", d.name);
                    println!("  dirs: {}", d.dirs.join(", "));
                    println!("  pages ({}):", d.pages.len());
                    for p in &d.pages {
                        println!("    {p}");
                    }
                    if d.components.is_empty() {
                        println!("  components: none");
                    } else {
                        println!("  components ({}):", d.components.len());
                        for c in &d.components {
                            println!("    {c}");
                        }
                    }
                }
                Format::Json => println!("{}", serde_json::to_string(&d).unwrap()),
            }
            ExitCode::SUCCESS
        }
        Err(f) => failure_exit("show", &f, format),
    }
}

/// `1920x1080` / `1920X1080` / `1920*1080` → (w, h)。非此形 → None（当未知子命令/参数走 usage）。
fn parse_design_size(s: &str) -> Option<(f32, f32)> {
    let s = s.trim();
    let (w, h) = s.split_once(['x', 'X', '*'])?;
    let w: f32 = w.trim().parse().ok()?;
    let h: f32 = h.trim().parse().ok()?;
    Some((w, h))
}

fn run_design(size: Option<(f32, f32)>, match_mode: Option<String>, clear: bool) -> ExitCode {
    let root = match locate_cwd() {
        Ok(l) => l.ui,
        Err(f) => return failure_exit("design", &f, Format::Human),
    };
    match ikat_pkg::workspace_cmd::set_design(&root, size, match_mode, clear) {
        Ok(echo) => {
            println!("{}", serde_json::to_string(&echo).unwrap());
            eprintln!(
                "design = {}, match_mode = {}; run `ikat build` to bake into ikat.runtime.json",
                match echo.design {
                    Some(d) => format!("{}x{}", d.w as u32, d.h as u32),
                    None => "(unset)".to_string(),
                },
                echo.match_mode.as_deref().unwrap_or("(unset)"),
            );
            ExitCode::SUCCESS
        }
        Err(f) => failure_exit("design", &f, Format::Human),
    }
}

fn run_font_add(file: PathBuf, family: Option<String>, default: bool, fallback: bool) -> ExitCode {
    // 非 TTY / 参数缺失不交互（clig.dev）：family 是必填。
    let Some(family) = family else {
        eprintln!("font add: --family is required (font family name, e.g. --family NotoSansSC)");
        return ExitCode::from(2);
    };
    let root = match locate_cwd() {
        Ok(l) => l.ui,
        Err(f) => return failure_exit("font add", &f, Format::Human),
    };
    match ikat_pkg::workspace_cmd::add_font(&root, &file, &family, default, fallback) {
        Ok(s) => {
            println!("{}", serde_json::to_string(&s).unwrap());
            eprintln!(
                "registered font family `{}` ({}); run `ikat check` to validate",
                s.family, s.file
            );
            ExitCode::SUCCESS
        }
        Err(f) => failure_exit("font add", &f, Format::Human),
    }
}

fn run_font_remove(family: String) -> ExitCode {
    let root = match locate_cwd() {
        Ok(l) => l.ui,
        Err(f) => return failure_exit("font remove", &f, Format::Human),
    };
    match ikat_pkg::workspace_cmd::remove_font(&root, &family) {
        Ok(s) => {
            println!("{}", serde_json::to_string(&s).unwrap());
            eprintln!(
                "removed font family `{}`; source file `{}` left in fonts/ (workspace-owned) — delete it manually if orphaned",
                s.family, s.file
            );
            if s.default {
                eprintln!(
                    "note: no default font family remains; set one with `ikat font default <family>`"
                );
            }
            ExitCode::SUCCESS
        }
        Err(f) => failure_exit("font remove", &f, Format::Human),
    }
}

fn run_font_default(family: String) -> ExitCode {
    let root = match locate_cwd() {
        Ok(l) => l.ui,
        Err(f) => return failure_exit("font default", &f, Format::Human),
    };
    match ikat_pkg::workspace_cmd::set_default_font(&root, &family) {
        Ok(s) => {
            println!("{}", serde_json::to_string(&s).unwrap());
            eprintln!("`{}` is now the single default font family", s.family);
            ExitCode::SUCCESS
        }
        Err(f) => failure_exit("font default", &f, Format::Human),
    }
}

fn run_atlas_add(
    dir: String,
    name: Option<String>,
    max_size: u32,
    padding: u32,
    standalone: bool,
) -> ExitCode {
    let root = match locate_cwd() {
        Ok(l) => l.ui,
        Err(f) => return failure_exit("atlas add", &f, Format::Human),
    };
    match ikat_pkg::workspace_cmd::add_atlas(&root, &dir, name, max_size, padding, standalone) {
        Ok(s) => {
            println!("{}", serde_json::to_string(&s).unwrap());
            eprintln!(
                "registered atlas `{}` (dirs: {}, {} sprite(s)); run `ikat check` to validate",
                s.name,
                s.dirs.join(", "),
                s.sprites
            );
            ExitCode::SUCCESS
        }
        Err(f) => failure_exit("atlas add", &f, Format::Human),
    }
}

fn failure_exit(command: &'static str, f: &BuildFailure, format: Format) -> ExitCode {
    match format {
        Format::Human => {
            // 诊断全量渲染（collect-all：含 warning——修 error 一轮顺带处理）。
            for d in &f.diagnostics {
                eprintln!("{}", d.render());
            }
            eprintln!("{command} failed: {}", f.message);
        }
        Format::Json => {
            // 失败也输出完整文档到 stdout（退出码另行表达成败）。
            println!("{}", CommandOutput::failure(command, f).to_json());
        }
    }
    ExitCode::from(f.exit_code)
}
