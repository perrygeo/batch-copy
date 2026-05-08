use batch_copy::{BatchCopy, Configuration, Copier};

// To be copied to a postgres table with the following DDL:
//   CREATE TABLE metrics (url TEXT, latency_ms BIGINT);
#[derive(Debug, Clone, BatchCopy)]
#[batch_copy(table = "metrics")]
struct RequestMetric {
    url: String,
    latency_ms: i64,
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
    let copier = Copier::<RequestMetric>::new(copy_cfg).await.unwrap();
    copier
        .send(RequestMetric {
            url: String::from("/hello/world"),
            latency_ms: 42,
        })
        .await;
    copier.flush().await;
}
