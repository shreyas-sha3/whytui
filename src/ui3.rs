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
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

pub use crate::ui_common::{
    blindly_trim, get_scrolling_text, get_visual_width, show_playlists, show_songs, stop_lyrics,
    truncate_safe, word_wrap_cjk,
};

// static LOCAL_TITLE_SCROLL: RwLock<usize> = RwLock::new(0);
// static LOCAL_TITLE_LAST: RwLock<Option<Instant>> = RwLock::new(None);

pub fn load_banner(song_name_opt: Option<&str>, queue: &[String], _toggle: &str) {
    let mut stdout = stdout();
    let (_, rows) = terminal::size().unwrap_or((80, 24));

    queue!(
        stdout,
        cursor::Hide,
        cursor::MoveTo(0, rows - 4),
        terminal::Clear(ClearType::FromCursorDown)
    )
    .unwrap();

    let next_song_name = if let Some(s) = queue.first() {
        let (title, _) = split_title_artist(s);
        format!("Up Next: {}", title)
    } else {
        "Up Next: ~".to_string()
    };

    let prompt_row = rows.saturating_sub(2);

    queue!(
        stdout,
        cursor::MoveTo(0, prompt_row),
        Print(format!("{}", "> ".bright_blue().bold())),
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

                let up_next_display = next_song_name.clone();

                let closure = move |title: &str,
                                    artist: &str,
                                    _full: &str,
                                    curr: f64,
                                    tot: f64,
                                    lyrics: &[crate::features::LrcLine],
                                    idx: usize| {
                    draw_minimal_ui(title, artist, curr, tot, lyrics, idx, &up_next_display);
                };

                let new_stop = start_monitor_thread(song_name.to_string(), closure);
                *monitor_guard = Some(new_stop);
            }
        }
    }
}

fn draw_minimal_ui(
    title: &str,
    artist: &str,
    curr: f64,
    tot: f64,
    lyrics: &[crate::features::LrcLine],
    current_idx: usize,
    mut up_next: &str,
) {
    let mut stdout = stdout();
    let (cols, rows) = terminal::size().unwrap_or((80, 24));
    let width_usize = cols as usize;

    let footer_row_up_next = rows.saturating_sub(4);

    let lyric_area_start = 4;
    let lyric_area_end = footer_row_up_next - 1;

    let available_lyric_height = lyric_area_end.saturating_sub(lyric_area_start);
    let lyric_width = width_usize.saturating_sub(4);

    queue!(stdout, cursor::Hide, cursor::SavePosition).unwrap();

    let title_scroll = get_scrolling_text(title, width_usize.saturating_sub(4));

    let artist_scroll = crate::ui_common::get_scrolling_text(artist, width_usize.saturating_sub(4));

    let fmt_time = |s: f64| format!("{:02}:{:02}", (s / 60.0) as u64, (s % 60.0) as u64);

    let bar_width = width_usize.saturating_sub(16);
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
        cursor::MoveTo(0, 0),
        terminal::Clear(ClearType::CurrentLine),
        Print(format!("  {}", title_scroll.white().bold())),
        cursor::MoveTo(0, 1),
        terminal::Clear(ClearType::CurrentLine),
        Print(format!("  {}", artist_scroll.dimmed())),
        cursor::MoveTo(0, 2),
        terminal::Clear(ClearType::CurrentLine),
        Print(format!(
            "  {} {} {}",
            fmt_time(curr).cyan(),
            bar_str,
            fmt_time(tot).cyan()
        ))
    )
    .unwrap();

    for r in lyric_area_start..lyric_area_end {
        queue!(
            stdout,
            cursor::MoveTo(0, r),
            terminal::Clear(ClearType::CurrentLine)
        )
        .unwrap();
    }

    if !lyrics.is_empty() && available_lyric_height > 0 {
        let center_row = lyric_area_start + (available_lyric_height / 2);

        let active_text = &lyrics[current_idx].get_current_text();
        let active_lines = word_wrap_cjk(active_text, lyric_width);
        let active_block_start = center_row.saturating_sub((active_lines.len() / 2) as u16);

        for (i, line) in active_lines.iter().enumerate() {
            let r = active_block_start + i as u16;
            if r >= lyric_area_start && r < lyric_area_end {
                let (prefix, text_line) = if i == 0 {
                    (format!("{} ", "→".cyan()), line.white().bold())
                } else {
                    (format!("  "), line.white().bold())
                };

                queue!(
                    stdout,
                    cursor::MoveTo(2, r),
                    terminal::Clear(ClearType::CurrentLine),
                    Print(format!("{}{}", prefix, text_line))
                )
                .unwrap();
            }
        }

        let mut cursor_row = active_block_start;
        for i in (0..current_idx).rev() {
            if cursor_row <= lyric_area_start {
                break;
            }
            let lines = word_wrap_cjk(&lyrics[i].get_current_text(), lyric_width);
            let count = lines.len() as u16;
            if cursor_row < count {
                break;
            }
            let start_draw_row = cursor_row - count;

            for (j, line) in lines.iter().enumerate() {
                let r = start_draw_row + j as u16;
                if r >= lyric_area_start && r < active_block_start {
                    queue!(
                        stdout,
                        cursor::MoveTo(4, r),
                        Print(line.truecolor(95, 95, 95))
                    )
                    .unwrap();
                }
            }
            cursor_row = start_draw_row;
        }

        let mut cursor_row = active_block_start + (active_lines.len() as u16);
        for i in (current_idx + 1)..lyrics.len() {
            if cursor_row >= lyric_area_end {
                break;
            }
            let lines = word_wrap_cjk(&lyrics[i].get_current_text(), lyric_width);
            for line in lines {
                if cursor_row < lyric_area_end {
                    queue!(
                        stdout,
                        cursor::MoveTo(4, cursor_row),
                        Print(line.truecolor(95, 95, 95))
                    )
                    .unwrap();
                    cursor_row += 1;
                }
            }
        }
    }

    up_next = blindly_trim(&up_next);
    let safe_up_next = truncate_safe(up_next, width_usize.saturating_sub(2));

    queue!(
        stdout,
        cursor::MoveTo(0, footer_row_up_next),
        terminal::Clear(ClearType::CurrentLine),
        Print(format!("{}", safe_up_next.dimmed().italic()))
    )
    .unwrap();

    queue!(stdout, cursor::RestorePosition, cursor::Hide).unwrap();
    stdout.flush().unwrap();
}
