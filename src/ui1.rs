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

const STATUS_LINE_ROW: u16 = 12;
const QUEUE_SIZE: usize = 7;

pub use crate::ui_common::{show_playlists, show_songs, stop_lyrics};

pub fn load_banner(song_name_opt: Option<&str>, queue: &[String], toggle: &str) {
    let mut stdout = stdout();
    queue!(stdout, cursor::Hide, cursor::MoveTo(0, 0)).unwrap();

    queue!(stdout, Print(get_banner_art()), Print("\n\n")).unwrap();

    let header_text = if toggle == "recent" {
        "recent"
    } else {
        "queue"
    };
    queue!(
        stdout,
        Print(format!(
            "\n\n\n\n{}\n",
            format!("¨˜ˆ”°⍣~•{}•~⍣°”ˆ˜¨", header_text).cyan()
        ))
    )
    .unwrap();

    queue!(
        stdout,
        cursor::MoveTo(0, 18),
        terminal::Clear(ClearType::FromCursorDown)
    )
    .unwrap();

    if !queue.is_empty() {
        for (i, name) in queue.iter().enumerate().take(QUEUE_SIZE) {
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

    queue!(
        stdout,
        Print(format!("\n\n{}", ">> ".bright_blue().bold())),
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
    lyrics: &[crate::api::LrcLine],
    current_idx: usize,
) {
    let mut stdout = stdout();
    let fmt = |s: f64| format!("{:02}:{:02}", (s / 60.0) as u64, (s % 60.0) as u64);

    let artist_scroll = get_scrolling_text(artist, 25);
    let title_display = format!("{} [{}]", title, artist_scroll.dimmed());

    let current_text = if current_idx < lyrics.len() {
        &lyrics[current_idx].text
    } else {
        ""
    };
    let next_text = if current_idx + 1 < lyrics.len() {
        &lyrics[current_idx + 1].text
    } else {
        ""
    };

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
        cursor::MoveTo(0, STATUS_LINE_ROW),
        terminal::Clear(ClearType::CurrentLine),
        Print(format!(
            "[{} / {}] {}{: <80}",
            fmt(curr).cyan(),
            fmt(tot).cyan(),
            title_display.white().bold(),
            ""
        )),
        cursor::MoveTo(0, STATUS_LINE_ROW + 2),
        terminal::Clear(ClearType::CurrentLine),
        Print(format!("{: <85}", current_display)),
        cursor::MoveTo(0, STATUS_LINE_ROW + 3),
        terminal::Clear(ClearType::CurrentLine),
        Print(format!("{: <80}", next_text.dimmed().italic())),
        cursor::RestorePosition,
        cursor::Hide
    )
    .unwrap();
    stdout.flush().unwrap();
}
