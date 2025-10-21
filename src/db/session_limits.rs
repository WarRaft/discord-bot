use bson::serde_helpers::datetime;
use chrono::{DateTime, Utc};
use mongodb::{Collection, bson::doc};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use crate::error::Result;

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionLimit {
    /// Total number of session starts allowed per day
    pub total: i32,
    
    /// Number of session starts remaining in the current 24-hour period
    pub remaining: i32,
    
    /// Milliseconds until the limit resets
    pub reset_after: i64,
    
    /// Maximum number of concurrent gateway sessions
    pub max_concurrency: i32,
    
    /// Number of shards recommended for this bot
    pub shards: i32,
    
    /// Timestamp when this information was last updated
    #[serde_as(as = "datetime::FromChrono04DateTime")]
    pub updated_at: DateTime<Utc>,
}

impl SessionLimit {
    const COLLECTION_NAME: &'static str = "discord_session_limits";
    const DOC_ID: &'static str = "session_limit";

    /// Update session limit information from Gateway Bot endpoint
    pub async fn update(
        db: &mongodb::Database,
        total: i32,
        remaining: i32,
        reset_after: i64,
        max_concurrency: i32,
        shards: i32,
    ) -> Result<()> {
        let collection: Collection<SessionLimit> = db.collection(Self::COLLECTION_NAME);

        let session_limit = SessionLimit {
            total,
            remaining,
            reset_after,
            max_concurrency,
            shards,
            updated_at: Utc::now(),
        };

        collection
            .replace_one(doc! { "_id": Self::DOC_ID }, &session_limit)
            .upsert(true)
            .await?;

        Ok(())
    }

    /// Get current session limit information
    #[allow(dead_code)]
    pub async fn get(db: &mongodb::Database) -> Result<Option<SessionLimit>> {
        let collection: Collection<SessionLimit> = db.collection(Self::COLLECTION_NAME);
        
        let limit = collection
            .find_one(doc! { "_id": Self::DOC_ID })
            .await?;

        Ok(limit)
    }

    /// Check if we can start a new session
    #[allow(dead_code)]
    pub fn can_start_session(&self) -> bool {
        self.remaining > 0
    }

    /// Get seconds to wait before we can start a new session
    #[allow(dead_code)]
    pub fn retry_after_seconds(&self) -> f64 {
        if self.remaining > 0 {
            return 0.0;
        }
        self.reset_after as f64 / 1000.0
    }
}
