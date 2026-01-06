use crate::Track;
use crate::api::split_title_artist;
use crate::ui_common::{self, *};
use colored::*;
use crossterm::{
    cursor, queue,
    style::Print,
    terminal::{self, ClearType},
};
use std::io::{Write, stdout};
use std::sync::RwLock;
use std::sync::atomic::Ordering;

static UP_NEXT_TEXT: RwLock<String> = RwLock::new(String::new());

pub fn load_banner(track_opt: Option<&Track>, queue: &[String], _toggle: &str) {
    let mut stdout = stdout();
    let (_, rows) = terminal::size().unwrap_or((80, 24));

    queue!(
        stdout,
        cursor::Hide,
        cursor::MoveTo(0, 0),
        terminal::Clear(ClearType::All)
    )
    .unwrap();

    let next_text = if let Some(s) = queue.first() {
        let (title, _) = split_title_artist(s);
        format!("Up Next: {}", title)
    } else {
        "Up Next: ~".to_string()
    };
    *UP_NEXT_TEXT.write().unwrap() = next_text;

    let prompt_row = rows.saturating_sub(1);
    queue!(
        stdout,
        cursor::MoveTo(0, prompt_row),
        Print("\n"),
        cursor::Hide
    )
    .unwrap();

    if let Some(track) = track_opt {
        if !track.title.is_empty() {
            let mut current_song_guard = CURRENT_LYRIC_SONG.write().unwrap();

            if *current_song_guard != track.title {
                *current_song_guard = track.title.clone();

                let mut monitor_guard = SONG_MONITOR.write().unwrap();
                if let Some(stop_signal) = monitor_guard.take() {
                    stop_signal.store(true, Ordering::Relaxed);
                }

                let new_stop = start_monitor_thread(track.clone(), draw_minimal_ui);
                *monitor_guard = Some(new_stop);
            }
        }
    }
}

fn draw_minimal_ui(
    title: &str,
    artist: &str,
    _album: &str,
    curr: f64,
    tot: f64,
    lyrics: &[crate::features::LrcLine],
    current_idx: usize,
) {
    let mut stdout = stdout();
    let (cols, rows) = terminal::size().unwrap_or((80, 24));
    let width_usize = cols as usize;

    let lyric_start_row = 4;
    let up_next_row = rows.saturating_sub(2);
    let lyric_end_row = up_next_row.saturating_sub(1);

    let available_lyric_height = lyric_end_row.saturating_sub(lyric_start_row);
    let content_width = width_usize.saturating_sub(4);

    queue!(stdout, cursor::Hide, cursor::SavePosition).unwrap();

    let title_scroll = get_scrolling_text(title, content_width);
    let artist_scroll = crate::ui_common::get_scrolling_text(artist, content_width);

    let fmt_time = |s: f64| format!("{:02}:{:02}", (s / 60.0) as u64, (s % 60.0) as u64);
    let bar_width = width_usize.saturating_sub(16);
    let ratio = if tot > 0.0 { curr / tot } else { 0.0 };
    let filled = (ratio * bar_width as f64).round() as usize;
    let empty = bar_width.saturating_sub(filled);
    let bar_str = format!(
        "{}{}",
        "━".repeat(filled).cyan(),
        "─".repeat(empty).dimmed()
    );

    queue!(
        stdout,
        cursor::MoveTo(2, 0),
        terminal::Clear(ClearType::CurrentLine),
        Print(title_scroll.white().bold()),
        cursor::MoveTo(2, 1),
        terminal::Clear(ClearType::CurrentLine),
        Print(artist_scroll.dimmed()),
        cursor::MoveTo(2, 2),
        terminal::Clear(ClearType::CurrentLine),
        Print(format!(
            "{} {} {}",
            fmt_time(curr).cyan(),
            bar_str,
            fmt_time(tot).cyan()
        ))
    )
    .unwrap();

    for r in lyric_start_row..lyric_end_row {
        queue!(
            stdout,
            cursor::MoveTo(0, r),
            terminal::Clear(ClearType::CurrentLine)
        )
        .unwrap();
    }

    if !lyrics.is_empty() && available_lyric_height > 0 {
        let center_row = lyric_start_row + (available_lyric_height / 2);

        let active_text = &lyrics[current_idx].get_current_text();
        let active_lines = word_wrap_cjk(active_text, content_width);
        let active_block_start = center_row.saturating_sub((active_lines.len() / 2) as u16);

        for (i, line) in active_lines.iter().enumerate() {
            let r = active_block_start + i as u16;
            if r >= lyric_start_row && r < lyric_end_row {
                let prefix = if i == 0 { "→ ".cyan() } else { "  ".into() };
                queue!(
                    stdout,
                    cursor::MoveTo(2, r),
                    Print(format!("{}{}", prefix, line.white().bold()))
                )
                .unwrap();
            }
        }

        let mut cursor_row = active_block_start;
        for i in (0..current_idx).rev() {
            if cursor_row <= lyric_start_row {
                break;
            }
            let lines = word_wrap_cjk(&lyrics[i].get_current_text(), content_width);
            let count = lines.len() as u16;
            if cursor_row < count {
                break;
            }
            let start_draw_row = cursor_row - count;

            for (j, line) in lines.iter().enumerate() {
                let r = start_draw_row + j as u16;
                if r >= lyric_start_row && r < active_block_start {
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
            if cursor_row >= lyric_end_row {
                break;
            }
            let lines = word_wrap_cjk(&lyrics[i].get_current_text(), content_width);
            for line in lines {
                if cursor_row < lyric_end_row {
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

    let up_next_raw = UP_NEXT_TEXT.read().unwrap();
    let safe_up_next = crate::ui_common::truncate_safe(&up_next_raw, content_width);

    queue!(
        stdout,
        cursor::MoveTo(2, up_next_row),
        terminal::Clear(ClearType::CurrentLine),
        Print(safe_up_next.dimmed().italic()),
        cursor::RestorePosition,
        cursor::Hide
    )
    .unwrap();

    stdout.flush().unwrap();
}
