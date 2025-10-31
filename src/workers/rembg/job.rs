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

#[serde_as]
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[define_field_names]
pub struct JobRembg {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,

    pub message: Message,

    pub reply: Option<Message>,

    pub threshold: u8,

    pub binary: bool,

    pub mask: bool,

    pub zip: bool,

    pub status: QueueStatus,

    #[serde_as(as = "datetime::FromChrono04DateTime")]
    pub created: DateTime<Utc>,

    #[serde_as(as = "Option<datetime::FromChrono04DateTime>")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<DateTime<Utc>>,

    #[serde(default)]
    pub retry: u32,
}

impl JobRembg {
    const COLLECTION: &'static str = "discord_command_rembg";
    const MAX_RETRIES: u32 = 3;

    /// Count total number of rembg tasks
    pub async fn count_total(db: &mongodb::Database) -> Result<u64, BotError> {
        let collection: Collection<JobRembg> = db.collection(Self::COLLECTION);
        let count = collection.count_documents(doc! {}).await?;
        Ok(count)
    }
}
