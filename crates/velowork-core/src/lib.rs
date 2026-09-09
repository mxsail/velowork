#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

pub mod api;
pub mod atomic_io;
pub mod charset;
pub mod data_root;
pub mod event;
pub mod keys;
pub mod logging;
pub mod memory;
pub mod process;
pub mod profiles;
pub mod storage;
pub mod version;
pub mod security;
pub mod selection;
pub mod send_payload;
pub mod shell;
pub mod term_type;
pub mod terminal_preview;
pub mod theme;
pub mod timing;
pub mod types;
pub mod ws;

pub use term_type::{DEFAULT_TERM_TYPE, SUPPORTED_TERM_TYPES};
