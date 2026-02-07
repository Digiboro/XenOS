//! Object Manager (Ob)
//!
//! Управление объектами ядра NT.
//!
//! Источники:
//! - ReactOS: ob/obinit.c, ob/oblife.c, ob/obref.c, ob/obhandle.c
//! - NT5: ob/obinit.c, ob/obcreate.c, ob/obref.c

#![allow(dead_code)]
#![allow(non_camel_case_types)]

pub mod dir;
pub mod handle;
pub mod header;
pub mod init;
pub mod life;
pub mod link;
pub mod refcount;
pub mod types;

pub use dir::*;
pub use handle::*;
pub use header::*;
pub use init::*;
pub use life::*;
pub use link::*;
pub use refcount::*;
pub use types::*;
