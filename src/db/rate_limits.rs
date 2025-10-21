use bson::serde_helpers::datetime;
use chrono::{DateTime, Utc};
use mongodb::{Collection, bson::doc};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use crate::error::Result;

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RateLimit {
    /// Endpoint route (e.g., "/gateway", "/interactions", "/channels/{channel_id}/messages")
    pub route: String,
    
    /// Maximum number of requests allowed in the time window
    pub limit: i32,
    
    /// Number of requests remaining in the current window
    pub remaining: i32,
    
    /// Unix timestamp (seconds) when the rate limit resets
    pub reset: f64,
    
    /// Time window duration in seconds
    pub reset_after: f64,
    
    /// Bucket identifier (Discord groups endpoints into buckets)
    pub bucket: Option<String>,
    
    /// Whether this is a global rate limit
    pub global: bool,
    
    /// Timestamp when this rate limit was last updated
    #[serde_as(as = "datetime::FromChrono04DateTime")]
    pub updated_at: DateTime<Utc>,
}

impl RateLimit {
    const COLLECTION_NAME: &'static str = "discord_rate_limits";

    /// Create or update rate limit information from Discord API response headers
    pub async fn update_from_headers(
        db: &mongodb::Database,
        route: String,
        headers: &reqwest::header::HeaderMap,
    ) -> Result<()> {
        let collection: Collection<RateLimit> = db.collection(Self::COLLECTION_NAME);

        // Parse rate limit headers
        let limit = headers
            .get("x-ratelimit-limit")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0);

        let remaining = headers
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0);

        let reset = headers
            .get("x-ratelimit-reset")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);

        let reset_after = headers
            .get("x-ratelimit-reset-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);

        let bucket = headers
            .get("x-ratelimit-bucket")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string());

        let global = headers
            .get("x-ratelimit-global")
            .and_then(|v| v.to_str().ok())
            .map(|v| v == "true")
            .unwrap_or(false);

        // Skip if no rate limit headers present
        if limit == 0 {
            return Ok(());
        }

        let rate_limit = RateLimit {
            route: route.clone(),
            limit,
            remaining,
            reset,
            reset_after,
            bucket,
            global,
            updated_at: Utc::now(),
        };

        // Upsert by route
        collection
            .replace_one(
                doc! { "route": &route },
                &rate_limit,
            )
            .upsert(true)
            .await?;

        Ok(())
    }

    /// Get current rate limit for a route
    pub async fn get(db: &mongodb::Database, route: &str) -> Result<Option<RateLimit>> {
        let collection: Collection<RateLimit> = db.collection(Self::COLLECTION_NAME);
        
        let rate_limit = collection
            .find_one(doc! { "route": route })
            .await?;

        Ok(rate_limit)
    }

    /// Check if we can make a request (remaining > 0 or reset time has passed)
    pub fn can_request(&self) -> bool {
        if self.remaining > 0 {
            return true;
        }

        // Check if reset time has passed
        let now = Utc::now().timestamp() as f64;
        now >= self.reset
    }

    /// Get seconds to wait before next request is allowed
    pub fn retry_after(&self) -> f64 {
        if self.remaining > 0 {
            return 0.0;
        }

        let now = Utc::now().timestamp() as f64;
        (self.reset - now).max(0.0)
    }
}
