use mongodb::{Client, Database};
use std::sync::Arc;
use tokio::sync::OnceCell;

static MONGO_POOL: OnceCell<Arc<Database>> = OnceCell::const_new();

pub async fn mongo_pool(url: &str, db_name: &str) -> Arc<Database> {
    MONGO_POOL
        .get_or_init(|| async {
            let client = Client::with_uri_str(url)
                .await
                .expect("Failed to connect to MongoDB");

            Arc::new(client.database(db_name))
        })
        .await
        .clone()
}
