use crate::assets::*;
use crate::discord::message::attachment::{AttachmentVecExt, ensure_unique_filenames};
use crate::discord::message::message::MessageReference;
use crate::discord::message::send::MessageSend;
use crate::error::BotError;
use crate::state;
use crate::workers::icon::job::JobIcon;
use crate::workers::processor::{TaskProcessor, notify_workers};
use crate::workers::queue::QueueStatus;
use async_trait::async_trait;
use blp::core::decode::decode_to_rgba;
use bson::{Bson, doc, serialize_to_bson};
use image::{DynamicImage, GenericImageView, ImageFormat, RgbaImage};
use mongodb::Collection;
use reqwest::Method;
use std::io::{Cursor, Write};
use zip::ZipWriter;
use zip::write::FileOptions;

pub struct IconProcessor;
#[async_trait]
impl TaskProcessor for IconProcessor {
    const POOL: &'static str = "icon";

    async fn process_queue_item() -> Result<bool, BotError> {
        let db = state::db().await;
        let collection: Collection<JobIcon> = db.collection(JobIcon::COLLECTION);

        let result = collection
            .find_one_and_update(
                doc! {
                    JobIcon::STATUS: QueueStatus::Pending.as_ref(),
                    JobIcon::RETRY: { "$lt": JobIcon::MAX_RETRIES }
                },
                doc! {
                    "$set": {
                        JobIcon::STATUS: QueueStatus::Processing.as_ref()
                    }
                },
            )
            .sort(doc! { JobIcon::CREATED: 1 })
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
                                JobIcon::REPLY: serialize_to_bson(&reply_msg)?,
                                JobIcon::STATUS: QueueStatus::Completed.as_ref(),
                            },
                        },
                    )
                    .await?;
            } else {
                let reply_msg = MessageSend {
                    content: Some(format!(
                        "✅ Added {} image(s) to icon conversion queue \n⏳ Processing...",
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
                                JobIcon::REPLY: serialize_to_bson(&reply_msg)?,
                                JobIcon::STATUS: QueueStatus::Pending.as_ref(),
                            },
                        },
                    )
                    .await?;
            }

            notify_workers::<IconProcessor>();
            return Ok(true);
        };

        let attachment = ensure_unique_filenames(job.message.attachments)
            .download_all(4)
            .await;

        let mut converted_files = Vec::new();
        let mut collage_images = Vec::new();

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

            let result: Result<Vec<(String, Vec<u8>)>, BotError> = async {
                // Decode input to RGBA image
                let img = decode_to_rgba(&attachment_memory.bytes)?;

                // Create square crop from center
                let (width, height) = img.dimensions();
                let size = width.min(height);
                let x = (width - size) / 2;
                let y = (height - size) / 2;

                let cropped = img.view(x, y, size, size).to_image();

                // Resize to 64x64
                let resized = image::imageops::resize(
                    &cropped,
                    64,
                    64,
                    image::imageops::FilterType::Lanczos3,
                );

                // Create versions with overlays and convert to BLP
                let mut files = Vec::new();

                let overlays = vec![
                    ("BTN", &*ICON_BTN, "ReplaceableTextures\\CommandButtons\\"),
                    (
                        "DISBTN",
                        &*ICON_DISBTN,
                        "ReplaceableTextures\\CommandButtonsDisabled\\",
                    ),
                    ("ATC", &*ICON_ATC, "ReplaceableTextures\\CommandButtons\\"),
                    (
                        "DISATC",
                        &*ICON_DISATC,
                        "ReplaceableTextures\\CommandButtonsDisabled\\",
                    ),
                    ("PAS", &*ICON_PAS, "ReplaceableTextures\\CommandButtons\\"),
                    (
                        "DISPAS",
                        &*ICON_DISPAS,
                        "ReplaceableTextures\\CommandButtonsDisabled\\",
                    ),
                ];

                for (prefix, overlay, path) in overlays {
                    // Apply overlay
                    let mut combined = resized.clone();
                    image::imageops::overlay(&mut combined, overlay, 0, 0);

                    // Save PNG version for collage (all variants for each image)
                    collage_images.push(combined.clone());

                    // Convert to BLP with high quality JPEG compression and all mip levels
                    let blp_bytes = tokio::task::spawn_blocking(move || {
                        let img =
                            blp::core::image::ImageBlp::from_rgba(&combined.into_raw(), 64, 64)?;
                        let mip_visible = vec![]; // Empty array = all mip levels visible by default
                        let ctx = img.encode_blp(95, &mip_visible)?; // High JPEG quality (95/100)
                        Ok::<_, blp::error::error::BlpError>(ctx.bytes)
                    })
                    .await??;

                    let filename = format!("{}{}.blp", prefix, attachment_memory.filename_stem);
                    let archive_path = format!("{}{}", path, filename);
                    files.push((archive_path, blp_bytes));
                }

                Ok(files)
            }
            .await;

            match result {
                Ok(files) => {
                    for (archive_path, bytes) in files {
                        converted_files.push((archive_path, bytes));
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

        // Create collage from processed images
        let collage = create_processed_icon_collage(&collage_images)?;

        // Add collage to archive
        converted_files.push(("icon_collage.png".to_string(), collage.clone()));

        // Create ZIP archive with proper Warcraft III structure
        let converted_count = converted_files.len();
        let zip_buffer = create_icon_archive(converted_files)?;

        // Send response
        {
            let conversion_time = format!(
                "{:.2}s",
                chrono::Utc::now()
                    .signed_duration_since(job.created)
                    .num_milliseconds() as f64
                    / 1000.0
            );

            let files_to_send = vec![
                ("icon_collage.png".to_string(), collage),
                ("icons.zip".to_string(), zip_buffer),
            ];

            let format_desc = "converted to icons".to_string();

            let _ = MessageSend {
                content: Some(format!(
                    "✅ Converted {} image(s) {}{}\n⏱️ Completed in {}",
                    converted_count, format_desc, "", conversion_time
                )),
                message_reference: None,
                attachments: Some(files_to_send),
            }
            .send(Method::PATCH, &job.message.channel_id, Some(&reply.id))
            .await?;
        }

        let collection: Collection<JobIcon> = db.collection(JobIcon::COLLECTION);

        collection
            .update_one(
                doc! { "_id": job.id.unwrap() },
                doc! {
                    "$set": {
                        JobIcon::STATUS: QueueStatus::Completed.as_ref(),
                        JobIcon::COMPLETED: Bson::DateTime(bson::DateTime::now())
                    }
                },
            )
            .await?;

        notify_workers::<IconProcessor>();
        Ok(true)
    }
}

fn create_builtin_icon_collage() -> Result<Vec<u8>, BotError> {
    let icons = vec![
        &*ICON_BTN,      // Active button
        &*ICON_DISBTN,   // Disabled button
        &*ICON_ATC,      // Attack command
        &*ICON_DISATC,   // Disabled attack command
        &*ICON_PAS,      // Passive command
        &*ICON_DISPAS,   // Disabled passive command
    ];

    let icons_per_row = 1usize; // 1 column
    let rows = 6usize; // 6 rows

    let icon_size = 64usize;
    let padding = 4usize;
    let collage_width = icons_per_row * (icon_size + padding) - padding;
    let collage_height = rows * (icon_size + padding) - padding;

    let mut collage = RgbaImage::new(collage_width as u32, collage_height as u32);

    for (i, icon) in icons.iter().enumerate() {
        let row = i / icons_per_row;
        let col = i % icons_per_row;

        let x = col * (icon_size + padding);
        let y = row * (icon_size + padding);

        // Copy icon to collage
        for (dx, dy, pixel) in icon.to_rgba8().enumerate_pixels() {
            let px = x + dx as usize;
            let py = y + dy as usize;
            if px < collage_width && py < collage_height {
                collage.put_pixel(px as u32, py as u32, *pixel);
            }
        }
    }

    // Encode collage to PNG
    let mut buf = Vec::new();
    let dyn_img = DynamicImage::ImageRgba8(collage);
    dyn_img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)?;

    Ok(buf)
}

fn create_processed_icon_collage(images: &[RgbaImage]) -> Result<Vec<u8>, BotError> {
    if images.is_empty() {
        // Fallback to builtin collage if no images
        return create_builtin_icon_collage();
    }

    // Each image has 6 variants (BTN, DISBTN, ATC, DISATC, PAS, DISPAS)
    let variants_per_image = 6;
    let num_images = images.len() / variants_per_image;
    
    if images.len() % variants_per_image != 0 {
        return create_builtin_icon_collage(); // Fallback if data is corrupted
    }

    // Calculate grid layout close to square
    // We have 'num_images' columns, each with 'variants_per_image' rows
    let icon_size = 64usize;
    let padding = 4usize;
    
    // Grid dimensions: variants_per_image rows × num_images columns
    let collage_width = num_images * (icon_size + padding) - padding;
    let collage_height = variants_per_image * (icon_size + padding) - padding;

    let mut collage = RgbaImage::new(collage_width as u32, collage_height as u32);

    for (i, icon) in images.iter().enumerate() {
        let image_idx = i / variants_per_image; // Which image (column)
        let variant_idx = i % variants_per_image; // Which variant (row)

        let x = image_idx * (icon_size + padding);
        let y = variant_idx * (icon_size + padding);

        // Copy icon to collage
        for (dx, dy, pixel) in icon.enumerate_pixels() {
            let px = x + dx as usize;
            let py = y + dy as usize;
            if px < collage_width && py < collage_height {
                collage.put_pixel(px as u32, py as u32, *pixel);
            }
        }
    }

    // Encode collage to PNG
    let mut buf = Vec::new();
    let dyn_img = DynamicImage::ImageRgba8(collage);
    dyn_img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)?;

    Ok(buf)
}

fn create_icon_archive(converted_files: Vec<(String, Vec<u8>)>) -> Result<Vec<u8>, BotError> {
    let mut zip_buffer = Vec::new();
    {
        let cursor = Cursor::new(&mut zip_buffer);
        let mut zip = ZipWriter::new(cursor);
        let options =
            FileOptions::<()>::default().compression_method(zip::CompressionMethod::Stored);

        for (archive_path, data) in converted_files {
            zip.start_file(archive_path, options)?;
            zip.write_all(&data)?;
        }

        zip.finish()?;
    }

    Ok(zip_buffer)
}
