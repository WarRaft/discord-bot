use reqwest::Client;
use crate::error::{BotError, Result};
use crate::types::discord::*;
use crate::state;

pub async fn get_gateway_bot_info(client: &Client, token: &str) -> Result<GatewayBotInfo> {
    let response = client
        .get("https://discord.com/api/v10/gateway/bot")
        .header("Authorization", format!("Bot {}", token))
        .send()
        .await?;

    // Store rate limits
    let _ = crate::db::rate_limits::RateLimit::update_from_headers(
        &*state::db().await,
        "/gateway/bot".to_string(),
        response.headers(),
    ).await;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        
        if let Ok(discord_err) = serde_json::from_str::<DiscordErrorResponse>(&error_text) {
            return Err(BotError::new("discord_api_error")
                .push_str(format!("GET /gateway/bot: {}", discord_err)));
        }
        
        return Err(BotError::new("http_error")
            .push_str(format!("GET /gateway/bot: {} - {}", status, error_text)));
    }

    let bot_info: GatewayBotInfo = response.json().await?;
    
    // Store session limits
    let _ = crate::db::session_limits::SessionLimit::update(
        &*state::db().await,
        bot_info.session_start_limit.total,
        bot_info.session_start_limit.remaining,
        bot_info.session_start_limit.reset_after,
        bot_info.session_start_limit.max_concurrency,
        bot_info.shards,
    ).await;
    
    Ok(bot_info)
}

pub async fn get_gateway_url(client: &Client, token: &str) -> Result<String> {
    let response = client
        .get("https://discord.com/api/v10/gateway")
        .header("Authorization", format!("Bot {}", token))
        .send()
        .await?;

    // Store rate limits
    let _ = crate::db::rate_limits::RateLimit::update_from_headers(
        &*state::db().await,
        "/gateway".to_string(),
        response.headers(),
    ).await;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        
        // Try to parse as Discord error
        if let Ok(discord_err) = serde_json::from_str::<DiscordErrorResponse>(&error_text) {
            return Err(BotError::new("discord_api_error")
                .push_str(format!("GET /gateway: {}", discord_err)));
        }
        
        return Err(BotError::new("http_error")
            .push_str(format!("GET /gateway: {} - {}", status, error_text)));
    }

    let gateway: GatewayResponse = response.json().await?;
    let url = if gateway.url.ends_with('/') {
        format!("{}?v=10&encoding=json", gateway.url)
    } else {
        format!("{}/?v=10&encoding=json", gateway.url)
    };
    Ok(url)
}

pub async fn get_application_id(client: &Client, token: &str) -> Result<String> {
    let response = client
        .get("https://discord.com/api/v10/oauth2/applications/@me")
        .header("Authorization", format!("Bot {}", token))
        .send()
        .await?;

    // Store rate limits
    let _ = crate::db::rate_limits::RateLimit::update_from_headers(
        &*state::db().await,
        "/oauth2/applications/@me".to_string(),
        response.headers(),
    ).await;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        
        if let Ok(discord_err) = serde_json::from_str::<DiscordErrorResponse>(&error_text) {
            return Err(BotError::new("discord_api_error")
                .push_str(format!("GET /oauth2/applications/@me: {}", discord_err)));
        }
        
        return Err(BotError::new("http_error")
            .push_str(format!("GET /oauth2/applications/@me: {} - {}", status, error_text)));
    }

    let app_info: ApplicationInfo = response.json().await?;
    Ok(app_info.id)
}

pub async fn register_slash_commands(client: &Client, token: &str, app_id: &str) -> Result<()> {
    let commands = crate::commands::all_commands();

    let response = client
        .put(&format!(
            "https://discord.com/api/v10/applications/{}/commands",
            app_id
        ))
        .header("Authorization", format!("Bot {}", token))
        .header("Content-Type", "application/json")
        .json(&commands)
        .send()
        .await?;

    // Store rate limits
    let _ = crate::db::rate_limits::RateLimit::update_from_headers(
        &*state::db().await,
        format!("/applications/{}/commands", app_id),
        response.headers(),
    ).await;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        
        if let Ok(discord_err) = serde_json::from_str::<DiscordErrorResponse>(&error_text) {
            return Err(BotError::new("discord_api_error")
                .push_str(format!("PUT /applications/{}/commands: {}", app_id, discord_err)));
        }
        
        return Err(BotError::new("http_error")
            .push_str(format!("PUT /applications/{}/commands: {} - {}", app_id, status, error_text)));
    }

    Ok(())
}

pub async fn respond_to_interaction(
    client: &Client,
    token: &str,
    interaction_id: &str,
    interaction_token: &str,
    content: String,
) -> Result<()> {
    let response_data = InteractionResponse {
        response_type: 4,
        data: Some(InteractionResponseData { content }),
    };

    let response = client
        .post(&format!(
            "https://discord.com/api/v10/interactions/{}/{}/callback",
            interaction_id, interaction_token
        ))
        .header("Authorization", format!("Bot {}", token))
        .header("Content-Type", "application/json")
        .json(&response_data)
        .send()
        .await?;

    // Store rate limits
    let _ = crate::db::rate_limits::RateLimit::update_from_headers(
        &*state::db().await,
        "/interactions/callback".to_string(),
        response.headers(),
    ).await;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        
        if let Ok(discord_err) = serde_json::from_str::<DiscordErrorResponse>(&error_text) {
            return Err(BotError::new("discord_api_error")
                .push_str(format!("POST /interactions/{}/{}/callback: {}", interaction_id, interaction_token, discord_err)));
        }
        
        return Err(BotError::new("http_error")
            .push_str(format!("POST /interactions/{}/{}/callback: {} - {}", interaction_id, interaction_token, status, error_text)));
    }

    Ok(())
}
