use colored::*;
use reqwest::Client;
use reqwest::header::{CONNECTION, HeaderMap, HeaderValue, ORIGIN, REFERER, USER_AGENT};
use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;
// use tokio::fs::File;
// use tokio::io::AsyncWriteExt;
use yt_search::{SearchFilters, VideoResult, YouTubeSearch};

/// -------------------------------------------------------------------
/// MAIN APPLICATION
/// -------------------------------------------------------------------
static SONG_MONITOR: RwLock<Option<Arc<AtomicBool>>> = RwLock::new(None);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let music_dir = prepare_music_dir()?;
    let client = build_client();
    let mut currently_playing: Option<Child> = None;

    load_banner("Nothing Playing");

    loop {
        let input = prompt("\n\n> Search / Command: ");

        if input.trim().is_empty() {
            stop_process(&mut currently_playing);
            if let Some((child, name)) = shuffle_play(&music_dir)? {
                currently_playing = Some(child);
                load_banner(&name);
            } else {
                println!("No local songs found to shuffle.");
            }
            continue;
        }

        // special commands
        match input.as_str() {
            "exit" => {
                stop_process(&mut currently_playing);
                load_banner("");
                break;
            }
            "stop" => {
                stop_process(&mut currently_playing);
                load_banner("Nothing Playing");
                continue;
            }
            "p" | "pause" => {
                if currently_playing.is_some() {
                    toggle_pause();
                    load_banner("");
                }
                continue;
            }
            _ => {}
        }

        if input.starts_with(">") {
            if let Ok(s) = input[1..].trim().parse::<i64>() {
                seek(s);
                load_banner("");
            }
            continue;
        }
        if input.starts_with("<") {
            if let Ok(s) = input[1..].trim().parse::<i64>() {
                seek(-s);
                load_banner("");
            }
            continue;
        }

        // search if not a command
        let songs = search_songs(&client, &input).await?;
        if songs.is_empty() {
            println!("No results.");
            continue;
        }

        show_songs(&songs);
        let choice = prompt("Select (1-5): ")
            .trim()
            .parse::<usize>()
            .unwrap_or(0);

        if choice == 0 || choice > songs.len() {
            continue;
        }

        let selected = &songs[choice - 1];
        let file_path = music_dir.join(format!("{}.webm", selected.title));

        stop_process(&mut currently_playing);

        // Play cached OR stream
        if file_path.exists() {
            currently_playing = Some(play_file(file_path.to_str().unwrap())?);
        } else {
            let url = fetch_stream_url(&client, selected).await?;
            currently_playing = Some(play_file(&url)?);
        }
        load_banner(&selected.title);
    }
    Ok(())
}

/// -------------------------------------------------------------------
/// UI & MONITORING THREAD
/// -------------------------------------------------------------------
fn start_monitor_thread(name: String) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();

    thread::spawn(move || {
        let mut stdout = std::io::stdout();
        while !stop_clone.load(Ordering::Relaxed) {
            // get time info fom ipc
            let (curr, tot) = get_time_info().unwrap_or((0.0, 0.0));
            let fmt = |s: f64| format!("{:02}:{:02}", (s / 60.0) as u64, (s % 60.0) as u64);

            //ANSI bs
            print!(
                "\x1B7\x1B[14;0H\x1B[2K{} [{} / {}] {}\x1B8",
                "▶︎".bright_green(),
                fmt(curr),
                fmt(tot),
                name.white().bold()
            );
            let _ = stdout.flush();
            thread::sleep(Duration::from_millis(1000));
        }
    });
    stop
}

fn load_banner(song_name: &str) {
    print!("\x1B[2J\x1B[1;1H\n");
    std::io::stdout().flush().unwrap();

    println!(
        "{}",
        r#"
    ==========================================================
    ██╗    ██╗ ██╗  ██╗ ██╗    ██╗    ████████╗ ██╗   ██╗ ██╗
    ██║    ██║ ██║  ██║ ╚██╗ ██╔╝     ╚══██╔══╝ ██║   ██║ ██║
    ██║ █╗ ██║ ███████║  ╚████╔╝ █████╗  ██║    ██║   ██║ ██║
    ██║███╗██║ ██╔══██║   ╚██╔╝  ╚════╝  ██║    ██║   ██║ ██║
    ╚███╔███╔╝ ██║  ██║    ██║           ██║    ╚██████╔╝ ██║
     ╚══╝╚══╝  ╚═╝  ╚═╝    ╚═╝           ╚═╝     ╚═════╝  ╚═╝
    𝕓𝕪:𝕤𝕙𝕒𝟛
    ==========================================================
    "#
        .truecolor(255, 126, 131)
    );
    println!("\n");

    if !song_name.is_empty() {
        // SONG MONITORING MANAGEMENT
        {
            let mut monitor_guard = SONG_MONITOR.write().unwrap();

            if let Some(stop_signal) = monitor_guard.take() {
                stop_signal.store(true, Ordering::Relaxed);
            }
            let new_stop = start_monitor_thread(song_name.to_string());
            *monitor_guard = Some(new_stop);
        }
    }
}

fn prompt(msg: &str) -> String {
    print!("{}", msg);
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    std::io::stdin().read_line(&mut s).unwrap();
    s.trim().to_string()
}

fn show_songs(list: &[VideoResult]) {
    println!();
    for (i, s) in list.iter().enumerate() {
        println!("{}: {} ({})", i + 1, s.title, s.duration);
    }
}

/// -------------------------------------------------------------------
/// MPV IPC & PLAYBACK
/// -------------------------------------------------------------------
fn get_ipc_path() -> String {
    "/tmp/ytcli.sock".to_string()
}

fn send_ipc(cmd: serde_json::Value) -> Option<String> {
    let path = get_ipc_path();
    let msg = format!("{}\n", cmd.to_string());

    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        if let Ok(mut stream) = UnixStream::connect(&path) {
            let _ = stream.write_all(msg.as_bytes());
            let _ = stream.flush();
            let mut reader = BufReader::new(&stream);
            let mut resp = String::new();
            stream
                .set_read_timeout(Some(Duration::from_millis(200)))
                .ok();
            if reader.read_line(&mut resp).is_ok() {
                return Some(resp);
            }
        }
    }
    None
}

fn get_time_info() -> Option<(f64, f64)> {
    let get = |p| {
        send_ipc(json!({"command": ["get_property", p]}))
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v["data"].as_f64())
    };
    Some((get("time-pos")?, get("duration")?))
}

fn toggle_pause() {
    send_ipc(json!({"command": ["cycle", "pause"]}));
}
fn seek(s: i64) {
    send_ipc(json!({"command": ["seek", s, "relative"]}));
}

fn play_file(source: &str) -> Result<Child, Box<dyn std::error::Error>> {
    let ipc = get_ipc_path();
    #[cfg(unix)]
    let _ = std::fs::remove_file(&ipc);

    let child = Command::new("mpv")
        .arg("--no-video")
        .arg("--really-quiet")
        .arg("--force-window=no")
        .arg(format!("--input-ipc-server={}", ipc))
        .arg(source)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(child)
}

fn stop_process(proc: &mut Option<Child>) {
    if let Some(mut child) = proc.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn shuffle_play(dir: &PathBuf) -> Result<Option<(Child, String)>, Box<dyn std::error::Error>> {
    use rand::seq::IndexedRandom;
    let entries = std::fs::read_dir(dir).ok().unwrap();
    let songs: Vec<_> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |e| e == "webm"))
        .collect();

    if songs.is_empty() {
        println!("No local songs.");
        return Ok(None);
    }

    let s = songs.choose(&mut rand::rng()).unwrap();
    let name = s
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    Ok(Some((play_file(s.to_str().unwrap())?, name)))
}

/// -------------------------------------------------------------------
/// NETWORK & SEARCH
/// -------------------------------------------------------------------
async fn search_songs(
    client: &Client,
    q: &str,
) -> Result<Vec<VideoResult>, Box<dyn std::error::Error>> {
    let yt = YouTubeSearch::new(None, true)?;
    let res = yt
        .search(
            &format!("{} official audio", q),
            SearchFilters {
                sort_by: None,
                duration: None,
            },
        )
        .await?;
    Ok(res.into_iter().take(5).collect())
}

async fn fetch_stream_url(
    client: &Client,
    song: &VideoResult,
) -> Result<String, Box<dyn std::error::Error>> {
    println!("Fetching URL...");
    let payload = json!({ "context": { "client": { "clientName": "ANDROID", "clientVersion": "19.09.37" }}, "videoId": song.video_id });
    let res = client
        .post("https://www.youtube.com/youtubei/v1/player")
        .json(&payload)
        .send()
        .await?;
    let data: serde_json::Value = res.json().await?;

    let formats = data["streamingData"]["adaptiveFormats"]
        .as_array()
        .ok_or("No formats")?;
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
        .ok_or("No URL")?;

    Ok(best.to_string())
}

fn build_client() -> Client {
    let mut h = HeaderMap::new();

    h.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0"));
    h.insert(
        REFERER,
        HeaderValue::from_static("https://www.youtube.com/"),
    );
    h.insert(ORIGIN, HeaderValue::from_static("https://www.youtube.com"));
    h.insert(CONNECTION, HeaderValue::from_static("keep-alive"));
    Client::builder().default_headers(h).build().unwrap()
}

fn prepare_music_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let mut d = dirs::audio_dir().ok_or("No audio dir")?;
    d.push("ytcli-songs");
    std::fs::create_dir_all(&d)?;
    Ok(d)
}
