use bson::serde_helpers::datetime;
use chrono::{DateTime, Utc};
use mongodb::{Collection, bson::doc};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use crate::error::Result;

#[serde_as]
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionEvent {
    pub event_type: String, // "identify", "resume", "resumed", "invalid_session"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    #[serde_as(as = "datetime::FromChrono04DateTime")]
    pub timestamp: DateTime<Utc>,
}

impl SessionEvent {
    const COLLECTION_NAME: &'static str = "discord_session_events";

    pub async fn log_identify(db: &mongodb::Database) -> Result<()> {
        let collection: Collection<SessionEvent> = db.collection(Self::COLLECTION_NAME);

        let event = SessionEvent {
            event_type: "identify".to_string(),
            session_id: None,
            sequence: None,
            timestamp: Utc::now(),
        };

        collection.insert_one(event).await?;
        Ok(())
    }

    pub async fn log_resume(
        db: &mongodb::Database,
        session_id: String,
        sequence: Option<u64>,
    ) -> Result<()> {
        let collection: Collection<SessionEvent> = db.collection(Self::COLLECTION_NAME);

        let event = SessionEvent {
            event_type: "resume".to_string(),
            session_id: Some(session_id),
            sequence,
            timestamp: Utc::now(),
        };

        collection.insert_one(event).await?;
        Ok(())
    }

    pub async fn log_resumed(db: &mongodb::Database) -> Result<()> {
        let collection: Collection<SessionEvent> = db.collection(Self::COLLECTION_NAME);

        let event = SessionEvent {
            event_type: "resumed".to_string(),
            session_id: None,
            sequence: None,
            timestamp: Utc::now(),
        };

        collection.insert_one(event).await?;
        Ok(())
    }

    pub async fn log_ready(db: &mongodb::Database, session_id: String) -> Result<()> {
        let collection: Collection<SessionEvent> = db.collection(Self::COLLECTION_NAME);

        let event = SessionEvent {
            event_type: "ready".to_string(),
            session_id: Some(session_id),
            sequence: None,
            timestamp: Utc::now(),
        };

        collection.insert_one(event).await?;
        Ok(())
    }

    pub async fn log_invalid_session(db: &mongodb::Database) -> Result<()> {
        let collection: Collection<SessionEvent> = db.collection(Self::COLLECTION_NAME);

        let event = SessionEvent {
            event_type: "invalid_session".to_string(),
            session_id: None,
            sequence: None,
            timestamp: Utc::now(),
        };

        collection.insert_one(event).await?;
        Ok(())
    }
}
