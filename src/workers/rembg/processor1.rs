use blp::core::decode::decode_to_rgba;
use image::{DynamicImage, ImageFormat, RgbImage, RgbaImage};
use once_cell::sync::Lazy;
use rembg_rs::manager::ModelManager;
use rembg_rs::options::{RemovalOptions, RemovalOptionsBuilder};
use rembg_rs::rembg::rembg;
use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::time::Instant;
use tokio::sync::{Notify, OnceCell};
use zip::CompressionMethod;
use zip::write::FileOptions;

use crate::db::rembg_queue::RembgQueueItem;
use crate::error::BotError;
use crate::state;

/// Global notifier for new tasks (like BLP worker pool)
static TASK_NOTIFY: OnceCell<Arc<Notify>> = OnceCell::const_new();

/// Global flag for rembg availability
static REMBG_AVAILABLE: AtomicBool = AtomicBool::new(false);

/// Check if rembg is available (ONNX Runtime installed and model loaded)
pub fn is_rembg_available() -> bool {
    REMBG_AVAILABLE.load(Ordering::Relaxed)
}

/// Notify workers that a new task is available
pub fn notify_rembg_task() {
    if let Some(notify) = TASK_NOTIFY.get() {
        // Wake ALL waiting workers (they will race to claim next pending item)
        notify.notify_waiters();
    }
}

/// Worker main loop - processes queue items continuously
/// Returns normally only when it should shut down gracefully
async fn worker_loop(worker_id: usize, notify: Arc<Notify>) {
    let worker_name = format!("rembg-worker-{}", worker_id);

    loop {
        match process_queue_item(&worker_name).await {
            Ok(true) => {
                continue;
            }
            Ok(false) => {
                notify.notified().await;
            }
            Err(e) => {
                eprintln!("[ERROR] {} error: {:?}", worker_name, e);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

/// Start worker threads for processing rembg queue
pub fn start_rembg_workers(worker_count: usize) {
    REMBG_AVAILABLE.store(true, Ordering::Relaxed);

    let notify = Arc::new(Notify::new());
    let _ = TASK_NOTIFY.set(notify.clone());

    for i in 0..worker_count {
        let notify_clone = Arc::clone(&notify);
        tokio::spawn(async move {
            worker_loop(i, notify_clone).await;
        });
    }
}

pub static MODEL_MANAGER: Lazy<Arc<ModelManager>> = Lazy::new(|| {
    let path = Path::new("models/u2net.onnx");
    let mgr = ModelManager::from_file(path)
        .unwrap_or_else(|e| panic!("❌ Failed to initialize model manager: {}", e));
    Arc::new(mgr)
});

async fn process_queue_item(worker_name: &str) -> Result<bool, BotError> {
    let db = state::db().await;

    // Try to get next pending item and mark as processing
    let item = match RembgQueueItem::get_next_pending(&db, worker_name.to_string()).await? {
        Some(it) => it,
        None => return Ok(false),
    };

    let item_id = item.id.unwrap();

    // Process attachments with timing
    let start_time = Instant::now();
    match process_attachments(&item).await {
        Ok(results) => {
            let duration = start_time.elapsed();
            // Mark as completed only after successful send
            if let Err(e) = send_response_editable(&item, results, duration).await {
                eprintln!("[REMBG][{}] Error sending response: {:?}", worker_name, e);
                let _ = RembgQueueItem::mark_failed(
                    &db,
                    &item_id,
                    format!("Failed to send response: {:?}", e),
                )
                .await;
            } else {
                let _ = RembgQueueItem::mark_completed(&db, &item_id).await;
            }
        }
        Err(e) => {
            eprintln!("[REMBG][{}] Error processing: {:?}", worker_name, e);
            let _ = RembgQueueItem::mark_failed(&db, &item_id, format!("{:?}", e)).await;
        }
    }

    Ok(true)
}

/// Result of processing a single attachment
#[allow(dead_code)]
struct ProcessedAttachment {
    filename: String,
    data: Vec<u8>,
    is_error: bool,
    is_mask: bool, // True if this is a mask image
}

/// Process all attachments
async fn process_attachments(item: &RembgQueueItem) -> Result<Vec<ProcessedAttachment>, BotError> {
    let client = state::client().await;
    let mut results = Vec::new();

    for attachment in &item.attachments {
        // Download image
        let response = client.get(&attachment.url).send().await?;
        let bytes = response.bytes().await?;

        // Try to process the image
        match process_single_image(
            &bytes,
            &RemovalOptionsBuilder::default()
                .threshold(item.threshold)
                .binary(item.binary_mode)
                .build()
                .unwrap(),
        )
        .await
        {
            Ok((image_data, mask_data)) => {
                // Add processed image
                let new_filename = change_extension(&attachment.filename, "png");
                results.push(ProcessedAttachment {
                    filename: new_filename,
                    data: image_data,
                    is_error: false,
                    is_mask: false,
                });

                // Add mask if requested
                if item.include_mask {
                    let mask_filename = change_extension(&attachment.filename, "mask.png");
                    results.push(ProcessedAttachment {
                        filename: mask_filename,
                        data: mask_data,
                        is_error: false,
                        is_mask: true,
                    });
                }
            }
            Err(e) => {
                eprintln!("[REMBG] Error processing {}: {:?}", attachment.filename, e);
                // Create error file instead of failing completely
                let error_filename = format!("{}.error.txt", attachment.filename);
                let error_content = format!(
                    "Failed to process image: {}\n\nError: {:?}\n\nTimestamp: {}",
                    attachment.filename,
                    e,
                    chrono::Utc::now().to_rfc3339()
                );
                results.push(ProcessedAttachment {
                    filename: error_filename,
                    data: error_content.into_bytes(),
                    is_error: true,
                    is_mask: false,
                });
            }
        }
    }

    // removed misplaced test module (moved to top-level tests below)

    Ok(results)
}

/// Process a single image with rembg
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

/// Send processed images to Discord (edit status message if possible)
async fn send_response_editable(
    item: &RembgQueueItem,
    results: Vec<ProcessedAttachment>,
    duration: Duration,
) -> Result<(), BotError> {
    let client = state::client().await;
    let token = state::token().await;
    let should_zip = item.zip;
    let limiter = state::rate_limiter().await;
    limiter.acquire().await;

    let seconds = duration.as_secs_f32();
    let file_count = if should_zip { 1 } else { results.len() };
    let content = format!(
        "✅ Processed {} file(s) for background removal in {:.2} seconds.",
        file_count, seconds
    );

    // PATCH (edit) the original status message
    use reqwest::multipart::{Form, Part};
    let mut form = Form::new();
    let payload = serde_json::json!({ "content": content });
    form = form.text("payload_json", payload.to_string());

    if should_zip {
        let zip_data = create_zip_archive(&results)?;
        let part = Part::bytes(zip_data)
            .file_name("processed.zip")
            .mime_str("application/zip")?;
        form = form.part("files[0]", part);
    } else {
        for (idx, result) in results.iter().enumerate() {
            let mime_type = if result.is_error {
                "text/plain"
            } else {
                "image/png"
            };
            let part = Part::bytes(result.data.clone())
                .file_name(result.filename.clone())
                .mime_str(mime_type)?;
            form = form.part(format!("files[{}]", idx), part);
        }
    }

    let response = client
        .patch(&format!(
            "https://discord.com/api/v10/channels/{}/messages/{}",
            item.channel_id, item.status_message_id
        ))
        .header("Authorization", format!("Bot {}", token))
        .multipart(form)
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        eprintln!(
            "[REMBG][worker] PATCH failed: HTTP {}: {}",
            status.as_u16(),
            error_text
        );
        return Err(BotError::new("message_edit_failed").push_str(format!(
            "HTTP {}: {}",
            status.as_u16(),
            error_text
        )));
    }

    Ok(())
}

/// Create ZIP archive from processed attachments
fn create_zip_archive(results: &[ProcessedAttachment]) -> Result<Vec<u8>, BotError> {
    let mut zip_buffer = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut zip_buffer);

    let options = FileOptions::<()>::default().compression_method(CompressionMethod::Stored);

    // Track filename usage to handle duplicates
    let mut filename_counts: HashMap<String, usize> = HashMap::new();

    for result in results {
        // Handle duplicate filenames
        let filename = if let Some(count) = filename_counts.get_mut(&result.filename) {
            *count += 1;
            let name_parts: Vec<&str> = result.filename.rsplitn(2, '.').collect();
            if name_parts.len() == 2 {
                format!("{}_{}.{}", name_parts[1], count, name_parts[0])
            } else {
                format!("{}_{}", result.filename, count)
            }
        } else {
            filename_counts.insert(result.filename.clone(), 1);
            result.filename.clone()
        };

        zip.start_file(filename, options)?;

        std::io::Write::write_all(&mut zip, &result.data)?;
    }

    zip.finish()?;

    Ok(zip_buffer.into_inner())
}

/// Change file extension
fn change_extension(filename: &str, new_ext: &str) -> String {
    let path = Path::new(filename);
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    format!("{}.{}", stem, new_ext)
}
