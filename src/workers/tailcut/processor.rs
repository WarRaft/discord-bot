use crate::discord::message::attachment::{AttachmentVecExt, ensure_unique_filenames};
use crate::discord::message::message::MessageReference;
use crate::discord::message::send::MessageSend;
use crate::error::BotError;
use crate::state;
use crate::workers::processor::{TaskProcessor, notify_workers};
use crate::workers::queue::QueueStatus;
use crate::workers::tailcut::job::JobTailcut;
use async_trait::async_trait;
use blp::core::decode::decode_to_rgba;
use bson::{Bson, doc, serialize_to_bson};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use mongodb::Collection;
use reqwest::Method;
use std::io::{Cursor, Write};
use zip::ZipWriter;
use zip::write::FileOptions;

pub struct TailcutProcessor;

#[async_trait]
impl TaskProcessor for TailcutProcessor {
    const POOL: &'static str = "tailcut";

    async fn process_queue_item() -> Result<bool, BotError> {
        let db = state::db().await;
        let collection: Collection<JobTailcut> = db.collection(JobTailcut::COLLECTION);

        let result = collection
            .find_one_and_update(
                doc! {
                    JobTailcut::STATUS: QueueStatus::Pending.as_ref(),
                    JobTailcut::RETRY: { "$lt": JobTailcut::MAX_RETRIES }
                },
                doc! {
                    "$set": {
                        JobTailcut::STATUS: QueueStatus::Processing.as_ref()
                    }
                },
            )
            .sort(doc! { JobTailcut::CREATED: 1 })
            .return_document(mongodb::options::ReturnDocument::After)
            .await?;

        let Some(job) = result else {
            return Ok(false);
        };

        let Some(ref reply) = job.reply else {
            if job.message.attachments.is_empty() {
                let reply_msg = MessageSend {
                    content: Some("❌ No attachments found — nothing to cut.".to_string()),
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
                                JobTailcut::REPLY: serialize_to_bson(&reply_msg)?,
                                JobTailcut::STATUS: QueueStatus::Completed.as_ref(),
                            },
                        },
                    )
                    .await?;
            } else {
                let reply_msg = MessageSend {
                    content: Some(format!(
                        "✅ Added {} image(s) to tailcut queue \n⏳ Processing...",
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
                                JobTailcut::REPLY: serialize_to_bson(&reply_msg)?,
                                JobTailcut::STATUS: QueueStatus::Pending.as_ref(),
                            },
                        },
                    )
                    .await?;
            }

            notify_workers::<TailcutProcessor>();
            return Ok(true);
        };

        // Process all attachments — returns (cut_files, overlay_files)
        let processing_result: Result<(Vec<(String, Vec<u8>)>, Vec<(String, Vec<u8>)>), BotError> = async {
            let attachments = ensure_unique_filenames(job.message.attachments.clone())
                .download_all(4)
                .await;

            let mut cut_files: Vec<(String, Vec<u8>)> = Vec::new();
            let mut overlay_files: Vec<(String, Vec<u8>)> = Vec::new();

            for attachment_memory in attachments {
                if let Some(ref error) = attachment_memory.error {
                    let error_filename =
                        format!("{}.error.txt", attachment_memory.filename_stem);
                    let error_content = format!(
                        "Error downloading file: {}\n\nError details:\n{}\n\nTimestamp: {}",
                        attachment_memory.meta.filename,
                        error,
                        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
                    );
                    cut_files.push((error_filename, error_content.into_bytes()));
                    continue;
                }

                let result: Result<(Vec<(String, Vec<u8>)>, Option<(String, Vec<u8>)>), BotError> = async {
                    let dyn_img = decode_to_rgba(&attachment_memory.bytes)?;
                    let img = dyn_img.to_rgba8();

                    let (cells, grid_info) = detect_grid(&img)?;

                    eprintln!("[tailcut] {} → {}", attachment_memory.meta.filename, grid_info);

                    // Generate debug overlay image
                    let overlay = if cells.len() > 1 {
                        let ov = draw_grid_overlay(&img, &cells);
                        let mut buf = Vec::new();
                        DynamicImage::ImageRgba8(ov).write_to(
                            &mut Cursor::new(&mut buf),
                            ImageFormat::Png,
                        )?;
                        Some((
                            format!("{}_grid.png", attachment_memory.filename_stem),
                            buf,
                        ))
                    } else {
                        None
                    };

                    // Cut cells
                    let mut files = Vec::new();
                    for (idx, cell) in cells.iter().enumerate() {
                        let sub = image::imageops::crop_imm(
                            &img, cell.x, cell.y, cell.w, cell.h,
                        )
                        .to_image();

                        let mut buf = Vec::new();
                        DynamicImage::ImageRgba8(sub).write_to(
                            &mut Cursor::new(&mut buf),
                            ImageFormat::Png,
                        )?;

                        let filename = format!(
                            "{}_{:03}.png",
                            attachment_memory.filename_stem,
                            idx + 1
                        );
                        files.push((filename, buf));
                    }

                    if files.is_empty() {
                        let mut buf = Vec::new();
                        DynamicImage::ImageRgba8(img).write_to(
                            &mut Cursor::new(&mut buf),
                            ImageFormat::Png,
                        )?;
                        let filename =
                            format!("{}.png", attachment_memory.filename_stem);
                        files.push((filename, buf));
                    }

                    Ok((files, overlay))
                }
                .await;

                match result {
                    Ok((files, overlay)) => {
                        cut_files.extend(files);
                        if let Some(ov) = overlay {
                            overlay_files.push(ov);
                        }
                    }
                    Err(e) => {
                        let error_filename =
                            format!("{}.error.txt", attachment_memory.filename_stem);
                        let error_content = format!(
                            "Error processing file: {}\n\nError details:\n{:?}\n\nTimestamp: {}",
                            attachment_memory.meta.filename,
                            e,
                            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
                        );
                        cut_files.push((error_filename, error_content.into_bytes()));
                    }
                }
            }

            Ok((cut_files, overlay_files))
        }
        .await;

        let (cut_files, overlay_files) = match processing_result {
            Ok(pair) => pair,
            Err(e) => {
                let error_content = format!(
                    "❌ Critical Error\n\n{}\n\nTimestamp: {}",
                    e,
                    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
                );

                let error_file =
                    vec![("error.txt".to_string(), error_content.into_bytes())];

                let _ = MessageSend {
                    content: Some(
                        "❌ Failed to process images due to a critical error. See attached file for details."
                            .to_string(),
                    ),
                    message_reference: None,
                    attachments: Some(error_file),
                }
                .send(Method::PATCH, &job.message.channel_id, Some(&reply.id))
                .await;

                collection
                    .update_one(
                        doc! { "_id": job.id.unwrap() },
                        doc! {
                            "$set": {
                                JobTailcut::STATUS: QueueStatus::Failed.as_ref(),
                                JobTailcut::COMPLETED: Bson::DateTime(bson::DateTime::now())
                            }
                        },
                    )
                    .await?;

                notify_workers::<TailcutProcessor>();
                return Ok(true);
            }
        };

        // Send response
        {
            let conversion_time = format!(
                "{:.2}s",
                chrono::Utc::now()
                    .signed_duration_since(job.created)
                    .num_milliseconds() as f64
                    / 1000.0
            );

            // Build attachments: overlay visible + cut files always zipped
            let cut_count = cut_files.len();

            let mut files_to_send: Vec<(String, Vec<u8>)> = Vec::new();

            // Overlays first (always visible as direct attachments)
            files_to_send.extend(overlay_files);

            // Always zip cut files
            if !cut_files.is_empty() {
                let mut zip_buffer = Vec::new();
                {
                    let cursor = Cursor::new(&mut zip_buffer);
                    let mut zip = ZipWriter::new(cursor);
                    let options = FileOptions::<()>::default()
                        .compression_method(zip::CompressionMethod::Stored);

                    for (filename, data) in &cut_files {
                        zip.start_file(filename, options)?;
                        zip.write_all(data)?;
                    }

                    zip.finish()?;
                }
                files_to_send.push(("tailcut_images.zip".to_string(), zip_buffer));
            }

            let _ = MessageSend {
                content: Some(format!(
                    "✅ Cut {} image(s)\n⏱️ Completed in {}",
                    cut_count,
                    conversion_time
                )),
                message_reference: None,
                attachments: Some(files_to_send),
            }
            .send(Method::PATCH, &job.message.channel_id, Some(&reply.id))
            .await?;
        }

        collection
            .update_one(
                doc! { "_id": job.id.unwrap() },
                doc! {
                    "$set": {
                        JobTailcut::STATUS: QueueStatus::Completed.as_ref(),
                        JobTailcut::COMPLETED: Bson::DateTime(bson::DateTime::now())
                    }
                },
            )
            .await?;

        notify_workers::<TailcutProcessor>();
        Ok(true)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Debug overlay: draw green outlines on original image
// ─────────────────────────────────────────────────────────────────────────────

fn draw_grid_overlay(img: &RgbaImage, cells: &[CellRect]) -> RgbaImage {
    let mut overlay = img.clone();
    let green = Rgba([0u8, 255, 0, 255]);
    let t = 3u32; // line thickness
    let (iw, ih) = (overlay.width(), overlay.height());

    for cell in cells {
        let x0 = cell.x;
        let y0 = cell.y;
        let x1 = (cell.x + cell.w).min(iw); // exclusive
        let y1 = (cell.y + cell.h).min(ih); // exclusive

        // Top edge (inward from y0)
        for dy in 0..t {
            let py = y0 + dy;
            if py >= ih { continue; }
            for px in x0..x1 {
                if px < iw { overlay.put_pixel(px, py, green); }
            }
        }
        // Bottom edge (inward from y1-1)
        for dy in 0..t {
            let py = y1.saturating_sub(1 + dy);
            if py < y0 { continue; }
            if py >= ih { continue; }
            for px in x0..x1 {
                if px < iw { overlay.put_pixel(px, py, green); }
            }
        }
        // Left edge (inward from x0)
        for dx in 0..t {
            let px = x0 + dx;
            if px >= iw { continue; }
            for py in y0..y1 {
                if py < ih { overlay.put_pixel(px, py, green); }
            }
        }
        // Right edge (inward from x1-1)
        for dx in 0..t {
            let px = x1.saturating_sub(1 + dx);
            if px < x0 { continue; }
            if px >= iw { continue; }
            for py in y0..y1 {
                if py < ih { overlay.put_pixel(px, py, green); }
            }
        }
    }

    overlay
}

// ─────────────────────────────────────────────────────────────────────────────
// Grid detection — square cells only
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct CellRect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// Returns (cells, debug_info_string).
fn detect_grid(img: &RgbaImage) -> Result<(Vec<CellRect>, String), BotError> {
    let (img_w, img_h) = (img.width(), img.height());
    if img_w < 2 || img_h < 2 {
        return Err(BotError::new("tailcut_empty_image"));
    }

    let bg = dominant_corner_color(img);
    let bg_thresh: u32 = 60;

    // Compute per-column and per-row background fraction over the full image
    let col_bg: Vec<f64> = (0..img_w)
        .map(|x| {
            (0..img_h)
                .filter(|&y| pixel_diff(img, x, y, &bg) <= bg_thresh)
                .count() as f64
                / img_h as f64
        })
        .collect();
    let row_bg: Vec<f64> = (0..img_h)
        .map(|y| {
            (0..img_w)
                .filter(|&x| pixel_diff(img, x, y, &bg) <= bg_thresh)
                .count() as f64
                / img_w as f64
        })
        .collect();

    eprintln!(
        "[tailcut] image {}x{}, bg {:?}",
        img_w, img_h, bg,
    );

    // ── Pass 1: gap-band detection with linear regression ────────────────────
    for &gap_thresh in &[0.80, 0.70, 0.60, 0.50] {
        if let Some((cells, info)) =
            try_gap_grid(&col_bg, &row_bg, gap_thresh, img_w, img_h)
        {
            if cells.len() > 1 {
                return Ok((cells, info));
            }
        }
    }

    // ── Pass 2: brute-force scored by boundary-vs-interior gradient ratio ───
    let (top, bottom, left, right) = content_bbox(img, &bg, bg_thresh);
    if right > left && bottom > top {
        let bbox_w = right - left;
        let bbox_h = bottom - top;
        let col_grad = col_gradient_profile(img, top, bottom);
        let row_grad = row_gradient_profile(img, left, right);

        if let Some((cells, info)) =
            try_fit_square_grid(bbox_w, bbox_h, left, top, &col_grad, &row_grad)
        {
            if cells.len() > 1 {
                return Ok((cells, info));
            }
        }

        let info = format!("no grid in {}x{} content", bbox_w, bbox_h);
        Ok((vec![CellRect { x: left, y: top, w: bbox_w, h: bbox_h }], info))
    } else {
        let info = format!("no content in {}x{}", img_w, img_h);
        Ok((vec![CellRect { x: 0, y: 0, w: img_w, h: img_h }], info))
    }
}

// ─── Pass 1: gap-band detection with linear regression ─────────────────────

/// Find contiguous runs where bg_fraction >= threshold (separator bands).
fn find_gap_bands(bg_frac: &[f64], threshold: f64) -> Vec<(u32, u32)> {
    let mut bands = Vec::new();
    let mut i = 0usize;
    while i < bg_frac.len() {
        if bg_frac[i] >= threshold {
            let start = i;
            while i < bg_frac.len() && bg_frac[i] >= threshold {
                i += 1;
            }
            bands.push((start as u32, i as u32));
        } else {
            i += 1;
        }
    }
    bands
}

/// Cell regions are the non-gap regions between gap bands.
fn cells_between_gaps(gaps: &[(u32, u32)], total_len: u32, min_cell: u32) -> Vec<(u32, u32)> {
    let mut cells = Vec::new();
    let mut pos = 0u32;
    for &(gap_start, gap_end) in gaps {
        if gap_start > pos {
            cells.push((pos, gap_start));
        }
        pos = gap_end;
    }
    if pos < total_len {
        cells.push((pos, total_len));
    }
    cells.into_iter().filter(|&(s, e)| e - s >= min_cell).collect()
}

/// Linear regression: values[i] ≈ origin + i * step.
/// Returns (origin, step).
fn linear_fit(values: &[f64]) -> (f64, f64) {
    let n = values.len() as f64;
    if n < 2.0 {
        return (values.first().copied().unwrap_or(0.0), 0.0);
    }
    let i_mean = (n - 1.0) / 2.0;
    let v_mean: f64 = values.iter().sum::<f64>() / n;

    let mut num = 0.0;
    let mut den = 0.0;
    for (i, &v) in values.iter().enumerate() {
        let di = i as f64 - i_mean;
        num += di * (v - v_mean);
        den += di * di;
    }

    let step = if den.abs() > 1e-10 { num / den } else { 0.0 };
    let origin = v_mean - step * i_mean;
    (origin, step)
}

fn median_f64(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

fn try_gap_grid(
    col_bg: &[f64],
    row_bg: &[f64],
    gap_thresh: f64,
    img_w: u32,
    img_h: u32,
) -> Option<(Vec<CellRect>, String)> {
    let min_cell = 8u32;

    let col_gaps = find_gap_bands(col_bg, gap_thresh);
    let row_gaps = find_gap_bands(row_bg, gap_thresh);

    let col_cells = cells_between_gaps(&col_gaps, img_w, min_cell);
    let row_cells = cells_between_gaps(&row_gaps, img_h, min_cell);

    let num_cols = col_cells.len();
    let num_rows = row_cells.len();
    if num_cols < 2 || num_rows < 2 {
        return None;
    }

    // Compute cell centers and fit a line to them via linear regression
    let col_centers: Vec<f64> = col_cells
        .iter()
        .map(|&(s, e)| (s as f64 + e as f64) / 2.0)
        .collect();
    let row_centers: Vec<f64> = row_cells
        .iter()
        .map(|&(s, e)| (s as f64 + e as f64) / 2.0)
        .collect();

    let (col_origin, col_step) = linear_fit(&col_centers);
    let (row_origin, row_step) = linear_fit(&row_centers);

    if col_step < min_cell as f64 || row_step < min_cell as f64 {
        return None;
    }

    // Cell size: median of detected strip sizes, then average across axes
    let col_sizes: Vec<f64> = col_cells.iter().map(|&(s, e)| (e - s) as f64).collect();
    let row_sizes: Vec<f64> = row_cells.iter().map(|&(s, e)| (e - s) as f64).collect();
    let median_cw = median_f64(&col_sizes);
    let median_rh = median_f64(&row_sizes);

    // Cap cell size at step to avoid overlap
    let cell_w = median_cw.min(col_step);
    let cell_h = median_rh.min(row_step);
    // Make it square
    let cell_size = ((cell_w + cell_h) / 2.0).round().max(8.0) as u32;
    let half = cell_size as f64 / 2.0;

    let mut cells = Vec::new();
    for r in 0..num_rows {
        for c in 0..num_cols {
            let cx = col_origin + c as f64 * col_step;
            let cy = row_origin + r as f64 * row_step;
            let x = (cx - half).round().max(0.0) as u32;
            let y = (cy - half).round().max(0.0) as u32;
            let w = cell_size.min(img_w.saturating_sub(x));
            let h = cell_size.min(img_h.saturating_sub(y));
            if w > 0 && h > 0 {
                cells.push(CellRect { x, y, w, h });
            }
        }
    }

    if cells.len() > 1 {
        let info = format!(
            "gap(thresh={:.2}): {}x{} grid, cell {}px, step {:.1}x{:.1}",
            gap_thresh, num_cols, num_rows, cell_size, col_step, row_step,
        );
        eprintln!("[tailcut] {}", info);
        Some((cells, info))
    } else {
        None
    }
}

// ─── Pass 2: brute-force with boundary-vs-interior scoring ─────────────────

fn col_gradient_profile(img: &RgbaImage, top: u32, bottom: u32) -> Vec<f64> {
    let w = img.width();
    let span = (bottom - top).max(1) as f64;
    let mut profile = vec![0.0f64; w as usize];
    for x in 1..w {
        let mut sum = 0u64;
        for y in top..bottom {
            let a = img.get_pixel(x - 1, y).0;
            let b = img.get_pixel(x, y).0;
            sum += channel_diff(&a, &b) as u64;
        }
        profile[x as usize] = sum as f64 / span;
    }
    profile
}

fn row_gradient_profile(img: &RgbaImage, left: u32, right: u32) -> Vec<f64> {
    let h = img.height();
    let span = (right - left).max(1) as f64;
    let mut profile = vec![0.0f64; h as usize];
    for y in 1..h {
        let mut sum = 0u64;
        for x in left..right {
            let a = img.get_pixel(x, y - 1).0;
            let b = img.get_pixel(x, y).0;
            sum += channel_diff(&a, &b) as u64;
        }
        profile[y as usize] = sum as f64 / span;
    }
    profile
}

fn score_grid(
    cols: u32,
    rows: u32,
    s: u32,
    g: u32,
    off_x: u32,
    off_y: u32,
    col_grad: &[f64],
    row_grad: &[f64],
) -> f64 {
    let step = s + g;
    let quarter = (s / 4).max(1);
    let mut ratios = Vec::new();

    for c in 1..cols {
        let bx = (off_x + c * step) as usize;
        if bx >= col_grad.len() { continue; }
        let boundary_val = col_grad[bx];
        let left_interior = bx.saturating_sub(quarter as usize);
        let right_interior = (bx + quarter as usize).min(col_grad.len() - 1);
        let interior_val = (col_grad[left_interior] + col_grad[right_interior]) / 2.0 + 0.1;
        ratios.push(boundary_val / interior_val);
    }

    for r in 1..rows {
        let by = (off_y + r * step) as usize;
        if by >= row_grad.len() { continue; }
        let boundary_val = row_grad[by];
        let top_interior = by.saturating_sub(quarter as usize);
        let bottom_interior = (by + quarter as usize).min(row_grad.len() - 1);
        let interior_val = (row_grad[top_interior] + row_grad[bottom_interior]) / 2.0 + 0.1;
        ratios.push(boundary_val / interior_val);
    }

    if ratios.is_empty() {
        return 0.0;
    }

    let log_sum: f64 = ratios.iter().map(|r| r.max(0.01).ln()).sum();
    let geomean = (log_sum / ratios.len() as f64).exp();
    geomean * (ratios.len() as f64).sqrt()
}

fn try_fit_square_grid(
    bbox_w: u32,
    bbox_h: u32,
    left: u32,
    top: u32,
    col_grad: &[f64],
    row_grad: &[f64],
) -> Option<(Vec<CellRect>, String)> {
    let min_cell = 16u32;
    let max_gap = 16u32;
    let max_dim = 30u32;

    let mut best_score = f64::NEG_INFINITY;
    let mut best_cells: Option<Vec<CellRect>> = None;
    let mut best_info = String::new();

    for g in 0..=max_gap {
        let eff_w = bbox_w + g;
        let eff_h = bbox_h + g;
        let max_cols = (eff_w / (min_cell + g)).min(max_dim);

        for cols in 2..=max_cols {
            let step_w = eff_w / cols;
            if step_w < min_cell + g {
                continue;
            }

            for &step in &[step_w, step_w + 1] {
                let s = step.saturating_sub(g);
                if s < min_cell {
                    continue;
                }

                let actual_w = cols * step - g;
                if bbox_w.abs_diff(actual_w) > 2 {
                    continue;
                }

                let rows = (eff_h + step / 2) / step;
                if rows < 2 || rows > max_dim {
                    continue;
                }

                let actual_h = rows * step - g;
                if bbox_h.abs_diff(actual_h) > 2 {
                    continue;
                }

                let off_x = left + bbox_w.saturating_sub(actual_w) / 2;
                let off_y = top + bbox_h.saturating_sub(actual_h) / 2;

                let score = score_grid(cols, rows, s, g, off_x, off_y, col_grad, row_grad);

                if score > best_score {
                    best_score = score;

                    let mut cells = Vec::new();
                    for r in 0..rows {
                        for c in 0..cols {
                            cells.push(CellRect {
                                x: off_x + c * step,
                                y: off_y + r * step,
                                w: s,
                                h: s,
                            });
                        }
                    }
                    best_info = format!(
                        "brute: {}x{} grid, cell {}px, gap {}px, score {:.2}",
                        cols, rows, s, g, score
                    );
                    best_cells = Some(cells);
                }
            }
        }
    }

    best_cells.map(|cells| (cells, best_info))
}

// ─── helpers ───────────────────────────────────────────────────────────────

fn content_bbox(img: &RgbaImage, bg: &[u8; 4], bg_thresh: u32) -> (u32, u32, u32, u32) {
    let (w, h) = (img.width(), img.height());
    let bg_line = 0.90;

    let row_bg: Vec<f64> = (0..h)
        .map(|y| {
            (0..w)
                .filter(|&x| pixel_diff(img, x, y, bg) <= bg_thresh)
                .count() as f64
                / w as f64
        })
        .collect();
    let col_bg: Vec<f64> = (0..w)
        .map(|x| {
            (0..h)
                .filter(|&y| pixel_diff(img, x, y, bg) <= bg_thresh)
                .count() as f64
                / h as f64
        })
        .collect();

    let top = row_bg.iter().position(|&f| f < bg_line).unwrap_or(0) as u32;
    let bottom = row_bg
        .iter()
        .rposition(|&f| f < bg_line)
        .map(|v| v as u32 + 1)
        .unwrap_or(h);
    let left = col_bg.iter().position(|&f| f < bg_line).unwrap_or(0) as u32;
    let right = col_bg
        .iter()
        .rposition(|&f| f < bg_line)
        .map(|v| v as u32 + 1)
        .unwrap_or(w);
    (top, bottom, left, right)
}

fn pixel_diff(img: &RgbaImage, x: u32, y: u32, bg: &[u8; 4]) -> u32 {
    let p = img.get_pixel(x, y).0;
    (p[0] as i32 - bg[0] as i32).unsigned_abs()
        + (p[1] as i32 - bg[1] as i32).unsigned_abs()
        + (p[2] as i32 - bg[2] as i32).unsigned_abs()
        + (p[3] as i32 - bg[3] as i32).unsigned_abs()
}

fn channel_diff(a: &[u8; 4], b: &[u8; 4]) -> u32 {
    (a[0] as i32 - b[0] as i32).unsigned_abs()
        + (a[1] as i32 - b[1] as i32).unsigned_abs()
        + (a[2] as i32 - b[2] as i32).unsigned_abs()
        + (a[3] as i32 - b[3] as i32).unsigned_abs()
}

fn dominant_corner_color(img: &RgbaImage) -> [u8; 4] {
    let (w, h) = (img.width(), img.height());
    let s = 8.min(w).min(h);
    let mut counts: std::collections::HashMap<[u8; 4], usize> = std::collections::HashMap::new();
    for &(cx, cy) in &[
        (0u32, 0u32),
        (w.saturating_sub(s), 0),
        (0, h.saturating_sub(s)),
        (w.saturating_sub(s), h.saturating_sub(s)),
    ] {
        for dy in 0..s {
            for dx in 0..s {
                let p = img.get_pixel((cx + dx).min(w - 1), (cy + dy).min(h - 1)).0;
                let q = [p[0] & 0xFC, p[1] & 0xFC, p[2] & 0xFC, p[3] & 0xFC];
                *counts.entry(q).or_insert(0) += 1;
            }
        }
    }
    counts
        .into_iter()
        .max_by_key(|&(_, c)| c)
        .map(|(color, _)| color)
        .unwrap_or([0, 0, 0, 255])
}


