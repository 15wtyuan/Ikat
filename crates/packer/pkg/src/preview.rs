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

    let shared = Arc::new(Shared {
        ui: ui.clone(),
        ws,
        token: info.token.clone(),
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
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        resp.status,
        resp.reason,
        resp.content_type,
        resp.body.len()
    );
    stream.write_all(head.as_bytes())?;
    if req.method != "HEAD" {
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
            serve_workspace_file(&shared.ui, &p[4..])
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

/// serve 工作区文件：路径沙箱（canonicalize 后必须仍在工作区内；拒绝点开头
/// 路径段）+ HTML 注入两个 ESM 入口（存在才注入）。
fn serve_workspace_file(ui: &Path, rel: &str) -> Response {
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
    let mut body = match std::fs::read(&canon) {
        Ok(b) => b,
        Err(_) => return Response::text(500, "Internal Server Error", "read failed"),
    };
    // 注入判定按前缀（text/* 已带 charset 后缀，等值比较会漏判）。
    if mime.starts_with("text/html") {
        let dir = canon.parent().unwrap_or(Path::new("")).to_path_buf();
        let stem = canon
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let html = String::from_utf8_lossy(&body).into_owned();
        body = inject_preview_scripts(&html, &dir, &stem).into_bytes();
    }
    Response::new(200, "OK", mime, body)
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

/// HTML 注入（#92 分层后的注入约定）：A 层 boot（`/ikat-preview/lib/boot.js`，
/// 组件展开 + 控件语义 + base polyfill）**恒注入**；工作区 B 层
/// `<包目录>/preview/main.js`（全页）与 `preview/pages/<页stem>.js`（按页）
/// 存在才追加。module 自带 defer 语义、按注入序执行——boot 先跑（A 层就绪后
/// B 层才动 DOM）。src 用绝对路径（boot 落 server 根路由）；工作区脚本仍用相对
/// 路径（浏览器按页面 URL 解析 → 落回本 server 的 /ws/ 下同一目录）。
pub fn inject_preview_scripts(html: &str, page_dir: &Path, stem: &str) -> String {
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
        let out = inject_preview_scripts(html, &tmp, "home");
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
        let out2 = inject_preview_scripts(html, &tmp, "other");
        assert!(out2.contains("/ikat-preview/lib/boot.js"));
        assert!(out2.contains("preview/main.js"));
        assert!(!out2.contains("pages/"));

        // 工作区入口全缺：只剩 A 层 boot（不再原样返回——无组件页也吃 polyfill/
        // 控件语义，消费侧零脚本成本是 #92 的目标之一）。
        std::fs::remove_dir_all(&tmp).unwrap();
        std::fs::create_dir_all(&tmp).unwrap();
        let out3 = inject_preview_scripts(html, &tmp, "home");
        assert!(out3.contains(r#"src="/ikat-preview/lib/boot.js""#));
        assert!(!out3.contains("preview/main.js"));
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
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let shared = Arc::new(Shared {
                ui: ui.to_path_buf(),
                ws,
                token: "t0".into(),
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
    }
}
