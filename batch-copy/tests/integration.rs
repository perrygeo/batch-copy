use batch_copy::{BatchCopy, Configuration, Copier, Reader};
use tokio_postgres::NoTls;

#[derive(Debug, Clone, BatchCopy)]
#[batch_copy(table = "testtable")]
struct TestRow {
    a: String,
    b: i64,
}

#[derive(Debug, Clone, PartialEq, BatchCopy)]
#[batch_copy(table = "roundtrip_test")]
struct RoundTripRow {
    a: String,
    b: i64,
}

#[tokio::test]
async fn test_copy_actor() {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or("postgresql://postgres:password@localhost:5432/postgres".to_owned());

    // Get DB client and spawn connection
    let (client, connection) = tokio_postgres::connect(url.as_ref(), NoTls).await.unwrap();
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("Connection error: {}", error);
        }
    });

    // Setup
    client
        .query("DROP TABLE IF EXISTS testtable", &[])
        .await
        .unwrap();
    client
        .query("CREATE TABLE testtable (a TEXT, b BIGINT)", &[])
        .await
        .unwrap();

    // Actual test
    let copy_cfg = Configuration::new().database_url(url).build();
    let copier = Copier::<TestRow>::new(copy_cfg).await.unwrap();
    let tr = TestRow {
        a: String::from("/hello/world"),
        b: 42,
    };
    copier.send(tr).await;
    copier.flush().await;

    // Assert contents
    let res = client
        .query("SELECT count(*) FROM testtable where b = 42", &[])
        .await
        .unwrap();

    let row = &res[0];
    let a: i64 = row.get(0);
    assert_eq!(a, 1);
}

#[tokio::test]
async fn test_copy_out_roundtrip() {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or("postgresql://postgres:password@localhost:5432/postgres".to_owned());

    let (client, connection) = tokio_postgres::connect(url.as_ref(), NoTls).await.unwrap();
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("Connection error: {}", error);
        }
    });
    client
        .query("DROP TABLE IF EXISTS roundtrip_test", &[])
        .await
        .unwrap();
    client
        .query("CREATE TABLE roundtrip_test (a TEXT, b BIGINT)", &[])
        .await
        .unwrap();

    let copy_cfg = Configuration::new().database_url(url.clone()).build();
    let copier = Copier::<RoundTripRow>::new(copy_cfg).await.unwrap();
    let original = vec![
        RoundTripRow {
            a: "first".to_string(),
            b: 1,
        },
        RoundTripRow {
            a: "second".to_string(),
            b: 2,
        },
        RoundTripRow {
            a: "third".to_string(),
            b: 3,
        },
    ];
    for row in &original {
        copier.send(row.clone()).await;
    }
    copier.flush().await;

    let read_cfg = Configuration::new().database_url(url.clone()).build();
    let reader = Reader::<RoundTripRow>::new(read_cfg).await.unwrap();
    let mut fetched = reader.fetch(None).await.unwrap();

    let mut expected = original.clone();
    expected.sort_by_key(|r| r.b);
    fetched.sort_by_key(|r| r.b);
    assert_eq!(fetched, expected);
}
