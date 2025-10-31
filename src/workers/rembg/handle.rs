use crate::discord::message::handle::CommandArgs;
use crate::discord::message::message::Message;
use crate::error::BotError;
use crate::state;
use crate::workers::processor::notify_workers;
use crate::workers::rembg::job::JobRembg;
use crate::workers::rembg::processor::RembgProcessor;
use mongodb::Collection;

pub async fn handle(message: Message, args: &CommandArgs) -> Result<(), BotError> {
    let db = state::db().await;
    let collection: Collection<JobRembg> = db.collection(JobRembg::COLLECTION);

    collection
        .insert_one(JobRembg {
            message,
            threshold: args.threshold,
            binary: args.binary,
            mask: args.mask,
            zip: args.zip,
            created: chrono::Utc::now(),
            ..Default::default()
        })
        .await?;

    notify_workers::<RembgProcessor>();

    Ok(())
}
