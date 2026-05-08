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
    let copy_cfg = Configuration::new().database_url(url).build();

    // Setup the copy actor
    let copier = Copier::<RequestMetric>::new(copy_cfg).await.unwrap();

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
