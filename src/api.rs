use reqwest::{
    Client,
    header::{
        ACCEPT, AUTHORIZATION, CONTENT_TYPE, COOKIE, HeaderMap, HeaderValue, ORIGIN, REFERER,
        USER_AGENT,
    },
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
    pub artists: Vec<String>,
    pub album: String,
    pub duration: String,
    pub thumbnail_url: Option<String>,
    pub video_id: String,
}

#[derive(Debug, Clone)]
pub struct PlaylistDetails {
    pub title: String,
    pub playlist_id: String,
    pub count: String,
    pub continuation_token: Option<String>,
}

#[derive(Clone)]
pub struct YTMusic {
    auth_client: Client,
    guest_client: Client,
}

impl YTMusic {
    pub fn new_with_cookies(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        let mut cookie_string = String::new();
        let mut sapisid = String::new();

        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty()
                || (trimmed.starts_with('#') && !trimmed.starts_with("#HttpOnly_"))
            {
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

        let user_agent = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36";

        let mut common_headers = HeaderMap::new();
        common_headers.insert(USER_AGENT, HeaderValue::from_static(user_agent));
        common_headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
        common_headers.insert(
            "Accept-Language",
            HeaderValue::from_static("en-GB,en;q=0.9"),
        );
        common_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        common_headers.insert(
            "Sec-Ch-Ua",
            HeaderValue::from_static("\"Not_A Brand\";v=\"99\", \"Chromium\";v=\"142\""),
        );
        common_headers.insert("Sec-Ch-Ua-Mobile", HeaderValue::from_static("?0"));
        common_headers.insert("Sec-Ch-Ua-Platform", HeaderValue::from_static("\"Linux\""));
        common_headers.insert(
            ORIGIN,
            HeaderValue::from_static("https://music.youtube.com"),
        );
        common_headers.insert(
            REFERER,
            HeaderValue::from_static("https://music.youtube.com/"),
        );

        let mut auth_headers = common_headers.clone();
        auth_headers.insert("X-Goog-AuthUser", HeaderValue::from_static("0"));

        auth_headers.insert("X-Youtube-Client-Name", HeaderValue::from_static("67"));
        auth_headers.insert(
            "X-Youtube-Client-Version",
            HeaderValue::from_static("1.20251215.03.00"),
        );

        if !sapisid.is_empty() {
            let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
            let hash_input = format!("{} {} {}", timestamp, sapisid, "https://music.youtube.com");
            let mut hasher = Sha1::new();
            hasher.update(hash_input);
            let hex_hash = hex::encode(hasher.finalize());
            let auth_header = format!("SAPISIDHASH {}_{}", timestamp, hex_hash);
            auth_headers.insert(AUTHORIZATION, HeaderValue::from_str(&auth_header)?);
        }

        if !cookie_string.is_empty() {
            if let Ok(val) = HeaderValue::from_str(&cookie_string) {
                auth_headers.insert(COOKIE, val);
            }
        }

        let auth_client = Client::builder().default_headers(auth_headers).build()?;

        let mut guest_headers = common_headers.clone();

        guest_headers.insert("X-Youtube-Client-Name", HeaderValue::from_static("67"));
        guest_headers.insert(
            "X-Youtube-Client-Version",
            HeaderValue::from_static("1.20251215.03.00"),
        );

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

    pub async fn fetch_stream_url(&self, video_id: &str) -> Result<String, Box<dyn Error>> {
        let payload = json!({
            "videoId": video_id,
            "context": {
                "client": {
                    "hl": "en",
                    "gl": "IN",
                    "remoteHost": "123.185.130.321",
                    "deviceMake": "",
                    "deviceModel": "",
                    "visitorData": "CgtYTkZURlh1U0hIdyjrzYrKBjIKCgJJThIEGgAgOw%3D%3D",
                    "userAgent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36,gzip(gfe)",
                    "clientName": "ANDROID",
                    "clientVersion": "19.09.37",
                    "osName": "X11",
                    "osVersion": "",
                    "originalUrl": "https://music.youtube.com/",
                    "platform": "DESKTOP",
                    "clientFormFactor": "UNKNOWN_FORM_FACTOR",
                    "configInfo": {
                        "appInstallData": "COvNisoGEMj3zxwQudnOHBDyndAcEL22rgUQlLbQHBCZjbEFEJX3zxwQ0eDPHBDhgoATENr3zhwQlP6wBRDatNAcEKefqRcQg57QHBCNsNAcEMGP0BwQmrnQHBDYltAcEI-50BwQvKTQHBD2q7AFENPhrwUQzN-uBRDKu9AcEK7WzxwQpbbQHBCd0LAFEIzpzxwQooW4IhDxnLAFEIeszhwQzrPQHBCBzc4cEJOD0BwQnNfPHBD8ss4cEMn3rwUQg6zQHBDevM4cELnA0BwQ5ofQHBC8s4ATEMzrzxwQvZmwBRC8v9AcEL2KsAUQlPLPHBDHttAcEKudzxwQ28HQHBC36v4SEKafqRcQ8rPQHBDwtNAcEIiHsAUQu9nOHBCL988cEPCdzxwQltvPHBDlpNAcELjkzhwQ4cGAExCJsM4cEMWM0BwQr7CAEypMQ0FNU014VW8tWnEtRE1lVUVvY09xZ0xNQmJiUDhBc3l2MV9wMVFVRHpmOEZvWUFHb2k3UDFBYjBMX1lQN3pDVzNRV1R2Z1lkQnc9PTAA"
                    },
                    "browserName": "Chrome",
                    "browserVersion": "142.0.0.0"
                },
                "user": {
                    "lockedSafetyMode": false
                },
                "request": {
                    "useSsl": true,
                    "internalExperimentFlags": [],
                    "consistencyTokenJars": []
                },
                "playbackContext": {
                    "contentPlaybackContext": {
                        "html5Preference": "HTML5_PREF_WANTS",
                        "signatureTimestamp": 20436,
                        "autoCaptionsDefaultOn": false
                    }
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
            .filter(|f| {
                f["mimeType"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with("audio/webm")
            })
            .max_by_key(|f| f["bitrate"].as_i64().unwrap_or(0))
            .ok_or("No suitable audio stream found")?;

        if let Some(url) = best["url"].as_str() {
            Ok(url.to_string())
        } else {
            Err("URL is encrypted (Signature Cipher).".into())
        }
    }

    pub async fn like_song(&self, video_id: &str) -> Result<(), Box<dyn Error>> {
        let url = "https://music.youtube.com/youtubei/v1/like/like";
        let body = json!({
            "context": {
                "client": {
                    "clientName": "WEB_REMIX",
                    "clientVersion": "1.20251215.03.00",
                    "hl": "en",
                    "gl": "IN"
                }
            },
            "target": {
                "videoId": video_id
            }
        });
        self.post_auth(url, &body).await?;
        Ok(())
    }

    pub async fn add_to_playlist(
        &self,
        playlist_id: &str,
        video_id: &str,
    ) -> Result<(), Box<dyn Error>> {
        let url = "https://music.youtube.com/youtubei/v1/browse/edit_playlist";
        let body = json!({
            "context": {
                "client": {
                    "clientName": "WEB_REMIX",
                    "clientVersion": "1.20251215.03.00",
                    "hl": "en",
                    "gl": "IN"
                }
            },
            "actions":[{
                "addedVideoId":video_id,
                "action":"ACTION_ADD_VIDEO",
                "dedupeOption":"DEDUPE_OPTION_CHECK"}],
            "playlistId":playlist_id
        });
        self.post_auth(url, &body).await?;
        Ok(())
    }
    pub async fn fetch_account_name(&self) -> Result<String, Box<dyn Error>> {
        let url = "https://music.youtube.com/youtubei/v1/account/account_menu";
        let body = json!({
            "context": {
                "client": { "clientName": "WEB_REMIX", "clientVersion": "1.20251215.03.00", "hl": "en", "gl": "IN" }
            }
        });

        let res = self.post_auth(url, &body).await?;

        let logged_in = res
            .pointer("/responseContext/serviceTrackingParams")
            .and_then(|v| v.as_array())
            .map(|params| {
                serde_json::to_string(params)
                    .unwrap_or_default()
                    .contains(r#""key":"logged_in","value":"1""#)
            })
            .unwrap_or(false);

        if !logged_in {
            return Ok("Guest".to_string());
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
        let url = "https://music.youtube.com/youtubei/v1/search";
        let body = json!({
            "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": "1.20251215.03.00", "hl": "en", "gl": "IN" } },
            "query": query,
            "params": "EgWKAQIIAWoKEAMQBRAKEAoQCQ=="
        });
        let res = self.post_auth(url, &body).await?;
        self.parse_search_results(res, limit)
    }

    pub async fn fetch_library_playlists(&self) -> Result<Vec<PlaylistDetails>, Box<dyn Error>> {
        let url = "https://music.youtube.com/youtubei/v1/browse";
        let body = json!({
            "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": "1.20251215.03.00", "hl": "en", "gl": "IN" } },
            "browseId": "FEmusic_liked_playlists"
        });
        let res = self.post_auth(url, &body).await?;
        self.parse_library_playlists(res)
    }

    pub async fn fetch_continuation(
        &self,
        token: &str,
    ) -> Result<(Vec<SongDetails>, Option<String>), Box<dyn Error>> {
        let body = json!({
            "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": "1.20251215.03.00", "hl": "en", "gl": "IN" } },
            "continuation": token
        });
        let res = self
            .post_auth("https://music.youtube.com/youtubei/v1/browse", &body)
            .await?;
        self.parse_playlist_songs(res)
    }

    pub async fn fetch_playlist_songs(
        &self,
        playlist_id: &str,
        limit: usize,
    ) -> Result<(Vec<SongDetails>, Option<String>), Box<dyn Error>> {
        let mut songs = Vec::new();
        let mut next_token = None;
        let mut is_first = true;

        while songs.len() < limit {
            let body = if is_first {
                let bid = if playlist_id.starts_with("VL") {
                    playlist_id.to_string()
                } else {
                    format!("VL{}", playlist_id)
                };
                json!({
                    "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": "1.20251215.03.00", "hl": "en", "gl": "IN" } },
                    "browseId": bid
                })
            } else {
                let t = next_token.take().ok_or("Token missing")?;
                json!({
                    "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": "1.20251215.03.00", "hl": "en", "gl": "IN" } },
                    "continuation": t
                })
            };

            let res = self
                .post_auth("https://music.youtube.com/youtubei/v1/browse", &body)
                .await?;
            let (batch, new_token) = self.parse_playlist_songs(res)?;

            if batch.is_empty() && new_token.is_none() {
                break;
            }
            let needed = limit - songs.len();
            if batch.len() > needed {
                songs.extend(batch.into_iter().take(needed));
            } else {
                songs.extend(batch);
            }

            next_token = new_token;
            is_first = false;

            if next_token.is_none() {
                break;
            }
        }
        Ok((songs, next_token))
    }

    pub async fn fetch_related_songs(
        &self,
        video_id: &str,
        playlist_id: Option<&str>,
        limit: usize,
        shuffle: bool,
    ) -> Result<Vec<SongDetails>, Box<dyn Error>> {
        let url = "https://music.youtube.com/youtubei/v1/next";

        let resolved_playlist_id = match playlist_id {
            Some(id) => id.to_string(),
            None => format!("RDAMVM{}", video_id),
        };

        let params = if shuffle { "wAEB8gECKAE%3D" } else { "wAEB" };

        let payload = json!({
            "context": {
                "client": {
                    "clientName": "WEB_REMIX",
                    "clientVersion": "1.20251215.03.00",
                    "hl": "en",
                    "gl": "IN"
                }
            },
            "videoId": video_id,
            "playlistId": resolved_playlist_id,
            "params": params,
            "isAudioOnly": true
        });

        let res = self.post_auth(url, &payload).await?;

        self.parse_related_songs(res, video_id, limit)
    }

    fn parse_search_results(
        &self,
        res: Value,
        limit: usize,
    ) -> Result<Vec<SongDetails>, Box<dyn Error>> {
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

    fn parse_library_playlists(&self, res: Value) -> Result<Vec<PlaylistDetails>, Box<dyn Error>> {
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

                    if title == "Episodes for Later" {
                        continue;
                    }

                    let id = data
                        .pointer("/navigationEndpoint/browseEndpoint/browseId")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim_start_matches("VL").to_string())
                        .unwrap_or_default();

                    let mut count = data
                        .pointer("/subtitle/runs")
                        .and_then(|v| v.as_array())
                        .and_then(|runs| runs.last())
                        .and_then(|run| run.pointer("/text"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    if count == "Auto playlist" {
                        count = "∞".to_string();
                    }
                    if !id.is_empty() {
                        playlists.push(PlaylistDetails {
                            title,
                            playlist_id: id,
                            count,
                            continuation_token: None,
                        });
                    }
                }
            }
        }
        Ok(playlists)
    }

    fn parse_playlist_songs(
        &self,
        res: Value,
    ) -> Result<(Vec<SongDetails>, Option<String>), Box<dyn Error>> {
        let mut songs = Vec::new();
        let mut token = None;

        let items = res.pointer("/contents/twoColumnBrowseResultsRenderer/secondaryContents/sectionListRenderer/contents/0/musicPlaylistShelfRenderer/contents")
                .or_else(|| res.pointer("/onResponseReceivedActions/0/appendContinuationItemsAction/continuationItems"))
                .or_else(|| res.pointer("/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/musicPlaylistShelfRenderer/contents"))
                .and_then(|v| v.as_array());

        if let Some(items) = items {
            for item in items {
                if let Some(data) = item.pointer("/musicResponsiveListItemRenderer") {
                    let title = data.pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string();
                    let video_id = data
                        .pointer("/playlistItemData/videoId")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();

                    if video_id.is_empty() {
                        continue;
                    }

                    let duration = data.pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text/runs/0/text").and_then(|v| v.as_str()).unwrap_or("0:00").to_string();
                    let mut artists = Vec::new();
                    let mut album = "Unknown".to_string();

                    if let Some(runs) = data
                        .pointer(
                            "/flexColumns/1/musicResponsiveListItemFlexColumnRenderer/text/runs",
                        )
                        .and_then(|v| v.as_array())
                    {
                        for run in runs {
                            let text = run.pointer("/text").and_then(|v| v.as_str()).unwrap_or("");
                            if text == " • "
                                || text.chars().all(char::is_numeric)
                                || text.contains(':')
                            {
                                continue;
                            }

                            match run.pointer("/navigationEndpoint/browseEndpoint/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType").and_then(|v| v.as_str()) {
                                    Some("MUSIC_PAGE_TYPE_ARTIST") => artists.push(text.to_string()),
                                    Some("MUSIC_PAGE_TYPE_ALBUM") => album = text.to_string(),
                                    _ => if artists.is_empty() && text != "E" { artists.push(text.to_string()); }
                                }
                        }
                    }

                    songs.push(SongDetails {
                        title,
                        video_id,
                        artists,
                        album,
                        duration,
                        thumbnail_url: parse_thumbnail(data),
                    });
                } else if let Some(t) = item
                    .pointer(
                        "/continuationItemRenderer/continuationEndpoint/continuationCommand/token",
                    )
                    .and_then(|v| v.as_str())
                {
                    token = Some(t.to_string());
                }
            }
        }
        Ok((songs, token))
    }

    fn parse_related_songs(
        &self,
        res: Value,
        video_id: &str,
        limit: usize,
    ) -> Result<Vec<SongDetails>, Box<dyn Error>> {
        let mut related = Vec::new();

        if let Some(tabs) = res
                    .pointer("/contents/singleColumnMusicWatchNextResultsRenderer/tabbedRenderer/watchNextTabbedResultsRenderer/tabs")
                    .and_then(|v| v.as_array())
                {
                    for tab in tabs {
                        if let Some(contents) = tab
                            .pointer("/tabRenderer/content/musicQueueRenderer/content/playlistPanelRenderer/contents")
                            .and_then(|v| v.as_array())
                        {
                            let mut found_current = false;

                            for item in contents {
                                let data = if let Some(renderer) = item.pointer("/playlistPanelVideoRenderer") {
                                    Some(renderer)
                                } else if let Some(renderer) = item.pointer("/playlistPanelVideoWrapperRenderer/primaryRenderer/playlistPanelVideoRenderer") {
                                    Some(renderer)
                                } else {
                                    None
                                };

                                if let Some(r) = data {
                                    let item_id = r.pointer("/videoId").and_then(|v| v.as_str()).unwrap_or("");
                                    let is_selected = r.pointer("/selected").and_then(|v| v.as_bool()).unwrap_or(false);

                                    if is_selected || (item_id == video_id && !found_current) {
                                        found_current = true;
                                        continue;
                                    }

                                    if found_current {
                                        let title = r.pointer("/title/runs/0/text").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string();
                                        let duration = r.pointer("/lengthText/runs/0/text").and_then(|v| v.as_str()).map(parse_duration).unwrap_or("0:00".to_string());

                                        let mut artists = Vec::new();
                                        let mut album = "Unknown".to_string();

                                        if let Some(runs) = r.pointer("/longBylineText/runs").and_then(|v| v.as_array()) {
                                            let mut is_album_section = false;
                                            for run in runs {
                                                let text = run.pointer("/text").and_then(|v| v.as_str()).unwrap_or("");

                                                if text == " • " {
                                                    is_album_section = true;
                                                    continue;
                                                }
                                                if text == ", " || text == " & " || text.trim().is_empty() {
                                                    continue;
                                                }

                                                if is_album_section {
                                                    album = text.to_string();
                                                } else {
                                                    artists.push(text.to_string());
                                                }
                                            }
                                        } else {
                                            let artist_text = r.pointer("/shortBylineText/runs/0/text").and_then(|v| v.as_str()).unwrap_or("Unknown");
                                            artists.push(artist_text.to_string());
                                        }

                                        let thumbnail_url = parse_thumbnail(r);

                                        related.push(SongDetails {
                                            title,
                                            video_id: item_id.to_string(),
                                            artists,
                                            album,
                                            duration,
                                            thumbnail_url,
                                        });

                                        if related.len() >= limit {
                                            return Ok(related);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
        Ok(related)
    }
}

fn parse_music_item(item: &Value) -> Option<SongDetails> {
    let r = item.pointer("/musicResponsiveListItemRenderer")?;
    let raw_title = r
        .pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text")?
        .as_str()?
        .to_string();
    let video_id = r
        .pointer("/playlistItemData/videoId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let acc_label = r.pointer("/flexColumns/1/musicResponsiveListItemFlexColumnRenderer/text/accessibility/accessibilityData/label").and_then(|v| v.as_str()).unwrap_or("");

    if !acc_label.is_empty() && !video_id.is_empty() {
        let parts: Vec<&str> = acc_label.split(" • ").collect();

        let artists: Vec<String> = parts[0]
            .split(&['&', ','][..])
            .map(|s| s.trim().to_string())
            .collect();

        // parse artist • album • duration else only artist • durations
        let (album, duration_str) = if parts.len() >= 3 {
            (parts[1].to_string(), parts[2])
        } else if parts.len() == 2 {
            ("Single".to_string(), parts[1])
        } else {
            ("Unknown".to_string(), "0:00")
        };

        let duration = parse_duration(duration_str);
        let thumbnail_url = parse_thumbnail(r);

        return Some(SongDetails {
            title: raw_title,
            video_id,
            artists,
            album,
            duration,
            thumbnail_url,
        });
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

fn parse_thumbnail(r: &Value) -> Option<String> {
    let thumbs = r
        .pointer("/thumbnail/thumbnails")
        .or_else(|| r.pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails"));

    thumbs
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.last())
        .and_then(|obj| obj.pointer("/url"))
        .and_then(|v| v.as_str())
        .map(|s| {
            let url = s.to_string();
            let target_res = "=w800-h800-l90-rj-c";

            if let Some(pos) = url.find('=') {
                return format!("{}{}", &url[..pos], target_res);
            }

            if url.contains("/s") {
                return url
                    .split('/')
                    .map(|part| {
                        if part.starts_with('s')
                            && part.chars().nth(1).map_or(false, |c| c.is_ascii_digit())
                        {
                            "s800"
                        } else {
                            part
                        }
                    })
                    .collect::<Vec<&str>>()
                    .join("/");
            }

            format!("{}{}", url, target_res)
        })
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
