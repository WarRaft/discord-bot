use crate::error::BotError;
use bytes::Bytes;
use futures_util::{StreamExt, stream::FuturesOrdered};
use reqwest::{Client, header::CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Semaphore;

/// https://discord.com/developers/docs/resources/message#attachment-object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub url: String,
    pub filename: String,
}

#[derive(Debug, Clone)]
pub struct AttachmentMemory {
    pub meta: Attachment,
    pub content_type: Option<String>,
    pub bytes: Bytes,
    pub error: Option<String>,
}

impl From<Attachment> for AttachmentMemory {
    fn from(meta: Attachment) -> Self {
        Self {
            meta,
            content_type: None,
            bytes: Bytes::new(),
            error: None,
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

    pub fn ok(&self) -> bool {
        self.error.is_none()
    }
    pub fn err(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }
}

pub trait AttachmentVecExt: Sized {
    fn to_memory(self) -> Vec<AttachmentMemory>;
    async fn download_all(self, concurrency: usize) -> Vec<AttachmentMemory>;
}

impl AttachmentVecExt for Vec<Attachment> {
    fn to_memory(self) -> Vec<AttachmentMemory> {
        self.into_iter().map(AttachmentMemory::from).collect()
    }

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
