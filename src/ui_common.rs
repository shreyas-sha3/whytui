use crate::api::{PlaylistDetails, SongDetails, split_title_artist};
use crate::features::{LrcLine, fetch_synced_lyrics};
use crate::{LYRIC_OFFSET, Track, UI_MODE, player};
use colored::*;
use core::time;
use crossterm::{
    cursor::{MoveTo, RestorePosition, SavePosition},
    execute, queue,
    style::Print,
    terminal::{self, Clear, ClearType},
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
pub static STATUS_LINE: RwLock<String> = RwLock::new(String::new());

pub static BASE_STATUS: RwLock<Option<String>> = RwLock::new(None);
pub static STATUS_TIMEOUT: RwLock<Option<Instant>> = RwLock::new(None);

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

use std::cmp::min;

fn _draw_status_line(status: Option<String>) {
    if UI_MODE.load(Ordering::Relaxed) == 2 {
        return;
    }
    let mut line = STATUS_LINE.write().unwrap();

    let global_indent = get_padding(50);
    let inner_width = 31;

    let (raw_text, styled_text) = if let Some(s) = status {
        (s.clone(), s.blue().dimmed().bold())
    } else {
        ("".to_string(), "".blue().dimmed().bold())
    };

    let visible_len = min(raw_text.len(), inner_width);
    let total_padding = inner_width - visible_len;
    let pad_l_len = total_padding / 2;
    let pad_r_len = total_padding - pad_l_len;

    let pad_l = " ".repeat(pad_l_len);
    let pad_r = " ".repeat(pad_r_len);

    let bottom_spacer = " ".repeat(inner_width);

    let left_art = "      ░   ░".blue().dimmed();
    let right_art = " ░   ░".blue().dimmed();
    execute!(
        std::io::stdout(),
        crossterm::cursor::SavePosition,
        crossterm::cursor::MoveTo(0, 9),
        crossterm::terminal::Clear(crossterm::terminal::ClearType::CurrentLine),
    )
    .ok();

    *line = format!(
        "\r{indent}{l_art}{pl}{text}{pr}{r_art}\r\n{indent}{l_art}{spacer}{r_art}",
        indent = global_indent,
        l_art = left_art,
        r_art = right_art,
        pl = pad_l,
        pr = pad_r,
        text = styled_text,
        spacer = bottom_spacer
    );

    print!("{}", *line);
    std::io::Write::flush(&mut std::io::stdout()).unwrap();

    execute!(std::io::stdout(), crossterm::cursor::RestorePosition).ok();
}

// Public wrapper for temporary status updates
pub fn set_status_line(status: Option<String>) {
    *STATUS_TIMEOUT.write().unwrap() = Some(Instant::now() + Duration::from_millis(1000));
    _draw_status_line(status);
}

fn get_padding(content_width: usize) -> String {
    let (cols, _) = crossterm::terminal::size().unwrap_or((80, 24));
    let term_width = cols as usize;
    let padding = term_width.saturating_sub(content_width) / 2;
    " ".repeat(padding)
}

pub fn get_banner_art() -> String {
    let status_line = crate::PLAYING_LOSSLESS.load(std::sync::atomic::Ordering::SeqCst);

    let art = r#"
   █     █░ ██░ ██▓ ██   ██▓ ▄███████▓ █    ██  ██▓
  ▓█░ █ ░█░▓██░ ██▒ ▒██  ██▒▓   ██▒ ▓▒ ██  ▓██ ▒▓██▒
  ▒█░ █ ░█ ▒██▀▀██░  ▒██ ██░▒  ▓██░ ▒░▓██  ▒██ ░▒██▒
  ░█░ █ ░█ ░▓█ ░██   ░ ▐██▓░░  ▓██▓ ░ ▓▓█  ░██ ░░██░
  ░░██▒██▓ ░▓█▒░██▓  ░ ██▒▓░   ▒██▒ ░ ▒▒█████▓  ░██░
  ░ ▓░▒ ▒   ▒ ░░▒░▒   ██▒▒▒    ▒ ░░   ░▒▓▒ ▒ ▒  ░▓
    ▒ ░ ░   ▒ ░▒░ ░ ▓██ ░▒░      ░    ░░▒░ ░ ░   ▒ ░
    ░   ░   ░  ░░ ░ ▒ ▒ ░░      ░      ░░░   ░   ▒ ░
        ░   ░                                ░   ░
        ░   ░                                ░   ░
"#;

    let pad = get_padding(54);
    let mut output = art
        .lines()
        .map(|l| format!("\r{pad}{l}"))
        .collect::<Vec<_>>()
        .join("\n");

    let current_status_line = STATUS_LINE.read().unwrap();

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
    print!("\r\n");
    for (i, s) in list.iter().enumerate() {
        print!(
            "\r\x1b[2K{}. {} [{}] [{}]\r\n",
            i + 1,
            s.title,
            s.artists.join(", ").dimmed(),
            s.duration.cyan().italic()
        );
    }
    print!(
        "\r\x1b[2K{}",
        format!("\n~ Select (1-{}): ", list.len())
            .bright_blue()
            .bold()
            .blink()
    );
    stdout().flush().unwrap();
}

pub fn show_playlists(list: &[PlaylistDetails]) {
    print!(
        "\r\x1b[2K\n{}\r\n",
        "--- YOUR LIBRARY ---".bold().underline()
    );
    for (i, p) in list.iter().enumerate() {
        print!(
            "\r\x1b[2K{}. {} {}\r\n",
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
        "\r\x1b[2K{}",
        format!("\n~ Select (1-{}): ", list.len())
            .bright_blue()
            .bold()
            .blink()
    );
    stdout().flush().unwrap();
}

pub fn start_monitor_thread<F>(track: Track, draw_callback: F) -> Arc<AtomicBool>
where
    F: Fn(&str, &str, &str, f64, f64, &[LrcLine], usize) + Send + 'static,
{
    let stop = Arc::new(AtomicBool::new(false));

    // fetch lyrics once per song
    if !track.url.is_empty() {
        spawn_lyrics_fetcher(track.clone());
        // show the quality of the song always
        update_quality_status();
    } else {
        LYRICS.write().unwrap().clear();
    }

    // 3. Start the UI Loop
    let stop_clone = stop.clone();
    let track_title = track.title.clone();
    let artist_str = track.artists.join(", ");
    let track_album = track.album.clone();
    let tot: f64 = duration_to_seconds(&track.duration) as f64;

    thread::spawn(move || {
        while !stop_clone.load(Ordering::Relaxed) {
            // status_bar updation
            check_status_timeout();

            // get current progress from playertitle
            let (curr, player_tot) = player::get_time_info().unwrap_or((0.0, 0.0));
            let lyrics = LYRICS.read().unwrap();

            //get lyric line
            let current_idx = get_current_lyric_index(&lyrics, curr);

            //draw screen
            draw_callback(
                &track_title,
                &artist_str,
                &track_title,
                curr,
                tot,
                &lyrics,
                current_idx,
            );

            thread::sleep(Duration::from_millis(300));
        }
    });

    stop
}

fn spawn_lyrics_fetcher(track: Track) {
    tokio::spawn(async move {
        let result = fetch_synced_lyrics(&track).await;
        LYRIC_OFFSET.store(0, Ordering::Relaxed);

        if *CURRENT_LYRIC_SONG.read().unwrap() != track.title {
            LYRICS.write().unwrap().clear();
            return;
        }

        let mut w = LYRICS.write().unwrap();
        match result {
            Ok(parsed) if !parsed.is_empty() => *w = parsed,
            _ => set_status_line(Some("No lyrics found >_<".to_string())),
        }
    });
}

fn update_quality_status() {
    let is_lossless = crate::PLAYING_LOSSLESS.load(Ordering::SeqCst);
    let game_mode = crate::config().game_mode;

    let text = if !game_mode {
        if is_lossless {
            Some(
                "     FLAC • LOSSLESS AUDIO     "
                    .dimmed()
                    .bold()
                    .to_string(),
            )
        } else {
            Some("     OPUS • STANDARD AUDIO     ".dimmed().to_string())
        }
    } else {
        None
    };

    *BASE_STATUS.write().unwrap() = text.clone();
    _draw_status_line(text);
}

fn check_status_timeout() {
    let timeout = STATUS_TIMEOUT
        .read()
        .unwrap()
        .map(|t| Instant::now() > t)
        .unwrap_or(false);
    if timeout {
        *STATUS_TIMEOUT.write().unwrap() = None;
        let base = BASE_STATUS.read().unwrap().clone();
        _draw_status_line(base);
    }
}

fn get_current_lyric_index(lyrics: &[LrcLine], curr_time: f64) -> usize {
    let offset = LYRIC_OFFSET.load(Ordering::Relaxed) as f64 / 1000.0;
    lyrics
        .iter()
        .position(|l| dur_to_secs(l.timestamp) > curr_time + 0.149 + offset)
        .unwrap_or(lyrics.len())
        .saturating_sub(1)
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

fn duration_to_seconds(duration: &str) -> f64 {
    let parts: Vec<f64> = duration
        .split(':')
        .map(|p| p.parse::<f64>().unwrap_or(0.0))
        .collect();

    match parts.as_slice() {
        [m, s] => m * 60.0 + s,
        [h, m, s] => h * 3600.0 + m * 60.0 + s,
        _ => 0.0,
    }
}
