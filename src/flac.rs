use crate::config;
use base64::prelude::*;
use reqwest::Client;
use serde_json::Value;
use std::error::Error;
use std::io::Write;
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

    let items = search_resp["data"]["items"]
        .as_array()
        .ok_or("Invalid search response format")?;

    if items.is_empty() {
        return Err("No search results found".into());
    }

    let selected_item = {
        let first = &items[0];
        let first_dur = first["duration"].as_i64().unwrap_or(0);

        if (first_dur - target_secs).abs() <= 3 {
            Some(first)
        } else {
            items.iter().skip(1).find(|item| {
                let dur = item["duration"].as_i64().unwrap_or(0);
                (dur - target_secs).abs() <= 1
            })
        }
    };

    let item = selected_item.ok_or("No results matched the duration criteria")?;
    let track_id = item["id"].as_i64().ok_or("Selected result has no ID")?;

    let quality = if config().peak_lossless_mode {
        "HI_RES_LOSSLESS"
    } else {
        "LOSSLESS"
    };

    let track_url = format!("{}/track/?id={}&quality={}", api_base, track_id, quality);

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
        return decode_manifest(manifest, track_id);
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

fn decode_manifest(encoded: &str, track_id: i64) -> Result<String, Box<dyn Error + Send + Sync>> {
    let decoded_bytes = BASE64_STANDARD.decode(encoded)?;

    if decoded_bytes.first().map(|&b| b == b'{').unwrap_or(false) {
        if let Ok(json) = serde_json::from_slice::<Value>(&decoded_bytes) {
            if let Some(url) = json["urls"]
                .as_array()
                .and_then(|arr| arr.get(0))
                .and_then(|v| v.as_str())
            {
                return Ok(url.to_string());
            }
        }
    }

    if decoded_bytes.first().map(|&b| b == b'<').unwrap_or(false) {
        let mut path = std::env::current_exe()?.parent().unwrap().to_path_buf();
        path.push("music_data");
        path.push("temp");
        std::fs::create_dir_all(&path)?;

        path.push(format!("{}.mpd", track_id));

        let mut file = std::fs::File::create(&path)?;
        file.write_all(&decoded_bytes)?;

        return Ok(path.to_string_lossy().to_string());
    }

    Err("Manifest decode failed: Unknown format".into())
}
