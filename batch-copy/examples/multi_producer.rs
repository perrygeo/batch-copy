#![allow(clippy::clone_on_copy)]
use anyhow::Result;

use batch_copy::{BatchCopy, Configuration, Handler};

#[derive(Debug, Clone, BatchCopy)]
#[batch_copy(table = "users")]
struct MyData {
    id: i64,
    id2: i64,
    name: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Setup the copy actor by database url
    let url = std::env::var("DATABASE_URL")
        .unwrap_or("postgresql://postgres:password@localhost:5432/postgres".to_owned());
    let copy_cfg = Configuration::new().database_url(url).build();
    let copier = Handler::<MyData>::new(copy_cfg).await?;

    // An async producer function which uses the copier to send rows
    async fn do_lotsa_inserts(id: i64, copier: Handler<MyData>) {
        for id2 in 0..20 {
            copier
                .send(MyData {
                    id,
                    id2,
                    name: format!("task {} emitting message {}", id, id2),
                })
                .await;
        }
    }

    // Spawn thousands of producer tasks
    let mut tasks = vec![];
    for i in 0..2048_i64 {
        let copier = copier.clone();
        let t = tokio::spawn(do_lotsa_inserts(i, copier));
        tasks.push(t);
    }

    // Wait for them
    for t in tasks {
        t.await?;
    }

    // Important: Send one final message to ensure everything is flushed
    copier.flush().await;

    Ok(())
}
