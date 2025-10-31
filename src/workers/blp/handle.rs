use crate::discord::message::handle::CommandArgs;
use crate::discord::message::message::Message;
use crate::error::BotError;
use crate::state;
use crate::workers::blp::job::{ConversionTarget, JobBlp};
use crate::workers::blp::processor::BlpProcessor;
use crate::workers::processor::notify_workers;
use mongodb::Collection;

pub async fn handle(
    message: Message,
    target: ConversionTarget,
    args: CommandArgs,
) -> Result<(), BotError> {
    let db = state::db().await;
    let collection: Collection<JobBlp> = db.collection(JobBlp::COLLECTION);

    collection
        .insert_one(JobBlp {
            message,
            target,
            quality: args.quality,
            zip: args.zip,
            created: chrono::Utc::now(),
            ..Default::default()
        })
        .await?;

    notify_workers::<BlpProcessor>();

    Ok(())
}
