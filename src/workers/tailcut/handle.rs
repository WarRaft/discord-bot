use crate::discord::message::handle::CommandArgs;
use crate::discord::message::message::Message;
use crate::error::BotError;
use crate::state;
use crate::workers::processor::notify_workers;
use crate::workers::tailcut::job::JobTailcut;
use crate::workers::tailcut::processor::TailcutProcessor;
use mongodb::Collection;

pub async fn handle(message: Message, _args: &CommandArgs) -> Result<(), BotError> {
    let db = state::db().await;
    let collection: Collection<JobTailcut> = db.collection(JobTailcut::COLLECTION);

    collection
        .insert_one(JobTailcut {
            message,
            created: chrono::Utc::now(),
            ..Default::default()
        })
        .await?;

    notify_workers::<TailcutProcessor>();

    Ok(())
}

