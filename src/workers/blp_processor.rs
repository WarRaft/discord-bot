use std::io::Cursor;
use std::sync::Arc;
use tokio::sync::{OnceCell, watch, Notify};
use tokio::time::{Duration, sleep};
use uuid::Uuid;
use zip::{ZipWriter, write::FileOptions};

use crate::db::blp_queue::{AttachmentItem, BlpQueueItem};
use crate::error::{BotError, Result};
use crate::state;

/// Global sender for controlling worker count
static WORKER_SCALE: OnceCell<watch::Sender<usize>> = OnceCell::const_new();

/// Global notifier for new tasks (efficient, doesn't spam)
static TASK_NOTIFY: OnceCell<Arc<Notify>> = OnceCell::const_new();

/// Notify workers that a new task is available
/// Uses Notify which is more efficient than channels for wake-ups
pub fn notify_blp_task() {
    if let Some(notify) = TASK_NOTIFY.get() {
        notify.notify_waiters(); // Wake ALL waiting workers
    }
}

/// Start BLP worker pool supervisor with 3 workers by default
/// The supervisor maintains the specified number of workers and allows dynamic scaling
pub fn start_blp_workers(worker_count: usize) {
    // Create a channel to control worker count
    let (tx, rx) = watch::channel(worker_count);

    // Create notification primitive
    let notify = Arc::new(Notify::new());

    // Store the senders globally
    let _ = WORKER_SCALE.set(tx);
    let _ = TASK_NOTIFY.set(notify.clone());

    // Spawn the supervisor
    tokio::spawn(async move {
        supervise_workers(rx, notify).await;
    });
}

/// Scale the number of workers at runtime
/// Returns true if scaling was successful, false if workers not initialized
#[allow(dead_code)]
pub fn scale_workers(new_count: usize) -> bool {
    if let Some(tx) = WORKER_SCALE.get() {
        tx.send(new_count).is_ok()
    } else {
        false
    }
}

/// Supervisor that maintains the target number of workers
async fn supervise_workers(mut worker_count_rx: watch::Receiver<usize>, notify: Arc<Notify>) {
    let mut handles = Vec::new();
    let mut current_count = *worker_count_rx.borrow();

    // Start initial workers
    for worker_id in 0..current_count {
        let notify_clone = notify.clone();
        let handle = tokio::spawn(worker_loop(worker_id, notify_clone));
        handles.push((worker_id, handle));
    }

    loop {
        tokio::select! {
            // Check if worker count changed
            Ok(()) = worker_count_rx.changed() => {
                let new_count = *worker_count_rx.borrow();
                if new_count != current_count {
                    if new_count > current_count {
                        // Add workers
                        for worker_id in current_count..new_count {
                            let notify_clone = notify.clone();
                            let handle = tokio::spawn(worker_loop(worker_id, notify_clone));
                            handles.push((worker_id, handle));
                        }
                    } else {
                        // Remove workers (they will finish their current task and exit)
                        handles.truncate(new_count);
                    }

                    current_count = new_count;
                }
            }

            // Check for finished workers and restart them
            _ = sleep(Duration::from_secs(1)) => {
                let mut i = 0;
                while i < handles.len() {
                    if handles[i].1.is_finished() {
                        let (worker_id, _) = handles.remove(i);
                        eprintln!("[ERROR] Worker {} died, restarting...", worker_id);
                        let notify_clone = notify.clone();
                        let handle = tokio::spawn(worker_loop(worker_id, notify_clone));
                        handles.push((worker_id, handle));
                    } else {
                        i += 1;
                    }
                }
            }
        }
    }
}

/// Worker main loop - processes queue items continuously
/// Returns normally only when it should shut down gracefully
async fn worker_loop(worker_id: usize, notify: Arc<Notify>) {
    let worker_name = format!("blp-worker-{}", worker_id);

    loop {
        // Try to process a task
        match process_queue_item(&worker_name).await {
            Ok(true) => {
                // Task was processed, immediately try to get another one
                // This handles the case when multiple tasks are queued
                continue;
            }
            Ok(false) => {
                // No tasks available, wait for notification
                notify.notified().await;
            }
            Err(e) => {
                eprintln!("[ERROR] {} error: {:?}", worker_name, e);
                // On error, wait a bit before retrying
                sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

async fn process_queue_item(worker_name: &str) -> Result<bool> {
    let db = state::db().await;

    // Try to claim next pending item
    let Some(mut item) = BlpQueueItem::claim_next(&*db, worker_name.to_string()).await? else {
        return Ok(false); // No pending items
    };

    let item_id = item.id.unwrap();

    // Process attachments
    match process_attachments(&mut item).await {
        Ok(converted_files) => {
            // Mark as completed
            BlpQueueItem::mark_completed(&*db, item_id).await?;

            // Send response with converted files
            if let Err(e) = send_response(&item, converted_files).await {
                eprintln!("[ERROR] {} failed to send response: {:?}", worker_name, e);
            }
        }
        Err(e) => {
            let error_msg = format!("{:?}", e);
            eprintln!(
                "[ERROR] {} failed item {}: {}",
                worker_name, item_id, error_msg
            );
            BlpQueueItem::mark_failed(&*db, item_id, error_msg).await?;
        }
    }

    Ok(true) // Task was processed
}

async fn process_attachments(item: &mut BlpQueueItem) -> Result<Vec<(String, Vec<u8>)>> {
    let mut converted_files = Vec::new();
    let mut filename_counts = std::collections::HashMap::new();

    for attachment in &mut item.attachments {
        let result = match item.conversion_type {
            crate::db::blp_queue::ConversionType::ToBLP => {
                convert_to_blp(attachment, item.quality).await
            }
            crate::db::blp_queue::ConversionType::ToPNG => {
                convert_to_png(attachment).await
            }
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
                let base_name = attachment.filename.rsplit('.').nth(1)
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
                
                eprintln!(
                    "[ERROR] Failed to convert {}: {:?}",
                    attachment.filename, e
                );
                // Continue processing other files instead of returning error
            }
        }
    }

    Ok(converted_files)
}

async fn convert_to_blp(
    attachment: &AttachmentItem,
    quality: u8,
) -> Result<(String, Vec<u8>)> {
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
    .await
    .map_err(|e| BotError::new("tokio_join_error").push_str(e.to_string()))?
    .map_err(|e| BotError::new("blp_conversion_failed").push_str(e.to_string()))?;

    Ok((output_filename, blp_bytes))
}

async fn convert_to_png(
    attachment: &AttachmentItem,
) -> Result<(String, Vec<u8>)> {
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
        img.decode(&blp_data, &[true, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false])?;

        // Get first mipmap
        let mipmap = img.mipmaps.get(0).ok_or_else(|| blp::error::error::BlpError::new("no_mipmap"))?;
        let rgba = mipmap.image.as_ref().ok_or_else(|| blp::error::error::BlpError::new("no_image_data"))?;

        // Encode to PNG in memory
        let mut png_buffer = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(rgba.clone())
            .write_to(&mut png_buffer, ImageFormat::Png)
            .map_err(|e| blp::error::error::BlpError::new("png_encode_failed").push_std(e))?;

        Ok::<_, blp::error::error::BlpError>(png_buffer.into_inner())
    })
    .await
    .map_err(|e| BotError::new("tokio_join_error").push_str(e.to_string()))?
    .map_err(|e| BotError::new("png_conversion_failed").push_str(e.to_string()))?;

    Ok((output_filename, png_bytes))
}

/// Create ZIP archive from converted files
fn create_zip_archive(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>> {
    use std::io::Write;
    
    let mut zip_buffer = Vec::new();
    {
        let cursor = Cursor::new(&mut zip_buffer);
        let mut zip = ZipWriter::new(cursor);
        let options = FileOptions::<()>::default()
            .compression_method(zip::CompressionMethod::Stored); // No compression for already compressed images

        for (filename, data) in files {
            zip.start_file(filename, options)
                .map_err(|e| BotError::new("zip_start_file_error").push_str(e.to_string()))?;
            zip.write_all(data)
                .map_err(|e| BotError::new("zip_write_error").push_str(e.to_string()))?;
        }

        zip.finish()
            .map_err(|e| BotError::new("zip_finish_error").push_str(e.to_string()))?;
    }
    Ok(zip_buffer)
}

async fn send_response(item: &BlpQueueItem, converted_files: Vec<(String, Vec<u8>)>) -> Result<()> {
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
        let zip_filename = match item.conversion_type {
            crate::db::blp_queue::ConversionType::ToBLP => "converted_images.blp.zip".to_string(),
            crate::db::blp_queue::ConversionType::ToPNG => "converted_images.png.zip".to_string(),
        };
        vec![(zip_filename, zip_data)]
    } else {
        // Send individual files
        converted_files.clone()
    };

    // If we have status_message_id, edit that message with attachments
    if let Some(status_msg_id) = &item.status_message_id {
        // Upload files to Discord using multipart form
        use reqwest::multipart::{Form, Part};

        let mut form = Form::new();

        // Update text content with completion status
        let format_desc = match item.conversion_type {
            crate::db::blp_queue::ConversionType::ToBLP => format!("to BLP (quality: {})", item.quality),
            crate::db::blp_queue::ConversionType::ToPNG => "to PNG".to_string(),
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
                .mime_str("application/octet-stream")
                .map_err(|e| BotError::new("mime_error").push_str(e.to_string()))?;

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
    } else {
        // Fallback: send new message if status_message_id not available
        use reqwest::multipart::{Form, Part};

        let mut form = Form::new();

        let format_desc = match item.conversion_type {
            crate::db::blp_queue::ConversionType::ToBLP => format!("to BLP (quality: {})", item.quality),
            crate::db::blp_queue::ConversionType::ToPNG => "to PNG".to_string(),
        };

        let message = format!(
            "✅ Converted {} image(s) {} in {}",
            converted_files.len(),
            format_desc,
            conversion_time
        );
        form = form.text("content", message);

        let payload = serde_json::json!({
            "message_reference": {
                "message_id": item.message_id
            }
        });
        form = form.text("payload_json", payload.to_string());

        for (idx, (filename, file_data)) in converted_files.iter().enumerate() {
            let part = Part::bytes(file_data.clone())
                .file_name(filename.clone())
                .mime_str("application/octet-stream")
                .map_err(|e| BotError::new("mime_error").push_str(e.to_string()))?;

            form = form.part(format!("files[{}]", idx), part);
        }

        let response = client
            .post(&format!(
                "https://discord.com/api/v10/channels/{}/messages",
                item.channel_id
            ))
            .header("Authorization", format!("Bot {}", token))
            .multipart(form)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(BotError::new("message_send_failed").push_str(format!(
                "HTTP {}: {}",
                status.as_u16(),
                error_text
            )));
        }
    }

    Ok(())
}
