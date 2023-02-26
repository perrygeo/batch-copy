use futures_util::pin_mut;
use std::fmt::Debug;
use std::mem;

use bb8_postgres::PostgresConnectionManager;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{interval, Duration};
use tokio_postgres::binary_copy::BinaryCopyInWriter;
use tokio_postgres::types::ToSql;
use tokio_postgres::NoTls;

use crate::BatchCopyRow;

type Pool = bb8::Pool<PostgresConnectionManager<NoTls>>;

pub struct BatchCopyActor<T: BatchCopyRow + Send> {
    recv: mpsc::Receiver<BatchCopyMessage<T>>,
    rows: Vec<T>,
    rows_per_batch: usize,
    pool: Pool,
}

#[derive(Debug)]
pub(crate) enum BatchCopyMessage<T: BatchCopyRow + Send> {
    InsertRow(T, oneshot::Sender<usize>),
    Flush(oneshot::Sender<usize>),
}

impl<T> BatchCopyActor<T>
where
    T: BatchCopyRow + Send,
{
    pub(crate) fn new(
        recv: mpsc::Receiver<BatchCopyMessage<T>>,
        pool: Pool,
        rows_per_batch: usize,
    ) -> Self {
        let rows = vec![];
        Self {
            recv,
            rows,
            rows_per_batch,
            pool,
        }
    }

    async fn flush(&mut self) -> u64 {
        // Exit early if there's nothing to flush
        if self.rows.is_empty() {
            return 0;
        }

        // Start connection
        let mut connection = self.pool.get().await.unwrap();
        let transaction = connection.transaction().await.unwrap();

        // Swap out rows
        let mut target_rows: Vec<T> = Vec::with_capacity(self.rows.len());
        mem::swap(&mut self.rows, &mut target_rows);
        let nrows = &target_rows.len();

        // see https://github.com/sfackler/rust-postgres/blob/master/tokio-postgres/tests/test/binary_copy.rs
        let sink_result = transaction.copy_in(T::COPY_STATEMENT).await;
        if let Err(e) = sink_result {
            log::error!("\tterminating transaction, COPY is invalid\n\t{e}");
            return 0;
        }
        let sink = sink_result.unwrap();

        let writer = BinaryCopyInWriter::new(sink, T::TYPES);
        pin_mut!(writer);

        let mut errors = false;
        for row in target_rows.into_iter() {
            let result = {
                // We own the row here, surely there's a better way to do this
                // without intermediate Vecs - TODO
                let boxes = row.binary_copy_vec();
                let row_refs: Vec<&_> = boxes
                    .iter()
                    .map(|x| {
                        // Convert the to a dyn reference
                        &**x as &(dyn ToSql + Sync)
                    })
                    .collect();

                writer.as_mut().write(row_refs.as_slice()).await
            };

            if let Err(e) = result {
                log::error!("Error in COPY:\n\t{e}");
                errors = true;
                break;
            };
        }

        if !errors {
            let nrows = writer.finish().await.unwrap();
            transaction.commit().await.unwrap();
            nrows
        } else {
            log::error!("\tterminating transaction, data loss has occured! {nrows} rows discarded");
            // explicit rollback seems to hang, instead we rely on the transaction Drop
            // transaction.rollback().await.unwrap();

            0
        }
    }

    async fn handle_message(&mut self, msg: BatchCopyMessage<T>) {
        match msg {
            BatchCopyMessage::InsertRow(row, output_chan) => {
                self.rows.push(row);
                if self.rows.len() >= self.rows_per_batch {
                    self.flush().await;
                    // set the last_flushed
                }
                output_chan.send(1).unwrap();
            }
            BatchCopyMessage::Flush(output_chan) => {
                self.flush().await;
                output_chan.send(1).unwrap();
            }
        }
    }
}

pub async fn run_batch_insert_actor<T>(mut actor: BatchCopyActor<T>, timeout: u64)
where
    T: BatchCopyRow + Send,
{
    let mut timer = interval(Duration::from_millis(timeout));
    loop {
        tokio::select! {
             msg = actor.recv.recv() => match msg {
                Some(msg) => actor.handle_message(msg).await,
                None => break,
            },
            _tick = timer.tick() => {
                actor.flush().await;
            }
        }
    }
}
