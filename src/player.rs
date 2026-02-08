use crate::Track;
use crate::config;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use lofty::tag::Tag;
use serde_json::json;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::Ordering;
use std::time::Duration;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

pub fn play_file(
    source: &str,
    _track: &Track,
    _music_dir: &PathBuf,
) -> Result<Child, Box<dyn std::error::Error>> {
    let ipc = get_ipc_path();
    let current_vol = crate::VOLUME.load(Ordering::Relaxed);

    crate::IS_PLAYING.store(true, Ordering::SeqCst);

    #[cfg(unix)]
    let _ = std::fs::remove_file(&ipc);

    if source.contains(".tidal") || source.ends_with(".flac") || source.ends_with(".mpd") {
        crate::PLAYING_LOSSLESS.store(true, Ordering::SeqCst);
    } else {
        crate::PLAYING_LOSSLESS.store(false, Ordering::SeqCst);
    }
let user_agent = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36,gzip(gfe)";
    let mut cmd = Command::new("mpv");
    cmd.arg("--no-video")
        .arg("--really-quiet")
        .arg("--force-window=no")
        .arg(format!("--input-ipc-server={}", ipc))
        .arg(format!("--volume={}", current_vol))
        .arg("--demuxer-lavf-o=protocol_whitelist=[file,http,https,tcp,tls,crypto,data]")
        .arg(format!("--user-agent={}", user_agent))
        .arg("--http-header-fields=Referer: https://music.youtube.com/,Origin: https://music.youtube.com")
        .arg(source)
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    Ok(cmd.spawn()?)
}

pub fn background_download(
    source: &str,
    track: &Track,
    music_dir: &PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let ext = if source.contains(".tidal") || source.ends_with(".flac") || source.ends_with(".mpd")
    {
        "flac"
    } else {
        "opus"
    };

    let file_name =
        the_naming_format_in_which_i_have_saved_the_track_locally(&track.title, &track.artists);

    let temp_path = music_dir
        .join("temp")
        .join(format!("{}.{}", file_name, ext));
    let final_path = music_dir.join(format!("{}.{}", file_name, ext));

    if final_path.exists() {
        return Ok(());
    }

    if let Some(parent) = temp_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut ffmpeg = Command::new("ffmpeg")
        .arg("-y")
        .arg("-protocol_whitelist")
        .arg("file,http,https,tcp,tls,crypto,data")
        .arg("-i")
        .arg(source)
        .arg("-vn")
        .arg("-c")
        .arg("copy")
        .arg(&temp_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let status = ffmpeg.wait()?;
    if !status.success() {
        return Err("Download failed".into());
    }

    let mut tagged_file = lofty::read_from_path(&temp_path)?;
    let tag = if let Some(t) = tagged_file.primary_tag_mut() {
        t
    } else {
        let tag_type = tagged_file.primary_tag_type();
        tagged_file.insert_tag(Tag::new(tag_type));
        tagged_file
            .primary_tag_mut()
            .ok_or("Could not create tags")?
    };

    tag.set_title(track.title.clone());

    tag.set_artist(track.artists.join(", "));

    if !track.album.is_empty() {
        tag.set_album(track.album.clone());
    }

    if let Some(url) = &track.thumbnail_url {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;

        if let Ok(resp) = client.get(url).send() {
            if let Ok(data) = resp.bytes() {
                tag.push_picture(Picture::new_unchecked(
                    PictureType::CoverFront,
                    Some(MimeType::Jpeg),
                    None,
                    data.to_vec(),
                ));
            }
        }
    }

    tag.save_to_path(&temp_path, lofty::config::WriteOptions::default())?;

    std::fs::rename(&temp_path, &final_path)?;

    Ok(())
}

pub fn stop_process(proc: &mut Option<Child>, _song_name: &str, _music_dir: &PathBuf) {
    crate::IS_PLAYING.store(false, Ordering::SeqCst);

    if let Some(mut child) = proc.take() {
        #[cfg(target_os = "windows")]
        {
            let _ = Command::new("taskkill")
                .args(["/F", "/T", "/PID", &child.id().to_string()])
                .creation_flags(0x08000000)
                .output();
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

pub fn prepare_music_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let mut d = dirs::audio_dir().ok_or("No audio dir")?;
    d.push("whytui");
    fs::create_dir_all(&d)?;
    fs::create_dir_all(d.join("temp"))?;
    let config_dir = d.join("config");
    fs::create_dir_all(&config_dir)?;
    let cookies_path = config_dir.join("cookies.txt");
    if !cookies_path.exists() {
        File::create(&cookies_path)?;
    }
    Ok(d)
}

pub fn clear_temp(music_dir: &PathBuf) {
    let temp_dir = music_dir.join("temp");
    if let Ok(entries) = std::fs::read_dir(&temp_dir) {
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.is_file() {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }
}

pub fn get_ipc_path() -> String {
    if cfg!(unix) {
        "/tmp/whytui.sock".to_string()
    } else {
        r"\\.\pipe\whytui.sock".to_string()
    }
}

pub fn send_ipc(cmd: serde_json::Value) -> Option<String> {
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

    #[cfg(windows)]
    {
        use std::fs::OpenOptions;
        if let Ok(mut file) = OpenOptions::new().read(true).write(true).open(&path) {
            let _ = file.write_all(msg.as_bytes());
            let _ = file.flush();
            let mut reader = BufReader::new(&file);
            let mut resp = String::new();
            if reader.read_line(&mut resp).is_ok() {
                return Some(resp);
            }
        }
    }
    None
}

pub fn get_time_info() -> Option<(f64, f64)> {
    let get = |p| {
        send_ipc(json!({"command": ["get_property", p]}))
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v["data"].as_f64())
    };
    Some((get("time-pos")?, get("duration")?))
}

pub fn toggle_pause() {
    send_ipc(json!({"command": ["cycle", "pause"]}));
}

pub fn seek(s: i64) {
    send_ipc(json!({"command": ["seek", s, "relative"]}));
}

pub fn vol_change(s: i64) {
    send_ipc(json!({ "command": ["add", "volume", s] }));
}

pub fn the_naming_format_in_which_i_have_saved_the_track_locally(
    title: &str,
    artists: &[String],
) -> String {
    let safe_title = title.replace(['/', '\\'], "-");
    let primary_artist = artists
        .get(0)
        .map(|s| s.replace(['/', '\\'], "-"))
        .unwrap_or_else(|| "Unknown".to_string());
    format!("{} - {}", safe_title, primary_artist)
}
