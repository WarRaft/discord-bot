use crate::error::BotError;
use bytes::Bytes;
use futures_util::{StreamExt, stream::FuturesOrdered};
use reqwest::{Client, header::CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// https://discord.com/developers/docs/resources/message#attachment-object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub url: String,
    pub filename: String,
}

pub fn ensure_unique_filenames(mut attachments: Vec<Attachment>) -> Vec<Attachment> {
    let mut counters: HashMap<String, usize> = HashMap::new();

    for att in attachments.iter_mut() {
        let path = Path::new(&att.filename);
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&att.filename);
        let ext = path.extension().and_then(|s| s.to_str());

        let count = counters.entry(stem.to_string()).or_insert(0);
        *count += 1;

        if *count > 1 {
            att.filename = match ext {
                Some(ext) => format!("{}_{}.{}", stem, count, ext),
                None => format!("{}_{}", stem, count),
            };
        }
    }

    attachments
}

#[derive(Debug, Clone)]
pub struct AttachmentMemory {
    pub meta: Attachment,
    pub content_type: Option<String>,
    pub bytes: Bytes,
    pub error: Option<String>,
    pub filename_stem: String,
    #[allow(dead_code)]
    pub filename_extension: Option<String>,
}

impl From<Attachment> for AttachmentMemory {
    fn from(meta: Attachment) -> Self {
        let path = Path::new(&meta.filename);
        let filename_stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&meta.filename)
            .to_string();
        let filename_extension = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());

        Self {
            meta,
            content_type: None,
            bytes: Bytes::new(),
            error: None,
            filename_stem,
            filename_extension,
        }
    }
}

impl AttachmentMemory {
    pub async fn download(&mut self, client: &Client) -> Result<(), BotError> {
        let resp = client
            .get(&self.meta.url)
            .send()
            .await?
            .error_for_status()?;
        self.content_type = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_owned());
        self.bytes = resp.bytes().await?;
        self.error = None;
        Ok(())
    }

    pub async fn try_from_remote(meta: Attachment, client: &Client) -> Self {
        let mut m = AttachmentMemory::from(meta);
        if let Err(e) = m.download(client).await {
            m.error = Some(e.to_string());
            m.bytes = Bytes::new();
            m.content_type = None;
        }
        m
    }
}

pub trait AttachmentVecExt: Sized {
    async fn download_all(self, concurrency: usize) -> Vec<AttachmentMemory>;
}

impl AttachmentVecExt for Vec<Attachment> {
    async fn download_all(self, concurrency: usize) -> Vec<AttachmentMemory> {
        let client = Client::new();
        let sem = Arc::new(Semaphore::new(concurrency.max(1)));
        let client = client.clone();
        let mut ordered = FuturesOrdered::new();

        for a in self {
            let c = client.clone();
            let sem = sem.clone();
            ordered.push_back(async move {
                let _permit = sem.acquire().await.expect("semaphore closed");
                AttachmentMemory::try_from_remote(a, &c).await
            });
        }

        let mut out = Vec::new();
        while let Some(item) = ordered.next().await {
            out.push(item);
        }
        out
    }
}
