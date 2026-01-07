use base64::prelude::*;
use reqwest::Client;
use serde_json::Value;
use std::error::Error;
use std::time::Instant;
use tokio::sync::OnceCell;

static ACTIVE_API: OnceCell<String> = OnceCell::const_new();

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const API_CANDIDATES: &[&str] = &[
    "https://triton.squid.wtf",
    "https://tidal.kinoplus.online",
    "https://tidal-api.binimum.org",
    "https://wolf.qqdl.site",
    "https://maus.qqdl.site",
    "https://vogel.qqdl.site",
    "https://katze.qqdl.site",
    "https://hund.qqdl.site",
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
    query: &str,
    target_duration: &str,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    let api_base = ACTIVE_API.get().ok_or("API not initialized")?;
    let client = Client::builder().user_agent(UA).build()?;
    let target_secs = parse_to_seconds(target_duration);

    let search_url = format!("{}/search/?s={}", api_base, urlencoding::encode(query));
    let search_resp: Value = client.get(&search_url).send().await?.json().await?;
    let first_item = search_resp["data"]["items"]
        .get(0)
        .ok_or("No search results found")?;

    let api_secs = first_item["duration"].as_i64().unwrap_or(0);

    if (api_secs - target_secs).abs() > 3 {
        return Err("First result doesn't match duration tolerance".into());
    }

    let track_id = first_item["id"].as_i64().ok_or("First result has no ID")?;

    let track_url = format!("{}/track/?id={}&quality=LOSSLESS", api_base, track_id);
    let track_data: Value = client.get(&track_url).send().await?.json().await?;

    let data = if track_data.get("data").is_some() {
        &track_data["data"]
    } else {
        &track_data
    };

    if let Some(stream_url) = data["OriginalTrackUrl"].as_str() {
        if !stream_url.is_empty() {
            return Ok(stream_url.to_string());
        }
    }

    if let Some(manifest) = data["manifest"].as_str() {
        return decode_manifest(manifest);
    }

    Err("No FLAC stream available for this track".into())
}

fn parse_to_seconds(duration_str: &str) -> i64 {
    let parts: Vec<&str> = duration_str.split(':').collect();
    if parts.len() == 2 {
        let mins: i64 = parts[0].parse().unwrap_or(0);
        let secs: i64 = parts[1].parse().unwrap_or(0);
        return mins * 60 + secs;
    }
    duration_str.parse::<i64>().unwrap_or(0)
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
