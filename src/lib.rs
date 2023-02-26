#![doc = include_str!("../README.md")]

use tokio_postgres::types::{ToSql, Type};

/// The batch copy actor recieves messages, buffers them, and periodically flushes to postgresql using binary COPY.
pub mod actor;
/// Potential error states
pub mod errors;
/// The handler takes BatchCopyRow values and sends them to the actor on a channel.
/// Handlers are inexpensive to clone and can be used on multiple threads/tasks.
pub mod handler;

// Public API

/// The handler takes BatchCopyRow values and sends them to the actor on a channel.
/// Handlers are inexpensive to clone and can be used on multiple threads/tasks.
pub use handler::{Configuration, Handler};

/// translate your struct to postgres details
pub trait BatchCopyRow {
    const TYPES: &'static [Type];
    const COPY_STATEMENT: &'static str;
    const CHECK_STATEMENT: &'static str;

    fn binary_copy_vec(&self) -> Vec<Box<(dyn ToSql + Sync + Send + '_)>>;
}
