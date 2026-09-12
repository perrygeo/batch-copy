use std::marker::PhantomData;

use bb8_postgres::PostgresConnectionManager;
use futures_util::{pin_mut, StreamExt};
use tokio::time::Duration;
use tokio_postgres::binary_copy::BinaryCopyOutStream;
use tokio_postgres::NoTls;

use crate::config::Configuration;
use crate::errors::BatchCopyDatabaseError;
use crate::BatchCopyRow;

type Pool = bb8::Pool<PostgresConnectionManager<NoTls>>;

/// Fetches `BatchCopyRow`s *from* the database using postgres `COPY`
pub struct Reader<T>
where
    T: BatchCopyRow + Send,
{
    pool: Pool,
    _marker: PhantomData<T>,
}

impl<T> Reader<T>
where
    T: BatchCopyRow + Send + Sync + 'static,
{
    pub async fn new(cfg: Configuration) -> Result<Self, BatchCopyDatabaseError> {
        let mgr = PostgresConnectionManager::new_from_stringlike(
            cfg.database_url,
            tokio_postgres::NoTls,
        )?;
        let pool = Pool::builder()
            .max_size(cfg.pool_max_size)
            .max_lifetime(Some(Duration::from_secs(cfg.pool_max_lifetime_sec)))
            .test_on_check_out(true)
            .connection_timeout(Duration::from_secs(cfg.pool_connect_timeout_sec))
            .build(mgr)
            .await?;

        match pool.get().await {
            Ok(conn) => match conn.simple_query(T::CHECK_STATEMENT).await.map(|_| ()) {
                Err(e) => {
                    return Err(BatchCopyDatabaseError::SchemaCheckFailed {
                        source: e,
                        ddl: T::DDL_STATEMENT.to_string(),
                    })
                }
                _ => true,
            },
            Err(_) => return Err(BatchCopyDatabaseError::BadConnection),
        };

        Ok(Self {
            pool,
            _marker: PhantomData,
        })
    }

    /// Copy rows out of the database, optionally filtered by a `WHERE` clause.
    ///
    /// The clause is interpolated verbatim into the generated statement and
    /// cannot be parameterized, so quote any untrusted values first.
    pub async fn fetch(
        &self,
        where_clause: Option<&str>,
    ) -> Result<Vec<T>, BatchCopyDatabaseError> {
        let conn = self
            .pool
            .get()
            .await
            .map_err(|_| BatchCopyDatabaseError::BadConnection)?;
        let stmt = T::copy_out_statement(where_clause);
        let raw = conn.copy_out(&stmt).await?;
        let stream = BinaryCopyOutStream::new(raw, T::TYPES);
        pin_mut!(stream);

        let mut rows = Vec::new();
        while let Some(row) = stream.next().await {
            let row = row?;
            rows.push(T::try_from_row(&row)?);
        }
        Ok(rows)
    }
}
