use crate::db::rembg_queue::RembgQueueItem;
use crate::discord::message::handle::CommandArgs;
use crate::discord::message::message::{Message, MessageReference};
use crate::discord::message::send::MessageSend;
use crate::error::BotError;
use crate::state;
use reqwest::Method;
use crate::workers::rembg::processor1::is_rembg_available;

/// Handle rembg (background removal) command
pub async fn handle(message: Message, arg: &CommandArgs) -> Result<(), BotError> {
    // Check if there are attachments
    if message.attachments.is_empty() {
        return Ok(());
    }

    // Check if rembg is available
    if !is_rembg_available() {
        MessageSend {
            content: Some("❌ **Background removal is currently unavailable**\n\nONNX Runtime is not installed on the server.\nPlease contact the administrator to run: `./signal-download-models.sh`".to_string()),
            message_reference: Some(MessageReference {
                message_id: Some(message.id), //
                ..Default::default()
            }),
        }.send(Method::POST, &message.channel_id).await?;

        return Ok(());
    }

    let status_message = MessageSend {
        content: Some(format!(
            "🔄 Processing {} image(s) for background removal...",
            message.attachments.len()
        )),
        message_reference: Some(MessageReference {
            message_id: Some(message.id.clone()), //
            ..Default::default()
        }),
    }
    .send(Method::POST, &message.channel_id)
    .await?;

    let db = state::db().await;
    let _queue_id = RembgQueueItem::new(
        message.author.id, //
        message.channel_id,
        message.id.clone(),
        message.id.clone(),
        String::new(),
        status_message.id,
        message.attachments,
        arg.threshold,
        arg.mode,
        arg.mask,
        arg.zip,
    )
    .insert(&*db)
    .await?;

    // Notify workers that a new task is available
    //crate::workers::notify_rembg_task();

    Ok(())
}
