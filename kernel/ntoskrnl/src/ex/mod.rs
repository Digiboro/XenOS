//! Executive subsystem (Ex)
//!
//! Предоставляет базовые сервисы ядра:
//! - Pool allocator (ExAllocatePool)
//! - NT-подобный pool с free lists и coalescing
//! - Lookaside lists (кэш блоков)
//! - Work items (отложенное выполнение)
//! - Handle tables
//! - System time

pub mod handle;
pub mod init;
pub mod lookaside;
pub mod pool;
pub mod pool_nt;
pub mod work;

pub use handle::*;
pub use init::*;
pub use lookaside::*;
pub use pool::*;
pub use pool_nt::*;
pub use work::*;
