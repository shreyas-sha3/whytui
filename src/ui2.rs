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

const PROGRESS_ROW: u16 = 12;
const CONTENT_START_ROW: u16 = 17;
const QUEUE_SIZE: usize = 6;
const PROMPT_ROW: u16 = CONTENT_START_ROW + (QUEUE_SIZE as u16) + 1;

pub use crate::ui_common::{show_playlists, show_songs, stop_lyrics};

fn get_visual_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

pub fn load_banner(song_name_opt: Option<&str>, queue: &[String], toggle: &str) {
    let mut stdout = stdout();
    let (term_cols, _) = terminal::size().unwrap_or((80, 24));

    let split_col = term_cols / 2;

    let left_center_x = split_col / 2;
    let right_width = term_cols - split_col;
    let right_center_x = split_col + (right_width / 2);

    queue!(stdout, cursor::Hide, cursor::MoveTo(0, 0)).unwrap();
    queue!(stdout, Print(get_banner_art())).unwrap();

    let queue_header_txt = if toggle == "recent" {
        "recent"
    } else {
        "queue"
    };
    let q_header_str = format!("¨˜ˆ”°⍣~•{}•~⍣°”ˆ˜¨", queue_header_txt);
    let l_header_str = " ¨˜ˆ”°⍣~•lyrics•~⍣°”ˆ˜¨";

    let l_len = get_visual_width(l_header_str) as u16;
    let l_pos = left_center_x.saturating_sub(l_len / 2);
    queue!(
        stdout,
        cursor::MoveTo(l_pos, CONTENT_START_ROW - 1),
        Print(l_header_str.cyan().bold().dimmed())
    )
    .unwrap();

    let q_len = get_visual_width(&q_header_str) as u16;
    let q_pos = right_center_x.saturating_sub(q_len / 2);
    queue!(
        stdout,
        cursor::MoveTo(q_pos, CONTENT_START_ROW - 1),
        Print(q_header_str.bright_cyan().bold().dimmed())
    )
    .unwrap();

    for i in 0..QUEUE_SIZE {
        queue!(
            stdout,
            cursor::MoveTo(split_col, CONTENT_START_ROW + (i as u16)),
            terminal::Clear(ClearType::UntilNewLine)
        )
        .unwrap();

        if i < queue.len() {
            let (clean_name, _) = split_title_artist(&queue[i]);

            let max_len = (right_width as usize).saturating_sub(2);

            let clean_name = blindly_trim(&clean_name);
            let safe_name = truncate_safe(&clean_name, max_len);

            let display_str = format!("{}", safe_name);

            let display_len = get_visual_width(&display_str) as u16;

            let final_x = right_center_x.saturating_sub(display_len / 2);

            let styled = match i {
                0 => display_str.truecolor(255, 255, 255).bold(),
                1 => display_str.truecolor(180, 180, 180),
                2 => display_str.truecolor(160, 160, 160),
                3 => display_str.truecolor(140, 140, 140),
                4 => display_str.truecolor(120, 120, 120),
                _ => display_str.truecolor(100, 100, 100),
            };

            queue!(
                stdout,
                cursor::MoveTo(final_x, CONTENT_START_ROW + (i as u16)),
                Print(styled)
            )
            .unwrap();
        }
    }

    queue!(
        stdout,
        cursor::MoveTo(0, PROMPT_ROW),
        terminal::Clear(ClearType::CurrentLine),
        terminal::Clear(ClearType::FromCursorDown),
        Print("> ".bright_blue().bold()),
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

                let new_stop = start_monitor_thread(song_name.to_string(), draw_ui2_status);
                *monitor_guard = Some(new_stop);
            }
        }
    }
}

fn draw_ui2_status(
    title: &str,
    artist: &str,
    _full_name: &str,
    curr: f64,
    tot: f64,
    lyrics: &[crate::features::LrcLine],
    current_idx: usize,
) {
    let mut stdout = stdout();
    let (term_cols, _) = terminal::size().unwrap_or((80, 24));
    let width_usize = term_cols as usize;

    let split_col = term_cols / 2;
    let left_center_x = split_col / 2;

    let lyric_area_width = split_col as usize;

    let max_lyric_width = lyric_area_width.saturating_sub(1);

    let artist_scroll = get_scrolling_text(artist, 25);
    let title = blindly_trim(&title);
    let display_title = truncate_safe(title, 35);

    let fmt_time = |s: f64| format!("{:02}:{:02}", (s / 60.0) as u64, (s % 60.0) as u64);
    let max_bar_width = 42;
    let available_width = width_usize.saturating_sub(16);
    let bar_width = std::cmp::min(available_width, max_bar_width);

    let total_bar_len = 12 + bar_width;
    let bar_pad = (width_usize.saturating_sub(total_bar_len)) / 2;

    let title_visual_len = get_visual_width(&display_title) + get_visual_width(&artist_scroll) + 6;
    let title_pad = (width_usize.saturating_sub(title_visual_len)) / 2;

    let ratio = if tot > 0.0 { curr / tot } else { 0.0 };
    let filled_len = (ratio * bar_width as f64).round() as usize;
    let empty_len = bar_width.saturating_sub(filled_len);

    let bar_str = format!(
        "{}{}",
        "━".repeat(filled_len).cyan(),
        "─".repeat(empty_len).dimmed()
    );

    queue!(
        stdout,
        cursor::Hide,
        cursor::SavePosition,
        cursor::MoveTo(title_pad as u16, PROGRESS_ROW),
        terminal::Clear(ClearType::CurrentLine),
        Print(format!(
            "{} {} [{}]",
            "▶︎".cyan(),
            display_title.white().bold(),
            artist_scroll.dimmed().italic()
        )),
        cursor::MoveTo(bar_pad as u16, PROGRESS_ROW + 1),
        terminal::Clear(ClearType::CurrentLine),
        Print(format!(
            "{} {} {}",
            fmt_time(curr).cyan(),
            bar_str,
            fmt_time(tot).cyan()
        ))
    )
    .unwrap();

    let cleaner = " ".repeat(lyric_area_width);

    if max_lyric_width > 9 {
        for offset in 0..6 {
            let target_idx = current_idx + offset;
            let text = if target_idx < lyrics.len() {
                &lyrics[target_idx].get_current_text()
            } else {
                ""
            };

            let safe_text = truncate_safe(text, max_lyric_width);

            let len = get_visual_width(&safe_text) as u16;

            let final_x = left_center_x.saturating_sub(len / 2);

            let styled = match offset {
                0 => safe_text.truecolor(255, 255, 255).bold(),
                1 => safe_text.truecolor(180, 180, 180),
                2 => safe_text.truecolor(160, 160, 160),
                3 => safe_text.truecolor(140, 140, 140),
                4 => safe_text.truecolor(120, 120, 120),
                _ => safe_text.truecolor(100, 100, 100),
            };

            queue!(
                stdout,
                cursor::MoveTo(0, CONTENT_START_ROW + offset as u16),
                Print(&cleaner),
                cursor::MoveTo(final_x, CONTENT_START_ROW + offset as u16),
                Print(styled)
            )
            .unwrap();
        }
    }

    queue!(stdout, cursor::RestorePosition, cursor::Show).unwrap();
    stdout.flush().unwrap();
}
