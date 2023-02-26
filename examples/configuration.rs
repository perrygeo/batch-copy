use batch_copy::{BatchCopyRow, Configuration, Handler};
use tokio_postgres::types::{ToSql, Type};

// To be copied to a postgres table with the following DDL:
//   CREATE TABLE metrics (url TEXT, latency_ms BIGINT);
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
    // Configuration
    let url = std::env::var("DATABASE_URL")
        .unwrap_or("postgresql://postgres:password@localhost:5432/postgres".to_owned());
    let copy_cfg = Configuration::new()
        //
        // Database URI like postgresql://postgres:password@localhost:5432/postgres
        .database_url(url)
        //
        // How often should the buffer be flushed in ms?
        // Useful for time-sensitive applications
        .flush_timer_ms(1000)
        //
        // Or, flush if the maximum row count has been reached.
        // Useful for optimizing throughput for ETL applications
        .max_rows_per_batch(20000)
        //
        // While the actor is copying, the channel can accumulate messages
        // up to this maximum, after which it applies backpressure
        .max_channel_capacity(20000)
        //
        // The internal database pool size, two connections for failover
        .pool_max_size(2)
        //
        // The internal database connections timeout, all flushes must complete the copy within this window.
        .pool_connect_timeout_sec(600)
        //
        // The internal database connection lifetime, periodically recycled.
        .pool_max_lifetime_sec(1200)
        .build();

    // Basic usage
    let copier = Handler::<RequestMetric>::new(copy_cfg).await.unwrap();
    copier
        .send(RequestMetric {
            url: String::from("/hello/world"),
            latency_ms: 42,
        })
        .await;
    copier.flush().await;
}
