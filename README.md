# batch-copy

An experimental crate for copying high-volumes of data into PostgreSQL using the tokio runtime. `batch_copy` provides an actor interface to PostgreSQL's binary `COPY` mechanism.

The core idea is to batch rows from multiple producers into (fewer, more efficient) COPY transactions.
The actor task will recieve messages on a mpsc channel, buffer them, and periodically COPY to postgres.
To send rows, create a `Handler` (generic over anything that implements the `BatchCopyRow` trait), clone it, move it into your tokio tasks, and invoke the `send` method.

Built using `bb8` connection pool, the `tokio-postgres` ecosystem, and the ideas in Alice Ryhl's blog post and presentation [Actors with Tokio](https://ryhl.io/blog/actors-with-tokio/).

## Resources

- [Source Repo](https://github.com/perrygeo/batch-copy)
- [Examples](https://github.com/perrygeo/batch-copy/tree/main/examples)
- TODO [Documentation]()
- TODO [Crates.io]()

## Rationale

Advantages

- Fits the Rust async model: clone the handler and send data from multiple producers using the tokio runtime.
- Your Rust type -> SQL types: Explicit type conversions using the [`ToSQL`](https://docs.rs/tokio-postgres/latest/tokio_postgres/types/trait.ToSql.html#types) trait from the `postgres-types` crate.
- Speed: Significantly faster than INSERT when you have many producers. Removes a network roundtrip in the critical path.
- Efficiency: less bandwidth, binary data over the wire, fewer transactions, and fewer connections required.
- Backpressure: when the database gets overloaded and the channel buffer fills up, the producing tasks start blocking.

Downsides

- Durabilty: Buffered messages are not written to storage until they are flushed and committed.
- Eventual consistency: Messages sent to the actor may not be immediately refelected by a SELECT.
- Best-effort delivery: Currently, if the batch copy fails, all rows are discarded - the original tasks may be long gone, who would we report errors to?

## Install

Pre-release. In your `Cargo.toml`

```toml
[dependencies]
batch-copy = { git = "https://github.com/perrygeo/batch-copy" }
```

Eventually,

```bash
cargo add batch-copy
```

## Quickstart

Let's say you have an existing postgres table with the following DDL

```sql
CREATE TABLE metrics (url TEXT, latency_ms BIGINT);
```

A minimal Rust application that copies a single metric to postgres

```rust,no_run
use batch_copy::{Configuration, Handler, BatchCopyRow};
use tokio_postgres::types::{ToSql, Type};

#[derive(Debug, Clone)]
struct RequestMetric {
    url: String,
    latency_ms: i64,
}

// Generic types of BatchCopyHandler must implement BatchCopyRow
// Here we map our struct fields to postgres copy semantics
impl BatchCopyRow for RequestMetric {
    const CHECK_STATEMENT: &'static str = "SELECT url, latency_ms FROM metrics LIMIT 0";
    const COPY_STATEMENT: &'static str =
        "COPY metrics (url, latency_ms) FROM STDIN (FORMAT binary)";
    const TYPES: &'static [Type] = &[Type::TEXT, Type::INT8];

    fn binary_copy_vec(&self) -> Vec<Box<(dyn ToSql + Sync + Send + '_)>> {
        vec![Box::from(&self.url), Box::from(&self.latency_ms)]
    }
}

#[tokio::main]
async fn main() {
    // Setup the copy actor
    let url = std::env::var("DATABASE_URL").unwrap();
    let copy_cfg = Configuration::new().database_url(url).build();
    let copier = Handler::<RequestMetric>::new(copy_cfg).await.unwrap();

    // Send a row, will be copied to database at a later time
    copier
        .send(RequestMetric {
            url: String::from("/hello/world"),
            latency_ms: 42,
        })
        .await;

    // Before you exit, ensure all rows have been flushed
    copier.flush().await;
}
```
