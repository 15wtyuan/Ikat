//! pkg.bin 布局锁（schema-hash 门）：固定 fixture 打包 → FNV-1a 哈希 → 与登记值比对。
//!
//! 背景：pkg 格式「字段增删必 bump PKG_FORMAT_VERSION」长期只是注释契约，曾发生
//! bincode 布局变了忘 bump、全部已发 pkg 静默损坏的事故。本测试把同一份 HTML 的
//! 打包字节锁死：任何改变字节的格式改动（bincode 结构字段增删改序 / 手编列增删 /
//! 编码方式变化）都会翻转哈希。哈希变了 = 有人改了布局——若是有意的格式演进，
//! bump `PKG_FORMAT_VERSION`（含 MIN/MAX）+ 重打全部 fixtures + 更新下方登记哈希；
//! 若不是有意的，这里就是防线。
//!
//! FNV-1a 64 手写实现（标准库 hasher 无跨版本稳定性承诺，自实现保证任意工具链下
//! 同字节同哈希）。fixture 见 `fixtures/schema-lock.html`。

use loomgui_pkg::build::{pack_components, Component, PackResult};

const HTML: &str = include_str!("fixtures/schema-lock.html");

/// 当前布局的登记哈希。改 pkg 布局（有意 bump 版本）时同步更新此值。
/// v44（#10 layout/box-shadow keyframes 通道，手编 keyframes blob 布局扩展）。
const LOCKED_HASH: u64 = 0x193c_9d7b_8f26_e3f1;

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn pack() -> Vec<u8> {
    let PackResult { bytes, .. } = pack_components(&[Component {
        name: "schema-lock".to_string(),
        src: HTML.to_string(),
        html_rel: "schema-lock.html".to_string(),
    }])
    .expect("pack_components");
    bytes
}

/// 布局锁本体：打包字节哈希 == 登记值。
#[test]
fn pkg_bytes_schema_hash_locked() {
    let bytes = pack();
    let h = fnv1a64(&bytes);
    assert_eq!(
        h, LOCKED_HASH,
        "pkg.bin 打包字节哈希与登记值不一致 —— pkg 布局变了。\
         有意的格式演进：bump PKG_FORMAT_VERSION（含 MIN/MAX）+ 重打全部 fixtures \
         + 用新哈希 {h:#x} 更新 LOCKED_HASH；意外变化：布局被无 bump 改动，先查根因。"
    );
}

/// 自洽性：同一输入两次打包字节全等（布局锁的前提——打包必须确定性，
/// 任何 HashMap 迭代序进字节流都会让布局锁变 flaky）。
#[test]
fn pkg_bytes_packing_is_deterministic() {
    assert_eq!(pack(), pack(), "same input must pack to identical bytes");
}
