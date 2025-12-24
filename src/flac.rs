use base64::prelude::*;
use reqwest::Client;
use serde_json::Value;
use std::error::Error;
use std::time::Instant;
use tokio::sync::OnceCell;

static ACTIVE_API: OnceCell<String> = OnceCell::const_new();

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const API_CANDIDATES: &[&str] = &[
    "https://maus.qqdl.site",
    "https://wolf.qqdl.site",
    "https://vogel.qqdl.site",
    "https://katze.qqdl.site",
    "https://hund.qqdl.site",
    "https://tidal.401658.xyz",
];

pub async fn init_api() -> Result<(), Box<dyn Error + Send + Sync>> {
    if ACTIVE_API.get().is_some() {
        return Ok(());
    }

    let client = Client::builder()
        .user_agent(UA)
        .timeout(std::time::Duration::from_secs(3))
        .build()?;

    println!("Finding fastest API...");

    for &url in API_CANDIDATES {
        let start = Instant::now();
        let test_url = format!("{}/track/?id=204567804", url);

        if let Ok(resp) = client.get(&test_url).send().await {
            if resp.status().is_success() {
                println!("Selected: {} ({}ms)", url, start.elapsed().as_millis());

                match ACTIVE_API.set(url.to_string()) {
                    Ok(_) => return Ok(()),
                    Err(_) => return Ok(()),
                }
            }
        }
    }

    Err("No working API servers found".into())
}

pub async fn fetch_flac_stream_url(
    song_name: &str,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    let api_base = ACTIVE_API
        .get()
        .ok_or("API not initialized! Call init_api() first.")?;

    let client = Client::builder().user_agent(UA).build()?;

    let search_url = format!("{}/search/?s={}", api_base, song_name);
    let resp: Value = client.get(&search_url).send().await?.json().await?;

    let track_id = resp
        .get("data")
        .and_then(|d| d.get("items"))
        .or_else(|| resp.get("items"))
        .or_else(|| resp.get("tracks").and_then(|t| t.get("items")))
        .and_then(|items| items.get(0))
        .and_then(|first| first["id"].as_i64())
        .ok_or("Track not found")?;

    let track_url = format!("{}/track/?id={}&quality=LOSSLESS", api_base, track_id);
    let track_data: Value = client.get(&track_url).send().await?.json().await?;

    let data = if track_data.get("data").is_some() {
        &track_data["data"]
    } else {
        &track_data
    };

    if let Some(url) = data["OriginalTrackUrl"].as_str() {
        if !url.is_empty() {
            return Ok(url.to_string());
        }
    }

    if let Some(manifest) = data["manifest"].as_str() {
        return decode_manifest(manifest);
    }

    Err("No FLAC stream found".into())
}

fn decode_manifest(encoded: &str) -> Result<String, Box<dyn Error + Send + Sync>> {
    let decoded_bytes = BASE64_STANDARD.decode(encoded)?;
    let decoded_str = String::from_utf8(decoded_bytes)?;

    if let Some(start) = decoded_str.find("http") {
        let end = decoded_str[start..]
            .find('"')
            .unwrap_or(decoded_str.len() - start);
        return Ok(decoded_str[start..start + end].to_string());
    }
    Err("Manifest decode failed".into())
}
