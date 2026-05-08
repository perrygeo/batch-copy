use thiserror::Error;

#[derive(Error, Debug)]
pub enum BatchCopyDatabaseError {
    #[error("Database connection is not valid, timeout reached.")]
    BadConnection,
    #[error("Database error")]
    BadTable(#[from] tokio_postgres::Error),
    #[error("Table schema check failed: {source}\n\nHint — try running:\n{ddl}")]
    SchemaCheckFailed {
        #[source]
        source: tokio_postgres::Error,
        ddl: String,
    },
    #[error("unknown data store error")]
    Unknown,
}
