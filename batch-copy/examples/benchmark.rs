use std::time::Instant;

use anyhow::Result;
use batch_copy::{BatchCopy, BatchCopyRow, Configuration, Copier};
use tokio_postgres::NoTls;

const N_ROWS: usize = 250_000;

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

async fn recreate_table(client: &tokio_postgres::Client) -> Result<()> {
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

    // Direct connection for the plain INSERT baseline and table setup.
    let (mut client, connection) = tokio_postgres::connect(url.as_str(), NoTls).await?;
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("Connection error: {error}");
        }
    });

    // Plain INSERTs inside a single transaction.
    recreate_table(&client).await?;
    let start = Instant::now();
    let tx = client.transaction().await?;
    let insert = tx
        .prepare("INSERT INTO benchmark_rows (id, name, value) VALUES ($1, $2, $3)")
        .await?;
    for row in &rows {
        tx.execute(&insert, &[&row.id, &row.name, &row.value])
            .await?;
    }
    tx.commit().await?;
    let insert_elapsed = start.elapsed();

    // Batch COPY via the Copier actor.
    recreate_table(&client).await?;
    let copy_cfg = Configuration::new()
        .database_url(url)
        .max_rows_per_batch(8000)
        .max_channel_capacity(8000)
        .flush_timer_ms(500)
        .build();
    let copier = Copier::<BenchmarkRow>::new(copy_cfg).await?;
    let start = Instant::now();
    for row in &rows {
        copier.send(row.clone()).await;
    }
    copier.flush().await;
    let copy_elapsed = start.elapsed();

    println!("rows inserted: {N_ROWS}");
    println!(
        "plain INSERT (1 txn): {:?} ({:.0} rows/sec)",
        insert_elapsed,
        N_ROWS as f64 / insert_elapsed.as_secs_f64()
    );
    println!(
        "batch COPY:           {:?} ({:.0} rows/sec)",
        copy_elapsed,
        N_ROWS as f64 / copy_elapsed.as_secs_f64()
    );
    println!(
        "speedup:              {:.1}x",
        insert_elapsed.as_secs_f64() / copy_elapsed.as_secs_f64()
    );

    Ok(())
}
