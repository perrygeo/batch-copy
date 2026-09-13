use std::time::Instant;

use anyhow::Result;
use batch_copy::{BatchCopy, BatchCopyRow, Configuration, Copier, Reader};
use tokio_postgres::NoTls;

const N_ROWS: usize = 750_000;

#[derive(Debug, Clone, BatchCopy)]
#[batch_copy(table = "benchmark_rows")]
struct BenchmarkRow {
    id: i64,
    name: String,
    value: f64,
}

fn make_rows() -> Vec<BenchmarkRow> {
    (0..N_ROWS)
        .map(|i| BenchmarkRow {
            id: i as i64,
            name: format!("row-{i}"),
            value: i as f64,
        })
        .collect()
}

async fn ensure_table(client: &tokio_postgres::Client) -> Result<()> {
    client
        .simple_query("DROP TABLE IF EXISTS benchmark_rows")
        .await?;
    client.simple_query(BenchmarkRow::DDL_STATEMENT).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or("postgresql://postgres:password@localhost:5432/postgres".to_owned());
    let rows = make_rows();

    // Direct connection for the plain SELECT baseline and table setup.
    let (client, connection) = tokio_postgres::connect(url.as_str(), NoTls).await?;
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("Connection error: {error}");
        }
    });

    // Load the table once via batch COPY (setup, not measured).
    ensure_table(&client).await?;
    let load_cfg = Configuration::new()
        .database_url(url.clone())
        .max_rows_per_batch(8000)
        .max_channel_capacity(8000)
        .flush_timer_ms(500)
        .build();
    let copier = Copier::<BenchmarkRow>::new(load_cfg).await?;
    for row in &rows {
        copier.send(row.clone()).await;
    }
    copier.flush().await;

    // Plain SELECT of the whole table.
    let start = Instant::now();
    let db_rows = client
        .query("SELECT id, name, value FROM benchmark_rows", &[])
        .await?;
    let select_rows: Vec<BenchmarkRow> = db_rows
        .iter()
        .map(|r| BenchmarkRow {
            id: r.get(0),
            name: r.get(1),
            value: r.get(2),
        })
        .collect();
    let select_elapsed = start.elapsed();

    // Batch COPY OUT via the Reader.
    let read_cfg = Configuration::new().database_url(url).build();
    let reader = Reader::<BenchmarkRow>::new(read_cfg).await?;
    let start = Instant::now();
    let copy_rows = reader.fetch(None).await?;
    let copy_elapsed = start.elapsed();

    assert_eq!(select_rows.len(), N_ROWS);
    assert_eq!(copy_rows.len(), N_ROWS);

    println!("rows read: {N_ROWS}");
    println!(
        "plain SELECT:     {:?} ({:.0} rows/sec)",
        select_elapsed,
        N_ROWS as f64 / select_elapsed.as_secs_f64()
    );
    println!(
        "batch COPY OUT:   {:?} ({:.0} rows/sec)",
        copy_elapsed,
        N_ROWS as f64 / copy_elapsed.as_secs_f64()
    );
    println!(
        "speedup:          {:.1}x",
        select_elapsed.as_secs_f64() / copy_elapsed.as_secs_f64()
    );

    Ok(())
}
