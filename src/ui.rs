use crate::api::SongDetails;
use crate::player;
use colored::*;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

static SONG_MONITOR: RwLock<Option<Arc<AtomicBool>>> = RwLock::new(None);

pub fn load_banner(song_name_opt: Option<&str>, queue: &[String]) {
    let mut output = String::new();
    output.push_str("\x1B[1;1H");
    // clear avoiding now playing
    for _ in 0..12 {
        output.push_str("\x1B[2K\n");
    }
    output.push_str("\x1B[2B");
    output.push_str("\x1B[J");
    output.push_str("\x1B[1;1H");
    output.push_str(&format!(
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
    ));

    output.push_str("\n\n\n");

    output.push_str(&format!(
        "\n\n{}\n",
        "-----QUEUE-----".bright_yellow().bold()
    ));

    // Queue items
    if !queue.is_empty() {
        for (i, name) in queue.iter().enumerate().take(7) {
            output.push_str(&format!("{}. {}\n", i + 1, name));
        }
    } else {
        output.push_str("(Empty)\n");
    }

    output.push_str(&format!(
        "\n\n{}",
        "> Search / Command: ".bright_blue().bold()
    ));

    // print buffer together
    print!("{}", output);
    std::io::stdout().flush().unwrap();

    // Handle monitor thread
    if let Some(song_name) = song_name_opt {
        //clear now playing area when new song
        print!("\x1b[s");
        print!("\x1b[13;1H\x1b[2K");
        print!("\x1b[14;1H\x1b[2K");
        print!("\x1b[u");
        if !song_name.is_empty() {
            let mut monitor_guard = SONG_MONITOR.write().unwrap();
            if let Some(stop_signal) = monitor_guard.take() {
                stop_signal.store(true, Ordering::Relaxed);
            }
            let new_stop = start_monitor_thread(song_name.to_string());
            *monitor_guard = Some(new_stop);
        } else {
            let mut monitor_guard = SONG_MONITOR.write().unwrap();
            if let Some(stop_signal) = monitor_guard.take() {
                stop_signal.store(true, Ordering::Relaxed);
            }
        }
    }
}

fn start_monitor_thread(name: String) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();

    thread::spawn(move || {
        let mut stdout = std::io::stdout();
        while !stop_clone.load(Ordering::Relaxed) {
            let (curr, tot) = player::get_time_info().unwrap_or((0.0, 0.0));
            let fmt = |s: f64| format!("{:02}:{:02}", (s / 60.0) as u64, (s % 60.0) as u64);

            print!(
                "\x1B7\x1B[13;0H\x1B[2K{} [{} / {}] {}\x1B8",
                "▶︎".bright_green(),
                fmt(curr).cyan(),
                fmt(tot).cyan(),
                name.white().bold()
            );
            let _ = stdout.flush();
            thread::sleep(Duration::from_millis(1000));
        }
    });
    stop
}

pub fn show_songs(list: &[SongDetails]) {
    println!("");
    for (i, s) in list.iter().enumerate() {
        println!("{}. {} [{}]", i + 1, s.title, s.duration.cyan().italic());
    }
    println!("{}", "~ Select (1-5): ".bright_blue().bold().blink());
}
