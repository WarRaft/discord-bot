use crate::discord::message::attachment::{AttachmentVecExt, ensure_unique_filenames};
use crate::discord::message::message::MessageReference;
use crate::discord::message::send::MessageSend;
use crate::error::BotError;
use crate::state;
use crate::workers::blp::job::ConversionTarget;
use crate::workers::processor::{TaskProcessor, notify_workers};
use crate::workers::queue::QueueStatus;
use crate::workers::rembg::job::JobRembg;
use async_trait::async_trait;
use blp::core::decode::decode_to_rgba;
use blp::core::image::ImageBlp;
use bson::{Bson, doc, serialize_to_bson};
use image::{DynamicImage, ImageFormat, RgbImage, RgbaImage};
use mongodb::Collection;
use once_cell::sync::Lazy;
use rembg_rs::manager::ModelManager;
use rembg_rs::options::RemovalOptions;
use rembg_rs::rembg::rembg;
use reqwest::Method;
use std::io::{Cursor, Write};
use std::path::Path;
use std::sync::Arc;
use zip::ZipWriter;
use zip::write::FileOptions;

pub static MODEL_MANAGER: Lazy<Arc<ModelManager>> = Lazy::new(|| {
    let path = Path::new("models/u2net.onnx");
    let mgr = ModelManager::from_file(path)
        .unwrap_or_else(|e| panic!("❌ Failed to initialize model manager: {}", e));
    Arc::new(mgr)
});

pub struct RembgProcessor;
#[async_trait]
impl TaskProcessor for RembgProcessor {
    const POOL: &'static str = "rembg";

    async fn process_queue_item() -> Result<bool, BotError> {
        let db = state::db().await;
        let collection: Collection<JobRembg> = db.collection(JobRembg::COLLECTION);

        let result = collection
            .find_one_and_update(
                doc! {
                    JobRembg::STATUS: QueueStatus::Pending.as_ref(),
                    JobRembg::RETRY: { "$lt": JobRembg::MAX_RETRIES }
                },
                doc! {
                    "$set": {
                        JobRembg::STATUS: QueueStatus::Processing.as_ref()
                    }
                },
            )
            .sort(doc! { JobRembg::CREATED: 1 })
            .return_document(mongodb::options::ReturnDocument::After)
            .await?;

        let Some(job) = result else {
            return Ok(false);
        };

        let Some(ref reply) = job.reply else {
            if job.message.attachments.is_empty() {
                let reply_msg = MessageSend {
                    content: Some("❌ No attachments found — nothing to convert.".to_string()),
                    message_reference: Some(MessageReference {
                        message_id: Some(job.message.id.clone()),
                        ..Default::default()
                    }),
                    attachments: None,
                }
                .send(Method::POST, &job.message.channel_id, None)
                .await?;

                collection
                    .update_one(
                        doc! { "_id": &job.id },
                        doc! {
                            "$set": {
                                JobRembg::REPLY: serialize_to_bson(&reply_msg)?,
                                JobRembg::STATUS: QueueStatus::Completed.as_ref(),
                            },
                        },
                    )
                    .await?;
            } else {
                let reply_msg = MessageSend {
                    content: Some(format!(
                        "✅ Added {} image(s) to conversion queue \n⏳ Processing...",
                        job.message.attachments.len(),
                    )),
                    message_reference: Some(MessageReference {
                        message_id: Some(job.message.id.clone()),
                        ..Default::default()
                    }),
                    attachments: None,
                }
                .send(Method::POST, &job.message.channel_id, None)
                .await?;

                collection
                    .update_one(
                        doc! { "_id": &job.id },
                        doc! {
                            "$set": {
                                JobRembg::REPLY: serialize_to_bson(&reply_msg)?,
                                JobRembg::STATUS: QueueStatus::Pending.as_ref(),
                            },
                        },
                    )
                    .await?;
            }

            notify_workers::<RembgProcessor>();
            return Ok(true);
        };

        let attachment = ensure_unique_filenames(job.message.attachments)
            .download_all(4)
            .await;

        let mut converted_files = Vec::new();

        for attachment_memory in attachment {
            if let Some(ref error) = attachment_memory.error {
                let error_filename = format!("{}.error.txt", attachment_memory.filename_stem);

                let error_content = format!(
                    "Error downloading file: {}\n\nError details:\n{}\n\nTimestamp: {}",
                    attachment_memory.meta.filename,
                    error,
                    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
                );
                converted_files.push((error_filename, error_content.into_bytes()));
                continue;
            }

            let result: Result<(String, Vec<u8>), BotError> = match job.target {
                ConversionTarget::BLP => {
                    let output_filename = format!("{}.blp", attachment_memory.filename_stem);

                    let blp_bytes = tokio::task::spawn_blocking({
                        let image_data = attachment_memory.bytes.to_vec();
                        move || {
                            let mut img = ImageBlp::from_buf(&image_data)?;

                            let mip_visible = vec![true; 16];
                            img.decode(&image_data, &mip_visible)?;

                            let ctx = img.encode_blp(job.quality, &mip_visible)?;

                            Ok::<_, blp::error::error::BlpError>(ctx.bytes)
                        }
                    })
                    .await??;

                    Ok((output_filename, blp_bytes))
                }
                ConversionTarget::PNG => {
                    let output_filename = format!("{}.png", attachment_memory.filename_stem);

                    let png_bytes = tokio::task::spawn_blocking({
                        let blp_data = attachment_memory.bytes.to_vec();
                        move || {
                            let mut img = ImageBlp::from_buf(&blp_data)?;

                            // Decode only first mip level
                            img.decode(
                                &blp_data,
                                &[
                                    true, false, false, false, false, false, false, false, false,
                                    false, false, false, false, false, false, false,
                                ],
                            )?;

                            let rgba = img
                                .mipmaps
                                .get(0)
                                .ok_or_else(|| blp::error::error::BlpError::new("no_mipmap"))?
                                .image
                                .as_ref()
                                .ok_or_else(|| blp::error::error::BlpError::new("no_image_data"))?;

                            let mut png_buffer = Cursor::new(Vec::new());
                            DynamicImage::ImageRgba8(rgba.clone())
                                .write_to(&mut png_buffer, ImageFormat::Png)?;

                            Ok::<_, blp::error::error::BlpError>(png_buffer.into_inner())
                        }
                    })
                    .await??;

                    Ok((output_filename, png_bytes))
                }
            };

            match result {
                Ok((filename, bytes)) => {
                    converted_files.push((filename, bytes));
                }
                Err(e) => {
                    let error_filename = format!("{}.error.txt", attachment_memory.filename_stem);

                    let error_content = format!(
                        "Error converting file: {}\n\nError details:\n{:?}\n\nTimestamp: {}",
                        attachment_memory.meta.filename,
                        e,
                        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
                    );
                    converted_files.push((error_filename, error_content.into_bytes()));
                }
            }
        }

        // Send response
        {
            let conversion_time = format!(
                "{:.2}s",
                chrono::Utc::now()
                    .signed_duration_since(job.created)
                    .num_milliseconds() as f64
                    / 1000.0
            );

            let files_to_send = if job.zip {
                let mut zip_buffer = Vec::new();
                {
                    let cursor = Cursor::new(&mut zip_buffer);
                    let mut zip = ZipWriter::new(cursor);
                    let options = FileOptions::<()>::default()
                        .compression_method(zip::CompressionMethod::Stored);

                    for (filename, data) in &converted_files {
                        zip.start_file(filename, options)?;
                        zip.write_all(data)?;
                    }

                    zip.finish()?;
                }
                let zip_filename = match job.target {
                    ConversionTarget::BLP => "converted_images.blp.zip".to_string(),
                    ConversionTarget::PNG => "converted_images.png.zip".to_string(),
                };
                vec![(zip_filename, zip_buffer)]
            } else {
                converted_files.clone()
            };

            let format_desc = match job.target {
                ConversionTarget::BLP => {
                    format!("to BLP (quality: {})", job.quality)
                }
                ConversionTarget::PNG => "to PNG".to_string(),
            };

            let _ = MessageSend {
                content: Some(format!(
                    "✅ Converted {} image(s) {}{}\n⏱️ Completed in {}",
                    converted_files.len(),
                    format_desc,
                    if job.zip { " (zipped)" } else { "" },
                    conversion_time
                )),
                message_reference: None,
                attachments: Some(files_to_send),
            }
            .send(Method::PATCH, &job.message.channel_id, Some(&reply.id))
            .await?;
        }

        let collection: Collection<JobRembg> = db.collection(JobRembg::COLLECTION);

        collection
            .update_one(
                doc! { "_id": job.id.unwrap() },
                doc! {
                    "$set": {
                        JobRembg::STATUS: QueueStatus::Completed.as_ref(),
                        JobRembg::COMPLETED: Bson::DateTime(bson::DateTime::now())
                    }
                },
            )
            .await?;

        notify_workers::<RembgProcessor>();
        Ok(true)
    }
}

pub async fn process_single_image(
    image_bytes: &[u8],
    options: &RemovalOptions,
) -> Result<(Vec<u8>, Vec<u8>), BotError> {
    // Decode input to RGBA image
    let img = decode_to_rgba(image_bytes)?;

    // Get global model manager
    let manager = Arc::clone(&MODEL_MANAGER);

    // Run background removal
    let result = rembg(&*manager, img, options)?;

    // Extract images
    let img: &RgbaImage = result.image();
    let mask_img: &RgbImage = result.mask();

    // Encode result to PNG bytes
    let mut buf_image = Vec::new();
    let mut buf_mask = Vec::new();

    // Rgba → PNG
    {
        let dyn_img = DynamicImage::ImageRgba8(img.clone());
        dyn_img.write_to(&mut Cursor::new(&mut buf_image), ImageFormat::Png)?;
    }

    // Mask → PNG
    {
        let dyn_mask = DynamicImage::ImageRgb8(mask_img.clone());
        dyn_mask.write_to(&mut Cursor::new(&mut buf_mask), ImageFormat::Png)?;
    }

    Ok((buf_image, buf_mask))
}
