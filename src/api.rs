use reqwest::{
    Client,
    header::{HeaderMap, HeaderValue},
};
use serde_json::{Value, json};
use std::fmt;

// --- ERROR HANDLING ---

#[derive(Debug)]
pub enum YTMusicError {
    Network(reqwest::Error),
    Json(serde_json::Error),
    Io(std::io::Error),
    Custom(String),
}

impl fmt::Display for YTMusicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            YTMusicError::Network(e) => write!(f, "Network Error: {}", e),
            YTMusicError::Json(e) => write!(f, "JSON Error: {}", e),
            YTMusicError::Io(e) => write!(f, "IO Error: {}", e),
            YTMusicError::Custom(e) => write!(f, "API Error: {}", e),
        }
    }
}

impl std::error::Error for YTMusicError {}

impl From<reqwest::Error> for YTMusicError {
    fn from(err: reqwest::Error) -> Self {
        YTMusicError::Network(err)
    }
}
impl From<serde_json::Error> for YTMusicError {
    fn from(err: serde_json::Error) -> Self {
        YTMusicError::Json(err)
    }
}
impl From<std::io::Error> for YTMusicError {
    fn from(err: std::io::Error) -> Self {
        YTMusicError::Io(err)
    }
}

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

    async fn post(&self, endpoint: &str, body: &Value) -> Result<Value, reqwest::Error> {
        self.client
            .post(endpoint)
            .json(body)
            .send()
            .await?
            .json()
            .await
    }

    pub async fn search_songs(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SongDetails>, YTMusicError> {
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
    ) -> Result<Vec<SongDetails>, YTMusicError> {
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

        if related.is_empty() {}

        Ok(related)
    }

    // UPDATED: Now returns Result<..., YTMusicError>
    pub async fn fetch_stream_url(&self, video_id: &str) -> Result<String, YTMusicError> {
        println!("\nFetching URL...");

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
            .ok_or_else(|| YTMusicError::Custom("No formats found".to_string()))?;

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
            .ok_or_else(|| YTMusicError::Custom("No suitable URL found".to_string()))?;

        Ok(best.to_string())
    }
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
