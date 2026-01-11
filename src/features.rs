use crate::Track;
use crate::api::split_title_artist;
use crate::ui_common::blindly_trim;
use reqwest::Client;
use serde_json::Value;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct LrcLine {
    pub timestamp: Duration,
    pub text: String,
    pub translation: Option<String>,
    pub romanized: Option<String>,
}

use crate::ui_common::LYRIC_DISPLAY_MODE;
impl LrcLine {
    pub fn get_current_text(&self) -> &str {
        let mode = LYRIC_DISPLAY_MODE.load(Ordering::Relaxed);
        match mode {
            0 => &self.text,
            1 => self.romanized.as_deref().unwrap_or(&self.text),
            2 => self.translation.as_deref().unwrap_or(&self.text),
            _ => &self.text,
        }
    }
}

fn if_title_contains_non_english_and_other_language_script_return_only_english_part(
    title: &str,
) -> String {
    let ascii_only: String = title.chars().filter(|c| c.is_ascii()).collect();
    ascii_only
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
}

fn duration_to_seconds(d: &str) -> String {
    let parts: Vec<&str> = d.split(':').collect();
    if parts.len() == 2 {
        let mins: u32 = parts[0].parse().unwrap_or(0);
        let secs: u32 = parts[1].parse().unwrap_or(0);
        return (mins * 60 + secs).to_string();
    }
    d.to_string()
}

pub async fn fetch_synced_lyrics(
    track: &Track,
) -> Result<Vec<LrcLine>, Box<dyn std::error::Error + Send + Sync>> {
    let client = Client::new();
    let mut search_urls = Vec::new();

    if let Some(artist1) = track.artists.get(0) {
        // let first_name_of_artist_1 = artist.split_whitespace().next().unwrap_or(artist);
        // /get
        search_urls.push(format!(
            "https://lrclib.net/api/get?track_name={}&artist_name={}&album={}&duration={}",
            urlencoding::encode(
                &if_title_contains_non_english_and_other_language_script_return_only_english_part(
                    &track.title
                )
            ),
            urlencoding::encode(artist1),
            urlencoding::encode(&track.album),
            duration_to_seconds(&track.duration)
        ));
    }
    // /search
    let first_name_of_first_two_artists = track
        .artists
        .iter()
        .take(2)
        .map(|a| a.split_whitespace().next().unwrap_or(a))
        .collect::<Vec<_>>()
        .join(" ");

    search_urls.push(format!(
        "https://lrclib.net/api/search?track_name={}&artist_name={}",
        urlencoding::encode(blindly_trim(&track.title)),
        urlencoding::encode(&first_name_of_first_two_artists),
    ));

    // /get
    if let Some(artist2) = track.artists.get(1) {
        // let first_name_of_artist_2 = artist.split_whitespace().next().unwrap_or(artist);
        search_urls.push(format!(
            "https://lrclib.net/api/get?track_name={}&artist_name={}&album={}&duration={}",
            urlencoding::encode(
                &if_title_contains_non_english_and_other_language_script_return_only_english_part(
                    &track.title
                )
            ),
            urlencoding::encode(artist2),
            urlencoding::encode(&track.album),
            duration_to_seconds(&track.duration)
        ));
    }

    for url in search_urls {
        if let Ok(resp) = client.get(&url).send().await {
            if !resp.status().is_success() {
                continue;
            }

            if let Ok(json) = resp.json::<Value>().await {
                // for /search
                if let Some(arr) = json.as_array() {
                    for entry in arr {
                        if let Some(sync) = entry["syncedLyrics"].as_str() {
                            if !sync.trim().is_empty() {
                                let mut lines = parse_lrc(sync);
                                if !is_mostly_english(&lines) {
                                    let _ = romanize_lyrics_google(&client, &mut lines).await;
                                }
                                return Ok(lines);
                            }
                        }
                    }
                }
                //for /get
                else if let Some(sync) = json["syncedLyrics"].as_str() {
                    if !sync.trim().is_empty() {
                        let mut lines = parse_lrc(sync);
                        if !is_mostly_english(&lines) {
                            let _ = romanize_lyrics_google(&client, &mut lines).await;
                        }
                        return Ok(lines);
                    }
                }
            }
        }
    }

    Err("No synced lyrics available".into())
}

fn is_mostly_english(lines: &[LrcLine]) -> bool {
    let mut total_chars = 0;
    let mut non_ascii_chars = 0;

    for line in lines {
        for c in line.text.chars() {
            if !c.is_whitespace() {
                total_chars += 1;
                if !c.is_ascii() {
                    non_ascii_chars += 1;
                }
            }
        }
    }

    if total_chars == 0 {
        return true;
    }

    (non_ascii_chars as f64 / total_chars as f64) < 0.15
}

async fn romanize_lyrics_google(
    client: &Client,
    lines: &mut Vec<LrcLine>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for chunk in lines.chunks_mut(40) {
        let delimiter = " / ";

        let full_text: String = chunk
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<&str>>()
            .join(delimiter);

        if full_text.trim().is_empty() {
            continue;
        }

        let url = "https://translate.googleapis.com/translate_a/single";

        let params = vec![
            ("client", "gtx"),
            ("sl", "auto"),
            ("tl", "en"),
            ("dt", "t"),  //translation
            ("dt", "rm"), //romanization
            ("q", &full_text),
        ];

        let resp = client.get(url).query(&params).send().await?;

        if resp.status().is_success() {
            let json: Value = resp.json().await?;

            if let Some(sentences) = json.get(0).and_then(|v| v.as_array()) {
                let mut full_trans_blob = String::new();
                for item in sentences {
                    if let Some(trans_segment) = item.get(0).and_then(|v| v.as_str()) {
                        full_trans_blob.push_str(trans_segment);
                    }
                }

                let trans_parts: Vec<&str> = full_trans_blob.split('/').map(|s| s.trim()).collect();

                let romanized_blob = sentences
                    .last()
                    .and_then(|item| item.get(3))
                    .and_then(|v| v.as_str());

                let mut rom_parts: Vec<&str> = Vec::new();
                if let Some(text) = romanized_blob {
                    rom_parts = text.split('/').map(|s| s.trim()).collect();
                }

                for (i, line_obj) in chunk.iter_mut().enumerate() {
                    if let Some(trans) = trans_parts.get(i) {
                        if !trans.is_empty() {
                            line_obj.translation = Some(trans.to_string());
                        }
                    }

                    if let Some(rom) = rom_parts.get(i) {
                        if !rom.is_empty() {
                            line_obj.romanized = Some(rom.to_string());
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn parse_lrc(lrc: &str) -> Vec<LrcLine> {
    let mut lines = Vec::new();
    for line in lrc.lines() {
        if let Some(start) = line.find('[') {
            if let Some(end) = line.find(']') {
                let ts = &line[start + 1..end];
                let text = line[end + 1..].trim().to_string();
                if let Some(dur) = parse_timestamp(ts) {
                    lines.push(LrcLine {
                        timestamp: dur,
                        text,
                        translation: None,
                        romanized: None,
                    });
                }
            }
        }
    }
    lines.sort_by_key(|l| l.timestamp);
    lines
}

fn parse_timestamp(ts: &str) -> Option<Duration> {
    let parts: Vec<&str> = ts.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let minutes: u64 = parts[0].parse().ok()?;
    let seconds: f64 = parts[1].parse().ok()?;
    let total_ms = ((minutes as f64) * 60.0 + seconds) * 1000.0;
    Some(Duration::from_millis(total_ms as u64))
}
