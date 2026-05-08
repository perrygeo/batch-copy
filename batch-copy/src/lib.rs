#![doc = include_str!("../README.md")]

use tokio_postgres::types::{ToSql, Type};

/// The batch copy actor recieves messages, buffers them, and periodically flushes to postgresql using binary COPY.
pub mod actor;
/// Potential error states
pub mod errors;
/// The copier takes BatchCopyRow values and sends them to the actor on a channel.
/// Copiers are inexpensive to clone and can be used on multiple threads/tasks.
pub mod handler;

// Public API

/// The copier takes BatchCopyRow values and sends them to the actor on a channel.
/// Copiers are inexpensive to clone and can be used on multiple threads/tasks.
pub use handler::{Configuration, Copier};

pub use batch_copy_derive::BatchCopy;

#[doc(hidden)]
pub mod __private {
    pub use tokio_postgres::types::{ToSql, Type};
}

/// translate your struct to postgres details
pub trait BatchCopyRow {
    const TYPES: &'static [Type];
    const COPY_STATEMENT: &'static str;
    const CHECK_STATEMENT: &'static str;
    const DDL_STATEMENT: &'static str;

    fn binary_copy_vec(&self) -> Vec<Box<dyn ToSql + Sync + Send + '_>>;
}
