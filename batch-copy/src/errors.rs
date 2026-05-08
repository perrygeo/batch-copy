use thiserror::Error;

#[derive(Error, Debug)]
pub enum BatchCopyDatabaseError {
    #[error("Database connection is not valid, timeout reached.")]
    BadConnection,
    #[error("Database error")]
    BadTable(#[from] tokio_postgres::Error),
    #[error("unknown data store error")]
    Unknown,
}
