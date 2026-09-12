use std::path::PathBuf;

use anyhow::Result;
use batch_copy::{BatchCopy, BatchCopyRow, Configuration, Copier, Reader};
use chrono::prelude::*;
use csv_async::AsyncReaderBuilder;
use futures::stream::StreamExt;
use glob::glob;
use tokio::fs::File;
use tokio_postgres::NoTls;

#[derive(Debug, Clone, PartialEq, BatchCopy)]
#[batch_copy(table = "spotprices")]
struct SpotPrice {
    dt: DateTime<Utc>,
    instance: String,
    os: String,
    region: String,
    az: String,
    price: f64,
}

fn sort_spots(rows: &mut [SpotPrice]) {
    rows.sort_by(|x, y| {
        x.dt.cmp(&y.dt)
            .then_with(|| x.instance.cmp(&y.instance))
            .then_with(|| x.os.cmp(&y.os))
            .then_with(|| x.region.cmp(&y.region))
            .then_with(|| x.az.cmp(&y.az))
            .then_with(|| x.price.to_bits().cmp(&y.price.to_bits()))
    });
}

async fn read_csv(path: PathBuf) -> Result<Vec<SpotPrice>> {
    let file = File::open(&path).await?;
    let mut rdr = AsyncReaderBuilder::new()
        .has_headers(false)
        .create_deserializer(file);

    type Record = (String, String, String, String, f64);
    let mut results = rdr.deserialize::<Record>();
    let mut rows = Vec::new();

    while let Some(result) = results.next().await {
        let (dtstr, instance, os, region_az, price) = result?;
        let dt = DateTime::parse_from_str(dtstr.as_ref(), "%Y-%m-%d %H:%M:%S%z")?.into();
        let len = region_az.len();
        let region = (region_az[..len - 1]).to_owned();
        let az = (region_az[len - 1..]).to_owned();
        rows.push(SpotPrice {
            dt,
            instance,
            os,
            region,
            az,
            price,
        });
    }
    Ok(rows)
}

#[tokio::main]
async fn main() -> Result<()> {
    let paths = glob(&format!(
        "{}/examples/spotprice*.csv",
        env!("CARGO_MANIFEST_DIR")
    ))?;
    let url = std::env::var("DATABASE_URL")
        .unwrap_or("postgresql://postgres:[REDACTED]@localhost:5432/postgres".to_owned());

    // Fetch the csv contents into memory
    let mut orig_spot_prices: Vec<SpotPrice> = Vec::new();
    for path in paths.flatten() {
        orig_spot_prices.extend(read_csv(path).await?);
    }
    sort_spots(&mut orig_spot_prices);

    // Setup a fresh, empty table so the roundtrip assertion is deterministic.
    let (client, connection) = tokio_postgres::connect(url.as_str(), NoTls).await?;
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("Connection error: {error}");
        }
    });
    client
        .simple_query("DROP TABLE IF EXISTS spotprices")
        .await?;
    client.simple_query(SpotPrice::DDL_STATEMENT).await?;

    // Copy rows TO the database
    let copy_cfg = Configuration::new()
        .database_url(url.clone())
        .flush_timer_ms(2000)
        .max_rows_per_batch(80000)
        .max_channel_capacity(80000)
        .build();
    let copier = Copier::<SpotPrice>::new(copy_cfg).await?;
    for row in &orig_spot_prices {
        copier.send(row.clone()).await;
    }
    copier.flush().await;

    // Copy rows FROM the database
    let read_cfg = Configuration::new().database_url(url).build();
    let reader = Reader::<SpotPrice>::new(read_cfg).await?;
    let mut roundtrip_spot_prices = reader.fetch(None).await?;
    sort_spots(&mut roundtrip_spot_prices);

    // Assert original == roundtrip
    assert_eq!(orig_spot_prices, roundtrip_spot_prices);
    println!("roundtrip ok: {} rows", roundtrip_spot_prices.len());
    Ok(())
}
