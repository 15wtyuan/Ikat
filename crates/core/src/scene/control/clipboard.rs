use crate::scene::node::{EditState, NodeKind};

use super::edit::{delete_selection, insert_text};

// core 是 cdylib，不能 extern 调宿主剪贴板（Unity GUIUtility.systemCopyBuffer / Win32
// clipboard）——宿主符号在 core 链接期不可解析，且 C# 宿主无法提供 linkable C 符号。
// 故由后端在启动时经 FFI `ikat_register_clipboard` 注册一对 set/get 函数指针，core
// 经这两指针间接调。未注册时 write_clipboard no-op、read_clipboard 返空串（防宿主未接线
// 时 panic）。
//
// 内存契约：get 回调返的缓冲区由宿主持有（至少活到下次 get 调用）；core 立即拷成 String，
// 不释放（避免跨分配器 free）。set 回调收 (ptr,len)，宿主在调用期间拷走，ptr 不需宿主释放。

use std::sync::Mutex;

/// 宿主「写剪贴板」回调签名。收 (ptr,len) 指向合法 UTF-8 字节，宿主拷走；返 0=成功。
pub type ClipboardSetFn = unsafe extern "C" fn(*const u8, usize) -> i32;
/// 宿主「读剪贴板」回调签名。宿主写 (out_ptr,out_len)，缓冲区宿主持有（活到下次 get）；
/// 返 0=成功。非 0 / null ptr 视作空。
pub type ClipboardGetFn = unsafe extern "C" fn(*mut *mut u8, *mut usize) -> i32;

/// 注册的回调槽。Option：None = 未注册（no-op）。Mutex 包串行注册/读写的并发安全。
static CLIPBOARD_SET: Mutex<Option<ClipboardSetFn>> = Mutex::new(None);
static CLIPBOARD_GET: Mutex<Option<ClipboardGetFn>> = Mutex::new(None);

/// 注册宿主剪贴板回调（FFI 层 `ikat_register_clipboard` 调）。传 None 可注销。
/// 重复注册覆盖旧值（测试需重注册）。后端应在 Stage 启动后尽早注册一次。
pub fn register_clipboard(set_fn: Option<ClipboardSetFn>, get_fn: Option<ClipboardGetFn>) {
    *CLIPBOARD_SET.lock().unwrap() = set_fn;
    *CLIPBOARD_GET.lock().unwrap() = get_fn;
}

/// 读剪贴板。未注册 get 回调 / 回调返非 0 / null ptr / 非 UTF-8 → 返空串（no-op 不 panic）。
/// 宿主缓冲区立即拷成 String（缓冲区宿主持有，见 [`ClipboardGetFn`] 契约）。
pub fn read_clipboard() -> String {
    // 拷出 fn 指针再解锁，回调在锁外调（防回调内再 lock 造成重入死锁）。
    let Some(get) = *CLIPBOARD_GET.lock().unwrap() else {
        return String::new();
    };
    let mut ptr: *mut u8 = std::ptr::null_mut();
    let mut len: usize = 0;
    // SAFETY: 宿主保证 rc=0 且 ptr 非空时 [ptr, ptr+len) 是合法字节切片。
    let rc = unsafe { get(&mut ptr, &mut len) };
    if rc != 0 || ptr.is_null() || len == 0 {
        return String::new();
    }
    // SAFETY: len 由宿主给出，已在上面非零校验；ptr 非空。
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// 写剪贴板。未注册 set 回调 → no-op（不 panic）。
pub fn write_clipboard(s: &str) {
    // 拷出 fn 指针再解锁，回调在锁外调。
    if let Some(set) = *CLIPBOARD_SET.lock().unwrap() {
        // SAFETY: 传 (ptr,len) 指向 s 的合法 UTF-8 字节，宿主在调用期间拷走。
        unsafe { set(s.as_ptr(), s.len()) };
    }
}

/// 当前选区文本（无选区 → 空串）。选区字节区间必落在 UTF-8 char 边界（selection_range
/// 返回的是 cursor/anchor 的 min/max，二者均由编辑原语维护在 char 边界）。
pub fn selected_text(e: &EditState) -> String {
    let (b, end) = e.selection_range();
    e.value[b..end].to_string()
}

/// 复制选区到剪贴板，返回选区文本。无选区 → 直接返空串，不碰剪贴板
///（照 HTML/浏览器：Ctrl+C 无选区时 no-op，不清空系统剪贴板）。
/// 有选区 → 写剪贴板并返回选区文本。不改 value（copy 是非破坏性）。
pub fn copy_selection(e: &EditState) -> String {
    if e.anchor == e.cursor {
        return String::new();
    }
    let s = selected_text(e);
    write_clipboard(&s);
    s
}

/// 剪切选区：先复制到剪贴板，再在非 readonly 时 [`delete_selection`]。返回 value 是否改变。
/// 照 HTML：readonly 不阻止 copy，但禁止修改——故复制永远发生，删除受 readonly 守卫
///（readonly 时 copy 后直接返 false，不动 value、不发 ValueChanged）。无选区时
/// copy 也是 no-op（见 [`copy_selection`]）。
/// `kind` 未使用（delete_selection 只动选区），保留参数为与 [`paste`] API 对称。
pub fn cut_selection(e: &mut EditState, _kind: NodeKind) -> bool {
    let s = selected_text(e);
    write_clipboard(&s); // 复制永远发生（readonly 允许 copy）
    if e.readonly {
        return false; // readonly 禁止删除
    }
    delete_selection(e)
}

/// 粘贴：读剪贴板后 [`insert_text`]（自带选区替换 + sanitize + max_length 校验）。
/// 返回 value 是否改变。readonly / 剪贴板空 / 超 max_length → no-op 返 false。
///
/// NumberField：照 process_text_input / commit_composition 的输入 guard，先滤成数字语法
/// 字符（[`filter_number_field_text`]）再插——三渠（textinput / IME commit / keydown-paste）
/// 共享同一过滤语义，避免漂移。
pub fn paste(e: &mut EditState, kind: NodeKind) -> bool {
    let raw = read_clipboard();
    let text = match kind {
        NodeKind::NumberField => crate::input::filter_number_field_text(&raw),
        _ => raw,
    };
    insert_text(e, kind, &text)
}
