use crate::discord::message::message::Message;
use crate::error::BotError;
use crate::workers::queue::QueueStatus;
use bson::serde_helpers::datetime;
use chrono::{DateTime, Utc};
use mongodb::Collection;
use mongodb::bson::{doc, oid::ObjectId};
use proc_macros::define_field_names;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use strum::{Display, EnumString};

#[serde_as]
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[define_field_names]
pub struct JobBlp {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,

    pub message: Message,

    pub reply: Option<Message>,

    pub target: ConversionTarget,

    pub quality: u8,

    pub zip: bool,

    pub status: QueueStatus,

    #[serde_as(as = "datetime::FromChrono04DateTime")]
    pub created: DateTime<Utc>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde_as(as = "Option<datetime::FromChrono04DateTime>")]
    pub completed: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    #[serde(default)]
    pub retry: u32,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Display, EnumString)]
#[serde(rename_all = "UPPERCASE")]
#[strum(serialize_all = "UPPERCASE")]
pub enum ConversionTarget {
    #[default]
    BLP, // PNG/JPG → BLP
    PNG, // BLP → PNG
}

impl JobBlp {
    pub const COLLECTION: &'static str = "discord_command_blp";
    pub const MAX_RETRIES: u32 = 3;

    /// Count pending items
    #[allow(dead_code)]
    pub async fn count_pending(db: &mongodb::Database) -> Result<u64, BotError> {
        let collection: Collection<JobBlp> = db.collection(Self::COLLECTION);
        let count = collection
            .count_documents(doc! { "status": "pending" })
            .await?;
        Ok(count)
    }

    /// Count processing items
    #[allow(dead_code)]
    pub async fn count_processing(db: &mongodb::Database) -> Result<u64, BotError> {
        let collection: Collection<JobBlp> = db.collection(Self::COLLECTION);
        let count = collection
            .count_documents(doc! { "status": "processing" })
            .await?;
        Ok(count)
    }

    /// Count all items by conversion type (total usage statistics)
    pub async fn count_total_by_type(
        db: &mongodb::Database,
        conversion_type: ConversionTarget,
    ) -> Result<u64, BotError> {
        let collection: Collection<JobBlp> = db.collection(Self::COLLECTION);

        let filter = doc! {
            "conversion_type": conversion_type.to_string()
        };

        let count = collection.count_documents(filter).await?;
        Ok(count)
    }
}
