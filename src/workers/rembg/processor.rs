use crate::discord::message::attachment::{AttachmentVecExt, ensure_unique_filenames};
use crate::discord::message::message::MessageReference;
use crate::discord::message::send::MessageSend;
use crate::error::BotError;
use crate::state;
use crate::workers::processor::{TaskProcessor, notify_workers};
use crate::workers::queue::QueueStatus;
use crate::workers::rembg::job::JobRembg;
use async_trait::async_trait;
use blp::core::decode::decode_to_rgba;
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

            let options = RemovalOptions {
                threshold: job.threshold,
                binary: job.binary,
                ..Default::default()
            };

            let result: Result<Vec<(String, Vec<u8>)>, BotError> = async {
                // Decode input to RGBA image
                let img = decode_to_rgba(&attachment_memory.bytes)?;

                // Get global model manager
                let manager = Arc::clone(&MODEL_MANAGER);

                // Run background removal
                let removal_result = rembg(&*manager, img, &options)?;

                // Extract images
                let img: &RgbaImage = removal_result.image();
                let mask_img: &RgbImage = removal_result.mask();

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

                let (image_bytes, mask_bytes) = (buf_image, buf_mask);

                let mut files = Vec::new();

                // Add processed image
                let image_filename = format!("{}_no_bg.png", attachment_memory.filename_stem);
                files.push((image_filename, image_bytes));

                // Add mask if requested
                if job.mask {
                    let mask_filename = format!("{}_mask.png", attachment_memory.filename_stem);
                    files.push((mask_filename, mask_bytes));
                }

                Ok(files)
            }
            .await;

            match result {
                Ok(files) => {
                    for (filename, bytes) in files {
                        converted_files.push((filename, bytes));
                    }
                }
                Err(e) => {
                    let error_filename = format!("{}.error.txt", attachment_memory.filename_stem);

                    let error_content = format!(
                        "Error processing file: {}\n\nError details:\n{:?}\n\nTimestamp: {}",
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
                let zip_filename = "processed_images.zip".to_string();
                vec![(zip_filename, zip_buffer)]
            } else {
                converted_files.clone()
            };

            let format_desc = "background removed".to_string();

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
