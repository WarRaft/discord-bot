use crate::workers::queue::QueueStatus;
use crate::discord::message::message::Attachment;
use crate::error::BotError;
use bson::serde_helpers::datetime;
use chrono::{DateTime, Utc};
use mongodb::Collection;
use mongodb::bson::{Bson, doc, oid::ObjectId};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use strum::{Display, EnumString};

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlpQueueItem {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,

    /// Discord user ID who requested conversion
    pub user_id: String,

    /// Discord channel ID where message was sent
    pub channel_id: String,

    /// Original message ID (for context)
    pub message_id: String,

    /// Status message ID (for editing progress updates)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_message_id: Option<String>,

    /// Interaction ID for response
    pub interaction_id: String,

    /// Interaction token for response
    pub interaction_token: String,

    /// List of attachment URLs to convert
    pub attachments: Vec<Attachment>,

    /// Conversion type (ToBLP or ToPNG)
    pub conversion_type: ConversionType,

    /// BLP quality (1-100, only used for ToBLP conversion)
    pub quality: u8,

    /// Whether to zip the converted files
    pub zip: bool,

    /// Current status
    pub status: QueueStatus,

    /// Worker ID (if being processed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,

    /// Created timestamp
    #[serde_as(as = "datetime::FromChrono04DateTime")]
    pub created_at: DateTime<Utc>,

    /// Started processing timestamp
    #[serde_as(as = "Option<datetime::FromChrono04DateTime>")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,

    /// Completed timestamp
    #[serde_as(as = "Option<datetime::FromChrono04DateTime>")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,

    /// Error message (if failed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Retry count
    #[serde(default)]
    pub retry_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Display, EnumString)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ConversionType {
    #[strum(serialize = "toblp")]
    ToBLP, // PNG/JPG → BLP
    #[strum(serialize = "topng")]
    ToPNG, // BLP → PNG
}

impl BlpQueueItem {
    const COLLECTION_NAME: &'static str = "discord_command_blp";
    const MAX_RETRIES: u32 = 3;

    /// Create new queue item
    pub fn new(
        user_id: String,
        channel_id: String,
        message_id: String,
        interaction_id: String,
        interaction_token: String,
        attachments: Vec<Attachment>,
        conversion_type: ConversionType,
        quality: u8,
        zip: bool,
        status_message_id: Option<String>,
    ) -> Self {
        Self {
            id: None,
            user_id,
            channel_id,
            message_id,
            status_message_id,
            interaction_id,
            interaction_token,
            attachments,
            conversion_type,
            quality,
            zip,
            status: QueueStatus::Pending,
            worker_id: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            error: None,
            retry_count: 0,
        }
    }

    /// Insert new item into queue
    pub async fn insert(&self, db: &mongodb::Database) -> Result<ObjectId, BotError> {
        let collection: Collection<BlpQueueItem> = db.collection(Self::COLLECTION_NAME);
        let result = collection.insert_one(self).await?;
        Ok(result.inserted_id.as_object_id().unwrap())
    }

    /// Get next pending item and mark as processing
    pub async fn claim_next(
        db: &mongodb::Database,
        worker_id: String,
    ) -> Result<Option<BlpQueueItem>, BotError> {
        let collection: Collection<BlpQueueItem> = db.collection(Self::COLLECTION_NAME);

        // Find and update pending item atomically
        let now = Bson::DateTime(bson::DateTime::now());

        let result = collection
            .find_one_and_update(
                doc! {
                    "status": "pending",
                    "retry_count": { "$lt": Self::MAX_RETRIES as i32 }
                },
                doc! {
                    "$set": {
                        "status": "processing",
                        "worker_id": &worker_id,
                        "started_at": now
                    }
                },
            )
            .sort(doc! { "created_at": 1 }) // FIFO
            .return_document(mongodb::options::ReturnDocument::After)
            .await?;

        Ok(result)
    }

    /// Mark item as completed
    pub async fn mark_completed(db: &mongodb::Database, id: ObjectId) -> Result<(), BotError> {
        let collection: Collection<BlpQueueItem> = db.collection(Self::COLLECTION_NAME);
        let now = Bson::DateTime(bson::DateTime::now());

        collection
            .update_one(
                doc! { "_id": id },
                doc! {
                    "$set": {
                        "status": "completed",
                        "completed_at": now
                    }
                },
            )
            .await?;

        Ok(())
    }

    /// Mark item as failed and increment retry count
    pub async fn mark_failed(
        db: &mongodb::Database,
        id: ObjectId,
        error: String,
    ) -> Result<(), BotError> {
        let collection: Collection<BlpQueueItem> = db.collection(Self::COLLECTION_NAME);
        let now = Bson::DateTime(bson::DateTime::now());

        collection
            .update_one(
                doc! { "_id": id },
                doc! {
                    "$set": {
                        "status": "failed",
                        "error": error,
                        "completed_at": now
                    },
                    "$inc": { "retry_count": 1 }
                },
            )
            .await?;

        Ok(())
    }

    /// Reset stuck processing items (e.g., after service restart)
    pub async fn reset_stuck_items(
        db: &mongodb::Database,
        timeout_minutes: i64,
    ) -> Result<u64, BotError> {
        let collection: Collection<BlpQueueItem> = db.collection(Self::COLLECTION_NAME);

        let threshold = Utc::now() - chrono::Duration::minutes(timeout_minutes);
        let threshold_bson =
            Bson::DateTime(bson::DateTime::from_millis(threshold.timestamp_millis()));

        let result = collection
            .update_many(
                doc! {
                    "status": "processing",
                    "started_at": { "$lt": threshold_bson }
                },
                doc! {
                    "$set": {
                        "status": "pending",
                        "worker_id": null
                    },
                    "$inc": { "retry_count": 1 }
                },
            )
            .await?;

        Ok(result.modified_count)
    }

    /// Count pending items
    #[allow(dead_code)]
    pub async fn count_pending(db: &mongodb::Database) -> Result<u64, BotError> {
        let collection: Collection<BlpQueueItem> = db.collection(Self::COLLECTION_NAME);
        let count = collection
            .count_documents(doc! { "status": "pending" })
            .await?;
        Ok(count)
    }

    /// Count processing items
    #[allow(dead_code)]
    pub async fn count_processing(db: &mongodb::Database) -> Result<u64, BotError> {
        let collection: Collection<BlpQueueItem> = db.collection(Self::COLLECTION_NAME);
        let count = collection
            .count_documents(doc! { "status": "processing" })
            .await?;
        Ok(count)
    }

    /// Count all items by conversion type (total usage statistics)
    pub async fn count_total_by_type(
        db: &mongodb::Database,
        conversion_type: ConversionType,
    ) -> Result<u64, BotError> {
        let collection: Collection<BlpQueueItem> = db.collection(Self::COLLECTION_NAME);

        let filter = doc! {
            "conversion_type": conversion_type.to_string()
        };

        let count = collection.count_documents(filter).await?;
        Ok(count)
    }
}
