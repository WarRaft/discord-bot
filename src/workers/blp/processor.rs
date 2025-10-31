use crate::discord::message::message::MessageReference;
use crate::discord::message::send::MessageSend;
use crate::error::BotError;
use crate::state;
use crate::workers::blp::job::{ConversionTarget, JobBlp};
use crate::workers::processor::{TaskProcessor, notify_workers};
use crate::workers::queue::QueueStatus;
use async_trait::async_trait;
use bson::doc;
use mongodb::Collection;
use reqwest::Method;

pub struct BlpProcessor;

#[async_trait]
impl TaskProcessor for BlpProcessor {
    const POOL: &'static str = "blp";

    async fn process_queue_item() -> Result<bool, BotError> {
        let db = state::db().await;
        let collection: Collection<JobBlp> = db.collection(JobBlp::COLLECTION);

        let result = collection
            .find_one_and_update(
                doc! {
                    JobBlp::STATUS: QueueStatus::Pending.as_ref(),
                    JobBlp::RETRY: { "$lt": JobBlp::MAX_RETRIES }
                },
                doc! {
                    "$set": {
                        JobBlp::STATUS: QueueStatus::Processing.as_ref()
                    }
                },
            )
            .sort(doc! { JobBlp::CREATED: 1 })
            .return_document(mongodb::options::ReturnDocument::After)
            .await?;

        let Some(job) = result else {
            return Ok(false);
        };

        let Some(reply) = job.reply else {
            // если нет вложений — сразу ответ и Complete
            if job.message.attachments.is_empty() {
                let reply_msg = MessageSend {
                    content: Some("❌ No attachments found — nothing to convert.".to_string()),
                    message_reference: Some(MessageReference {
                        message_id: Some(job.message.id.clone()),
                        ..Default::default()
                    }),
                }
                .send(Method::POST, &job.message.channel_id)
                .await?;

                collection
                    .update_one(
                        doc! { "_id": &job.id },
                        doc! {
                            "$set": {
                                JobBlp::REPLY: &reply_msg.id,
                                JobBlp::STATUS: QueueStatus::Completed.as_ref(),
                            },
                        },
                    )
                    .await?;
            } else {
                // иначе — обычный сценарий
                let reply_msg = MessageSend {
                    content: Some(format!(
                        "✅ Added {} image(s) to conversion queue {}\n⏳ Processing...",
                        job.message.attachments.len(),
                        match job.target {
                            ConversionTarget::BLP => format!("to BLP (quality: {})", job.quality),
                            ConversionTarget::PNG => "to PNG".to_string(),
                        }
                    )),
                    message_reference: Some(MessageReference {
                        message_id: Some(job.message.id.clone()),
                        ..Default::default()
                    }),
                }
                .send(Method::POST, &job.message.channel_id)
                .await?;

                collection
                    .update_one(
                        doc! { "_id": &job.id },
                        doc! {
                            "$set": {
                                JobBlp::REPLY: &reply_msg.id,
                                JobBlp::STATUS: QueueStatus::Pending.as_ref(),
                            },
                        },
                    )
                    .await?;
            }

            notify_workers::<BlpProcessor>();
            return Ok(true);
        };

        let attachment = job.message.attachments.download_all(4).await;


        Ok(true)
    }
}

async fn process_attachments(item: &mut BlpQueueItem) -> Result<Vec<(String, Vec<u8>)>, BotError> {
    let mut converted_files = Vec::new();
    let mut filename_counts = std::collections::HashMap::new();

    for attachment in &mut item.attachments {
        let result = match item.target {
            crate::workers::blp::queue::ConversionTarget::BLP => {
                convert_to_blp(attachment, item.quality).await
            }
            crate::workers::blp::queue::ConversionTarget::PNG => convert_to_png(attachment).await,
        };

        match result {
            Ok((mut filename, bytes)) => {
                // Handle duplicate filenames
                let count = filename_counts.entry(filename.clone()).or_insert(0);
                *count += 1;
                if *count > 1 {
                    let (name, ext) = if let Some(dot_pos) = filename.rfind('.') {
                        (filename[..dot_pos].to_string(), &filename[dot_pos..])
                    } else {
                        (filename.clone(), "")
                    };
                    filename = format!("{}_{}{}", name, count, ext);
                }
                converted_files.push((filename, bytes));
            }
            Err(e) => {
                // Create error text file instead of failing the whole batch
                let base_name = attachment
                    .filename
                    .rsplit('.')
                    .nth(1)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| attachment.filename.clone());
                let mut error_filename = format!("{}.error.txt", base_name);

                // Handle duplicate error filenames too
                let count = filename_counts.entry(error_filename.clone()).or_insert(0);
                *count += 1;
                if *count > 1 {
                    error_filename = format!("{}.error_{}.txt", base_name, count);
                }

                let error_content = format!(
                    "Error converting file: {}\n\nError details:\n{:?}\n\nTimestamp: {}",
                    attachment.filename,
                    e,
                    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
                );
                converted_files.push((error_filename, error_content.into_bytes()));

                eprintln!("[ERROR] Failed to convert {}: {:?}", attachment.filename, e);
                // Continue processing other files instead of returning error
            }
        }
    }

    Ok(converted_files)
}

async fn convert_to_blp(
    attachment: &Attachment,
    quality: u8,
) -> Result<(String, Vec<u8>), BotError> {
    // Download image
    let client = state::client().await;
    let response = client.get(&attachment.url).send().await?;

    if !response.status().is_success() {
        return Err(BotError::new("download_failed").push_str(format!(
            "HTTP {}: {}",
            response.status(),
            attachment.url
        )));
    }

    let image_data = response.bytes().await?.to_vec();

    // Generate output filename
    let output_filename = std::path::Path::new(&attachment.filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| format!("{}.blp", s))
        .unwrap_or_else(|| format!("{}.blp", Uuid::new_v4()));

    // Convert to BLP in memory using blp-rs
    let blp_bytes = tokio::task::spawn_blocking(move || {
        use blp::core::image::ImageBlp;

        // Parse image
        let mut img = ImageBlp::from_buf(&image_data)?;

        // Decode with all mips enabled
        let mip_visible = vec![true; 16];
        img.decode(&image_data, &mip_visible)?;

        // Encode to BLP (returns Ctx with bytes in memory)
        let ctx = img.encode_blp(quality, &mip_visible)?;

        Ok::<_, blp::error::error::BlpError>(ctx.bytes)
    })
    .await??;

    Ok((output_filename, blp_bytes))
}

async fn convert_to_png(attachment: &Attachment) -> Result<(String, Vec<u8>), BotError> {
    // Download BLP file
    let client = state::client().await;
    let response = client.get(&attachment.url).send().await?;

    if !response.status().is_success() {
        return Err(BotError::new("download_failed").push_str(response.status().to_string()));
    }

    let blp_data = response.bytes().await?.to_vec();

    // Generate output filename (replace .blp with .png)
    let output_filename = attachment
        .filename
        .strip_suffix(".blp")
        .unwrap_or(&attachment.filename)
        .to_string()
        + ".png";

    // Convert BLP → PNG in memory using blp-rs
    let png_bytes = tokio::task::spawn_blocking(move || {
        use blp::core::image::ImageBlp;
        use image::{DynamicImage, ImageFormat};

        // Parse BLP
        let mut img = ImageBlp::from_buf(&blp_data)?;

        // Decode only first mip level
        img.decode(
            &blp_data,
            &[
                true, false, false, false, false, false, false, false, false, false, false, false,
                false, false, false, false,
            ],
        )?;

        // Get first mipmap
        let mipmap = img
            .mipmaps
            .get(0)
            .ok_or_else(|| blp::error::error::BlpError::new("no_mipmap"))?;
        let rgba = mipmap
            .image
            .as_ref()
            .ok_or_else(|| blp::error::error::BlpError::new("no_image_data"))?;

        // Encode to PNG in memory
        let mut png_buffer = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(rgba.clone()).write_to(&mut png_buffer, ImageFormat::Png)?;

        Ok::<_, blp::error::error::BlpError>(png_buffer.into_inner())
    })
    .await??;

    Ok((output_filename, png_bytes))
}

/// Create ZIP archive from converted files
fn create_zip_archive(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, BotError> {
    use std::io::Write;

    let mut zip_buffer = Vec::new();
    {
        let cursor = Cursor::new(&mut zip_buffer);
        let mut zip = ZipWriter::new(cursor);
        let options =
            FileOptions::<()>::default().compression_method(zip::CompressionMethod::Stored); // No compression for already compressed images

        for (filename, data) in files {
            zip.start_file(filename, options)?;
            zip.write_all(data)?;
        }

        zip.finish()?;
    }
    Ok(zip_buffer)
}

async fn send_response(
    item: &BlpQueueItem,
    converted_files: Vec<(String, Vec<u8>)>,
) -> Result<(), BotError> {
    // Acquire rate limit token BEFORE making request
    let limiter = state::rate_limiter().await;
    limiter.acquire().await;

    let client = state::client().await;
    let token = state::token().await;

    // Calculate conversion time
    let conversion_time = if let Some(started) = item.started_at {
        let duration = chrono::Utc::now().signed_duration_since(started);
        format!("{:.2}s", duration.num_milliseconds() as f64 / 1000.0)
    } else {
        "unknown".to_string()
    };

    // Prepare files for sending - either as ZIP or individual files
    let files_to_send = if item.zip {
        // Create ZIP archive (even for single files if requested)
        let zip_data = create_zip_archive(&converted_files)?;
        let zip_filename = match item.target {
            crate::workers::blp::queue::ConversionTarget::BLP => {
                "converted_images.blp.zip".to_string()
            }
            crate::workers::blp::queue::ConversionTarget::PNG => {
                "converted_images.png.zip".to_string()
            }
        };
        vec![(zip_filename, zip_data)]
    } else {
        // Send individual files
        converted_files.clone()
    };

    use reqwest::multipart::{Form, Part};

    let mut form = Form::new();

    // Update text content with completion status
    let format_desc = match item.target {
        crate::workers::blp::queue::ConversionTarget::BLP => {
            format!("to BLP (quality: {})", item.quality)
        }
        crate::workers::blp::queue::ConversionTarget::PNG => "to PNG".to_string(),
    };

    let payload = serde_json::json!({
        "content": format!(
            "✅ Converted {} image(s) {}{}\n⏱️ Completed in {}",
            converted_files.len(),
            format_desc,
            if item.zip { " (zipped)" } else { "" },
            conversion_time
        )
    });
    form = form.text("payload_json", payload.to_string());

    // Add files from memory
    for (idx, (filename, file_data)) in files_to_send.iter().enumerate() {
        let part = Part::bytes(file_data.clone())
            .file_name(filename.clone())
            .mime_str("application/octet-stream")?;

        form = form.part(format!("files[{}]", idx), part);
    }

    // Edit the status message (PATCH instead of POST)
    let response = client
        .patch(&format!(
            "https://discord.com/api/v10/channels/{}/messages/{}",
            item.channel_id, status_msg_id
        ))
        .header("Authorization", format!("Bot {}", token))
        .multipart(form)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(BotError::new("message_edit_failed").push_str(format!(
            "HTTP {}: {}",
            status.as_u16(),
            error_text
        )));
    }

    Ok(())
}
