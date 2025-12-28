use crate::api::split_title_artist;
use crate::ui_common::{self, *};
use colored::*;
use crossterm::{
    cursor, queue,
    style::Print,
    terminal::{self, ClearType},
};
use std::io::{Write, stdout};
use std::sync::atomic::Ordering;

use unicode_width::UnicodeWidthStr;

const STATUS_LINE_ROW: u16 = 12;
const QUEUE_SIZE: usize = 5;

pub use crate::ui_common::{show_playlists, show_songs, stop_lyrics};

fn get_visual_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

pub fn load_banner(song_name_opt: Option<&str>, queue: &[String], toggle: &str) {
    let mut stdout = stdout();
    let (cols, _) = terminal::size().unwrap_or((80, 24));
    let width_usize = cols as usize;

    queue!(stdout, cursor::Hide, cursor::MoveTo(0, 0)).unwrap();
    queue!(stdout, Print(get_banner_art()), Print("\n\n")).unwrap();

    let header_text = if toggle == "recent" {
        "recent"
    } else {
        "queue"
    };

    let raw_header = format!("¨˜ˆ”°⍣~•{}•~⍣°”ˆ˜¨", header_text);

    let header_len = get_visual_width(&raw_header);
    let header_pad = (width_usize.saturating_sub(header_len)) / 2;

    queue!(
        stdout,
        cursor::MoveTo(0, 18),
        terminal::Clear(ClearType::FromCursorDown)
    )
    .unwrap();
    queue!(
        stdout,
        cursor::MoveTo(header_pad as u16, 18),
        Print(raw_header.cyan())
    )
    .unwrap();

    if !queue.is_empty() {
        for (i, name) in queue.iter().enumerate().take(QUEUE_SIZE) {
            let (title, _) = split_title_artist(&name);
            let clean_name = blindly_trim(&title);

            let display_str = format!("{}", clean_name);

            let len = get_visual_width(&display_str);
            let pad = (width_usize.saturating_sub(len)) / 2;

            queue!(
                stdout,
                cursor::MoveTo(pad as u16, 20 + i as u16),
                Print(display_str.dimmed())
            )
            .unwrap();
        }
    } else {
        let msg = "~";
        let pad = (width_usize.saturating_sub(1)) / 2;
        queue!(stdout, cursor::MoveTo(pad as u16, 20), Print(msg)).unwrap();
    }

    // let prompt_str = "> ";

    queue!(
        stdout,
        cursor::MoveTo(0, 20 + QUEUE_SIZE as u16 + 1),
        // Print(format!("{}", prompt_str.bright_blue().bold())),
        Print("\n"),
        cursor::Hide
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

                let new_stop = start_monitor_thread(song_name.to_string(), draw_ui1_status);
                *monitor_guard = Some(new_stop);
            }
        }
    }
}

fn draw_ui1_status(
    title: &str,
    artist: &str,
    _full_name: &str,
    curr: f64,
    tot: f64,
    lyrics: &[crate::features::LrcLine],
    current_idx: usize,
) {
    let mut stdout = stdout();
    let (cols, _) = terminal::size().unwrap_or((80, 24));
    let width_usize = cols as usize;

    let fmt_time = |s: f64| format!("{:02}:{:02}", (s / 60.0) as u64, (s % 60.0) as u64);
    let max_bar_width = 42;
    let available_width = width_usize.saturating_sub(16);
    let bar_width = std::cmp::min(available_width, max_bar_width);

    let ratio = if tot > 0.0 { curr / tot } else { 0.0 };
    let filled_len = (ratio * bar_width as f64).round() as usize;
    let empty_len = bar_width.saturating_sub(filled_len);

    let bar_str = format!(
        "{}{}",
        "━".repeat(filled_len).cyan(),
        "─".repeat(empty_len).dimmed()
    );

    let artist_scroll = get_scrolling_text(artist, 25);
    let trimmed_title = blindly_trim(&title);
    let truncated_title = truncate_safe(trimmed_title, 35);
    let final_title = format!("{} [{}]", truncated_title, artist_scroll.dimmed());

    let title_visual_len =
        get_visual_width(&truncated_title) + get_visual_width(&artist_scroll) + 5;
    let title_pad = (width_usize.saturating_sub(title_visual_len)) / 2;

    let total_bar_len = 12 + bar_width;
    let bar_pad = (width_usize.saturating_sub(total_bar_len)) / 2;

    let current_text = if current_idx < lyrics.len() {
        &lyrics[current_idx].get_current_text()
    } else {
        ""
    };
    let next_text = if current_idx + 1 < lyrics.len() {
        &lyrics[current_idx + 1].get_current_text()
    } else {
        ""
    };

    let curr_lyric_len = get_visual_width(current_text);
    let next_lyric_len = get_visual_width(next_text);

    let curr_lyric_pad = (width_usize.saturating_sub(curr_lyric_len)) / 2;
    let next_lyric_pad = (width_usize.saturating_sub(next_lyric_len)) / 2;

    let current_display = if current_text.trim().is_empty() {
        "~".white().bold().blink().to_string()
    } else {
        current_text
            .truecolor(255, 255, 255)
            .bold()
            .italic()
            .to_string()
    };

    queue!(
        stdout,
        cursor::Hide,
        cursor::SavePosition,
        cursor::MoveTo(title_pad as u16, STATUS_LINE_ROW),
        terminal::Clear(ClearType::CurrentLine),
        Print(format!("{} {}", "▶︎".cyan(), final_title.white().bold())),
        cursor::MoveTo(bar_pad as u16, STATUS_LINE_ROW + 1),
        terminal::Clear(ClearType::CurrentLine),
        Print(format!(
            "{} {} {}",
            fmt_time(curr).cyan(),
            bar_str,
            fmt_time(tot).cyan()
        )),
        cursor::MoveTo(curr_lyric_pad as u16, STATUS_LINE_ROW + 3),
        terminal::Clear(ClearType::CurrentLine),
        Print(current_display),
        cursor::MoveTo(next_lyric_pad as u16, STATUS_LINE_ROW + 4),
        terminal::Clear(ClearType::CurrentLine),
        Print(next_text.dimmed().italic()),
        cursor::MoveTo(next_lyric_pad as u16, STATUS_LINE_ROW + 4),
        terminal::Clear(ClearType::CurrentLine),
        cursor::RestorePosition,
        cursor::Hide
    )
    .unwrap();
    stdout.flush().unwrap();
}
