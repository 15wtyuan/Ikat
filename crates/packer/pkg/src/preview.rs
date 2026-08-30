//! `ikat preview` — 人类预览工作台：本地只读 HTTP server，serve 工作区源文件 +
//! 内嵌外壳，把「AI 写完页面 → 人类浏览器过目 → 进 Unity」补成闭环。
//!
//! 设计要点（grilling 定案，2026-08；#92 分层修订）：
//! - 忠实预览：分辨率切换由外壳按 match_mode 缩放设计分辨率渲染，永不触发浏览器
//!   reflow——core 按固定设计分辨率 solve，预览必须预测运行时。
//! - 注入分层（#92）：A 层「单真相语义」（组件展开 / 控件语义 / 结构 polyfill）
//!   嵌在二进制里，boot 入口对每个 HTML 页**恒注入**、经 `/ikat-preview/lib/*`
//!   路由供给——与 CLI 版本天然同版本，工作区不拷贝（复刻物 = 第二真相源）。
//!   B 层「消费侧语义」（演示数据 / 页面导航 / 页面交互）归工作区：
//!   `<包目录>/preview/main.js` 全页 + `preview/pages/<页名>.js` 按页，文件存在
//!   才追加注入。HTML 源零引用、不进打包。
//! - 生命周期（吸收 superpowers 踩坑）：per-workspace 稳定端口（路径哈希
//!   41000–41999，浏览器 tab 跨重启不断链）；server-info 落盘
//!   `<会话根>/.ikat/preview.json` 兜底「AI 后台起进程 stdout 被吞」；空闲超时
//!   自杀（默认 4h——30 分钟会打断人类评审）；`--stop` 先验 token 归属再杀、
//!   确认死亡才算成功；属主进程死亡追踪尽力而为，空闲超时兜底一切。
//! - 安全：只绑 127.0.0.1；Host 头白名单（防 DNS rebinding）；路径沙箱
//!   （canonicalize 后必须仍在工作区内，拒绝点开头的路径段——`.ikat`/`.git`
//!   不可达）。

use crate::config::Located;
use crate::workspace::{self, Workspace};
use serde_json::json;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 稳定端口段：[41000, 42000)。
const PORT_BASE: u16 = 41_000;
const PORT_SPAN: u16 = 1_000;
/// 空闲超时缺省：4 小时（superpowers 实证 30 分钟会打断人类评审会话）。
const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 4 * 60 * 60;
/// server-info 落盘文件（会话根 `.ikat/` 下，路径沙箱使其不经 HTTP 可达）。
pub const INFO_FILE: &str = "preview.json";

// ---------------------------------------------------------------------------
// 入口（CLI 侧调用）
// ---------------------------------------------------------------------------

pub struct PreviewOpts {
    pub port: Option<u16>,
    pub idle_timeout: Option<u64>,
}

/// `ikat preview`（长驻）：定位工作区 → 复用/起服务 → stdout 吐单 JSON → 阻塞至
/// 超时/被停/属主死亡。返回值只在启动失败时出现（成功不返回）。
pub fn start(located: &Located, opts: &PreviewOpts) -> Result<i32, String> {
    let ui = &located.ui;
    let ws = workspace::load_workspace(ui).map_err(|e| format!("{e}（preview 需要合法工作区）"))?;

    // 复用：已在跑且同一工作区 → 直接报 URL 退出（不叠第二个服务）。
    if let Some(prev) = read_info(&located.root) {
        if prev.workspace == ui_string(ui) {
            if let Some(info) = ping(&prev) {
                let _ = info; // ping 通即活
                print_started(&prev, true);
                return Ok(0);
            }
        }
    }

    let idle_timeout = Duration::from_secs(opts.idle_timeout.unwrap_or(DEFAULT_IDLE_TIMEOUT_SECS));
    let listener = bind_listener(ui, opts.port)?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("local addr: {e}"))?
        .port();
    let info = ServerInfo {
        pid: std::process::id(),
        port,
        token: new_token(),
        workspace: ui_string(ui),
    };
    write_info(&located.root, &info);

    let fonts_css = build_fonts_css(ui, &ws.fonts);
    let shared = Arc::new(Shared {
        ui: ui.clone(),
        ws,
        token: info.token.clone(),
        fonts_css,
        last_request: Mutex::new(Instant::now()),
        shutdown: AtomicBool::new(false),
    });
    // accept 线程拿克隆的 Arc；主线程留原 Arc 做监控（shutdown flag 共享可见）。
    let accept_shared = Arc::clone(&shared);
    print_started(&info, false);
    eprintln!(
        "preview: serving {} (pid {}, idle timeout {}s); Ctrl+C or `ikat preview --stop` to quit",
        info.workspace,
        info.pid,
        idle_timeout.as_secs()
    );

    std::thread::spawn(move || accept_loop(listener, accept_shared));
    // 主线程 = 监控：关停 flag / 空闲超时 / 属主死亡。空闲超时兜底一切。
    let owner = parent_pid();
    let mut tick: u32 = 0;
    loop {
        std::thread::sleep(Duration::from_secs(1));
        if shared.shutdown.load(Ordering::SeqCst) {
            break;
        }
        let idle_for = shared
            .last_request
            .lock()
            .map(|t| t.elapsed())
            .unwrap_or_default();
        if idle_for > idle_timeout {
            eprintln!("preview: idle {}s > timeout, exiting", idle_for.as_secs());
            break;
        }
        tick = tick.wrapping_add(1);
        if tick.is_multiple_of(30) {
            if let Some(pid) = owner.filter(|p| !pid_alive(*p)) {
                eprintln!("preview: owner process {pid} gone, exiting");
                break;
            }
        }
    }
    let _ = std::fs::remove_file(info_path(&located.root));
    Ok(0)
}

/// `ikat preview --stop`：读 server-info → ping 验明正身 → 请求关停 → 确认死亡
/// （强杀兜底）。归属验证失败绝不杀（防 PID 复用误杀无关进程）。
pub fn stop(located: &Located) -> Result<i32, String> {
    let Some(info) = read_info(&located.root) else {
        return Err(
            "no preview server on record for this workspace (missing .ikat/preview.json)".into(),
        );
    };
    let Some(got) = ping(&info) else {
        // 记录在案但 ping 不通：残留文件（如断电）。清掉记录即可。
        let _ = std::fs::remove_file(info_path(&located.root));
        return Err(format!(
            "recorded preview (pid {}, port {}) is not responding; stale record removed",
            info.pid, info.port
        ));
    };
    if got.get("token").and_then(|t| t.as_str()) != Some(info.token.as_str()) {
        return Err(format!(
            "port {} is now owned by a different process (token mismatch); refusing to kill pid {}",
            info.port, info.pid
        ));
    }
    request_shutdown(&info);
    for _ in 0..30 {
        if ping(&info).is_none() {
            let _ = std::fs::remove_file(info_path(&located.root));
            println!(
                "{}",
                json!({"command": "preview", "action": "stop", "success": true,
                       "port": info.port, "pid": info.pid})
            );
            return Ok(0);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    force_kill(info.pid)?;
    let _ = std::fs::remove_file(info_path(&located.root));
    println!(
        "{}",
        json!({"command": "preview", "action": "stop", "success": true, "forced": true,
               "port": info.port, "pid": info.pid})
    );
    Ok(0)
}

fn print_started(info: &ServerInfo, reused: bool) {
    let url = format!("http://127.0.0.1:{}", info.port);
    println!(
        "{}",
        json!({
            "command": "preview",
            "format_version": crate::report::FORMAT_VERSION,
            "success": true,
            "url": url,
            "port": info.port,
            "pid": info.pid,
            "workspace": info.workspace,
            "reused": reused,
        })
    );
}

// ---------------------------------------------------------------------------
// server-info 落盘
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ServerInfo {
    pub pid: u32,
    pub port: u16,
    pub token: String,
    /// 工作区绝对路径的字符串形（比对复用/归属用）。
    pub workspace: String,
}

fn info_path(root: &Path) -> PathBuf {
    root.join(".ikat").join(INFO_FILE)
}

fn read_info(root: &Path) -> Option<ServerInfo> {
    let text = std::fs::read_to_string(info_path(root)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_info(root: &Path, info: &ServerInfo) {
    let path = info_path(root);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut text) = serde_json::to_string_pretty(info) {
        text.push('\n'); // 尾换行：文本文件常规收尾，防重写伪 diff（同 save_workspace）
        let _ = std::fs::write(path, text);
    }
}

fn ui_string(ui: &Path) -> String {
    // 正斜杠归一：Windows 下不同调用路径可能产生 / 与 \ 混排，比对要稳定。
    ui.to_string_lossy().replace('\\', "/")
}

// ---------------------------------------------------------------------------
// 端口选择：per-workspace 稳定哈希 + 段内确定性探测
// ---------------------------------------------------------------------------

/// FNV-1a 32 位（手搓而非 DefaultHasher：后者跨 Rust 版本不保证稳定，端口
/// 必须跨版本不漂移，否则浏览器 tab 的「稳定端口」承诺失效）。
fn fnv1a(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

fn bind_listener(ui: &Path, explicit: Option<u16>) -> Result<TcpListener, String> {
    if let Some(p) = explicit {
        return TcpListener::bind(("127.0.0.1", p)).map_err(|e| {
            format!("bind 127.0.0.1:{p}: {e}（端口被占？`ikat preview --port <n>` 换端口）")
        });
    }
    let base = PORT_BASE + (fnv1a(&ui_string(ui)) % u32::from(PORT_SPAN)) as u16;
    let mut last_err = String::new();
    for i in 0..PORT_SPAN {
        let port = PORT_BASE + (base - PORT_BASE + i) % PORT_SPAN;
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(l) => return Ok(l),
            Err(e) => last_err = format!("bind 127.0.0.1:{port}: {e}"),
        }
    }
    Err(format!(
        "no free port in {PORT_BASE}..{}: {last_err}",
        PORT_BASE + PORT_SPAN - 1
    ))
}

// ---------------------------------------------------------------------------
// 进程工具（属主追踪 / 强杀）——Windows 主平台，尽力而为
// ---------------------------------------------------------------------------

/// 本进程的父进程 PID（AI 后台起服务时 = agent shell；取不到则放弃属主追踪）。
fn parent_pid() -> Option<u32> {
    #[cfg(windows)]
    {
        let me = std::process::id();
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "(Get-CimInstance Win32_Process -Filter 'ProcessId={me}').ParentProcessId"
                ),
            ])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn pid_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        // CSV 行以引号开头（映像名）；无匹配时 tasklist 输出本地化 INFO 行，不带引号。
        match std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
        {
            Ok(o) => String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|l| l.starts_with('"')),
            Err(_) => true, // 查不动时保守视为活着（空闲超时兜底）
        }
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .is_ok_and(|o| o.status.success())
    }
}

fn force_kill(pid: u32) -> Result<(), String> {
    #[cfg(windows)]
    {
        std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output()
            .map_err(|e| format!("taskkill {pid}: {e}"))
            .and_then(|o| {
                if o.status.success() {
                    Ok(())
                } else {
                    Err(format!(
                        "taskkill {pid} failed: {}",
                        String::from_utf8_lossy(&o.stderr)
                    ))
                }
            })
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output()
            .map_err(|e| format!("kill {pid}: {e}"))
            .and_then(|o| {
                o.status
                    .success()
                    .then_some(())
                    .ok_or_else(|| "kill failed".into())
            })
    }
}

/// 起一个随机 token（RandomState 的哈希键每次进程随机 → 输出不可预测）。
fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u32(std::process::id());
    h.write_u64(Instant::now().elapsed().as_nanos() as u64);
    format!("{:016x}", h.finish())
}

// ---------------------------------------------------------------------------
// HTTP server（手搓最小实现：GET/HEAD/POST-shutdown，Connection: close）
// ---------------------------------------------------------------------------

struct Shared {
    ui: PathBuf,
    ws: Workspace,
    token: String,
    /// 启动时按 ws.fonts 生成的字体样式（#104，快照语义与 ws 一致：改 fonts
    /// 段重启 preview 生效）。空 = 工作区无可注入字体。
    fonts_css: String,
    last_request: Mutex<Instant>,
    shutdown: AtomicBool,
}

fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    for conn in listener.incoming() {
        let Ok(stream) = conn else { continue };
        let shared = Arc::clone(&shared);
        std::thread::spawn(move || handle_conn(stream, &shared));
    }
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn handle_conn(mut stream: TcpStream, shared: &Shared) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let Some(req) = read_request(&mut stream) else {
        return;
    };
    let resp = route(&req, shared);
    *shared
        .last_request
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Instant::now();
    let _ = write_response(&mut stream, &req, resp);
}

/// 读到头部结束（\r\n\r\n）+ 有 Content-Length 时把 body 读完（否则对端可能
/// 收 RST 而非响应）。上限 64KB 头 / 1MB body，防恶意/异常连接占内存。
fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(pos) = find_subsequence(&buf, b"\r\n\r\n") {
            break pos;
        }
        if buf.len() > 64 * 1024 {
            return None;
        }
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let mut headers = Vec::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let content_length: usize = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    if content_length > 1024 * 1024 {
        return None;
    }
    let mut body_read = buf.len() - head_end - 4;
    while body_read < content_length {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        body_read += n;
    }
    Some(Request {
        method,
        path,
        headers,
    })
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

struct Response {
    status: u16,
    reason: &'static str,
    content_type: String,
    body: Vec<u8>,
    /// 工作区静态资产的 revalidate 缓存（#96）：`no-cache` + `Last-Modified`。
    /// None = 恒 no-store（HTML 注入产物与内嵌资产：注入内容依赖外部文件存在性，
    /// 不能按 mtime 复用）。
    last_modified: Option<String>,
}

impl Response {
    fn new(
        status: u16,
        reason: &'static str,
        content_type: impl Into<String>,
        body: impl Into<Vec<u8>>,
    ) -> Self {
        Self {
            status,
            reason,
            content_type: content_type.into(),
            body: body.into(),
            last_modified: None,
        }
    }
    fn json(status: u16, reason: &'static str, value: serde_json::Value) -> Self {
        Self::new(status, reason, "application/json", value.to_string())
    }
    fn text(status: u16, reason: &'static str, msg: &str) -> Self {
        Self::new(status, reason, "text/plain; charset=utf-8", msg.to_string())
    }
}

fn write_response(stream: &mut TcpStream, req: &Request, resp: Response) -> std::io::Result<()> {
    // #96：静态资产 revalidate 化——no-store 会让 25MB 级工作区字体在每次导航
    // 全量重传，把 @font-face `font-display: block` 的隐形文字窗拉成常态（布局
    // 占位在、字形不画、!important 救不了——非级联问题）。no-cache + Last-Modified
    // + 304 保持「每次校验、改动即刻生效」的活文件语义，只免重传。
    let (status, reason, extra) = if resp.status == 304 {
        let lm = resp.last_modified.as_deref().unwrap_or_default();
        (
            304,
            "Not Modified",
            format!("Cache-Control: no-cache\r\nLast-Modified: {lm}\r\n"),
        )
    } else if let Some(lm) = resp.last_modified {
        (
            resp.status,
            resp.reason,
            format!("Cache-Control: no-cache\r\nLast-Modified: {lm}\r\n"),
        )
    } else {
        (
            resp.status,
            resp.reason,
            "Cache-Control: no-store\r\n".to_string(),
        )
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
        resp.content_type,
        resp.body.len()
    );
    stream.write_all(head.as_bytes())?;
    if req.method != "HEAD" && resp.status != 304 {
        stream.write_all(&resp.body)?;
    }
    stream.flush()
}

fn route(req: &Request, shared: &Shared) -> Response {
    // Host 白名单：防 DNS rebinding（恶意域名解析到 127.0.0.1 后同源读内容）。
    let host_ok = req
        .header("host")
        .map(|h| {
            let bare = h.rsplit_once(':').map(|(a, _)| a).unwrap_or(h);
            bare == "127.0.0.1" || bare == "localhost" || bare == "[::1]"
        })
        .unwrap_or(false);
    if !host_ok {
        return Response::text(403, "Forbidden", "bad Host (preview binds 127.0.0.1 only)");
    }
    let path = percent_decode(req.path.split('?').next().unwrap_or(""));

    match (req.method.as_str(), path.as_str()) {
        ("GET", "/") | ("GET", "/index.html") | ("HEAD", "/") => {
            Response::new(200, "OK", "text/html; charset=utf-8", shell::INDEX)
        }
        ("GET", "/shell/app.js") | ("HEAD", "/shell/app.js") => {
            Response::new(200, "OK", "text/javascript", shell::APP_JS)
        }
        ("GET", "/shell/style.css") | ("HEAD", "/shell/style.css") => {
            Response::new(200, "OK", "text/css", shell::STYLE_CSS)
        }
        // A 层库路由（#92）：boot 自动注入引这里；工作区 B 层脚本 import 这些
        // 绝对 URL——内容嵌在二进制里，与 CLI 版本天然同版本。
        (m, p) if p.starts_with("/ikat-preview/lib/") && (m == "GET" || m == "HEAD") => {
            match p.rsplit('/').next().unwrap_or("") {
                "boot.js" => {
                    Response::new(200, "OK", "text/javascript; charset=utf-8", simlib::BOOT_JS)
                }
                "expand.js" => Response::new(
                    200,
                    "OK",
                    "text/javascript; charset=utf-8",
                    simlib::EXPAND_JS,
                ),
                "controls.js" => Response::new(
                    200,
                    "OK",
                    "text/javascript; charset=utf-8",
                    simlib::CONTROLS_JS,
                ),
                "fill.js" => {
                    Response::new(200, "OK", "text/javascript; charset=utf-8", simlib::FILL_JS)
                }
                "base.css" => Response::new(200, "OK", "text/css; charset=utf-8", simlib::BASE_CSS),
                _ => Response::text(404, "Not Found", "not found"),
            }
        }
        ("GET", "/api/workspace.json") => api_workspace(shared),
        // 组件作用域 CSS（#95 / #94 前半步）：server 用 fence 同一入口抽组件样式，
        // 双分支改写后供给——客户端 expand.js 只注入本链接，CSS 语义单真相在 Rust。
        (m, p) if p.starts_with("/ikat-preview/comp-style/") && (m == "GET" || m == "HEAD") => {
            serve_comp_style(shared, &p["/ikat-preview/comp-style/".len()..])
        }
        ("GET", "/_ikat/ping") => Response::json(
            200,
            "OK",
            json!({"ok": true, "token": shared.token, "pid": std::process::id()}),
        ),
        ("POST", "/_ikat/shutdown") => match req.header("x-ikat-token") {
            Some(t) if t == shared.token => {
                shared.shutdown.store(true, Ordering::SeqCst);
                Response::json(200, "OK", json!({"ok": true}))
            }
            _ => Response::text(403, "Forbidden", "bad token"),
        },
        (m, p) if p.starts_with("/ws/") => {
            if m != "GET" && m != "HEAD" {
                return Response::text(405, "Method Not Allowed", "GET/HEAD only");
            }
            serve_workspace_file(&shared.ui, &p[4..], &shared.fonts_css, req)
        }
        ("GET", "/favicon.ico") | ("HEAD", "/favicon.ico") => {
            Response::new(204, "No Content", "image/x-icon", [])
        }
        _ => Response::text(404, "Not Found", "not found"),
    }
}

/// `/api/workspace.json`：外壳左树 + 模拟脚本（组件展开 recipe）的唯一数据源。
/// 包/页结构用与打包同一套 resolve_html_list（口径不漂移）；组件注册表直接
/// 复用打包期扫描器（名字 → 源文件路径）。
fn api_workspace(shared: &Shared) -> Response {
    let ws = &shared.ws;
    let mut packages = Vec::new();
    for pkg in &ws.packages {
        let pages = crate::build::resolve_html_list(&shared.ui, pkg)
            .unwrap_or_default()
            .into_iter()
            .map(|rel| {
                let stem = Path::new(&rel)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string();
                let page_dir = Path::new(&rel).parent().unwrap_or(Path::new(""));
                json!({
                    "name": stem,
                    "path": rel,
                    "url": format!("/ws/{rel}"),
                    "has_sim": shared.ui.join(page_dir).join(format!("preview/pages/{stem}.js")).is_file(),
                })
            })
            .collect::<Vec<_>>();
        packages.push(json!({"name": pkg.name, "pages": pages}));
    }
    let components = crate::expand::scan_component_registry(&shared.ui, &ws.packages)
        .map(|(reg, _)| {
            serde_json::Map::from_iter(
                reg.entries()
                    .map(|(name, def)| (name.clone(), json!(def.html_rel))),
            )
        })
        .unwrap_or_default();
    let design = ws
        .design
        .as_ref()
        .map(|d| json!([d.w, d.h]))
        .unwrap_or(serde_json::Value::Null);
    Response::json(
        200,
        "OK",
        json!({
            "design": design,
            "match_mode": ws.match_mode.clone().unwrap_or_else(|| "letterbox".into()),
            "packages": packages,
            "components": serde_json::Value::Object(components),
        }),
    )
}

/// `/ikat-preview/comp-style/<name>.css`：组件样式的浏览器作用域版。fence 同一
/// 入口（`parse_html_to_ir_named`）抽 `<style>` 文本 + `<link rel=stylesheet>`
/// 内容，`preview_comp_style::rewrite_component_css` 双分支改写后供给。注册表
/// 复用打包期扫描器（live 读源——预览哲学：读活文件，不快照打包期解析）。
fn serve_comp_style(shared: &Shared, rest: &str) -> Response {
    let Some(name) = rest.strip_suffix(".css") else {
        return Response::text(404, "Not Found", "expected <name>.css");
    };
    // 组件名围栏 [a-z0-9-]（注册表入口已验）：内插进改写器的属性选择器前的最后防线。
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Response::text(404, "Not Found", "bad component name");
    }
    let Ok((reg, _)) = crate::expand::scan_component_registry(&shared.ui, &shared.ws.packages)
    else {
        return Response::text(
            500,
            "Internal Server Error",
            "component registry has errors (see `ikat check`)",
        );
    };
    let Some((_, def)) = reg.entries().find(|(n, _)| n.as_str() == name) else {
        return Response::text(404, "Not Found", "unknown component");
    };
    let html_rel = def.html_rel.clone();
    let Ok(src) = std::fs::read_to_string(shared.ui.join(&html_rel)) else {
        return Response::text(404, "Not Found", "component source unreadable");
    };
    let raw = ikat_fence::tree_builder::parse_html_to_ir_named(&src, html_rel.clone());
    let mut css = String::new();
    for st in &raw.style_texts {
        css.push_str(st);
        css.push('\n');
    }
    // 组件 <link rel=stylesheet> 同页面待遇：href 相对组件文件解析读入。找不到 →
    // 留注释（打包侧是 FenceStylesheetNotFound error——preview 不放行假样式）。
    let comp_dir = html_rel
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default();
    for (href, _) in &raw.stylesheet_links {
        let loaded = crate::preview_comp_style::resolve_ws_rel(&comp_dir, href.trim())
            .filter(|_| !href.contains("://"))
            .and_then(|rel| std::fs::read_to_string(shared.ui.join(&rel)).ok());
        match loaded {
            Some(text) => {
                css.push_str(&text);
                css.push('\n');
            }
            None => css.push_str(&format!(
                "/* preview: component stylesheet `{href}` not found — `ikat build` errors on it */\n"
            )),
        }
    }
    let body = crate::preview_comp_style::rewrite_component_css(name, &css, &html_rel);
    Response::new(200, "OK", "text/css; charset=utf-8", body)
}

/// serve 工作区文件：路径沙箱（canonicalize 后必须仍在工作区内；拒绝点开头
/// 路径段）+ HTML 注入两个 ESM 入口（存在才注入）。
///
/// 缓存口径（#96）：HTML 恒 no-store（注入产物依赖外部脚本存在性，不能按 mtime
/// 复用）；其余静态资产（字体/图/CSS/JS）revalidate——`no-cache`、`Last-Modified`、
/// `If-Modified-Since` 命中即 304。活文件语义不变（每次导航都校验 mtime，改动
/// 即刻 200 新字节），但免重传——工作区 25MB 级字体在 no-store 下每次导航全量
/// 重传，把 @font-face `font-display: block` 的隐形文字窗拉成常态。
fn serve_workspace_file(ui: &Path, rel: &str, fonts_css: &str, req: &Request) -> Response {
    let Some(safe_rel) = normalize_rel(rel) else {
        return Response::text(403, "Forbidden", "path escapes workspace");
    };
    let full = ui.join(&safe_rel);
    let canon_root = match std::fs::canonicalize(ui) {
        Ok(p) => p,
        Err(_) => return Response::text(500, "Internal Server Error", "workspace root unreadable"),
    };
    let canon = match std::fs::canonicalize(&full) {
        Ok(p) => p,
        Err(_) => return Response::text(404, "Not Found", "not found"),
    };
    if !canon.starts_with(&canon_root) {
        return Response::text(403, "Forbidden", "path escapes workspace");
    }
    if !canon.is_file() {
        return Response::text(404, "Not Found", "not found");
    }
    let mime = mime_for(&canon);
    let mtime = std::fs::metadata(&canon)
        .and_then(|m| m.modified())
        .ok()
        .map(http_date);
    // 注入判定按前缀（text/* 已带 charset 后缀，等值比较会漏判）。
    if mime.starts_with("text/html") {
        let mut body = match std::fs::read(&canon) {
            Ok(b) => b,
            Err(_) => return Response::text(500, "Internal Server Error", "read failed"),
        };
        let dir = canon.parent().unwrap_or(Path::new("")).to_path_buf();
        let stem = canon
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let html = String::from_utf8_lossy(&body).into_owned();
        body = inject_preview_scripts(&html, &dir, &stem, fonts_css).into_bytes();
        return Response::new(200, "OK", mime, body);
    }
    // revalidate：浏览器回传的 If-Modified-Since 与我们发出的 Last-Modified 同源
    // （浏览器原样回显），字符串等值即命中——省去 HTTP-date 解析。
    if let (Some(lm), Some(ims)) = (&mtime, req.header("if-modified-since")) {
        if lm == ims {
            let mut resp = Response::new(304, "Not Modified", mime, []);
            resp.last_modified = Some(lm.clone());
            return resp;
        }
    }
    let mut resp = match std::fs::read(&canon) {
        Ok(b) => Response::new(200, "OK", mime, b),
        Err(_) => return Response::text(500, "Internal Server Error", "read failed"),
    };
    resp.last_modified = mtime;
    resp
}

/// SystemTime → RFC 7231 HTTP-date（`Sun, 06 Nov 1994 08:49:37 GMT`）。仅服务端
/// 自产自销（Last-Modified 产出 + 与浏览器回显串等值比较），不解析外来日期。
fn http_date(t: std::time::SystemTime) -> String {
    let secs = t
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    // civil_from_days（Howard Hinnant）：天数 → (y, m, d)
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    const WEEKDAY: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"]; // 1970-01-01 = 周四
    const MONTH: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
        WEEKDAY[days.rem_euclid(7) as usize],
        d,
        MONTH[(mo - 1) as usize],
        y,
        h,
        m,
        s
    )
}

/// 相对路径规整：URL 解码后逐段校验——拒绝 `..`/反斜杠/点开头段（`.ikat`、
/// `.git` 不可达）。返回以正斜杠重组的工作区相对路径。
fn normalize_rel(rel: &str) -> Option<String> {
    let mut parts = Vec::new();
    for seg in rel.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." || seg.contains('\\') || seg.starts_with('.') {
            return None;
        }
        parts.push(seg);
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            // to_digit(16) 对非十六进制字符天然返回 None。
            if let (Some(h), Some(l)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn mime_for(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        // 文本类一律带 charset=utf-8（#92）：浏览器对无 charset 的 text/* 按本地
        // 传统编码解码，静态中文会乱码；ESM 因规范恒 UTF-8 不受影响——恰好证明
        // 同一 server 两套解码真相的不一致。application/json 例外：RFC 8259 JSON
        // 恒 UTF-8 且未定义 charset 参数，fetch/XHR 的 .json() 不看它，带上无效果。
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "txt" | "md" => "text/plain; charset=utf-8",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// 按工作区 fonts 段生成注入页面的字体样式（#104）。每字体一条 `@font-face`，
/// URL 用 `/ws/` 绝对路径——fonts[].file 是工作区根相对路径，任意页面目录深度
/// 下都解析到同一文件。`default=true` 的 family 追加一条 `body` 规则：core 里
/// default 的语义是「未声明 font-family 的文本兜底」，浏览器 UA 默认是系统
/// serif，不镜像则在「无 font-family 文本」上预览与运行时分叉。
///
/// 浏览器吃不了的格式跳过并告警（stderr 进度通道）：`.ttc` 无浏览器支持；
/// file 缺失（json 与磁盘脱节）同样跳过 + 告警——静默回落系统字体正是 #104
/// 报的「排版失真且无提示」。
fn build_fonts_css(ui: &Path, fonts: &[workspace::FontCfg]) -> String {
    let mut css = String::new();
    let mut default_family: Option<String> = None;
    for f in fonts {
        let full = ui.join(&f.file);
        if !full.is_file() {
            eprintln!(
                "preview: font '{}' file missing: {} (skipped)",
                f.family, f.file
            );
            continue;
        }
        let ext = full
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        let fmt = match ext.as_deref() {
            Some("ttf") => "truetype",
            Some("otf") => "opentype",
            Some("woff") => "woff",
            Some("woff2") => "woff2",
            Some("ttc") => {
                eprintln!(
                    "preview: font '{}' is .ttc — browsers cannot load TrueType Collections (skipped)",
                    f.family
                );
                continue;
            }
            _ => {
                eprintln!(
                    "preview: font '{}' has non-web extension: {} (skipped)",
                    f.family, f.file
                );
                continue;
            }
        };
        let url = format!("/ws/{}", f.file.replace(' ', "%20"));
        let fam = f.family.replace('\\', "\\\\").replace('\'', "\\'");
        css.push_str(&format!(
            "@font-face {{ font-family: '{fam}'; src: url({url}) format('{fmt}'); font-display: block; }}\n"
        ));
        if f.default && default_family.is_none() {
            default_family = Some(fam);
        }
    }
    if let Some(fam) = default_family {
        css.push_str(&format!("body {{ font-family: '{fam}'; }}\n"));
    }
    css
}

/// HTML 注入（#92 分层后的注入约定）：A 层 boot（`/ikat-preview/lib/boot.js`，
/// 组件展开 + 控件语义 + base polyfill）**恒注入**；工作区 B 层
/// `<包目录>/preview/main.js`（全页）与 `preview/pages/<页stem>.js`（按页）
/// 存在才追加。module 自带 defer 语义、按注入序执行——boot 先跑（A 层就绪后
/// B 层才动 DOM）。src 用绝对路径（boot 落 server 根路由）；工作区脚本仍用相对
/// 路径（浏览器按页面 URL 解析 → 落回本 server 的 /ws/ 下同一目录）。
/// 字体样式（#104）先插到 `<head>` 开标签后——在页面自身 `<style>`/`<link>`
/// 之前，页面 CSS 想覆盖默认 family 仍能赢（工作区手写是真相源）。
pub fn inject_preview_scripts(html: &str, page_dir: &Path, stem: &str, fonts_css: &str) -> String {
    let html = insert_fonts_style(html, fonts_css);
    let mut tags = String::new();
    tags.push_str(r#"<script type="module" src="/ikat-preview/lib/boot.js"></script>"#);
    if page_dir.join("preview/main.js").is_file() {
        tags.push_str(r#"<script type="module" src="preview/main.js"></script>"#);
    }
    if page_dir.join(format!("preview/pages/{stem}.js")).is_file() {
        tags.push_str(&format!(
            r#"<script type="module" src="preview/pages/{stem}.js"></script>"#
        ));
    }
    // 插在 </head> 前（页面自带 <style> 之后，模拟脚本不抢样式声明）；无 head
    // 的退化 HTML 直接前置。大小写不敏感找 tag（HTML 大小写不敏感）。
    if let Some(pos) = html.to_ascii_lowercase().rfind("</head>") {
        let mut out = String::with_capacity(html.len() + tags.len());
        out.push_str(&html[..pos]);
        out.push_str(&tags);
        out.push_str(&html[pos..]);
        out
    } else {
        format!("{tags}{html}")
    }
}

/// 字体样式插到 `<head>` 开标签之后。注意 `<head` 前缀也匹配 `<header>`——
/// 须校验后一个字符是 tag 终结（`>` 或空白）。无 head 的退化 HTML 直接前置
///（CSS 位置无关紧要，只有优先级受文档序影响）。
fn insert_fonts_style(html: &str, fonts_css: &str) -> String {
    if fonts_css.is_empty() {
        return html.to_string();
    }
    let style = format!("<style id=\"ikat-preview-fonts\">\n{fonts_css}</style>");
    let lower = html.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(pos) = lower[search_from..].find("<head") {
        let at = search_from + pos;
        match lower[at + 5..].chars().next() {
            Some('>') | Some(' ') | Some('\t') | Some('\r') | Some('\n') => {
                // 开标签内首个 '>' 是其终点（属性值里出现 '>' 在合法 HTML 里
                // 须转义，不做引号感知）。
                if let Some(gt) = lower[at..].find('>') {
                    let insert_at = at + gt + 1;
                    let mut out = String::with_capacity(html.len() + style.len());
                    out.push_str(&html[..insert_at]);
                    out.push_str(&style);
                    out.push_str(&html[insert_at..]);
                    return out;
                }
            }
            _ => {} // <header> 等：继续找下一个 <head 前缀
        }
        search_from = at + 5;
    }
    format!("{style}{html}")
}

// ---------------------------------------------------------------------------
// 迷你 HTTP 客户端（ping / shutdown 复用检查与 --stop）
// ---------------------------------------------------------------------------

fn http_call(port: u16, method: &str, path: &str, token: Option<&str>) -> Option<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let mut req =
        format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n");
    if let Some(t) = token {
        req.push_str(&format!("X-Ikat-Token: {t}\r\n"));
    }
    req.push_str("Content-Length: 0\r\n\r\n");
    stream.write_all(req.as_bytes()).ok()?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).ok()?;
    String::from_utf8_lossy(&buf)
        .split("\r\n\r\n")
        .nth(1)
        .map(|b| b.to_string())
}

/// ping 通则返回 server 的 JSON body（含 token）。
fn ping(info: &ServerInfo) -> Option<serde_json::Value> {
    let body = http_call(info.port, "GET", "/_ikat/ping", None)?;
    serde_json::from_str(&body).ok()
}

fn request_shutdown(info: &ServerInfo) {
    let _ = http_call(info.port, "POST", "/_ikat/shutdown", Some(&info.token));
}

/// 内嵌外壳资产（templates/preview-shell/，手写无构建）。
mod shell {
    pub const INDEX: &str = include_str!("../templates/preview-shell/index.html");
    pub const APP_JS: &str = include_str!("../templates/preview-shell/app.js");
    pub const STYLE_CSS: &str = include_str!("../templates/preview-shell/style.css");
}

/// 预览 A 层库（templates/preview/lib/，#92）：boot 引导 / 组件展开 / 控件语义 /
/// 演示填充 / 结构性 polyfill——「单真相语义」的框架真相副本，经路由
/// `/ikat-preview/lib/*` 供给（与运行二进制天然同版本），工作区不拷贝。
mod simlib {
    pub const BOOT_JS: &str = include_str!("../templates/preview/lib/boot.js");
    pub const EXPAND_JS: &str = include_str!("../templates/preview/lib/expand.js");
    pub const CONTROLS_JS: &str = include_str!("../templates/preview/lib/controls.js");
    pub const FILL_JS: &str = include_str!("../templates/preview/lib/fill.js");
    pub const BASE_CSS: &str = include_str!("../templates/preview/lib/base.css");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_date_known_instants() {
        // RFC 7231 样例时刻 + Unix 纪元（周四）。只产出不解析，钉格式防回归。
        let at = |secs: u64| {
            use std::time::{Duration, SystemTime};
            http_date(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
        };
        assert_eq!(at(0), "Thu, 01 Jan 1970 00:00:00 GMT");
        assert_eq!(at(784_111_777), "Sun, 06 Nov 1994 08:49:37 GMT");
        // 闰日 + 年界：2024-02-29 12:00:00 = 1709208000（周四）。
        assert_eq!(at(1_709_208_000), "Thu, 29 Feb 2024 12:00:00 GMT");
        // 2026-09-20 00:00:00 = 1789862400（周日，Python datetime 交叉验证）。
        assert_eq!(at(1_789_862_400), "Sun, 20 Sep 2026 00:00:00 GMT");
    }

    #[test]
    fn fnv1a_stable() {
        // 端口哈希跨版本不漂移是浏览器 tab 稳定端口承诺的基础——钉住样本值。
        assert_eq!(fnv1a("F:/ws/showcase"), fnv1a("F:/ws/showcase"));
        assert_ne!(fnv1a("F:/ws/a"), fnv1a("F:/ws/b"));
        assert_eq!(fnv1a(""), 0x811c_9dc5);
    }

    #[test]
    fn stable_port_in_range() {
        let p = PORT_BASE + (fnv1a("F:/ws/showcase") % u32::from(PORT_SPAN)) as u16;
        assert!((PORT_BASE..PORT_BASE + PORT_SPAN).contains(&p));
    }

    #[test]
    fn normalize_rejects_escape_and_dotfiles() {
        assert_eq!(normalize_rel("a/b.html").as_deref(), Some("a/b.html"));
        assert_eq!(normalize_rel("a//b.html").as_deref(), Some("a/b.html"));
        assert_eq!(normalize_rel("a/./b.html").as_deref(), Some("a/b.html"));
        assert_eq!(normalize_rel("../x"), None);
        assert_eq!(normalize_rel("a/../../x"), None);
        assert_eq!(normalize_rel(".ikat/preview.json"), None);
        assert_eq!(normalize_rel("a/.git/config"), None);
        assert_eq!(normalize_rel("a\\b"), None);
        assert_eq!(normalize_rel(""), None);
        assert_eq!(normalize_rel("/"), None);
    }

    #[test]
    fn percent_decode_basics() {
        assert_eq!(percent_decode("/ws/a%20b.html"), "/ws/a b.html");
        assert_eq!(percent_decode("/ws/%E4%B8%AD.html"), "/ws/中.html");
        assert_eq!(percent_decode("/plain"), "/plain");
        assert_eq!(percent_decode("/bad%2"), "/bad%2");
        assert_eq!(percent_decode("/bad%zz"), "/bad%zz");
    }

    #[test]
    fn mime_coverage() {
        // 文本类一律带 charset=utf-8（#92：静态中文按浏览器本地编码解码会乱码）。
        assert_eq!(
            mime_for(Path::new("a.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            mime_for(Path::new("a.mjs")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(mime_for(Path::new("a.html")), "text/html; charset=utf-8");
        assert_eq!(mime_for(Path::new("a.css")), "text/css; charset=utf-8");
        // json 不带 charset：RFC 8259 JSON 恒 UTF-8，参数无效果（见 mime_for 注释）。
        assert_eq!(mime_for(Path::new("a.json")), "application/json");
        assert_eq!(mime_for(Path::new("a.png")), "image/png");
        assert_eq!(mime_for(Path::new("a.unknown")), "application/octet-stream");
    }

    #[test]
    fn inject_only_existing_entries() {
        let tmp = std::env::temp_dir().join(format!("ikat_preview_inject_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("preview/pages")).unwrap();
        std::fs::write(tmp.join("preview/main.js"), "// shared").unwrap();
        std::fs::write(tmp.join("preview/pages/home.js"), "// page").unwrap();

        let html = "<html><head><style>x{}</style></head><body></body></html>";
        let out = inject_preview_scripts(html, &tmp, "home", "");
        // A 层 boot 恒注入（#92）+ 工作区两入口存在才追加。
        assert!(out.contains(r#"src="/ikat-preview/lib/boot.js""#));
        assert!(out.contains(r#"<script type="module" src="preview/main.js"></script>"#));
        assert!(out.contains(r#"<script type="module" src="preview/pages/home.js"></script>"#));
        // 注入位置在 </head> 前、页面 <style> 之后。
        let style_pos = out.find("<style>").unwrap();
        let boot_pos = out.find("/ikat-preview/lib/boot.js").unwrap();
        let head_pos = out.find("</head>").unwrap();
        assert!(style_pos < boot_pos && boot_pos < head_pos);

        // 无按页脚本的页：boot + main。
        let out2 = inject_preview_scripts(html, &tmp, "other", "");
        assert!(out2.contains("/ikat-preview/lib/boot.js"));
        assert!(out2.contains("preview/main.js"));
        assert!(!out2.contains("pages/"));

        // 工作区入口全缺：只剩 A 层 boot（不再原样返回——无组件页也吃 polyfill/
        // 控件语义，消费侧零脚本成本是 #92 的目标之一）。
        std::fs::remove_dir_all(&tmp).unwrap();
        std::fs::create_dir_all(&tmp).unwrap();
        let out3 = inject_preview_scripts(html, &tmp, "home", "");
        assert!(out3.contains(r#"src="/ikat-preview/lib/boot.js""#));
        assert!(!out3.contains("preview/main.js"));
    }

    #[test]
    fn fonts_css_builds_faces_and_default_body_rule() {
        let tmp = std::env::temp_dir().join(format!("ikat_preview_fonts_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("res/fonts")).unwrap();
        std::fs::write(tmp.join("res/fonts/a.ttf"), b"ttf").unwrap();
        std::fs::write(tmp.join("res/fonts/b.otf"), b"otf").unwrap();
        std::fs::write(tmp.join("res/fonts/c.ttc"), b"ttc").unwrap();

        let fonts = vec![
            cfg_font("Pixel", "res/fonts/missing.ttf", false),
            cfg_font("Main", "res/fonts/a.ttf", true),
            cfg_font("Mono", "res/fonts/b.otf", false),
            cfg_font("CJK", "res/fonts/c.ttc", false),
        ];
        let css = build_fonts_css(&tmp, &fonts);
        // 缺失文件与 .ttc 跳过（告警走 stderr，不在断言面）；其余两条注入，
        // URL 是 /ws/ 绝对路径。
        assert!(css.contains(
            "@font-face { font-family: 'Main'; src: url(/ws/res/fonts/a.ttf) format('truetype');"
        ));
        assert!(css.contains(
            "@font-face { font-family: 'Mono'; src: url(/ws/res/fonts/b.otf) format('opentype');"
        ));
        assert!(!css.contains("missing.ttf"));
        assert!(!css.contains(".ttc"));
        // default family → body 规则（core 的「未声明 font-family 兜底」镜像）。
        assert!(css.contains("body { font-family: 'Main'; }"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn fonts_css_no_default_means_no_body_rule() {
        let tmp = std::env::temp_dir().join(format!("ikat_preview_fonts2_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("fonts")).unwrap();
        std::fs::write(tmp.join("fonts/x.ttf"), b"x").unwrap();
        let css = build_fonts_css(&tmp, &[cfg_font("X", "fonts/x.ttf", false)]);
        assert!(css.contains("@font-face"));
        assert!(!css.contains("body"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn fonts_style_inserts_after_head_and_before_page_styles() {
        let html = r#"<html><head lang="en"><link rel="stylesheet" href="a.css"><style>x{}</style></head><body></body></html>"#;
        let css = "@font-face { font-family: 'M'; }\nbody { font-family: 'M'; }\n";
        let out = inject_preview_scripts(html, Path::new("."), "home", css);
        let style_pos = out.find(r#"<style id="ikat-preview-fonts">"#).unwrap();
        let link_pos = out.find(r#"<link rel="stylesheet""#).unwrap();
        let page_style_pos = out.rfind("<style>x{}").unwrap();
        // 字体样式在 <head lang="en"> 之后、页面 <link>/<style> 之前——页面
        // CSS 可覆盖默认 family。
        assert!(style_pos < link_pos && link_pos < page_style_pos);

        // <header> 不是 <head>：不得误插（前缀碰撞回归）。
        let html2 = "<html><header>nav</header><body><style>x{}</style></body></html>";
        let out2 = insert_fonts_style(html2, css);
        assert!(
            out2.starts_with("<style id=\"ikat-preview-fonts\">"),
            "no <head> -> prepend"
        );

        // 空 css = 原样返回（无字体的工作区零扰动）。
        assert_eq!(insert_fonts_style(html, ""), html);
    }

    fn cfg_font(family: &str, file: &str, default: bool) -> workspace::FontCfg {
        workspace::FontCfg {
            family: family.to_string(),
            file: file.to_string(),
            default,
            fallback: false,
        }
    }

    // ------------------------------------------------------------------
    // 集成：真实 listener + 原始 TCP 请求（Host/沙箱/注入/api/关停全链路）
    // ------------------------------------------------------------------

    mod integration {
        use super::super::*;
        use std::io::{Read, Write as IoWrite};

        fn write_ws(tag: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
            let tmp =
                std::env::temp_dir().join(format!("ikat_preview_{tag}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&tmp);
            for (rel, content) in files {
                let path = tmp.join(rel);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, content).unwrap();
            }
            tmp
        }

        const WS_JSON: &str = r#"{"version":1,"output_dir":"../out","design":{"w":1920,"h":1080},
            "match_mode":"letterbox","packages":[{"name":"game","dirs":["ui"]}]}"#;

        fn spawn_server(ui: &Path) -> (u16, Arc<Shared>) {
            let ws = workspace::load_workspace(ui).expect("workspace");
            let fonts_css = build_fonts_css(ui, &ws.fonts);
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let shared = Arc::new(Shared {
                ui: ui.to_path_buf(),
                ws,
                token: "t0".into(),
                fonts_css,
                last_request: Mutex::new(Instant::now()),
                shutdown: AtomicBool::new(false),
            });
            let s2 = Arc::clone(&shared);
            std::thread::spawn(move || accept_loop(listener, s2));
            (port, shared)
        }

        /// 原始请求（可控 Host 头——host 白名单的负测必需）。
        fn raw(port: u16, req: &str) -> (u16, String) {
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(req.replace("{P}", &port.to_string()).as_bytes())
                .unwrap();
            let mut buf = String::new();
            s.read_to_string(&mut buf).unwrap();
            let status: u16 = buf
                .split_whitespace()
                .nth(1)
                .and_then(|c| c.parse().ok())
                .unwrap_or(0);
            let body = buf.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
            (status, body)
        }

        fn get(port: u16, path: &str) -> (u16, String) {
            raw(
                port,
                &format!(
                    "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{{P}}\r\nConnection: close\r\n\r\n"
                ),
            )
        }

        #[test]
        fn serves_pages_with_injection_and_sandbox() {
            let ui = write_ws(
                "serve",
                &[
                    ("ikat.workspace.json", WS_JSON),
                    ("ui/home.html", "<html><head></head><body>hi</body></html>"),
                    (
                        "ui/plain.html",
                        "<html><head></head><body>no sim</body></html>",
                    ),
                    ("ui/preview/main.js", "export const x = 1;"),
                    (
                        "ui/preview/pages/home.js",
                        "import '/ws/ui/preview/main.js';",
                    ),
                    ("ui/style.css", "body{margin:0}"),
                ],
            );
            let (port, _shared) = spawn_server(&ui);

            // 页 + 两个入口都注入；boot 恒在先（A 层先于 B 层执行），工作区入口
            // 相对 src 由浏览器按页面 URL 解析回 /ws/ui/preview/。
            let (st, body) = get(port, "/ws/ui/home.html");
            assert_eq!(st, 200);
            assert!(body.contains(r#"src="/ikat-preview/lib/boot.js""#));
            assert!(body.contains(r#"<script type="module" src="preview/main.js"></script>"#));
            assert!(body.contains(r#"<script type="module" src="preview/pages/home.js"></script>"#));
            let boot_pos = body.find("/ikat-preview/lib/boot.js").unwrap();
            let main_pos = body.find("preview/main.js").unwrap();
            assert!(boot_pos < main_pos, "A 层 boot 先于 B 层注入");

            // 无按页脚本的页：boot + main。
            let (st2, body2) = get(port, "/ws/ui/plain.html");
            assert_eq!(st2, 200);
            assert!(body2.contains("/ikat-preview/lib/boot.js"));
            assert!(body2.contains("preview/main.js"));
            assert!(!body2.contains("pages/"));
            // 该工作区无 fonts 段：零字体扰动（负向——无字体的工作区行为不变）。
            assert!(!body2.contains("ikat-preview-fonts"));

            // A 层库路由：内容嵌在二进制、charset 齐全（#92）。
            for p in [
                "/ikat-preview/lib/boot.js",
                "/ikat-preview/lib/expand.js",
                "/ikat-preview/lib/controls.js",
                "/ikat-preview/lib/fill.js",
                "/ikat-preview/lib/base.css",
            ] {
                let (stl, bodyl) = get(port, p);
                assert_eq!(stl, 200, "{p}");
                assert!(!bodyl.is_empty(), "{p}");
            }

            // 模拟脚本本体以 JS MIME 可达（ESM 必需）。
            let (st3, body3) = get(port, "/ws/ui/preview/main.js");
            assert_eq!(st3, 200);
            assert_eq!(body3.trim(), "export const x = 1;");

            // CSS/404。
            let (st4, _) = get(port, "/ws/ui/style.css");
            assert_eq!(st4, 200);
            let (st5, _) = get(port, "/ws/ui/nope.html");
            assert_eq!(st5, 404);

            // 沙箱：路径逃逸与点开头段一律 403。
            let (st6, _) = get(port, "/ws/../ikat.workspace.json");
            assert_eq!(st6, 403);
            let (st7, _) = get(port, "/ws/ui/../../ikat.workspace.json");
            assert_eq!(st7, 403);
            let (st8, _) = get(port, "/ws/.ikat/preview.json");
            assert_eq!(st8, 403);

            // Host 白名单：非本机 Host 拒（防 DNS rebinding）。
            let (st9, _) = raw(
                port,
                "GET /api/workspace.json HTTP/1.1\r\nHost: evil.example\r\nConnection: close\r\n\r\n",
            );
            assert_eq!(st9, 403);

            // 外壳可达。
            let (st10, body10) = get(port, "/");
            assert_eq!(st10, 200);
            assert!(body10.contains("ikat preview"));
            let _ = std::fs::remove_dir_all(&ui);
        }

        #[test]
        fn serves_pages_with_font_injection() {
            let ws_json = r#"{"version":1,"output_dir":"../out","design":{"w":1920,"h":1080},
                "match_mode":"letterbox","packages":[{"name":"game","dirs":["ui"]}],
                "fonts":[{"family":"Pixel","file":"res/fonts/p.ttf","default":true}]}"#;
            let ui = write_ws(
                "serve_fonts",
                &[
                    ("ikat.workspace.json", ws_json),
                    ("res/fonts/p.ttf", "fake-ttf"),
                    (
                        "ui/home.html",
                        "<html><head><style>body{}</style></head><body>hi</body></html>",
                    ),
                ],
            );
            let (port, _shared) = spawn_server(&ui);

            // fonts 段 → 页面里出现 @font-face（/ws/ 绝对路径）+ 默认 family 的
            // body 规则，且在页面自身 <style> 之前（#104）。
            let (st, body) = get(port, "/ws/ui/home.html");
            assert_eq!(st, 200);
            assert!(body.contains("@font-face { font-family: 'Pixel'; src: url(/ws/res/fonts/p.ttf) format('truetype');"));
            assert!(body.contains("body { font-family: 'Pixel'; }"));
            let font_style_pos = body.find(r#"<style id="ikat-preview-fonts">"#).unwrap();
            let page_style_pos = body.rfind("<style>body{}").unwrap();
            assert!(font_style_pos < page_style_pos, "注入字体样式先于页面样式");

            // 字体文件本体经 /ws/ 可达（@font-face 的 src 解析闭环）。
            let (stf, bodyf) = get(port, "/ws/res/fonts/p.ttf");
            assert_eq!(stf, 200);
            assert_eq!(bodyf, "fake-ttf");
            let _ = std::fs::remove_dir_all(&ui);
        }

        #[test]
        fn api_reports_tree_and_sim_flags() {
            let ui = write_ws(
                "api",
                &[
                    ("ikat.workspace.json", WS_JSON),
                    ("ui/home.html", "<html><head></head><body></body></html>"),
                    ("ui/shop.html", "<html><head></head><body></body></html>"),
                    ("ui/preview/main.js", "// sim"),
                    ("ui/preview/pages/home.js", "// home sim"),
                ],
            );
            let (port, _shared) = spawn_server(&ui);
            let (st, body) = get(port, "/api/workspace.json");
            assert_eq!(st, 200);
            let v: serde_json::Value = serde_json::from_str(&body).unwrap();
            assert_eq!(v["design"], serde_json::json!([1920.0, 1080.0]));
            assert_eq!(v["match_mode"], "letterbox");
            let pages = v["packages"][0]["pages"].as_array().unwrap();
            assert_eq!(pages.len(), 2, "自动态扫顶层 *.html 排序");
            let home = pages.iter().find(|p| p["name"] == "home").unwrap();
            assert_eq!(home["has_sim"], serde_json::json!(true));
            assert_eq!(home["url"], "/ws/ui/home.html");
            let shop = pages.iter().find(|p| p["name"] == "shop").unwrap();
            assert_eq!(shop["has_sim"], serde_json::json!(false));
            let _ = std::fs::remove_dir_all(&ui);
        }

        #[test]
        fn ping_and_shutdown_roundtrip() {
            let ui = write_ws("stop", &[("ikat.workspace.json", WS_JSON)]);
            let (port, shared) = spawn_server(&ui);

            let (st, body) = get(port, "/_ikat/ping");
            assert_eq!(st, 200);
            let v: serde_json::Value = serde_json::from_str(&body).unwrap();
            assert_eq!(v["token"], "t0");

            // 错 token 拒关停、flag 不动。
            let (st2, _) = raw(
                port,
                "POST /_ikat/shutdown HTTP/1.1\r\nHost: 127.0.0.1:{P}\r\nX-Ikat-Token: wrong\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            assert_eq!(st2, 403);
            assert!(!shared.shutdown.load(Ordering::SeqCst));

            // 对 token → flag 置位（真正退出由监控线程负责，单测只验协议面）。
            let (st3, _) = raw(
                port,
                "POST /_ikat/shutdown HTTP/1.1\r\nHost: 127.0.0.1:{P}\r\nX-Ikat-Token: t0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            assert_eq!(st3, 200);
            assert!(shared.shutdown.load(Ordering::SeqCst));
            let _ = std::fs::remove_dir_all(&ui);
        }

        #[test]
        fn serves_component_scoped_style() {
            // #95 复现形状：模板根带类 + 样式写在组件 <style> 的根类上。
            let comp = "<style>.tip { background: #eee; width: 320px; }\n\
                        tip-panel.is-press .slot { color: #ff0000; }\n\
                        .tip .slot:hover { text-decoration: underline; }</style>\n\
                        <div class=\"tip\"><slot></slot></div>";
            let ui = write_ws(
                "compstyle",
                &[
                    ("ikat.workspace.json", WS_JSON),
                    ("ui/components/tip-panel.html", comp),
                ],
            );
            let (port, _shared) = spawn_server(&ui);

            let (st, body) = get(port, "/ikat-preview/comp-style/tip-panel.css");
            assert_eq!(st, 200);
            // 双分支：根类规则必须能命中携带标记的模板根（旧 JS 前缀只有后代分支）。
            assert!(body.contains(
                "[data-ikat-comp=\"tip-panel\"] .tip, [data-ikat-comp=\"tip-panel\"].tip"
            ));
            // host 链剥标签（host 类由镜像机制落到根上）+ 链中根 + 伪类。
            assert!(body.contains(
                "[data-ikat-comp=\"tip-panel\"] .is-press .slot, [data-ikat-comp=\"tip-panel\"].is-press .slot"
            ));
            assert!(body.contains(
                "[data-ikat-comp=\"tip-panel\"] .tip .slot:hover, [data-ikat-comp=\"tip-panel\"].tip .slot:hover"
            ));
            // 声明原文透传。
            assert!(body.contains("width: 320px"));

            // 未知名 / 缺后缀 / 大写（围栏外字符）→ 404。
            let (st2, _) = get(port, "/ikat-preview/comp-style/nope.css");
            assert_eq!(st2, 404);
            let (st3, _) = get(port, "/ikat-preview/comp-style/tip-panel");
            assert_eq!(st3, 404);
            let (st4, _) = get(port, "/ikat-preview/comp-style/TipPanel.css");
            assert_eq!(st4, 404);
            let _ = std::fs::remove_dir_all(&ui);
        }

        /// #96：静态资产 revalidate——Last-Modified/304 免重传（25MB 字体在
        /// no-store 下每次导航重传，放大 @font-face block 的隐形文字窗）。
        #[test]
        fn workspace_assets_revalidate_with_304() {
            let ui = write_ws(
                "revalidate",
                &[
                    ("ikat.workspace.json", WS_JSON),
                    ("ui/home.html", "<html><head></head><body>hi</body></html>"),
                    ("ui/font.ttf", "\u{0}\u{1}fake-ttf-bytes"),
                ],
            );
            let (port, _shared) = spawn_server(&ui);
            // raw() 只回 body；这里要响应头，就地取整段响应。
            let fetch = |extra: &str| {
                let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
                s.write_all(
                    format!(
                        "GET /ws/ui/font.ttf HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
                         {extra}Connection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .unwrap();
                let mut buf = String::new();
                s.read_to_string(&mut buf).unwrap();
                buf
            };

            // 首取：200 + Last-Modified + no-cache（revalidate 语义，非 no-store）。
            let full1 = fetch("");
            assert!(full1.starts_with("HTTP/1.1 200"));
            assert!(full1
                .to_ascii_lowercase()
                .contains("cache-control: no-cache"));
            let lm = full1
                .lines()
                .find(|l| l.to_ascii_lowercase().starts_with("last-modified:"))
                .expect("Last-Modified header")
                .split_once(' ')
                .unwrap()
                .1
                .to_string();
            assert!(full1.ends_with("\u{0}\u{1}fake-ttf-bytes"));

            // 回传同值 If-Modified-Since → 304 空体。
            let full2 = fetch(&format!("If-Modified-Since: {lm}\r\n"));
            assert!(full2.starts_with("HTTP/1.1 304"));
            assert_eq!(
                full2.split("\r\n\r\n").nth(1).unwrap_or(""),
                "",
                "304 must carry no body"
            );

            // HTML 恒 no-store（注入产物依赖外部脚本存在性，不按 mtime 复用）。
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(
                format!(
                    "GET /ws/ui/home.html HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
                     Connection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .unwrap();
            let mut html_resp = String::new();
            s.read_to_string(&mut html_resp).unwrap();
            assert!(html_resp
                .to_ascii_lowercase()
                .contains("cache-control: no-store"));
            let _ = std::fs::remove_dir_all(&ui);
        }
    }
}
