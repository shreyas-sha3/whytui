use reqwest::{
    Client,
    header::{HeaderMap, HeaderValue},
};
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use std::error::Error;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct SongDetails {
    pub title: String,
    pub video_id: String,
    pub artists: Vec<String>,
    pub duration: String,
}

#[derive(Debug, Clone)]
pub struct PlaylistDetails {
    pub title: String,
    pub playlist_id: String,
    pub count: String,
}

#[derive(Debug, Clone)]
pub struct LrcLine {
    pub timestamp: Duration,
    pub text: String,
}

#[derive(Clone)]
pub struct YTMusic {
    auth_client: Client,
    guest_client: Client,
}

impl YTMusic {
    pub fn new_with_cookies(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        println!("Loading cookies from: {}", path);

        let file = File::open(path)?;
        let reader = BufReader::new(file);

        let mut cookie_string = String::new();
        let mut sapisid = String::new();

        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if trimmed.starts_with('#') && !trimmed.starts_with("#HttpOnly_") {
                continue;
            }

            let parts: Vec<&str> = trimmed.split('\t').collect();
            if parts.len() >= 7 {
                let name = parts[5].trim();
                let value = parts[6].trim();
                cookie_string.push_str(&format!("{}={}; ", name, value));

                if name == "SAPISID" {
                    sapisid = value.to_string();
                }
            }
        }

        if sapisid.is_empty() {
            println!("⚠️ WARNING: 'SAPISID' not found.");
        }

        let mut auth_headers = HeaderMap::new();
        auth_headers.insert("User-Agent", HeaderValue::from_static("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36"));
        auth_headers.insert(
            "Origin",
            HeaderValue::from_static("https://music.youtube.com"),
        );
        auth_headers.insert(
            "Referer",
            HeaderValue::from_static("https://music.youtube.com/"),
        );
        auth_headers.insert("Content-Type", HeaderValue::from_static("application/json"));
        auth_headers.insert("X-Goog-AuthUser", HeaderValue::from_static("0"));

        if !sapisid.is_empty() {
            let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
            let hash_input = format!("{} {} {}", timestamp, sapisid, "https://music.youtube.com");
            let mut hasher = Sha1::new();
            hasher.update(hash_input);
            let hex_hash = hex::encode(hasher.finalize());
            let auth_header = format!("SAPISIDHASH {}_{}", timestamp, hex_hash);
            auth_headers.insert("Authorization", HeaderValue::from_str(&auth_header)?);
        }

        if !cookie_string.is_empty() {
            if let Ok(val) = HeaderValue::from_str(&cookie_string) {
                auth_headers.insert("Cookie", val);
            }
        }

        let auth_client = Client::builder().default_headers(auth_headers).build()?;

        let mut guest_headers = HeaderMap::new();
        guest_headers.insert("User-Agent", HeaderValue::from_static("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36"));

        let guest_client = Client::builder().default_headers(guest_headers).build()?;

        Ok(Self {
            auth_client,
            guest_client,
        })
    }

    async fn post_auth(&self, endpoint: &str, body: &Value) -> Result<Value, Box<dyn Error>> {
        let res = self.auth_client.post(endpoint).json(body).send().await?;
        if !res.status().is_success() {}
        Ok(res.json().await?)
    }

    pub async fn fetch_account_name(&self) -> Result<String, Box<dyn Error>> {
        let url = "https://music.youtube.com/youtubei/v1/account/account_menu?key=AIzaSyC9XL3ZjWddXya6X74dJoCTL-WEYFDNX30";
        let body = json!({
            "context": {
                "client": { "clientName": "WEB_REMIX", "clientVersion": "1.20251210.03.00", "hl": "en", "gl": "US" }
            }
        });

        let res = self.post_auth(url, &body).await?;

        let logged_in = res
            .pointer("/responseContext/serviceTrackingParams")
            .and_then(|v| v.as_array())
            .map(|params| {
                let s = serde_json::to_string(params).unwrap_or_default();
                s.contains(r#""key":"logged_in","value":"1""#)
            })
            .unwrap_or(false);

        if !logged_in {
            return Ok("Guest (Not Logged In)".to_string());
        }

        if let Some(name) = res.pointer("/actions/0/openPopupAction/popup/multiPageMenuRenderer/header/activeAccountHeaderRenderer/accountName/runs/0/text") {
            return Ok(name.as_str().unwrap_or("Unknown").to_string());
        }
        Ok("Logged In".to_string())
    }

    pub async fn search_songs(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SongDetails>, Box<dyn Error>> {
        let url = "https://music.youtube.com/youtubei/v1/search?key=AIzaSyC9XL3ZjWddXya6X74dJoCTL-WEYFDNX30";
        let body = json!({
            "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": "1.20251210.03.00", "hl": "en", "gl": "US" } },
            "query": query,
            "params": "EgWKAQIIAWoKEAMQBRAKEAoQCQ=="
        });

        let res = self.post_auth(url, &body).await?;
        let mut songs = Vec::new();

        if let Some(tabs) = res.pointer("/contents/tabbedSearchResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents").and_then(|v| v.as_array()) {
            for section in tabs {
                if let Some(contents) = section.pointer("/musicShelfRenderer/contents").and_then(|v| v.as_array()) {
                    for item in contents {
                        if let Some(song) = parse_music_item(item) {
                            songs.push(song);
                            if songs.len() >= limit { return Ok(songs); }
                        }
                    }
                }
            }
        }
        Ok(songs)
    }

    pub async fn fetch_library_playlists(&self) -> Result<Vec<PlaylistDetails>, Box<dyn Error>> {
        let url = "https://music.youtube.com/youtubei/v1/browse?key=AIzaSyC9XL3ZjWddXya6X74dJoCTL-WEYFDNX30";
        let body = json!({
            "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": "1.20251210.03.00", "hl": "en", "gl": "US" } },
            "browseId": "FEmusic_liked_playlists"
        });

        let res = self.post_auth(url, &body).await?;
        let mut playlists = Vec::new();

        let items = res.pointer("/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/gridRenderer/items")
            .or_else(|| res.pointer("/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/itemSectionRenderer/contents"))
            .and_then(|v| v.as_array());

        if let Some(items) = items {
            for item in items {
                if let Some(data) = item.pointer("/musicTwoRowItemRenderer") {
                    let title = data
                        .pointer("/title/runs/0/text")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown")
                        .to_string();
                    let id = data
                        .pointer("/navigationEndpoint/browseEndpoint/browseId")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim_start_matches("VL").to_string())
                        .unwrap_or_default();
                    let count = data
                        .pointer("/subtitle/runs")
                        .and_then(|v| v.as_array())
                        .and_then(|runs| runs.last())
                        .and_then(|run| run.pointer("/text"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if !id.is_empty() {
                        playlists.push(PlaylistDetails {
                            title,
                            playlist_id: id,
                            count,
                        });
                    }
                }
            }
        }
        Ok(playlists)
    }

    pub async fn fetch_playlist_songs(
        &self,
        playlist_id: &str,
        limit: usize,
    ) -> Result<Vec<SongDetails>, Box<dyn Error>> {
        let browse_id = if playlist_id == "LM" {
            "LM".to_string()
        } else if playlist_id.starts_with("VL") {
            playlist_id.to_string()
        } else {
            format!("VL{}", playlist_id)
        };

        let url = "https://music.youtube.com/youtubei/v1/browse?key=AIzaSyC9XL3ZjWddXya6X74dJoCTL-WEYFDNX30";
        let body = json!({
            "context": {
                "client": { "clientName": "WEB_REMIX", "clientVersion": "1.20251210.03.00", "hl": "en", "gl": "US" }
            },
            "browseId": browse_id
        });

        let res = self.post_auth(url, &body).await?;
        let mut songs = Vec::new();

        let path = "/contents/twoColumnBrowseResultsRenderer/secondaryContents/sectionListRenderer/contents/0/musicPlaylistShelfRenderer/contents";

        if let Some(items) = res.pointer(path).and_then(|v| v.as_array()) {
            for item in items {
                if let Some(r) = item.pointer("/musicResponsiveListItemRenderer") {
                    let title = r.pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text")
                            .and_then(|v| v.as_str()).unwrap_or("Unknown").to_string();

                    let video_id = r.pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/navigationEndpoint/watchEndpoint/videoId")
                            .and_then(|v| v.as_str()).unwrap_or("").to_string();

                    let duration = r.pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text/runs/0/text")
                            .and_then(|v| v.as_str()).unwrap_or("0:00").to_string();

                    let mut artists = Vec::new();
                    if let Some(runs) = r
                        .pointer(
                            "/flexColumns/1/musicResponsiveListItemFlexColumnRenderer/text/runs",
                        )
                        .and_then(|v| v.as_array())
                    {
                        for run in runs {
                            if let Some(pt) = run.pointer("/navigationEndpoint/browseEndpoint/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType").and_then(|v| v.as_str()) {
                                    if pt == "MUSIC_PAGE_TYPE_ARTIST" {
                                        if let Some(name) = run.pointer("/text").and_then(|v| v.as_str()) {
                                            artists.push(name.to_string());
                                        }
                                    }
                                }
                        }
                    }

                    if !video_id.is_empty() {
                        let title = format!("{} [{}]", title, artists.join(", "));

                        songs.push(SongDetails {
                            title,
                            video_id,
                            artists,
                            duration,
                        });
                        if songs.len() >= limit {
                            break;
                        }
                    }
                }
            }
        }

        if songs.is_empty() {
            println!("⚠️ Warning: Playlist seems empty or path changed.");
        }

        Ok(songs)
    }

    pub async fn fetch_stream_url(&self, video_id: &str) -> Result<String, Box<dyn Error>> {
        let payload = json!({
            "context": {
                "client": {
                    "clientName": "ANDROID",
                    "clientVersion": "19.09.37",
                    "hl": "en",
                    "gl": "US"
                }
            },
            "videoId": video_id,
            "playbackContext": {
                "contentPlaybackContext": {
                    "html5Preference": "HTML5_PREF_WANTS"
                }
            }
        });

        let res = self.guest_client
            .post("https://music.youtube.com/youtubei/v1/player?key=AIzaSyC9XL3ZjWddXya6X74dJoCTL-WEYFDNX30")
            .json(&payload)
            .send()
            .await?;

        let data: Value = res.json().await?;

        if let Some(status) = data
            .pointer("/playabilityStatus/status")
            .and_then(|s| s.as_str())
        {
            if status != "OK" {
                let reason = data
                    .pointer("/playabilityStatus/reason")
                    .and_then(|s| s.as_str())
                    .unwrap_or("Unknown error");
                return Err(format!("Video unavailable: {}", reason).into());
            }
        }

        let formats = data
            .pointer("/streamingData/adaptiveFormats")
            .and_then(|v| v.as_array())
            .ok_or("No formats found")?;

        let best = formats
            .iter()
            .filter(|f| f["mimeType"].as_str().unwrap_or("").starts_with("audio/"))
            .max_by_key(|f| f["bitrate"].as_i64().unwrap_or(0))
            .ok_or("No suitable audio stream found")?;

        if let Some(url) = best["url"].as_str() {
            Ok(url.to_string())
        } else if !best["signatureCipher"].is_null() {
            Err("URL is encrypted (Signature Cipher).".into())
        } else {
            Err("No URL found.".into())
        }
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

        let res = self.post_auth(url, &payload).await?;
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
    let acc_label = r.pointer("/flexColumns/1/musicResponsiveListItemFlexColumnRenderer/text/accessibility/accessibilityData/label").and_then(|v| v.as_str()).unwrap_or("");
    if !acc_label.is_empty() && !video_id.is_empty() {
        let parts: Vec<&str> = acc_label.split(" • ").collect();
        if parts.len() >= 3 {
            let artists: Vec<String> = parts[0]
                .split(&['&', ','][..])
                .map(|s| s.trim().to_string())
                .collect();
            let duration = parse_duration(parts[2]);
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
