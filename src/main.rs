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
use std::sync::mpsc;
use yt_search::{SearchFilters, VideoResult, YouTubeSearch};

/// -------------------------------------------------------------------
/// MAIN APPLICATION
/// -------------------------------------------------------------------
static SONG_MONITOR: RwLock<Option<Arc<AtomicBool>>> = RwLock::new(None);
static SONG_QUEUE: RwLock<Vec<(String, String)>> = RwLock::new(Vec::new());

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let music_dir = prepare_music_dir()?;
    let client = build_client();
    let mut currently_playing: Option<Child> = None;

    //transmitter (sends any input for Search/Command)
    //reciever (running every 250 ms.. checks if song ended plays next in queue if yes)
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        loop {
            let mut s = String::new();
            if std::io::stdin().read_line(&mut s).is_ok() {
                let _ = tx.send(s.trim().to_string());
            }
        }
    });

    load_banner("Nothing Playing");
    std::io::stdout().flush()?;

    loop {
        //firstly checks if song is playing
        if let Some(child) = &mut currently_playing {
            if let Ok(Some(_)) = child.try_wait() {
                currently_playing = None;
                if let Some((src, name)) = queue_next() {
                    currently_playing = Some(play_file(&src, &name, &music_dir)?);
                    load_banner(&name);
                } else {
                    load_banner("Nothing Playing");
                }
                load_banner("");
                std::io::stdout().flush()?;
            }
        }

        //check for any input from tx
        let input = match rx.try_recv() {
            Ok(s) => s,
            //if nothing sleep for 250 ms for cpu relief (not the best idea)
            Err(_) => {
                thread::sleep(Duration::from_millis(250));
                continue;
            }
        };
        //if empty query play local (add to queue if something playing)
        if input.is_empty() {
            if let Some((path, name)) = shuffle_play(&music_dir)? {
                if currently_playing.is_some() {
                    queue_add(path, name);
                    load_banner("");
                } else {
                    currently_playing = Some(play_file(&path, &name, &music_dir)?);
                    load_banner(&name);
                }
            } else {
                println!("No local songs found.");
            }
            continue;
        }

        // special commands
        match input.to_lowercase().as_str() {
            "exit" => {
                stop_process(&mut currently_playing);
                load_banner("");
                break;
            }
            "stop" => {
                stop_process(&mut currently_playing);
                load_banner("Nothing Playing");
                load_banner("");
                std::io::stdout().flush()?;
                continue;
            }
            "c" | "clear" => {
                {
                    let mut q = SONG_QUEUE.write().unwrap();
                    q.clear();
                }
                load_banner("");
                continue;
            }
            "p" | "pause" => {
                if currently_playing.is_some() {
                    toggle_pause();
                    load_banner("");
                    std::io::stdout().flush()?;
                }
                continue;
            }
            "n" | "next" => {
                stop_process(&mut currently_playing);
                if let Some((p, name)) = queue_next() {
                    currently_playing = Some(play_file(&p, &name, &music_dir)?);
                    load_banner(&name);
                } else {
                    load_banner("Nothing Playing");
                }
                load_banner("");
                std::io::stdout().flush()?;
                continue;
            }
            _ => {}
        }

        if input.starts_with('>') || input.starts_with('<') {
            if let Ok(s) = input[1..].trim().parse::<i64>() {
                seek(if input.starts_with('<') { -s } else { s });
            }
            load_banner("");
            std::io::stdout().flush()?;
            continue;
        }

        let songs = search_songs(&client, &input).await?;
        if songs.is_empty() {
            println!("No results.");
            load_banner("");
            std::io::stdout().flush()?;
            continue;
        }

        show_songs(&songs);
        print!("{}", "> Select (1-5): ".bright_white().bold());
        std::io::stdout().flush()?;
        //check if q is entered before index
        if let Ok(sel) = rx.recv() {
            let (idx, is_queue) = if sel.to_lowercase().starts_with('q') {
                (sel[1..].parse::<usize>().unwrap_or(0), true)
            } else {
                (sel.parse::<usize>().unwrap_or(0), false)
            };

            if idx >= 1 && idx <= songs.len() {
                let selected = &songs[idx - 1];
                let path = music_dir.join(format!("{}.webm", selected.title));
                let src = if path.exists() {
                    path.to_string_lossy().to_string()
                } else {
                    fetch_stream_url(&client, selected).await?
                };

                if is_queue {
                    queue_add(src, selected.title.clone());
                } else {
                    stop_process(&mut currently_playing);
                    currently_playing = Some(play_file(&src, &selected.title, &music_dir)?);
                    load_banner(&selected.title);
                }
            } else {
                println!("Invalid index.");
            }
        }
        load_banner("");
        std::io::stdout().flush()?;
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
    //print queue along with banner
    queue_show();
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
    print!("{}", "\n\n> Search / Command: ".bright_blue().bold());
}

fn show_songs(list: &[VideoResult]) {
    println!();
    for (i, s) in list.iter().enumerate() {
        println!("{}: {} ({})", i + 1, s.title, s.duration);
    }
}

/// -------------------------------------------------------------------
/// QUEUE & MPV IPC & PLAYBACK
/// -------------------------------------------------------------------

fn queue_add(source: String, name: String) {
    let mut q = SONG_QUEUE.write().unwrap();
    println!("Added to queue: {}", name.green());
    q.push((source, name));
}

fn queue_next() -> Option<(String, String)> {
    let mut q = SONG_QUEUE.write().unwrap();
    if q.is_empty() {
        None
    } else {
        Some(q.remove(0))
    }
}

fn queue_show() {
    let q = SONG_QUEUE.read().unwrap();
    println!("\n\n{}", "-----QUEUE-----".bright_yellow().bold(),);

    for (i, (_, name)) in q.iter().enumerate() {
        println!("{}. {}", i + 1, name);
    }
}

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

fn play_file(
    source: &str,
    title: &str,
    music_dir: &PathBuf,
) -> Result<Child, Box<dyn std::error::Error>> {
    let ipc = get_ipc_path();
    #[cfg(unix)]
    let _ = std::fs::remove_file(&ipc);

    let mut cmd = Command::new("mpv");
    cmd.arg("--no-video")
        .arg("--really-quiet")
        .arg("--force-window=no")
        .arg(format!("--input-ipc-server={}", ipc));

    //if streaming... record to file simultaneously
    if source.starts_with("http") {
        let file_path = music_dir.join(format!("{}.webm", title));
        cmd.arg(format!("--stream-record={}", file_path.to_string_lossy()));
    }

    cmd.arg(source).stdout(Stdio::null()).stderr(Stdio::null());

    Ok(cmd.spawn()?)
}

fn stop_process(proc: &mut Option<Child>) {
    if let Some(mut child) = proc.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn shuffle_play(dir: &PathBuf) -> Result<Option<(String, String)>, Box<dyn std::error::Error>> {
    use rand::seq::IndexedRandom;

    let entries = std::fs::read_dir(dir)?;
    let songs: Vec<_> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |e| e == "webm"))
        .collect();

    if songs.is_empty() {
        return Ok(None);
    }

    let s = songs.choose(&mut rand::rng()).unwrap();

    let name = s
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let path = s.to_string_lossy().to_string();

    Ok(Some((path, name)))
}

/// -------------------------------------------------------------------
/// NETWORK & SEARCH
/// -------------------------------------------------------------------
async fn search_songs(
    _client: &Client,
    q: &str,
) -> Result<Vec<VideoResult>, Box<dyn std::error::Error>> {
    let yt = YouTubeSearch::new(None, true)?;
    let res = yt
        .search(
            &format!("{} single songs", q),
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
    println!("\nFetching URL...");
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
    d.push("whytui");
    std::fs::create_dir_all(&d)?;
    Ok(d)
}
