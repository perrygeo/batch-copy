use batch_copy::{BatchCopyRow, Configuration, Handler};
use tokio_postgres::{
    types::{ToSql, Type},
    NoTls,
};

#[derive(Debug, Clone)]
struct TestRow {
    a: String,
    b: i64,
}

impl BatchCopyRow for TestRow {
    const CHECK_STATEMENT: &'static str = "SELECT a, b FROM testtable LIMIT 0";
    const COPY_STATEMENT: &'static str = "COPY testtable (a, b) FROM STDIN (FORMAT binary)";
    const TYPES: &'static [Type] = &[Type::TEXT, Type::INT8];

    fn binary_copy_vec(&self) -> Vec<Box<(dyn ToSql + Sync + Send + '_)>> {
        vec![Box::from(&self.a), Box::from(&self.b)]
    }
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
    let copier = Handler::<TestRow>::new(copy_cfg).await.unwrap();
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
