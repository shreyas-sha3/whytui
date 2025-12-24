use crate::api::{PlaylistDetails, SongDetails, split_title_artist};
use crate::features::{LrcLine, fetch_synced_lyrics};
use crate::player;
use colored::*;
use crossterm::{
    cursor, queue,
    style::Print,
    terminal::{self, ClearType},
};
use std::io::{Write, stdout};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

pub static SONG_MONITOR: RwLock<Option<Arc<AtomicBool>>> = RwLock::new(None);
pub static LYRICS: RwLock<Vec<LrcLine>> = RwLock::new(Vec::new());
pub static CURRENT_LYRIC_SONG: RwLock<String> = RwLock::new(String::new());
pub static TITLE_SCROLL: RwLock<usize> = RwLock::new(0);
pub static LAST_SCROLL: RwLock<Option<Instant>> = RwLock::new(None);

pub static LYRIC_DISPLAY_MODE: AtomicU8 = AtomicU8::new(0);

pub fn cycle_lyric_display_mode() {
    LYRIC_DISPLAY_MODE
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |x| Some((x + 1) % 3))
        .unwrap();
}
pub fn stop_lyrics() {
    let mut monitor_guard = SONG_MONITOR.write().unwrap();
    if let Some(stop_signal) = monitor_guard.take() {
        stop_signal.store(true, Ordering::Relaxed);
    }
    *CURRENT_LYRIC_SONG.write().unwrap() = String::new();
}

pub fn clear_lyrics() {
    LYRICS.write().unwrap().clear();
}

pub fn get_banner_art() -> String {
    let is_lossless = crate::PLAYING_LOSSLESS.load(Ordering::SeqCst);

    let quality_line = if is_lossless {
        "    ░     ░  ░  FLAC • LOSSLESS AUDIO  ░       ░"
            .dimmed()
            .bold()
            .blink()
    } else {
        "    ░     ░  ░  WEBM • STANDARD AUDIO  ░       ░".dimmed()
    };

    let art = r#"
   █     █░ ██░ ██▓ ██   ██▓ ▄███████▓ █    ██  ██▓
  ▓█░ █ ░█░▓██░ ██▒ ▒██  ██▒▓   ██▒ ▓▒ ██  ▓██ ▒▓██▒
  ▒█░ █ ░█ ▒██▀▀██░  ▒██ ██░▒  ▓██░ ▒░▓██  ▒██ ░▒██▒
  ░█░ █ ░█ ░▓█ ░██   ░ ▐██▓░░  ▓██▓ ░ ▓▓█  ░██ ░░██░
  ░░██▒██▓ ░▓█▒░██▓  ░ ██▒▓░   ▒██▒ ░ ▒▒█████▓  ░██░
  ░ ▓░▒ ▒   ▒ ░░▒░▒   ██▒▒▒    ▒ ░░   ░▒▓▒ ▒ ▒  ░▓
    ▒ ░ ░   ▒ ░▒░ ░ ▓██ ░▒░      ░    ░░▒░ ░ ░   ▒ ░
    ░   ░   ░  ░░ ░ ▒ ▒ ░░     ░       ░░░ ░  ░  ▒ ░
"#;

    let (cols, _) = crossterm::terminal::size().unwrap_or((80, 24));
    let width = cols as usize;

    let art_width = 54;
    let padding = width.saturating_sub(art_width) / 2;
    let pad = " ".repeat(padding);

    let mut output = art
        .lines()
        .map(|l| format!("{pad}{l}"))
        .collect::<Vec<_>>()
        .join("\n");

    output.push('\n');
    output.push_str(&format!("{pad}{}", quality_line));

    output.blue().dimmed().to_string()
}

pub fn dur_to_secs(d: Duration) -> f64 {
    d.as_millis() as f64 / 1000.0
}

pub fn get_scrolling_text(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }

    let mut scroll = TITLE_SCROLL.write().unwrap();
    let mut last_lock = LAST_SCROLL.write().unwrap();
    let now = Instant::now();

    if last_lock.is_none() {
        *last_lock = Some(now);
    }

    if now.duration_since(last_lock.unwrap()) >= Duration::from_millis(300) {
        *scroll = (*scroll + 1) % (text.chars().count() + 2);
        *last_lock = Some(now);
    }

    let padded = format!("{}  {}", text, text);
    let chars: Vec<char> = padded.chars().collect();
    let start = *scroll % chars.len();

    chars.iter().cycle().skip(start).take(width).collect()
}

pub fn show_songs(list: &[SongDetails]) {
    println!();
    for (i, s) in list.iter().enumerate() {
        println!("{}. {} [{}]", i + 1, s.title, s.duration.cyan().italic());
    }
    print!(
        "{}",
        format!("\n~ Select (1-{}): ", list.len())
            .bright_blue()
            .bold()
            .blink()
    );
    stdout().flush().unwrap();
}

pub fn show_playlists(list: &[PlaylistDetails]) {
    println!("\n{}", "--- YOUR LIBRARY ---".bold().underline());
    for (i, p) in list.iter().enumerate() {
        println!(
            "{}. {} {}",
            i + 1,
            p.title.bold(),
            if p.count.is_empty() {
                "".to_string().cyan()
            } else {
                format!("[{}]", p.count).cyan()
            },
        );
    }
    print!(
        "{}",
        format!("\n~ Select (1-{}): ", list.len())
            .bright_blue()
            .bold()
            .blink()
    );
    stdout().flush().unwrap();
}

pub fn start_monitor_thread<F>(name: String, draw_callback: F) -> Arc<AtomicBool>
where
    F: Fn(&str, &str, &str, f64, f64, &[LrcLine], usize) + Send + 'static,
{
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();

    if name != "Nothing Playing" {
        let song_name = name.clone();
        tokio::spawn(async move {
            let (_, tot) = player::get_time_info().unwrap_or((0.0, 0.0));
            let duration_secs = tot as u32;
            let result = fetch_synced_lyrics(&song_name, duration_secs).await;

            if *CURRENT_LYRIC_SONG.read().unwrap() != song_name {
                LYRICS.write().unwrap().clear();
                return;
            }
            let mut w = LYRICS.write().unwrap();
            match result {
                Ok(parsed) if !parsed.is_empty() => *w = parsed,
                _ => {
                    *w = vec![LrcLine {
                        timestamp: Duration::from_secs(0),
                        text: ">_<".dimmed().to_string(),
                        translation: None,
                        romanized: None,
                    }]
                }
            }
        });
    }

    thread::spawn(move || {
        while !stop_clone.load(Ordering::Relaxed) {
            let (curr, tot) = player::get_time_info().unwrap_or((0.0, 0.0));
            let (title, artist) = split_title_artist(&name);

            let lyrics = LYRICS.read().unwrap();
            let mut current_idx = 0;

            for (i, l) in lyrics.iter().enumerate() {
                let ts = dur_to_secs(l.timestamp);
                if curr + 0.249 >= ts {
                    current_idx = i;
                } else {
                    break;
                }
            }

            draw_callback(&title, &artist, &name, curr, tot, &lyrics, current_idx);

            thread::sleep(Duration::from_millis(500));
        }
    });

    stop
}

pub fn get_visual_width(s: &str) -> usize {
    s.chars()
        .map(|c| if c.len_utf8() > 1 { 2 } else { 1 })
        .sum()
}

pub fn truncate_safe(s: &str, max_width: usize) -> String {
    if get_visual_width(s) <= max_width {
        return s.to_string();
    }
    let mut result = String::new();
    let mut width = 0;
    for c in s.chars() {
        let w = if c.len_utf8() > 1 { 2 } else { 1 };
        if width + w > max_width.saturating_sub(3) {
            result.push_str("...");
            break;
        }
        result.push(c);
        width += w;
    }
    result
}

pub fn blindly_trim(text: &str) -> &str {
    let separators = ['-', '(', '[', '_', '|'];

    let mut cut = text.len();

    for sep in separators {
        let pattern = format!(" {}", sep);
        if let Some(idx) = text.find(&pattern) {
            cut = cut.min(idx);
        }
    }
    &text[..cut]
}

pub fn word_wrap_cjk(text: &str, max_width: usize) -> Vec<String> {
    if text.trim().is_empty() {
        return vec!["".to_string()];
    }
    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;

    let word_visual_width = |w: &str| -> usize {
        w.chars()
            .map(|c| if c.len_utf8() > 1 { 2 } else { 1 })
            .sum()
    };

    for word in text.split_whitespace() {
        let w_len = word_visual_width(word);
        if current_width + w_len + (if current_width > 0 { 1 } else { 0 }) <= max_width {
            if current_width > 0 {
                current_line.push(' ');
                current_width += 1;
            }
            current_line.push_str(word);
            current_width += w_len;
        } else {
            if !current_line.is_empty() {
                lines.push(current_line);
            }
            if w_len > max_width {
                current_line = String::new();
                current_width = 0;
                for c in word.chars() {
                    let c_width = if c.len_utf8() > 1 { 2 } else { 1 };
                    if current_width + c_width > max_width {
                        lines.push(current_line);
                        current_line = String::new();
                        current_width = 0;
                    }
                    current_line.push(c);
                    current_width += c_width;
                }
            } else {
                current_line = word.to_string();
                current_width = w_len;
            }
        }
    }
    if !current_line.is_empty() {
        lines.push(current_line);
    }
    lines
}
