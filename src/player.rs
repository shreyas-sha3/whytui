use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub fn prepare_music_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let mut d = dirs::audio_dir().ok_or("No audio dir")?;
    d.push("whytui");
    std::fs::create_dir_all(&d)?;
    std::fs::create_dir_all(d.join("temp"))?;
    Ok(d)
}

pub fn play_file(
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
        let temp_path = music_dir.join("temp").join(format!("{}.webm", title));
        cmd.arg(format!("--stream-record={}", temp_path.to_string_lossy()));
    }

    cmd.arg(source).stdout(Stdio::null()).stderr(Stdio::null());

    Ok(cmd.spawn()?)
}

pub fn stop_process(proc: &mut Option<Child>, song_name: &str, music_dir: &PathBuf) {
    if let Some(mut child) = proc.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    // Delete partial download
    if !song_name.is_empty() {
        let temp = music_dir.join("temp").join(format!("{}.webm", song_name));
        if temp.exists() {
            std::fs::remove_file(temp).ok();
        }
    }
}

pub fn get_ipc_path() -> String {
    if cfg!(unix) {
        "/tmp/ytcli.sock".to_string()
    } else {
        r"\\.\pipe\ytcli.sock".to_string()
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
