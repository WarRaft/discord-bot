use crate::db::blp_queue::{BlpQueueItem, ConversionType};
use crate::discord::message::handle::CommandArgs;
use crate::discord::message::message::{Message, MessageReference};
use crate::discord::message::send::MessageSend;
use crate::error::BotError;
use crate::state;
use reqwest::Method;

/// Handle BLP/PNG conversion commands extracted from `handle_message`.
pub async fn handle(
    message: Message,
    conversion_type: ConversionType,
    args: CommandArgs,
) -> Result<(), BotError> {
    // Check if there are attachments
    if message.attachments.is_empty() {
        return Ok(());
    }

    let format_desc = match conversion_type {
        ConversionType::ToBLP => format!("to BLP (quality: {})", args.quality),
        ConversionType::ToPNG => "to PNG".to_string(),
    };

    let status_message = MessageSend {
        content: Some(format!(
            "✅ Added {} image(s) to conversion queue {}\n⏳ Processing...",
            message.attachments.len(),
            format_desc
        )),
        message_reference: Some(MessageReference {
            message_id: Some(message.id.clone()), //
            ..Default::default()
        }),
    }
    .send(Method::POST, &message.channel_id)
    .await?;

    let db = state::db().await;

    BlpQueueItem::new(
        message.author.id.clone(),
        message.channel_id.clone(),
        message.id.clone(),
        String::new(), // No interaction_id for messages
        String::new(), // No interaction_token for messages
        message.attachments,
        conversion_type,
        args.quality,
        args.should_zip,
        Some(status_message.id),
    )
    .insert(&*db)
    .await?;

    // Notify workers that a new task is available
    crate::workers::notify_blp_task();

    Ok(())
}
