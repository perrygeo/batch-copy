use std::fmt::Debug;

use bb8_postgres::PostgresConnectionManager;
use builder_pattern::Builder;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Duration;

use tokio_postgres::NoTls;

use crate::actor::{run_batch_insert_actor, BatchCopyActor, BatchCopyMessage};
use crate::errors::BatchCopyDatabaseError;
use crate::BatchCopyRow;

type Pool = bb8::Pool<PostgresConnectionManager<NoTls>>;

#[derive(Clone, Debug)]
pub struct Handler<T>
where
    T: BatchCopyRow + Send,
{
    sender: mpsc::Sender<BatchCopyMessage<T>>,
}

#[derive(Builder)]
pub struct Configuration {
    pub database_url: String,

    /// the actor's internal buffer
    #[default(8000)]
    pub max_rows_per_batch: usize,

    /// the mspc channel's buffer (fills while actor is busy with postgres io)
    #[default(8000)]
    pub max_channel_capacity: usize,

    /// send a flush message every _ ms. will be ignored if no rows in buffer
    #[default(500)]
    pub flush_timer_ms: u64,

    /// pool size, min 2 for failover purposes
    #[default(2)]
    pub pool_max_size: u32,

    /// duration to hold onto connection, recycle them ocassionally
    #[default(1200)]
    pub pool_max_lifetime_sec: u64,

    /// duration to wait for a connection, seconds
    #[default(2)]
    pub pool_connect_timeout_sec: u64,
}

impl<T> Handler<T>
where
    T: BatchCopyRow + Send + Sync + Clone + Debug + 'static,
{
    pub async fn new(cfg: Configuration) -> Result<Self, BatchCopyDatabaseError> {
        // Create internal database pool
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

        // Check connection and bail in case of fatal errors
        match pool.get().await {
            Ok(conn) => match conn.simple_query(T::CHECK_STATEMENT).await.map(|_| ()) {
                Err(e) => return Err(BatchCopyDatabaseError::BadTable(e)),
                _ => true, // good
            },
            Err(_) => return Err(BatchCopyDatabaseError::BadConnection),
        };

        // Construct the channel pair and spawn the actor
        let (tx, rx) = mpsc::channel::<BatchCopyMessage<T>>(cfg.max_channel_capacity);
        let actor = BatchCopyActor::<T>::new(rx, pool, cfg.max_rows_per_batch);
        tokio::spawn(run_batch_insert_actor(actor, cfg.flush_timer_ms));

        Ok(Self { sender: tx })
    }

    pub async fn send(&self, row: T) {
        let (tx, rx) = oneshot::channel();
        let imsg = BatchCopyMessage::<T>::InsertRow(row, tx);
        self.sender
            .send(imsg)
            .await
            .expect("sending a message should not fail");
        rx.await.expect("actor was killed");
    }

    pub async fn flush(&self) {
        let (tx, rx) = oneshot::channel();
        let imsg = BatchCopyMessage::Flush(tx);
        self.sender
            .send(imsg)
            .await
            .expect("sending a message should not fail");
        rx.await.expect("actor was killed");
    }
}
