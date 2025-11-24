use colored::*;
use reqwest::Client;
use reqwest::header::{CONNECTION, HeaderMap, HeaderValue, ORIGIN, REFERER, USER_AGENT};
use serde_json::json;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use yt_search::{SearchFilters, VideoResult, YouTubeSearch};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Save songs to ~/Music/ytcli-songs
    let mut music_dir = dirs::home_dir().ok_or("Cannot find home directory")?;
    music_dir.push("Music/ytcli-songs");
    std::fs::create_dir_all(&music_dir)?;

    let client = build_client();
    let mut current_process: Option<Child> = None;
    load_banner(true);

    loop {
        let query = prompt_input("\n============\nSearch Song:\n============");
        if query.trim().is_empty() {
            println!("Playing offline song...");
            if let Some(mut child) = current_process.take() {
                let _ = child.kill();
                let _ = child.wait();
            }

            // start new shuffled song
            current_process = shuffle_playback(&music_dir);
            continue;
        }
        if query.eq_ignore_ascii_case("exit") {
            if let Some(mut child) = current_process {
                let _ = child.kill();
            }
            break;
        }
        if query.eq_ignore_ascii_case("stop") {
            if let Some(mut child) = current_process.take() {
                let _ = child.kill();
                println!("Music stopped.");
            }
            continue;
        }

        let songs = search_songs(&query).await?;
        if songs.is_empty() {
            println!("No results.");
            continue;
        }

        display_songs(&songs);
        let choice = prompt_input(&format!(
            "\n=============\nSelect (1-{}):\n=============",
            songs.len()
        ))
        .trim()
        .parse::<usize>()
        .unwrap_or(0);
        if choice == 0 || choice > songs.len() {
            continue;
        }

        let selected_song = &songs[choice - 1];
        let mut file_path = music_dir.clone();
        file_path.push(format!("{}.webm", selected_song.title));

        download_if_needed(&client, selected_song, &file_path).await?;

        if let Some(mut old_child) = current_process.take() {
            let _ = old_child.kill();
            let _ = old_child.wait();
        }

        // println!("\n▶︎ Playing: {}", selected_song.title);
        current_process = Some(play_song(&file_path)?);
    }

    Ok(())
}

// ------------------ FUNCTIONS ------------------

fn build_client() -> Client {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36"));
    headers.insert(
        REFERER,
        HeaderValue::from_static("https://www.youtube.com/"),
    );
    headers.insert(ORIGIN, HeaderValue::from_static("https://www.youtube.com"));
    headers.insert(CONNECTION, HeaderValue::from_static("keep-alive"));

    Client::builder().default_headers(headers).build().unwrap()
}

async fn search_songs(query: &str) -> Result<Vec<VideoResult>, Box<dyn std::error::Error>> {
    let search = YouTubeSearch::new(None, true)?;
    let results = search
        .search(
            &format!("{} official audio", query),
            SearchFilters {
                sort_by: None,
                duration: None,
            },
        )
        .await?;
    Ok(results.into_iter().take(5).collect())
}

fn display_songs(songs: &[VideoResult]) {
    println!();
    for (i, song) in songs.iter().enumerate() {
        println!("{}: {} [{}]", i + 1, song.title, song.duration);
    }
}

fn prompt_input(message: &str) -> String {
    println!("{}", message);
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
    input.trim().to_string()
}

fn load_banner(first: bool) {
    use std::{
        io::{Write, stdout},
        thread,
        time::Duration,
    };

    if !first {
        thread::sleep(Duration::from_millis(1000));
    }
    print!("\x1B[2J\x1B[1;1H\n");
    stdout().flush().unwrap();
    println!(
        "{}",
        r#"
   =============================================

    ██╗   ██╗████████╗        ██████╗██╗     ██╗
    ╚██╗ ██╔╝╚══██╔══╝       ██╔════╝██║     ██║
     ╚████╔╝    ██║   █████╗ ██║     ██║     ██║
      ╚██╔╝     ██║   ╚════╝ ██║     ██║     ██║
       ██║      ██║          ╚██████╗███████╗██║
       ╚═╝      ╚═╝           ╚═════╝╚══════╝╚═╝

        𝕓𝕪:𝕤𝕙𝕒𝟛
   =============================================
"#
        .bright_red()
    );
}

async fn download_if_needed(
    client: &Client,
    song: &VideoResult,
    file_path: &PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    if file_path.exists() {
        println!("Song found in cache.");
        return Ok(());
    }

    println!("Downloading metadata...");
    let payload = json!({
        "context": { "client": { "clientName": "ANDROID", "clientVersion": "19.09.37" } },
        "videoId": song.video_id
    });
    let res = client
        .post("https://www.youtube.com/youtubei/v1/player")
        .json(&payload)
        .send()
        .await?;
    let data: serde_json::Value = res.json().await?;
    let formats = data["streamingData"]["adaptiveFormats"]
        .as_array()
        .expect("Format Error");

    let mut best_url = String::new();
    let mut best_bitrate = 0;
    let mut total_size: u64 = 0;

    for f in formats {
        if let Some(mime) = f["mimeType"].as_str() {
            if mime.starts_with("audio/webm") {
                let bitrate = f["bitrate"].as_i64().unwrap_or(0);
                if bitrate > best_bitrate {
                    best_bitrate = bitrate;
                    best_url = f["url"].as_str().unwrap_or("").to_string();
                    total_size = f["contentLength"]
                        .as_str()
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or(0);
                }
            }
        }
    }

    if total_size == 0 {
        println!("Error: No size.");
        return Ok(());
    }

    println!(
        "Downloading Highest Quality -> {} kbps...",
        best_bitrate / 1000
    );
    let start_last = total_size * 3 / 4;
    let end_last = total_size - 1;

    let last_bytes = client
        .get(&best_url)
        .header("Range", format!("bytes={}-{}", start_last, end_last))
        .send()
        .await?
        .bytes()
        .await?;
    let first_bytes = client
        .get(&best_url)
        .header("Range", format!("bytes=0-{}", start_last - 1))
        .send()
        .await?
        .bytes()
        .await?;

    let mut file = File::create(file_path).await?;
    file.write_all(&first_bytes).await?;
    file.write_all(&last_bytes).await?;
    file.flush().await?;
    println!("Download Complete");
    Ok(())
}

fn shuffle_playback(music_dir: &PathBuf) -> Option<Child> {
    use rand::rng;
    use rand::seq::IndexedRandom;

    let entries = std::fs::read_dir(music_dir).ok()?;
    let mut songs = Vec::new();

    for e in entries.flatten() {
        let path = e.path();
        if let Some(ext) = path.extension() {
            if ext == "webm" {
                songs.push(path);
            }
        }
    }

    if songs.is_empty() {
        println!("No local songs found.");
        return None;
    }

    let mut rng = rng();
    let song = songs.choose(&mut rng)?;
    play_song(song).ok()
}

fn play_song(file_path: &PathBuf) -> Result<Child, Box<dyn std::error::Error>> {
    load_banner(false);
    if let Some(name) = file_path.file_stem() {
        println!(
            "\n{} {}",
            "▶︎ Playing:".bright_green().bold(),
            name.to_string_lossy().bright_white()
        );
    }
    let child = Command::new("ffplay")
        .arg("-nodisp")
        .arg("-autoexit")
        .arg("-hide_banner")
        .arg(file_path.to_str().unwrap())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(child)
}
