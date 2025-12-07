use crate::api::LrcLine;
use crate::api::SongDetails;
use crate::api::fetch_synced_lyrics;
use crate::api::split_title_artist;
use crate::player;
use colored::*;
use crossterm::{
    cursor, queue,
    style::Print,
    terminal::{self, ClearType},
};
use std::io::{Write, stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

static SONG_MONITOR: RwLock<Option<Arc<AtomicBool>>> = RwLock::new(None);

static LYRICS: RwLock<Vec<LrcLine>> = RwLock::new(Vec::new());
const _BANNER_HEIGHT: u16 = 11;
const STATUS_LINE_ROW: u16 = 12;

pub fn load_banner(song_name_opt: Option<&str>, queue: &[String]) {
    let mut stdout = stdout();

    queue!(
        stdout,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0)
    )
    .unwrap();

    let banner_art = format!(
        "{}",
        r#"
 █     █░ ██░ ██▓ ██   ██▓ ▄███████▓ █    ██  ██▓
▓█░ █ ░█░▓██░ ██▒ ▒██  ██▒▓   ██▒ ▓▒ ██  ▓██ ▒▓██▒
▒█░ █ ░█ ▒██▀▀██░  ▒██ ██░▒  ▓██░ ▒░▓██  ▒██ ░▒██▒
░█░ █ ░█ ░▓█ ░██   ░ ▐██▓░░  ▓██▓ ░ ▓▓█  ░██ ░░██░
░░██▒██▓ ░▓█▒░██▓  ░ ██▒▓░   ▒██▒ ░ ▒▒█████▓  ░██░
░ ▓░▒ ▒   ▒ ░░▒░▒   ██▒▒▒    ▒ ░░   ░▒▓▒ ▒ ▒  ░▓
  ▒ ░ ░   ▒ ░▒░ ░ ▓██ ░▒░      ░    ░░▒░ ░ ░   ▒ ░
  ░   ░   ░  ░░ ░ ▒ ▒ ░░     ░       ░░░ ░  ░  ▒ ░
    ░     ░  ░  ░ ░ ░                  ░       ░
                ░  ░
"#
        .blue()
        .dimmed()
    );

    queue!(stdout, Print(banner_art), Print("\n\n")).unwrap();

    queue!(
        stdout,
        Print(format!(
            "\n\n\n\n{}\n",
            "¨˜ˆ”°⍣~•QUEUE•~⍣°”ˆ˜¨".bright_cyan().bold()
        ))
    )
    .unwrap();

    if !queue.is_empty() {
        for (i, name) in queue.iter().enumerate().take(7) {
            // shorten names?
            let safe_name = if name.len() > 70 {
                format!("{}...", &name[..69])
            } else {
                name.clone()
            };
            queue!(stdout, Print(format!("{}. {}\n", i + 1, safe_name))).unwrap();
        }
    } else {
        queue!(stdout, Print("\t  ~\n")).unwrap();
    }

    queue!(
        stdout,
        Print(format!(
            "\n\n{}",
            "> Search / Command: ".bright_blue().bold()
        ))
    )
    .unwrap();

    stdout.flush().unwrap();

    if let Some(song_name) = song_name_opt {
        let mut monitor_guard = SONG_MONITOR.write().unwrap();
        if let Some(stop_signal) = monitor_guard.take() {
            stop_signal.store(true, Ordering::Relaxed);
        }

        if !song_name.is_empty() {
            queue!(
                stdout,
                cursor::SavePosition,
                cursor::MoveTo(0, STATUS_LINE_ROW),
                terminal::Clear(ClearType::CurrentLine),
                cursor::MoveTo(0, STATUS_LINE_ROW + 1),
                terminal::Clear(ClearType::CurrentLine),
                cursor::RestorePosition
            )
            .unwrap();
            stdout.flush().unwrap();

            let new_stop = start_monitor_thread(song_name.to_string());
            *monitor_guard = Some(new_stop);
        }
    }
}

fn start_monitor_thread(name: String) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();

    // clear prev lyrics
    {
        let mut w = LYRICS.write().unwrap();
        w.clear();
        if name != "Nothing Playing" {
            w.push(LrcLine {
                timestamp: Duration::from_secs(0),
                text: "Searching Lyrics...".dimmed().to_string(),
            });
        } else {
            w.push(LrcLine {
                timestamp: Duration::from_secs(0),
                text: "Play a song first...".dimmed().to_string(),
            });
        }
    }

    // async fetch lyrics
    if name != "Nothing Playing" {
        let song_name = name.clone();
        tokio::spawn(async move {
            match fetch_synced_lyrics(&song_name).await {
                Ok(parsed) => {
                    if parsed.is_empty() {
                        *LYRICS.write().unwrap() = vec![LrcLine {
                            timestamp: Duration::from_secs(0),
                            text: "No lyrics found :(".dimmed().to_string(),
                        }];
                    } else {
                        *LYRICS.write().unwrap() = parsed;
                    }
                }
                Err(_) => {
                    *LYRICS.write().unwrap() = vec![LrcLine {
                        timestamp: Duration::from_secs(0),
                        text: "No lyrics found :(".dimmed().to_string(),
                    }];
                }
            }
        });
    }

    //  thread to display title and lyrics
    thread::spawn(move || {
        let mut stdout = stdout();

        while !stop_clone.load(Ordering::Relaxed) {
            let (curr, tot) = player::get_time_info().unwrap_or((0.0, 0.0));
            let fmt = |s: f64| format!("{:02}:{:02}", (s / 60.0) as u64, (s % 60.0) as u64);

            let lyrics = LYRICS.read().unwrap();
            let mut current = "";
            let mut next = "";
            {
                // normal lyric matching
                for (i, l) in lyrics.iter().enumerate() {
                    let ts = dur_to_secs(l.timestamp);
                    if curr + 249.0 >= ts
                        && (i + 1 == lyrics.len() || curr < dur_to_secs(lyrics[i + 1].timestamp))
                    {
                        current = &l.text;
                        if i + 1 < lyrics.len() {
                            next = &lyrics[i + 1].text;
                        }
                        break;
                    }
                }
            }
            let (title, _) = split_title_artist(&name);
            queue!(
                stdout,
                cursor::SavePosition,
                cursor::MoveTo(0, STATUS_LINE_ROW),
                terminal::Clear(ClearType::CurrentLine),
                Print(format!(
                    "{} [{} / {}] {}",
                    "▶︎".cyan().blink(),
                    fmt(curr).cyan(),
                    fmt(tot).cyan(),
                    title.white().bold()
                )),
                cursor::MoveTo(0, STATUS_LINE_ROW + 2),
                terminal::Clear(ClearType::CurrentLine),
                Print(current.bright_white().bold().italic()),
                cursor::MoveTo(0, STATUS_LINE_ROW + 3),
                terminal::Clear(ClearType::CurrentLine),
                Print(next.dimmed().italic()),
                cursor::RestorePosition
            )
            .unwrap();

            stdout.flush().unwrap();
            thread::sleep(Duration::from_millis(500));
        }
    });

    stop
}

// helper
fn dur_to_secs(d: Duration) -> f64 {
    d.as_millis() as f64 / 1000.0
}

pub fn show_songs(list: &[SongDetails]) {
    println!();
    for (i, s) in list.iter().enumerate() {
        println!("{}. {} [{}]", i + 1, s.title, s.duration.cyan().italic());
    }
    print!("{}", "~ Select (1-5): ".bright_blue().bold().blink());
    stdout().flush().unwrap();
}
