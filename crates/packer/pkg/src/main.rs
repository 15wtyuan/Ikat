//! loom CLI：LoomGUI UI 工作区的校验 / 打包 / 初始化 / workspace 编排。
//!
//! 用法（详情见 --help）：
//!   loom check <dir> [--format human|json]
//!   loom build <dir> [--format human|json]
//!   loom init <dir> [--agent claude|agents]... [--unity-root <path>] [--output <dir>] [--force]
//!   loom new <name>
//!   loom list pkg|atlas|font [--format json]
//!   loom show <pkg> [--format json]
//!   loom font add <file> --family <f> [--default] [--fallback]
//!   loom atlas add <dir> [--name <n>] [--max-size <n>] [--padding <n>] [--standalone]
//!   loom version [--format json]
//!
//! 输出约定：human 模式（默认）诊断/进度走 stderr；`--format json` 时 stdout 输出单个
//! JSON 文档（数据），进度走 stderr。退出码：0 干净 · 1 有 Error 级诊断 / 写命令冲突 ·
//! 2 用法/配置/io 错。`new`/`list`/`show`/`font add`/`atlas add` 在当前目录（须为工作区
//! 根）执行；`check`/`build`/`init` 接目录参数。

use loomgui_pkg::diag::BuildFailure;
use loomgui_pkg::report::{CommandOutput, VersionInfo};
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
    Init {
        root: PathBuf,
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
    AtlasAdd {
        dir: String,
        name: Option<String>,
        max_size: u32,
        padding: u32,
        standalone: bool,
    },
    Version {
        format: Format,
    },
}

fn usage() -> ! {
    let bin = bin_name();
    eprintln!("loom — LoomGUI UI workspace CLI");
    eprintln!();
    eprintln!("usage:");
    eprintln!(
        "  {bin} check [<dir>] [--format human|json]     validate (fence/registry/assets), writes nothing"
    );
    eprintln!(
        "  {bin} build [<dir>] [--format human|json]     check + write artifacts to output_dir"
    );
    eprintln!(
        "  {bin} init <dir> [--agent <kind>]... [--unity-root <path>] [--output <dir>] [--force]"
    );
    eprintln!(
        "                                            scaffold workspace + copy CLI to <dir>/.loom/"
    );
    eprintln!(
        "  {bin} new <name>                            create ui/<name>/main.html + register package"
    );
    eprintln!("  {bin} list pkg|atlas|font [--format json]   summary of workspace entities");
    eprintln!("  {bin} show <pkg> [--format json]            package detail (pages + components)");
    eprintln!("  {bin} font add <file> --family <f> [--default] [--fallback]");
    eprintln!(
        "  {bin} atlas add <dir> [--name <n>] [--max-size <n>] [--padding <n>] [--standalone]"
    );
    eprintln!("  {bin} version [--format json]");
    eprintln!();
    eprintln!("exit codes: 0 clean (warnings allowed) · 1 errors / command conflict · 2 usage/config failure");
    eprintln!();
    eprintln!(
        "`new`/`list`/`show`/`font add`/`atlas add` run in the current directory (workspace root)."
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
        .unwrap_or_else(|| "loom".to_string())
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
                || a == "--unity-root"
                || a == "--output"
                || a == "--family"
                || a == "--name"
                || a == "--max-size"
                || a == "--padding"
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
        "check" | "build" => {
            // <dir> 可选：缺省当前目录（AI 在工作区内裸跑 `loom check` 的主形态）。
            let root = match scan.positional() {
                Some(d) => PathBuf::from(d),
                None => std::env::current_dir().ok()?,
            };
            let format = parse_format(rest)?;
            Some(if sub == "check" {
                Cmd::Check { root, format }
            } else {
                Cmd::Build { root, format }
            })
        }
        "init" => Some(Cmd::Init {
            root: PathBuf::from(scan.positional()?),
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
        "font" => {
            // font add <file> --family <f> [--default] [--fallback]
            if scan.positional()?.as_str() != "add" {
                return None;
            }
            Some(Cmd::FontAdd {
                file: PathBuf::from(scan.positional()?),
                family: scan.flag_value("--family"),
                default: scan.has("--default"),
                fallback: scan.has("--fallback"),
            })
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
        Cmd::Init {
            root,
            agents,
            unity_root,
            output,
            force,
        } => run_init(root, agents, unity_root, output, force),
        Cmd::New { name } => run_new(&name),
        Cmd::List { kind, format } => run_list(kind, format),
        Cmd::Show { pkg, format } => run_show(&pkg, format),
        Cmd::FontAdd {
            file,
            family,
            default,
            fallback,
        } => run_font_add(file, family, default, fallback),
        Cmd::AtlasAdd {
            dir,
            name,
            max_size,
            padding,
            standalone,
        } => run_atlas_add(dir, name, max_size, padding, standalone),
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
    match loomgui_pkg::build::analyze(root) {
        Ok(outcome) => {
            // 全量 warning：registry 侧 + 各包侧（与 build 成功时的 BuildReport 同口径）。
            let mut warnings = outcome.warnings;
            for (_, pr) in &outcome.packages {
                warnings.extend(pr.warnings.iter().cloned());
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
    match loomgui_pkg::build::build(root) {
        Ok(report) => {
            match format {
                Format::Human => {
                    // 围栏一致性 warning（W1/W2）打到 stderr：合法但预览≠运行时的
                    // 不一致，不阻断打包，但作者须看到以补全声明。
                    for w in &report.warnings {
                        eprintln!("{}", w.render());
                    }
                    for line in &report.log {
                        eprintln!("{line}");
                    }
                    eprintln!(
                        "OK: {} atlases, {} fonts{}",
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

fn run_init(
    root: PathBuf,
    agents: Vec<String>,
    unity_root: Option<PathBuf>,
    output: String,
    force: bool,
) -> ExitCode {
    match loomgui_pkg::init::init(
        &root,
        loomgui_pkg::init::InitOptions {
            agents,
            unity_root,
            output_dir: output,
            force,
            cli_source: loomgui_pkg::init::CliSource::CurrentExe,
        },
    ) {
        Ok(out) => {
            // cargo new 风格的后续步骤提示（stderr——stdout 保持数据纯净）。
            eprintln!("initialized LoomGUI workspace at {}", out.root.display());
            eprintln!();
            eprintln!("next steps:");
            eprintln!("  1. cd into the workspace and open an agent session (AGENTS.md is ready)");
            eprintln!("  2. loom new <package>          — create the first UI package");
            eprintln!("  3. put fonts somewhere and:    loom font add <file> --family <name>");
            eprintln!("  4. put PNGs under a dir and:   loom atlas add <dir>");
            eprintln!("  5. loom check                  — iterate until clean, then loom build");
            if !out.unity_root_written {
                eprintln!();
                eprintln!("note: no --unity-root given — build outputs stay local (output_dir).");
                eprintln!("      re-run from the GUI packer (LoomGUI > Open Packer) to bind a Unity project.");
            }
            if !out.cli_copied {
                eprintln!();
                eprintln!("warning: could not copy the CLI into .loom/ (exe busy?); the workspace");
                eprintln!(
                    "         still works via a loom on PATH or the GitHub Release download."
                );
            }
            ExitCode::SUCCESS
        }
        Err(f) => failure_exit("init", &f, Format::Human),
    }
}

/// 当前目录即工作区根（new/list/show/font add/atlas add 的根解析）。
fn cwd_root() -> Result<PathBuf, BuildFailure> {
    std::env::current_dir().map_err(|e| BuildFailure::config(format!("current dir: {e}")))
}

fn run_new(name: &str) -> ExitCode {
    let root = match cwd_root() {
        Ok(r) => r,
        Err(f) => return failure_exit("new", &f, Format::Human),
    };
    match loomgui_pkg::workspace_cmd::new_pkg(&root, name) {
        Ok(s) => {
            println!("{}", serde_json::to_string(&s).unwrap());
            eprintln!(
                "created ui/{name}/main.html (package `{}`, registered); run `loom check` next",
                s.name
            );
            ExitCode::SUCCESS
        }
        Err(f) => failure_exit("new", &f, Format::Human),
    }
}

fn run_list(kind: ListKind, format: Format) -> ExitCode {
    let root = match cwd_root() {
        Ok(r) => r,
        Err(f) => return failure_exit("list", &f, format),
    };
    let res: Result<serde_json::Value, BuildFailure> = match kind {
        ListKind::Pkg => list_to_json(loomgui_pkg::workspace_cmd::list_pkgs(&root), "packages"),
        ListKind::Atlas => list_to_json(loomgui_pkg::workspace_cmd::list_atlases(&root), "atlases"),
        ListKind::Font => list_to_json(loomgui_pkg::workspace_cmd::list_fonts(&root), "fonts"),
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
        "format_version": loomgui_pkg::report::FORMAT_VERSION,
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

fn run_show(pkg: &str, format: Format) -> ExitCode {
    let root = match cwd_root() {
        Ok(r) => r,
        Err(f) => return failure_exit("show", &f, format),
    };
    match loomgui_pkg::workspace_cmd::show_pkg(&root, pkg) {
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

fn run_font_add(file: PathBuf, family: Option<String>, default: bool, fallback: bool) -> ExitCode {
    // 非 TTY / 参数缺失不交互（clig.dev）：family 是必填。
    let Some(family) = family else {
        eprintln!("font add: --family is required (font family name, e.g. --family NotoSansSC)");
        return ExitCode::from(2);
    };
    let root = match cwd_root() {
        Ok(r) => r,
        Err(f) => return failure_exit("font add", &f, Format::Human),
    };
    match loomgui_pkg::workspace_cmd::add_font(&root, &file, &family, default, fallback) {
        Ok(s) => {
            println!("{}", serde_json::to_string(&s).unwrap());
            eprintln!(
                "registered font family `{}` ({}); run `loom check` to validate",
                s.family, s.file
            );
            ExitCode::SUCCESS
        }
        Err(f) => failure_exit("font add", &f, Format::Human),
    }
}

fn run_atlas_add(
    dir: String,
    name: Option<String>,
    max_size: u32,
    padding: u32,
    standalone: bool,
) -> ExitCode {
    let root = match cwd_root() {
        Ok(r) => r,
        Err(f) => return failure_exit("atlas add", &f, Format::Human),
    };
    match loomgui_pkg::workspace_cmd::add_atlas(&root, &dir, name, max_size, padding, standalone) {
        Ok(s) => {
            println!("{}", serde_json::to_string(&s).unwrap());
            eprintln!(
                "registered atlas `{}` (dirs: {}, {} sprite(s)); run `loom check` to validate",
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
