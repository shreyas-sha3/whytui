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
use std::time::{Duration, Instant};

static SONG_MONITOR: RwLock<Option<Arc<AtomicBool>>> = RwLock::new(None);
static LYRICS: RwLock<Vec<LrcLine>> = RwLock::new(Vec::new());
static CURRENT_LYRIC_SONG: RwLock<String> = RwLock::new(String::new());

// Scrolling state from new version
static TITLE_SCROLL: RwLock<usize> = RwLock::new(0);
static LAST_SCROLL: RwLock<Option<Instant>> = RwLock::new(None);

const PROGRESS_ROW: u16 = 12;
const CONTENT_START_ROW: u16 = 15;
const LYRIC_COLUMN: u16 = 32;
const QUEUE_SIZE: usize = 7;
const PROMPT_ROW: u16 = CONTENT_START_ROW + (QUEUE_SIZE as u16) + 2;

// Updated signature to match new features (added `toggle`)
pub fn load_banner(song_name_opt: Option<&str>, queue: &[String], toggle: &str) {
    let mut stdout = stdout();

    queue!(stdout, cursor::Hide, cursor::MoveTo(0, 0)).unwrap();

    let banner_art = format!(
        "{}",
        r#"
      █      █░ ██░ ██▓ ██   ██▓ ▄███████▓ █    ██  ██▓
     ▓█░ █ ░█░▓██░ ██▒ ▒██   ██▒▓   ██▒ ▓▒ ██   ▓██ ▒▓██▒
     ▒█░ █ ░█ ▒██▀▀██░  ▒██ ██░▒   ▓██░ ▒░▓██   ▒██ ░▒██▒
     ░█░ █ ░█ ░▓█ ░██   ░ ▐██▓░░   ▓██▓ ░ ▓▓█   ░██ ░░██░
     ░░██▒██▓ ░▓█▒░██▓  ░ ██▒▓░    ▒██▒ ░ ▒▒█████▓  ░██░
     ░ ▓░▒ ▒   ▒ ░░▒░▒   ██▒▒▒     ▒ ░░   ░▒▓▒ ▒ ▒  ░▓
       ▒ ░ ░   ▒ ░▒░ ░ ▓██ ░▒░       ░     ░░▒░ ░ ░   ▒ ░
       ░   ░   ░  ░░ ░ ▒ ▒ ░░      ░         ░░░ ░  ░  ▒ ░
         ░      ░  ░  ░ ░ ░                   ░        ░
                    ░  ░
"#
        .blue()
        .dimmed()
    );
    queue!(stdout, Print(banner_art)).unwrap();

    // Feature Update: Toggle the header text based on mode
    let queue_header = if toggle == "recent" {
        "¨˜ˆ”°⍣~•RECENT•~⍣°”ˆ˜¨"
    } else {
        "¨˜ˆ”°⍣~•QUEUE•~⍣°”ˆ˜¨"
    };

    queue!(
        stdout,
        cursor::MoveTo(0, CONTENT_START_ROW - 1),
        Print(format!("{}\n", queue_header.bright_cyan().bold().dimmed()))
    )
    .unwrap();

    queue!(
        stdout,
        cursor::MoveTo(LYRIC_COLUMN, CONTENT_START_ROW - 1),
        Print(" ¨˜ˆ”°⍣~•LYRICS•~⍣°”ˆ˜¨\n".cyan().bold().dimmed())
    )
    .unwrap();

    // Print Queue (Left Side) - Preserved Old Look
    for i in 0..QUEUE_SIZE {
        let (plain_str, display_str) = if i < queue.len() {
            let (clean_name, _) = split_title_artist(&queue[i]);
            let max_len = (LYRIC_COLUMN as usize).saturating_sub(8);
            let safe_name = truncate_safe(&clean_name, max_len);
            let item_str = format!("{}. {}", i + 1, safe_name);

            let styled = match i {
                0 => item_str.truecolor(255, 255, 255).bold(),
                1 => item_str.truecolor(220, 220, 220),
                2 => item_str.truecolor(180, 180, 180),
                3 => item_str.truecolor(140, 140, 140),
                _ => item_str.truecolor(100, 100, 100),
            };
            (item_str, styled.to_string())
        } else {
            (" ".to_string(), " ".to_string())
        };

        let vis_width = get_visual_width(&plain_str);
        let target_width = (LYRIC_COLUMN as usize) - 1;
        let padding_needed = target_width.saturating_sub(vis_width);
        let padding = " ".repeat(padding_needed);

        queue!(
            stdout,
            cursor::MoveTo(0, CONTENT_START_ROW + (i as u16) + 1),
            Print(display_str),
            Print(padding)
        )
        .unwrap();
    }

    // Print Prompt
    queue!(
        stdout,
        cursor::MoveTo(0, PROMPT_ROW),
        terminal::Clear(ClearType::CurrentLine),
        terminal::Clear(ClearType::FromCursorDown),
        Print("> Search / Command: ".bright_blue().bold()),
        cursor::Show
    )
    .unwrap();

    stdout.flush().unwrap();

    if let Some(song_name) = song_name_opt {
        if !song_name.is_empty() {
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

    // Feature Update: Better "Nothing Playing" logic from new version
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
        let stdout_handle = stdout();

        while !stop_clone.load(Ordering::Relaxed) {
            {
                let mut stdout = stdout_handle.lock();

                let (curr, tot) = player::get_time_info().unwrap_or((0.0, 0.0));
                let fmt = |s: f64| format!("{:02}:{:02}", (s / 60.0) as u64, (s % 60.0) as u64);

                let (term_cols, _) = terminal::size().unwrap_or((80, 24));
                let max_lyric_width =
                    (term_cols.saturating_sub(LYRIC_COLUMN) as usize).saturating_sub(2);

                let lyrics = LYRICS.read().unwrap();
                let mut current_idx = 0;

                // Feature Update: Adjusted offset to 0.249 to match new version
                for (i, l) in lyrics.iter().enumerate() {
                    let ts = dur_to_secs(l.timestamp);
                    if curr + 0.249 >= ts
                        && (i + 1 == lyrics.len() || curr < dur_to_secs(lyrics[i + 1].timestamp))
                    {
                        current_idx = i;
                        break;
                    }
                }

                // Feature Update: Scrolling Artist Name implementation
                let (clean_title, clean_artist) = split_title_artist(&name);
                // Slightly shorter width than new version due to layout
                let scroll_width = 25;

                let artist_scroll = if clean_artist.chars().count() <= scroll_width {
                    clean_artist.to_string()
                } else {
                    let chars: Vec<char> = clean_artist.chars().collect();
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
                    for i in 0..scroll_width {
                        let idx = (*scroll + i) % doubled.len();
                        out.push(doubled[idx]);
                    }

                    out
                };

                // Truncate title so it fits with the scrolling artist
                let display_title = truncate_safe(&clean_title, 40);

                queue!(
                    stdout,
                    cursor::Hide,
                    cursor::SavePosition,
                    cursor::MoveTo(0, PROGRESS_ROW),
                    Print(format!(
                        "{} [{} / {}] {} [{}]",
                        "▶︎".cyan().blink(),
                        fmt(curr).cyan(),
                        fmt(tot).cyan(),
                        display_title.white().bold(),
                        artist_scroll.dimmed().italic()
                    )),
                    terminal::Clear(ClearType::UntilNewLine)
                )
                .unwrap();

                // Render Lyrics (Old Look preserved: 7 lines of context)
                for offset in 0..7 {
                    let target_idx = current_idx + offset;
                    let text = if target_idx < lyrics.len() {
                        &lyrics[target_idx].text
                    } else {
                        ""
                    };

                    let safe_text = truncate_safe(text, max_lyric_width);

                    let styled_text = match offset {
                        0 => safe_text.truecolor(255, 255, 255).bold().italic(),
                        1 => safe_text.truecolor(180, 180, 180).italic(),
                        2 => safe_text.truecolor(140, 140, 140).italic(),
                        3 => safe_text.truecolor(100, 100, 100).italic(),
                        _ => safe_text.truecolor(80, 80, 80).italic(),
                    };

                    queue!(
                        stdout,
                        cursor::MoveTo(LYRIC_COLUMN, CONTENT_START_ROW + offset as u16 + 1),
                        terminal::Clear(ClearType::UntilNewLine),
                        Print(styled_text)
                    )
                    .unwrap();
                }

                queue!(stdout, cursor::RestorePosition, cursor::Show).unwrap();
                stdout.flush().unwrap();
            }

            thread::sleep(Duration::from_millis(500));
        }
    });

    stop
}

fn get_visual_width(s: &str) -> usize {
    s.chars()
        .map(|c| if c.len_utf8() > 1 { 2 } else { 1 })
        .sum()
}

fn truncate_safe(s: &str, max_width: usize) -> String {
    let mut width = 0;
    let mut result = String::new();
    for c in s.chars() {
        let char_width = if c.len_utf8() > 1 { 2 } else { 1 };
        if width + char_width > max_width.saturating_sub(3) {
            result.push_str("...");
            return result;
        }
        width += char_width;
        result.push(c);
    }
    result
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
