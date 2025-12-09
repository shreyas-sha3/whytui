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
use std::time::Instant;

static SONG_MONITOR: RwLock<Option<Arc<AtomicBool>>> = RwLock::new(None);

static LYRICS: RwLock<Vec<LrcLine>> = RwLock::new(Vec::new());
static CURRENT_LYRIC_SONG: RwLock<String> = RwLock::new(String::new());

const _BANNER_HEIGHT: u16 = 11;
const STATUS_LINE_ROW: u16 = 12;
static TITLE_SCROLL: RwLock<usize> = RwLock::new(0);
static LAST_SCROLL: RwLock<Option<Instant>> = RwLock::new(None);

pub fn load_banner(song_name_opt: Option<&str>, queue: &[String], toggle: &str) {
    let mut stdout = stdout();
    queue!(stdout, cursor::Hide, cursor::MoveTo(0, 0)).unwrap();

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
    if toggle == "queue" {
        queue!(
            stdout,
            Print(format!("\n\n\n\n{}\n", "¨˜ˆ”°⍣~•queue•~⍣°”ˆ˜¨".cyan()))
        )
        .unwrap();
    } else if toggle == "recent" {
        queue!(
            stdout,
            Print(format!("\n\n\n\n{}\n", "¨˜ˆ”°⍣~•recent•~⍣°”ˆ˜¨".cyan()))
        )
        .unwrap();
    }

    queue!(
        stdout,
        cursor::MoveTo(0, 18),
        terminal::Clear(ClearType::FromCursorDown)
    )
    .unwrap();
    if !queue.is_empty() {
        for (i, name) in queue.iter().enumerate().take(7) {
            let (title, _) = split_title_artist(&name);
            queue!(
                stdout,
                Print(format!(
                    "{}. {}\n",
                    (i + 1).to_string().dimmed(),
                    title.dimmed()
                ))
            )
            .unwrap();
        }
    } else {
        queue!(stdout, Print("\t  ~\n")).unwrap();
    }

    queue!(stdout, Print(format!("\n\n{}", ">> ".bright_blue().bold()))).unwrap();

    stdout.flush().unwrap();

    if let Some(song_name) = song_name_opt {
        if !song_name.is_empty() {
            queue!(
                stdout,
                cursor::SavePosition,
                cursor::MoveTo(0, STATUS_LINE_ROW),
                terminal::Clear(ClearType::CurrentLine),
                cursor::MoveTo(0, STATUS_LINE_ROW + 1),
                terminal::Clear(ClearType::CurrentLine),
                cursor::MoveTo(0, STATUS_LINE_ROW + 2),
                terminal::Clear(ClearType::CurrentLine),
                cursor::MoveTo(0, STATUS_LINE_ROW + 3),
                terminal::Clear(ClearType::CurrentLine),
                cursor::RestorePosition
            )
            .unwrap();
            stdout.flush().unwrap();
            let mut current_song_guard = CURRENT_LYRIC_SONG.write().unwrap();

            if *current_song_guard != song_name {
                *current_song_guard = song_name.to_string();

                let mut monitor_guard = SONG_MONITOR.write().unwrap();
                if let Some(stop_signal) = monitor_guard.take() {
                    stop_signal.store(true, Ordering::Relaxed);
                }

                let new_stop = start_monitor_thread(song_name.to_string());
                *monitor_guard = Some(new_stop);
            }
        }
    }
}

fn start_monitor_thread(name: String) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();

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

    if name != "Nothing Playing" {
        let song_name = name.clone();
        tokio::spawn(async move {
            match fetch_synced_lyrics(&song_name).await {
                Ok(parsed) => {
                    if *CURRENT_LYRIC_SONG.read().unwrap() != song_name {
                        return;
                    }
                    if parsed.is_empty() {
                        *LYRICS.write().unwrap() = vec![LrcLine {
                            timestamp: Duration::from_secs(0),
                            text: "No lyrics found :(".into(),
                        }];
                    } else {
                        *LYRICS.write().unwrap() = parsed;
                    }
                }
                Err(_) => {
                    if *CURRENT_LYRIC_SONG.read().unwrap() != song_name {
                        return;
                    }
                    *LYRICS.write().unwrap() = vec![LrcLine {
                        timestamp: Duration::from_secs(0),
                        text: "No lyrics found :(".into(),
                    }];
                }
            }
        });
    }

    thread::spawn(move || {
        let mut stdout = stdout();

        while !stop_clone.load(Ordering::Relaxed) {
            let (curr, tot) = player::get_time_info().unwrap_or((0.0, 0.0));
            let fmt = |s: f64| format!("{:02}:{:02}", (s / 60.0) as u64, (s % 60.0) as u64);

            let lyrics = LYRICS.read().unwrap();
            let mut current = "";
            let mut next = "";

            for (i, l) in lyrics.iter().enumerate() {
                let ts = dur_to_secs(l.timestamp);
                if curr + 0.249 >= ts
                    && (i + 1 == lyrics.len() || curr < dur_to_secs(lyrics[i + 1].timestamp))
                {
                    current = &l.text;
                    if i + 1 < lyrics.len() {
                        next = &lyrics[i + 1].text;
                    }
                    break;
                }
            }

            let current_display = if current.trim().is_empty() {
                "~".white().bold().blink().to_string()
            } else {
                current.truecolor(255, 255, 255).bold().italic().to_string()
            };

            let next_display = if next.trim().is_empty() {
                "".into()
            } else {
                next.dimmed().italic().to_string()
            };

            let (title, artist) = split_title_artist(&name);
            let width = 30;

            let artist_scroll = if artist.chars().count() <= width {
                artist.to_string()
            } else {
                let chars: Vec<char> = artist.chars().collect();
                let len = chars.len();

                let mut scroll = TITLE_SCROLL.write().unwrap();
                let mut last_lock = LAST_SCROLL.write().unwrap();
                let now = Instant::now();

                let last = match *last_lock {
                    Some(t) => t,
                    None => {
                        *last_lock = Some(now);
                        now
                    }
                };

                let scroll_speed = Duration::from_millis(300);

                if now.duration_since(last) >= scroll_speed {
                    *scroll = (*scroll + 1) % len;
                    *last_lock = Some(now);
                }

                let mut doubled = chars.clone();
                doubled.push(' ');
                doubled.push(' ');
                doubled.extend(chars.clone());

                let mut out = String::new();
                for i in 0..width {
                    let idx = (*scroll + i) % doubled.len();
                    out.push(doubled[idx]);
                }

                out
            };

            let title = format!("{} [{}]", title, artist_scroll.dimmed());

            queue!(
                stdout,
                cursor::SavePosition,
                cursor::MoveTo(0, STATUS_LINE_ROW),
                Print(format!(
                    "[{} / {}] {}{: <80}",
                    fmt(curr).cyan(),
                    fmt(tot).cyan(),
                    title.white().bold(),
                    ""
                )),
                cursor::MoveTo(0, STATUS_LINE_ROW + 2),
                terminal::Clear(ClearType::CurrentLine),
                Print(format!("{: <85}", current_display)),
                cursor::MoveTo(0, STATUS_LINE_ROW + 3),
                terminal::Clear(ClearType::CurrentLine),
                Print(format!("{: <80}", next_display)),
                cursor::RestorePosition
            )
            .unwrap();

            stdout.flush().unwrap();
            thread::sleep(Duration::from_millis(500));
        }
    });

    stop
}

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
