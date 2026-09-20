# batch-copy

[![batch-copy](https://github.com/perrygeo/batch-copy/actions/workflows/tests.yml/badge.svg)](https://github.com/perrygeo/batch-copy/actions/workflows/tests.yml)

A Rust library for high-throughput asynchronous PostgreSQL I/O using the [binary `COPY` protocol](https://www.postgresql.org/docs/current/sql-copy.html).

`batch-copy` batches rows from many concurrent producers into efficient bulk `COPY` transactions.
The actor task receives rows on an MPSC channel, buffers them, and periodically flushes to Postgres —
freeing your producers from caring about batching or connection management.

Built on `bb8`, `tokio-postgres`, and the actor pattern from [Actors with Tokio](https://ryhl.io/blog/actors-with-tokio/).

## Resources

* [Source Repo](https://github.com/perrygeo/batch-copy)
* [Examples](https://github.com/perrygeo/batch-copy/tree/main/batch-copy/examples)

## Rationale

**Advantages**

* Fits the Rust async model: clone the copier and send data from multiple producers in parallel.
* Schema-validated at startup: `Copier::new()` runs a `CHECK_STATEMENT` against the live table so type mismatches fail fast, not silently.
* Fast: binary `COPY` is significantly faster than `INSERT` for bulk loads; no per-row round-trips.
* Efficient: binary encoding, fewer transactions, fewer connections.
* Backpressure: when the database falls behind, the MPSC channel fills and producers block automatically.

**Trade-offs**

* Durability: buffered rows are not persisted until the next flush/commit.
* Eventual consistency: rows sent to the actor may not be immediately visible to `SELECT`.
* Best-effort delivery: if a batch fails, rows are discarded (producers are long gone by then).

## Install

The derive macro is re-exported from the main crate, so you only need one dependency:

```toml
[dependencies]
batch-copy = { git = "https://github.com/perrygeo/batch-copy" }
```

## Quickstart

Annotate your struct with `#[derive(BatchCopy)]`. The table name defaults to the `snake_case` of the
struct name; override it with `#[batch_copy(table = "my_table")]`.

```sql
CREATE TABLE metrics (url TEXT, latency_ms BIGINT);
```

```rust,no_run
use batch_copy::{BatchCopy, Configuration, Copier};

#[derive(Debug, Clone, BatchCopy)]
#[batch_copy(table = "metrics")]
struct RequestMetric {
    url: String,
    latency_ms: i64,
}

#[tokio::main]
async fn main() {
    let url = std::env::var("DATABASE_URL").unwrap();
    let copy_cfg = Configuration::new().database_url(url).build();
    let copier = Copier::<RequestMetric>::new(copy_cfg).await.unwrap();

    copier
        .send(RequestMetric {
            url: String::from("/hello/world"),
            latency_ms: 42,
        })
        .await;

    // Flush before exit to ensure all buffered rows are written
    copier.flush().await;
}
```

`Copier` is cheap to clone — move clones into as many tokio tasks as you need:

```rust,ignore
let mut tasks = vec![];
for i in 0..2048_i64 {
    let copier = copier.clone();
    tasks.push(tokio::spawn(async move {
        for j in 0..20_i64 {
            copier.send(RequestMetric { url: format!("/path/{i}/{j}"), latency_ms: j }).await;
        }
    }));
}
for t in tasks { t.await.unwrap(); }
copier.flush().await;
```

## Reading with `Reader`

`Reader` copies rows *out* of Postgres back into a `Vec<T>`. It is a separate type from the
write-only `Copier`, but uses the same `BatchCopy` derive and the same `Configuration`:

```rust,no_run
use batch_copy::{BatchCopy, Configuration, Reader};

#[derive(Debug, Clone, PartialEq, BatchCopy)]
#[batch_copy(table = "metrics")]
struct RequestMetric {
    url: String,
    latency_ms: i64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL").unwrap();
    let read_cfg = Configuration::new().database_url(url).build();
    let reader = Reader::<RequestMetric>::new(read_cfg).await?;

    // Fetch every row into a Vec
    let all = reader.fetch(None).await?;

    // Or filter with a WHERE-only fragment
    let slow = reader.fetch(Some("latency_ms > 100")).await?;

    Ok(())
}
```

`fetch` takes an `Option<&str>` where fragment. The clause is interpolated verbatim into the
generated `COPY (SELECT ...) TO STDOUT` statement and cannot be parameterized, so quote any
untrusted values yourself.

## Type mapping

`#[derive(BatchCopy)]` maps Rust types to PostgreSQL types automatically:

| Rust type                        | PostgreSQL type |
|----------------------------------|-----------------|
| `bool`                           | `BOOL`          |
| `i8`, `i16`                      | `INT2`          |
| `i32`                            | `INT4`          |
| `i64`                            | `INT8`          |
| `f32`                            | `FLOAT4`        |
| `f64`                            | `FLOAT8`        |
| `String`, `&str`                 | `TEXT`          |
| `Vec<u8>`                        | `BYTEA`         |
| `NaiveDate`                      | `DATE`          |
| `NaiveTime`                      | `TIME`          |
| `NaiveDateTime`                  | `TIMESTAMP`     |
| `DateTime<Tz>`                   | `TIMESTAMPTZ`   |
| `Uuid`                           | `UUID`          |
| `Option<T>`                      | same as `T`     |

For any type not in this list, annotate the field with `#[pg(TYPE)]`:

```rust,ignore
#[derive(Debug, Clone, BatchCopy)]
struct Event {
    id: i64,
    #[pg(JSONB)]
    payload: serde_json::Value,
    #[pg(TIMESTAMPTZ)]
    occurred_at: chrono::DateTime<chrono::Utc>,
}
```

## Performance

How much faster is COPY? Generally significant compared to INSERTs:

```text,ignore
$ cargo run --release --example benchmark
    Finished `release` profile [optimized] target(s) in ...s
     Running `target/release/examples/benchmark`
rows inserted: 250000
plain INSERT (1 txn): 15.843347826s (15779 rows/sec)
batch COPY:           2.232294222s (111992 rows/sec)
speedup:              7.1x
```

And only a slight improvement compared to SELECT:

```text,ignore
$ cargo run --release --example benchmark-read
    Finished `release` profile [optimized] target(s) in ...s
     Running `target/release/examples/benchmark-read`
rows read: 750000
plain SELECT:     208.909744ms (3590067 rows/sec)
batch COPY OUT:   162.60214ms (4612485 rows/sec)
speedup:          1.3x
````

## DDL generation

`copier.ddl()` returns a best-approximation `CREATE TABLE` statement based on the struct's field names and types. This is useful for bootstrapping a new table or quickly checking the expected schema:

```rust,ignore
let copier = Copier::<RequestMetric>::new(copy_cfg).await?;
println!("{}", copier.ddl());
// CREATE TABLE metrics (
//     url TEXT NOT NULL,
//     latency_ms BIGINT NOT NULL
// );
```

`Option<T>` fields are rendered without `NOT NULL`; all other fields include it.

If `Copier::new()` fails because the schema check query returns an error (table missing, column type mismatch, etc.), the error message automatically includes the DDL hint so you know exactly what to run:

```text,ignore
Table schema check failed: relation "metrics" does not exist

Hint — try running:
CREATE TABLE metrics (
    url TEXT NOT NULL,
    latency_ms BIGINT NOT NULL
);
```

## Configuration

All settings are optional and have sensible defaults:

```rust,no_run

use batch_copy::{Configuration};
let copy_cfg = Configuration::new()
    .database_url("postgresql://postgres:password@localhost:5432/db".into())
    // Flush at least this often (milliseconds)
    .flush_timer_ms(1000)
    // Or flush when this many rows have accumulated
    .max_rows_per_batch(20_000)
    // Channel capacity before backpressure kicks in
    .max_channel_capacity(20_000)
    // Connection pool size (minimum 2)
    .pool_max_size(2)
    // Timeout for acquiring a connection (seconds)
    .pool_connect_timeout_sec(600)
    // Maximum connection lifetime before recycling (seconds)
    .pool_max_lifetime_sec(1200)
    .build();
```

