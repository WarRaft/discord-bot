use crate::discord::message::handle::CommandArgs;
use crate::discord::message::message::Message;
use crate::error::BotError;
use crate::state;
use crate::workers::processor::notify_workers;
use crate::workers::icon::job::JobIcon;
use crate::workers::icon::processor::IconProcessor;
use mongodb::Collection;

pub async fn handle(message: Message, _args: &CommandArgs) -> Result<(), BotError> {
    let db = state::db().await;
    let collection: Collection<JobIcon> = db.collection(JobIcon::COLLECTION);

    collection
        .insert_one(JobIcon {
            message,
            zip: true, // Always create archive
            created: chrono::Utc::now(),
            ..Default::default()
        })
        .await?;

    notify_workers::<IconProcessor>();

    Ok(())
}