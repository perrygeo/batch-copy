#![doc = include_str!("../../README.md")]

use tokio_postgres::types::{ToSql, Type};

/// The batch copy actor mechanism, buffers messages and periodically copies to postgres.
pub mod actor;
pub mod config;
pub mod copier;
/// Potential error states
pub mod errors;
/// see `Reader`
pub mod reader;

// Public API

#[doc(inline)]
pub use config::Configuration;

#[doc(inline)]
pub use copier::Copier;

#[doc(inline)]
pub use reader::Reader;

pub use batch_copy_derive::BatchCopy;

#[doc(hidden)]
pub mod __private {
    pub use tokio_postgres::binary_copy::BinaryCopyOutRow;
    pub use tokio_postgres::types::{ToSql, Type};
    pub use tokio_postgres::Error;
}

/// Any struct with this trait can be copied to/read from postgres.
pub trait BatchCopyRow {
    const TYPES: &'static [Type];
    const COPY_STATEMENT: &'static str;
    const CHECK_STATEMENT: &'static str;
    const DDL_STATEMENT: &'static str;

    fn fill_copy_refs<'a>(&'a self, out: &mut Vec<&'a (dyn ToSql + Sync)>);

    /// Build a binary `COPY ... TO STDOUT` statement for this row shape.
    fn copy_out_statement(where_clause: Option<&str>) -> String;

    /// Decode one row returned by `COPY ... TO STDOUT`.
    fn try_from_row(
        row: &tokio_postgres::binary_copy::BinaryCopyOutRow,
    ) -> Result<Self, tokio_postgres::Error>
    where
        Self: Sized;
}
