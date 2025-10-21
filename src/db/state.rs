use mongodb::{Collection, bson::doc, options::ReplaceOptions};
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscordState {
    #[serde(rename = "_id")]
    pub id: String,
    pub session_id: Option<String>,
    pub sequence: Option<u64>,
}

impl DiscordState {
    const COLLECTION_NAME: &'static str = "discord_state";
    const STATE_ID: &'static str = "bot_state";

    pub async fn load(db: &mongodb::Database) -> Result<Self> {
        let collection: Collection<DiscordState> = db.collection(Self::COLLECTION_NAME);
        
        let state = collection
            .find_one(doc! { "_id": Self::STATE_ID })
            .await?;

        Ok(state.unwrap_or(DiscordState {
            id: Self::STATE_ID.to_string(),
            session_id: None,
            sequence: None,
        }))
    }

    pub async fn save(&self, db: &mongodb::Database) -> Result<()> {
        let collection: Collection<DiscordState> = db.collection(Self::COLLECTION_NAME);
        let options = ReplaceOptions::builder().upsert(true).build();
        
        collection
            .replace_one(doc! { "_id": Self::STATE_ID }, self)
            .with_options(options)
            .await?;

        Ok(())
    }
}
