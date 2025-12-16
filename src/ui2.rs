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

const PROGRESS_ROW: u16 = 12;
const CONTENT_START_ROW: u16 = 15;
const LYRIC_COLUMN: u16 = 32;
const QUEUE_SIZE: usize = 7;
const PROMPT_ROW: u16 = CONTENT_START_ROW + (QUEUE_SIZE as u16) + 2;

pub use crate::ui_common::{show_playlists, show_songs, stop_lyrics};

pub fn load_banner(song_name_opt: Option<&str>, queue: &[String], toggle: &str) {
    let mut stdout = stdout();
    queue!(stdout, cursor::Hide, cursor::MoveTo(0, 0)).unwrap();

    queue!(stdout, Print(get_banner_art())).unwrap();

    let queue_header = if toggle == "recent" {
        "RECENT"
    } else {
        "QUEUE"
    };
    queue!(
        stdout,
        cursor::MoveTo(0, CONTENT_START_ROW - 1),
        Print(
            format!("¨˜ˆ”°⍣~•{}•~⍣°”ˆ˜¨", queue_header)
                .bright_cyan()
                .bold()
                .dimmed()
        )
    )
    .unwrap();

    queue!(
        stdout,
        cursor::MoveTo(LYRIC_COLUMN, CONTENT_START_ROW - 1),
        Print(" ¨˜ˆ”°⍣~•LYRICS•~⍣°”ˆ˜¨".cyan().bold().dimmed())
    )
    .unwrap();

    for i in 0..QUEUE_SIZE {
        queue!(
            stdout,
            cursor::MoveTo(0, CONTENT_START_ROW + (i as u16) + 1),
            terminal::Clear(ClearType::UntilNewLine)
        )
        .unwrap();
        if i < queue.len() {
            let (clean_name, _) = split_title_artist(&queue[i]);
            let max_len = (LYRIC_COLUMN as usize).saturating_sub(5);
            let safe_name = truncate_safe(&clean_name, max_len);

            let display_str = format!("{}. {}", i + 1, safe_name);
            let styled = match i {
                0 => display_str.truecolor(255, 255, 255).bold(),
                1 => display_str.truecolor(220, 220, 220),
                2 => display_str.truecolor(180, 180, 180),
                _ => display_str.truecolor(100, 100, 100),
            };
            queue!(stdout, Print(styled)).unwrap();
        } else {
            queue!(stdout, Print(" ")).unwrap();
        }
    }

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
    lyrics: &[crate::api::LrcLine],
    current_idx: usize,
) {
    let mut stdout = stdout();
    let fmt = |s: f64| format!("{:02}:{:02}", (s / 60.0) as u64, (s % 60.0) as u64);
    let (term_cols, _) = terminal::size().unwrap_or((80, 24));
    let max_lyric_width = (term_cols.saturating_sub(LYRIC_COLUMN) as usize).saturating_sub(2);

    let artist_scroll = get_scrolling_text(artist, 25);
    let display_title = truncate_safe(title, 35);

    queue!(
        stdout,
        cursor::Hide,
        cursor::SavePosition,
        cursor::MoveTo(0, PROGRESS_ROW),
        terminal::Clear(ClearType::CurrentLine),
        Print(format!(
            "{} [{} / {}] {} [{}]",
            "▶︎".cyan(),
            fmt(curr).cyan(),
            fmt(tot).cyan(),
            display_title.white().bold(),
            artist_scroll.dimmed().italic()
        ))
    )
    .unwrap();

    if max_lyric_width > 5 {
        for offset in 0..7 {
            let target_idx = current_idx + offset;
            let text = if target_idx < lyrics.len() {
                &lyrics[target_idx].text
            } else {
                ""
            };

            let safe_text = truncate_safe(text, max_lyric_width);
            let styled = match offset {
                0 => safe_text.white().bold(),
                1 => safe_text.truecolor(200, 200, 200),
                2 => safe_text.truecolor(150, 150, 150),
                _ => safe_text.truecolor(100, 100, 100),
            };

            queue!(
                stdout,
                cursor::MoveTo(LYRIC_COLUMN, CONTENT_START_ROW + offset as u16 + 1),
                terminal::Clear(ClearType::UntilNewLine),
                Print(styled)
            )
            .unwrap();
        }
    }

    queue!(stdout, cursor::RestorePosition, cursor::Show).unwrap();
    stdout.flush().unwrap();
}
