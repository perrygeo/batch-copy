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
    let copy_cfg = Configuration::new().database_url(url).build();

    // Setup the copy actor
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
