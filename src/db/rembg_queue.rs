use bson::serde_helpers::datetime;
use chrono::{DateTime, Utc};
use mongodb::bson::{doc, oid::ObjectId};
use mongodb::Collection;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use crate::error::Result;

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RembgQueueItem {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    
    /// Discord user ID who requested removal
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
    
    /// List of attachment URLs to process
    pub attachments: Vec<AttachmentItem>,
    
    /// Threshold for background removal (1-100, default 60)
    pub threshold: u8,
    
    /// Use binary mode (clean cutout vs soft edges)
    pub binary_mode: bool,
    
    /// Whether to include mask image in output
    pub include_mask: bool,
    
    /// Whether to zip the processed files
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentItem {
    /// Original URL
    pub url: String,
    
    /// Original filename
    pub filename: String,
    
    /// Processed file path (if completed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processed_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum QueueStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

impl RembgQueueItem {
    const COLLECTION_NAME: &'static str = "discord_command_rembg";
    const MAX_RETRIES: u32 = 3;

    /// Create new queue item
    pub fn new(
        user_id: String,
        channel_id: String,
        message_id: String,
        interaction_id: String,
        interaction_token: String,
        attachments: Vec<AttachmentItem>,
        threshold: u8,
        binary_mode: bool,
        include_mask: bool,
        zip: bool,
    ) -> Self {
        Self {
            id: None,
            user_id,
            channel_id,
            message_id,
            status_message_id: None,
            interaction_id,
            interaction_token,
            attachments,
            threshold,
            binary_mode,
            include_mask,
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

    /// Insert into queue
    pub async fn insert(&self, db: &mongodb::Database) -> Result<ObjectId> {
        let collection: Collection<RembgQueueItem> = db.collection(Self::COLLECTION_NAME);
        let result = collection.insert_one(self).await?;
        Ok(result.inserted_id.as_object_id().unwrap())
    }

    /// Get next pending item and mark as processing
    pub async fn get_next_pending(db: &mongodb::Database, worker_id: String) -> Result<Option<RembgQueueItem>> {
        let collection: Collection<RembgQueueItem> = db.collection(Self::COLLECTION_NAME);
        
        let filter = doc! {
            "status": "pending",
            "retry_count": { "$lt": Self::MAX_RETRIES as i32 }
        };
        
        let update = doc! {
            "$set": {
                "status": "processing",
                "worker_id": worker_id,
                "started_at": Utc::now()
            }
        };
        
        let options = mongodb::options::FindOneAndUpdateOptions::builder()
            .return_document(mongodb::options::ReturnDocument::After)
            .build();
        
        Ok(collection.find_one_and_update(filter, update).with_options(options).await?)
    }

    /// Mark as completed
    pub async fn mark_completed(db: &mongodb::Database, id: &ObjectId) -> Result<()> {
        let collection: Collection<RembgQueueItem> = db.collection(Self::COLLECTION_NAME);
        
        collection.update_one(
            doc! { "_id": id },
            doc! {
                "$set": {
                    "status": "completed",
                    "completed_at": Utc::now()
                }
            },
        ).await?;
        
        Ok(())
    }

    /// Mark as failed
    pub async fn mark_failed(db: &mongodb::Database, id: &ObjectId, error: String) -> Result<()> {
        let collection: Collection<RembgQueueItem> = db.collection(Self::COLLECTION_NAME);
        
        collection.update_one(
            doc! { "_id": id },
            doc! {
                "$set": {
                    "status": "failed",
                    "error": error,
                    "completed_at": Utc::now()
                },
                "$inc": { "retry_count": 1 }
            },
        ).await?;
        
        Ok(())
    }

    /// Count total number of rembg tasks
    pub async fn count_total(db: &mongodb::Database) -> Result<u64> {
        let collection: Collection<RembgQueueItem> = db.collection(Self::COLLECTION_NAME);
        let count = collection.count_documents(doc! {}).await?;
        Ok(count)
    }
}
