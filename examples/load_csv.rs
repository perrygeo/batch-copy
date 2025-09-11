#![allow(clippy::clone_on_copy)]
use futures::stream::StreamExt;
use std::path::PathBuf;

use anyhow::Result;
use chrono::prelude::*;
use csv_async::AsyncReaderBuilder;
use glob::glob;
use tokio::fs::File;
use tokio::task::JoinSet;
use tokio_postgres::types::{ToSql, Type};

use batch_copy::{BatchCopyRow, Configuration, Handler};

#[derive(Debug, Clone)]
struct SpotPrice {
    dt: DateTime<Utc>,
    instance: String,
    os: String,
    region: String,
    az: String,
    price: f64,
}

// Generic types of Handler must implement BatchCopyRow
impl BatchCopyRow for SpotPrice {
    const CHECK_STATEMENT: &'static str =
        "SELECT dt, instance, os, region, az, price FROM spotprices LIMIT 0";
    const COPY_STATEMENT: &'static str =
        "COPY spotprices (dt, instance, os, region, az, price) FROM STDIN (FORMAT binary)";
    const TYPES: &'static [Type] = &[
        Type::TIMESTAMPTZ,
        Type::TEXT,
        Type::TEXT,
        Type::TEXT,
        Type::TEXT,
        Type::FLOAT8,
    ];

    fn binary_copy_vec(&self) -> Vec<Box<(dyn ToSql + Sync + Send + '_)>> {
        vec![
            Box::from(&self.dt),
            Box::from(&self.instance),
            Box::from(&self.os),
            Box::from(&self.region),
            Box::from(&self.az),
            Box::from(&self.price),
        ]
    }
}

// An async producer function which uses the copier to send rows
async fn copy_csv(path: PathBuf, copier: Handler<SpotPrice>) -> (PathBuf, usize, usize) {
    let file = File::open(&path).await.unwrap();
    let mut rdr = AsyncReaderBuilder::new()
        .has_headers(false)
        .create_deserializer(file);

    // Counters
    let mut sent = 0;
    let mut skipped = 0;

    // Parse the CSV record into a SpotPrice
    type Record = (String, String, String, String, f64);
    let mut results = rdr.deserialize::<Record>();

    // Loop through rows and send to copier
    eprintln!("Started copy of {path:#?}");
    while let Some(result) = results.next().await {
        if let Ok((dtstr, instance, os, region_az, price)) = result {
            // Parse date, support two forms
            let dt = match DateTime::parse_from_str(dtstr.as_ref(), "%Y-%m-%d %H:%M:%S%z") {
                Ok(d) => d.into(),
                Err(_) => {
                    eprintln!("error, cannot parse datetime, skipping");
                    skipped += 1;
                    continue;
                }
            };

            // split region and az (the last char)
            let len = region_az.len();
            let region = (region_az[..len - 1]).to_owned();
            let az = (region_az[len - 1..]).to_owned();

            // Send the SpotPrice to the copy actor
            copier
                .send(SpotPrice {
                    dt,
                    instance,
                    os,
                    region,
                    az,
                    price,
                })
                .await;
            sent += 1;
        } else {
            eprintln!("error, invalid row, cannot parse, skipping");
            skipped += 1;
        };
    }
    (path, sent, skipped)
}

#[tokio::main]
async fn main() -> Result<()> {
    let paths = glob("./examples/spotprice*.csv")?;

    // Setup the copy actor with the database url
    let url = std::env::var("DATABASE_URL")
        .unwrap_or("postgresql://postgres:password@localhost:5432/postgres".to_owned());
    let copy_cfg = Configuration::new()
        .database_url(url)
        .flush_timer_ms(2000)
        .max_rows_per_batch(80000)
        .max_channel_capacity(80000)
        .build();
    let copier = Handler::<SpotPrice>::new(copy_cfg).await?;

    // Spawn a task for each input CSV
    let mut tasks = JoinSet::new();
    for path in paths {
        let path = path?;
        let copier = copier.clone();
        tasks.spawn(copy_csv(path, copier));
    }

    // gather the results
    while let Some(res) = tasks.join_next().await {
        let (path, sent, skipped) = res.unwrap();
        eprintln!("Completed copy, {sent} rows from {path:#?} (skipped {skipped})");
    }

    // Important: Send one final message to ensure everything is flushed
    copier.flush().await;
    Ok(())
}
