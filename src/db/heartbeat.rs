use bson::serde_helpers::datetime;
use chrono::{DateTime, Utc};
use mongodb::{Collection, bson::doc, options::ReplaceOptions};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use crate::error::Result;

#[serde_as]
#[derive(Debug, Serialize, Deserialize)]
pub struct Heartbeat {
    #[serde(rename = "_id")]
    pub id: String,
    pub count: i64,
    #[serde_as(as = "datetime::FromChrono04DateTime")]
    pub last_sent: DateTime<Utc>,
}

impl Heartbeat {
    const COLLECTION_NAME: &'static str = "discord_heartbeat";
    const HEARTBEAT_ID: &'static str = "bot_heartbeat";

    pub async fn increment(db: &mongodb::Database) -> Result<i64> {
        let collection: Collection<Heartbeat> = db.collection(Self::COLLECTION_NAME);
        
        // Load current heartbeat
        let current = collection
            .find_one(doc! { "_id": Self::HEARTBEAT_ID })
            .await?;

        let new_count = current.map(|h| h.count + 1).unwrap_or(1);

        let heartbeat = Heartbeat {
            id: Self::HEARTBEAT_ID.to_string(),
            count: new_count,
            last_sent: Utc::now(),
        };

        let options = ReplaceOptions::builder().upsert(true).build();
        
        collection
            .replace_one(doc! { "_id": Self::HEARTBEAT_ID }, heartbeat)
            .with_options(options)
            .await?;

        Ok(new_count)
    }
}
