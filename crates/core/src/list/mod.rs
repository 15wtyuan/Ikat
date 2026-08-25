//! ListView 虚拟化内核：HeightCache + 可见区算法 + slot 池 + spacer 撑高 + anchoring。
//! side table 模式（照 scroll.rs / EditState），不塞进 Node。

#[cfg(test)]
mod tests;

mod binds;
mod enter;
mod execute;
mod heights;
mod notify;
mod plan;
mod pool;
mod scroll;
mod state;
mod viewport;

// 路径稳定：crate 内引用点与外部消费者（FFI/examples）经 `crate::list::X` 取全部导出面
//（pub use glob 只再导出各子模块的 pub 项）。
pub use binds::*;
pub use enter::*;
pub use execute::*;
pub use heights::*;
pub use notify::*;
pub use plan::*;
pub use scroll::*;
pub use state::*;
