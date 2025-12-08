use reqwest::{
    Client,
    header::{HeaderMap, HeaderValue},
};
use serde_json::{Value, json};
use std::error::Error;
use std::time::Duration;
//add ability to clone for tokiko later
#[derive(Debug, Clone)]
pub struct SongDetails {
    pub title: String,
    pub video_id: String,
    pub artists: Vec<String>,
    pub duration: String,
}

#[derive(Clone)]
pub struct YTMusic {
    client: Client,
}

impl YTMusic {
    pub fn new() -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Origin",
            HeaderValue::from_static("https://music.youtube.com"),
        );
        headers.insert("X-Goog-Visitor-Id", HeaderValue::from_static("CgtwAQIIAQ"));
        headers.insert(
            "Origin",
            HeaderValue::from_static("https://music.youtube.com"),
        );
        headers.insert(
            "Referer",
            HeaderValue::from_static("https://music.youtube.com/search"),
        );
        headers.insert(
            "User-Agent",
            HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"),
        );

        Self {
            client: Client::builder().default_headers(headers).build().unwrap(),
        }
    }

    async fn post(&self, endpoint: &str, body: &Value) -> Result<Value, Box<dyn Error>> {
        let res = self
            .client
            .post(endpoint)
            .json(body)
            .send()
            .await?
            .json()
            .await?;

        Ok(res)
    }

    pub async fn search_songs(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SongDetails>, Box<dyn Error>> {
        let url = "https://music.youtube.com/youtubei/v1/search?key=AIzaSyC9XL3ZjWddXya6X74dJoCTL-WEYFDNX30";
        let body = json!({
            "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": "1.20231206.01.00", "hl": "en", "gl": "US" } },
            "query": query,
            "params": "EgWKAQIIAWoKEAMQBRAKEAoQCQ=="
        });

        let res = self.post(url, &body).await?;

        fn arr<'a>(v: &'a Value, p: &str) -> &'a [Value] {
            v.pointer(p).and_then(|v| v.as_array()).map_or(&[], |v| v)
        }

        let mut songs = Vec::new();
        let sections = arr(
            &res,
            "/contents/tabbedSearchResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents",
        );

        for section in sections {
            for item in arr(section, "/musicShelfRenderer/contents") {
                if let Some(song) = parse_music_item(item) {
                    songs.push(song);
                    if songs.len() >= limit {
                        return Ok(songs);
                    }
                }
            }
        }

        Ok(songs)
    }

    pub async fn fetch_related_songs(
        &self,
        video_id: &str,
        limit: usize,
    ) -> Result<Vec<SongDetails>, Box<dyn Error>> {
        let url = "https://music.youtube.com/youtubei/v1/next?key=AIzaSyC9XL3ZjWddXya6X74dJoCTL-WEYFDNX30";
        let playlist_id = format!("RDAMVM{}", video_id);

        let payload = json!({
            "context": {
                "client": { "clientName": "WEB_REMIX", "clientVersion": "1.20231206.01.00", "hl": "en", "gl": "US" }
            },
            "videoId": video_id,
            "playlistId": playlist_id
        });

        let res = self.post(url, &payload).await?;

        let mut related = Vec::new();
        if let Some(tabs) = res.pointer("/contents/singleColumnMusicWatchNextResultsRenderer/tabbedRenderer/watchNextTabbedResultsRenderer/tabs").and_then(|v| v.as_array()) {
            for tab in tabs {
                if let Some(contents) = tab.pointer("/tabRenderer/content/musicQueueRenderer/content/playlistPanelRenderer/contents").and_then(|v| v.as_array()) {
                    for item in contents {
                        if let Some(song) = parse_queue_item(item) {
                            if song.video_id != video_id {
                                related.push(song);
                            }
                            if related.len() >= limit { break; }
                        }
                    }
                }
            }
        }

        Ok(related)
    }

    pub async fn fetch_stream_url(&self, video_id: &str) -> Result<String, Box<dyn Error>> {
        println!("Fetching URL...");

        let payload = json!({
            "context": {
                "client": {
                    "clientName": "ANDROID", "clientVersion": "19.09.37"
                }
            },
            "videoId": video_id
        });

        let res = self.client
            .post("https://music.youtube.com/youtubei/v1/player?key=AIzaSyC9XL3ZjWddXya6X74dJoCTL-WEYFDNX30")
            .json(&payload)
            .send()
            .await?;

        let data: Value = res.json().await?;

        let formats = data["streamingData"]["adaptiveFormats"]
            .as_array()
            .ok_or("No formats found")?;

        let best = formats
            .iter()
            .filter(|f| {
                f["mimeType"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with("audio/webm")
            })
            .max_by_key(|f| f["bitrate"].as_i64().unwrap_or(0))
            .and_then(|f| f["url"].as_str())
            .ok_or("No suitable URL found")?;

        Ok(best.to_string())
    }
}
pub fn split_title_artist(input: &str) -> (String, String) {
    if let (Some(start), Some(end)) = (input.rfind('['), input.rfind(']')) {
        if end > start {
            let title = input[..start].trim().to_string();
            let artist = input[start + 1..end].trim().to_string();
            return (title, artist);
        }
    }
    (input.trim().to_string(), String::new())
}

pub async fn _fetch_lyrics(title_artist: &str) -> Result<String, Box<dyn Error>> {
    let (title, artist) = split_title_artist(title_artist);

    let client = Client::new();

    let url = format!(
        "https://lrclib.net/api/get?track_name={}&artist_name={}",
        urlencoding::encode(&title),
        urlencoding::encode(&artist),
    );

    let resp = client.get(&url).send().await?;

    if !resp.status().is_success() {
        return Err("No lyrics found".into());
    }

    let json: Value = resp.json().await?;
    let lyrics = json["plainLyrics"].as_str().unwrap_or("").to_string();

    Ok(lyrics)
}

#[derive(Debug, Clone)]
pub struct LrcLine {
    pub timestamp: Duration,
    pub text: String,
}

pub async fn fetch_synced_lyrics(
    title_artist: &str,
) -> Result<Vec<LrcLine>, Box<dyn std::error::Error + Send + Sync>> {
    let (title, artist) = split_title_artist(title_artist);
    let client = reqwest::Client::new();

    let mut search_urls = Vec::new();

    search_urls.push(format!(
        "https://lrclib.net/api/search?track_name={}&artist_name={}",
        urlencoding::encode(&title),
        urlencoding::encode(&artist)
    ));

    if let Some((clean_title, _)) = title.split_once(" - ") {
        search_urls.push(format!(
            "https://lrclib.net/api/search?track_name={}",
            urlencoding::encode(clean_title)
        ));
    }

    search_urls.push(format!(
        "https://lrclib.net/api/search?q={}",
        urlencoding::encode(title_artist)
    ));

    for url in search_urls {
        if let Ok(resp) = client.get(&url).send().await {
            if resp.status().is_success() {
                if let Ok(json) = resp.json::<Value>().await {
                    if let Some(arr) = json.as_array() {
                        for entry in arr {
                            if let Some(sync) = entry["syncedLyrics"].as_str() {
                                if !sync.trim().is_empty() {
                                    return Ok(parse_lrc(sync));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Err("No synced lyrics available".into())
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

fn parse_queue_item(item: &Value) -> Option<SongDetails> {
    let r = item.pointer("/playlistPanelVideoRenderer")?;

    let raw_title = r.pointer("/title/runs/0/text")?.as_str()?.to_string();
    let video_id = r.pointer("/videoId")?.as_str()?.to_string();

    let duration = r
        .pointer("/lengthText/runs/0/text")
        .and_then(|v| v.as_str())
        .map(parse_duration)
        .unwrap_or("0:00".to_string());

    let artist_text = r
        .pointer("/longBylineText/runs/0/text")
        .or_else(|| r.pointer("/shortBylineText/runs/0/text"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");

    let artists: Vec<String> = artist_text
        .split(&['&', ','][..])
        .map(|s| s.trim().to_string())
        .collect();

    let title = format!("{} [{}]", raw_title, artists.join(", "));

    Some(SongDetails {
        title,
        video_id,
        artists,
        duration,
    })
}

fn parse_music_item(item: &Value) -> Option<SongDetails> {
    let r = item.pointer("/musicResponsiveListItemRenderer")?;

    let raw_title = r
        .pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();

    let video_id = r
        .pointer("/playlistItemData/videoId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let acc_label = r
        .pointer("/flexColumns/1/musicResponsiveListItemFlexColumnRenderer/text/accessibility/accessibilityData/label")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if !acc_label.is_empty() && !video_id.is_empty() {
        let parts: Vec<&str> = acc_label.split(" • ").collect();

        if parts.len() >= 3 {
            let artists: Vec<String> = parts[0]
                .split(&['&', ','][..])
                .map(|s| s.trim().to_string())
                .collect();

            let duration = parse_duration(parts[2]);

            //combinig title,artists  before sending to main.rs
            let title = format!("{} [{}]", raw_title, artists.join(", "));

            return Some(SongDetails {
                title,
                video_id,
                artists,
                duration,
            });
        }
    }

    None
}

fn parse_duration(s: &str) -> String {
    let nums: Vec<u32> = s
        .split(|c: char| !c.is_ascii_digit())
        .filter(|x| !x.is_empty())
        .filter_map(|x| x.parse().ok())
        .collect();

    match nums.as_slice() {
        [h, m, s] => format!("{}:{:02}:{:02}", h, m, s),
        [m, s] => format!("{}:{:02}", m, s),
        [s] => format!("0:{:02}", s),
        _ => "0:00".to_string(),
    }
}
