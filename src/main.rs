mod api;
mod offline;
mod player;
mod ui1;
mod ui2;
mod ui3;
mod ui_common;
use crossterm::event::{self, Event};
use std::collections::VecDeque;
use std::process::Child;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{RwLock, mpsc};
use std::thread;
use std::time::Duration;

use crate::ui1::show_playlists;
/// -------------------------------------------------------------------
/// DATA STRUCTURES
/// -------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub title: String,
    pub url: String,
    pub video_id: Option<String>,
}

impl Track {
    pub fn new(title: String, url: String, video_id: Option<String>) -> Self {
        Self {
            title,
            url,
            video_id,
        }
    }
}

/// -------------------------------------------------------------------
/// GLOBAL STATE
/// -------------------------------------------------------------------

static SONG_QUEUE: RwLock<Vec<Track>> = RwLock::new(Vec::new());
// HOLDS just VIDEO ID of related songs upto 50 songs. SONG_QUEUE is populated by fetching urls from this list
static RELATED_SONG_LIST: RwLock<Vec<(String, String)>> = RwLock::new(Vec::new());
static RECENTLY_PLAYED: RwLock<VecDeque<Track>> = RwLock::new(VecDeque::new());
const HISTORY_LIMIT: usize = 50;

static VIEW_MODE: RwLock<String> = RwLock::new(String::new());
static UI_MODE: AtomicUsize = AtomicUsize::new(0);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    //collect arguments
    let args: Vec<String> = std::env::args().collect();
    let offline_mode = args.contains(&"--offline-playback".to_string());
    let no_autoplay = args.contains(&"--no-autoplay".to_string());
    *VIEW_MODE.write().unwrap() = "queue".to_string();

    print!("\x1B[2J\x1B[1;1H\n");

    //create music_dir and temp dir to store currently playing song
    let music_dir = player::prepare_music_dir()?;
    //Custom unofficial api
    let yt_client = api::YTMusic::new_with_cookies("cookies.txt").unwrap();

    let mut currently_playing: Option<Child> = None;
    let mut current_track: Option<Track> = None;

    // transmitter (sends any input for Search/Command)
    // receiver (running every 250 ms.. checks if song ended plays next in queue if yes)
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        loop {
            let mut s = String::new();
            if std::io::stdin().read_line(&mut s).is_ok() {
                let _ = tx.send(s.trim().to_string());
            }
        }
    });

    // STARTUP LOGIC
    if offline_mode {
        let exclude = get_excluded_titles();
        {
            let mut q = SONG_QUEUE.write().unwrap();
            offline::populate_queue_offline(&music_dir, &mut q, &exclude);
        }

        if let Some(track) = queue_next() {
            current_track = Some(track.clone());
            currently_playing = Some(player::play_file(&track.url, &track.title, &music_dir)?);

            let exclude = get_excluded_titles();
            {
                let mut q = SONG_QUEUE.write().unwrap();
                offline::populate_queue_offline(&music_dir, &mut q, &exclude);
            }
            refresh_ui(Some(&track.title));
        } else {
            println!("No local songs found in {:?}", music_dir);
            refresh_ui(Some("Nothing Playing"));
        }
    } else {
        refresh_ui(Some("Nothing Playing"));
    }

    loop {
        if event::poll(std::time::Duration::from_millis(0))? {
            match event::read()? {
                Event::Resize(_, _) => {
                    print!("\x1B[2J\x1B[1;1H\n");

                    refresh_ui(None);
                }
                _ => {}
            }
        }
        // firstly checks if song is playing
        if let Some(child) = &mut currently_playing {
            if let Ok(Some(_)) = child.try_wait() {
                // if playback of previous song completed
                if let Some(track) = &current_track {
                    let temp = music_dir.join("temp").join(format!("{}.webm", track.title));
                    let full = music_dir.join(format!("{}.webm", track.title));

                    // Rename only if the temp(previously played song fully buffered) exists
                    if temp.exists() {
                        std::fs::rename(&temp, &full).ok();
                    }

                    add_to_history(track.clone());
                }

                currently_playing = None;

                // Play Next
                if let Some(track) = queue_next() {
                    current_track = Some(track.clone());
                    currently_playing =
                        Some(player::play_file(&track.url, &track.title, &music_dir)?);

                    //default mode
                    if !no_autoplay {
                        if offline_mode {
                            let exclude = get_excluded_titles();
                            {
                                let mut q = SONG_QUEUE.write().unwrap();
                                offline::populate_queue_offline(&music_dir, &mut q, &exclude);
                            }
                        } else if let Some(vid) = &track.video_id {
                            let yt = yt_client.clone();
                            let v = vid.clone();
                            tokio::spawn(async move {
                                queue_auto_add_online(yt, v).await;
                            });
                        }
                    }
                    refresh_ui(Some(&track.title));
                } else {
                    current_track = None;
                    refresh_ui(Some("Nothing Playing"));
                }
            }
        }

        // check for any input from tx
        let input = match rx.try_recv() {
            Ok(s) => s,
            Err(_) => {
                // if nothing sleep for 250 ms for cpu relief (not the best idea)
                thread::sleep(Duration::from_millis(250));
                continue;
            }
        };

        if input.is_empty() {
            print!("\x1B[2J\x1B[1;1H\n");
            refresh_ui(None);
            continue;
        }

        // special commands
        match input.to_lowercase().as_str() {
            "q" | "quit" => {
                if let Some(track) = &current_track {
                    add_to_history(track.clone());
                    player::stop_process(&mut currently_playing, &track.title, &music_dir);
                }
                refresh_ui(None);
                break;
            }
            "s" | "stop" => {
                if let Some(track) = &current_track {
                    add_to_history(track.clone());
                    player::stop_process(&mut currently_playing, &track.title, &music_dir);
                }
                current_track = None;
                refresh_ui(Some("Nothing Playing"));
                continue;
            }
            "c" | "clear" => {
                SONG_QUEUE.write().unwrap().clear();
                refresh_ui(None);
                continue;
            }
            "p" | "pause" => {
                if currently_playing.is_some() {
                    player::toggle_pause();
                }
                refresh_ui(None);
                continue;
            }

            "u" | "user" => {
                let user_status = yt_client
                    .fetch_account_name()
                    .await
                    .unwrap_or("Error".to_string());
                println!("Status: {}", user_status);
                continue;
            }
            "v" | "view" => {
                let current_mode = UI_MODE.load(Ordering::Relaxed);

                match current_mode {
                    0 => ui1::stop_lyrics(),
                    1 => ui2::stop_lyrics(),
                    2 => ui3::stop_lyrics(),
                    _ => {}
                }

                let next_ui_mode = (current_mode + 1) % 3;
                UI_MODE.store(next_ui_mode, Ordering::Relaxed);

                print!("\x1B[2J\x1B[1;1H\n");

                let title_ref = current_track.as_ref().map(|t| t.title.as_str());
                refresh_ui(title_ref);

                continue;
            }
            "t" | "toggle" => {
                {
                    let mut mode = VIEW_MODE.write().unwrap();
                    *mode = if *mode == "queue" {
                        "recent".to_string()
                    } else {
                        "queue".to_string()
                    };
                }
                refresh_ui(None);
                continue;
            }
            "n" | "next" => {
                if let Some(track) = &current_track {
                    add_to_history(track.clone());
                    player::stop_process(&mut currently_playing, &track.title, &music_dir);
                }

                if let Some(track) = queue_next() {
                    current_track = Some(track.clone());
                    currently_playing =
                        Some(player::play_file(&track.url, &track.title, &music_dir)?);

                    if !no_autoplay {
                        if offline_mode {
                            let exclude = get_excluded_titles();
                            {
                                let mut q = SONG_QUEUE.write().unwrap();
                                offline::populate_queue_offline(&music_dir, &mut q, &exclude);
                            }
                        } else if let Some(vid) = &track.video_id {
                            let yt = yt_client.clone();
                            let v = vid.clone();
                            tokio::spawn(async move {
                                queue_auto_add_online(yt, v).await;
                            });
                        }
                    }
                    refresh_ui(Some(&track.title));
                } else {
                    current_track = None;
                    refresh_ui(Some("Nothing Playing"));
                }
                continue;
            }

            "b" | "back" => {
                if let Some(track) = &current_track {
                    player::stop_process(&mut currently_playing, &track.title, &music_dir);
                    queue_add_front(track.clone());
                }

                if let Some(prev_track) = get_prev_track() {
                    current_track = Some(prev_track.clone());
                    currently_playing = Some(player::play_file(
                        &prev_track.url,
                        &prev_track.title,
                        &music_dir,
                    )?);
                    refresh_ui(Some(&prev_track.title));
                } else {
                    refresh_ui(None);
                }
                continue;
            }
            _ => {}
        }

        if input.starts_with('>') || input.starts_with('<') {
            if let Ok(s) = input[1..].trim().parse::<i64>() {
                player::seek(if input.starts_with('<') { -s } else { s });
            }
            refresh_ui(None);
            continue;
        }

        //search using custom api
        if offline_mode {
            refresh_ui(None);
            continue;
        }

        let mut songs: Vec<api::SongDetails> = Vec::new();
        if input != "l" && input != "library" {
            songs = match yt_client.search_songs(&input, 5).await {
                Ok(s) => s,
                Err(_) => {
                    println!("Search failed.");
                    continue;
                }
            };

            if songs.is_empty() {
                println!("No results.");
                refresh_ui(None);
                continue;
            }

            //Auto Selection for minimal mode (ui=0)
            if UI_MODE.load(Ordering::Relaxed) == 2 {
                let auto_selected = &songs[0];

                let path = music_dir.join(format!("{}.webm", auto_selected.title));

                let src = if path.exists() {
                    path.to_string_lossy().to_string()
                } else {
                    match yt_client.fetch_stream_url(&auto_selected.video_id).await {
                        Ok(u) => u,
                        Err(_) => {
                            println!("Failed to get URL");
                            continue;
                        }
                    }
                };

                let new_track = Track::new(
                    auto_selected.title.clone(),
                    src,
                    Some(auto_selected.video_id.clone()),
                );
                if let Some(track) = &current_track {
                    add_to_history(track.clone());
                    player::stop_process(&mut currently_playing, &track.title, &music_dir);
                }

                current_track = Some(new_track.clone());
                currently_playing = Some(player::play_file(
                    &new_track.url,
                    &new_track.title,
                    &music_dir,
                )?);
                if !no_autoplay {
                    //add similar songs in background
                    let yt = yt_client.clone();
                    let vid = auto_selected.video_id.clone();
                    SONG_QUEUE.write().unwrap().clear();
                    RELATED_SONG_LIST.write().unwrap().clear();
                    tokio::spawn(async move {
                        queue_auto_add_online(yt, vid).await;
                    });
                }
                refresh_ui(Some(&new_track.title));

                continue;
            }
            //Auto Selection code over
        } else {
            println!("Fetching Library...");
            let playlists = yt_client.fetch_library_playlists().await?;
            show_playlists(&playlists);
            if let Ok(sel_str) = rx.recv() {
                let sel = sel_str.trim().parse::<usize>().unwrap_or(0);

                if sel >= 1 && sel <= playlists.len() {
                    let selected_playlist = &playlists[sel - 1];
                    println!("Loading '{}'...", selected_playlist.title);

                    songs = yt_client
                        .fetch_playlist_songs(&selected_playlist.playlist_id, 100)
                        .await?;
                }
                refresh_ui(None);
            }
        }

        ui1::show_songs(&songs);

        // check if q is entered before index
        if let Ok(sel) = rx.recv() {
            let (idx, is_queue) = if sel.to_lowercase().starts_with('q') {
                (sel[1..].parse::<usize>().unwrap_or(0), true)
            } else {
                (sel.parse::<usize>().unwrap_or(0), false)
            };

            if idx >= 1 && idx <= songs.len() {
                let selected = &songs[idx - 1];
                let path = music_dir.join(format!("{}.webm", selected.title));

                let src = if path.exists() {
                    path.to_string_lossy().to_string()
                } else {
                    match yt_client.fetch_stream_url(&selected.video_id).await {
                        Ok(u) => u,
                        Err(_) => {
                            println!("Failed to get URL");
                            continue;
                        }
                    }
                };

                let new_track =
                    Track::new(selected.title.clone(), src, Some(selected.video_id.clone()));

                if is_queue {
                    queue_add(new_track);
                    refresh_ui(None);
                } else {
                    if let Some(track) = &current_track {
                        add_to_history(track.clone());
                        player::stop_process(&mut currently_playing, &track.title, &music_dir);
                    }

                    current_track = Some(new_track.clone());
                    currently_playing = Some(player::play_file(
                        &new_track.url,
                        &new_track.title,
                        &music_dir,
                    )?);

                    if !no_autoplay {
                        //add similar songs in background
                        let yt = yt_client.clone();
                        let vid = selected.video_id.clone();
                        SONG_QUEUE.write().unwrap().clear();
                        RELATED_SONG_LIST.write().unwrap().clear();
                        tokio::spawn(async move {
                            queue_auto_add_online(yt, vid).await;
                        });
                    }
                    refresh_ui(Some(&new_track.title));
                }
            } else {
                print!("\x1B[2J\x1B[1;1H\n");
                refresh_ui(None);
            }
        }
    }
    Ok(())
}

/// -------------------------------------------------------------------
/// QUEUE & MPV IPC & PLAYBACK
/// -------------------------------------------------------------------

fn refresh_ui(song_name: Option<&str>) {
    let mode = VIEW_MODE.read().unwrap().clone();
    let ui_mode = UI_MODE.load(Ordering::Relaxed);
    // prepare data to send
    let titles: Vec<String> = if mode == "recent" {
        let h = RECENTLY_PLAYED.read().unwrap();
        h.iter().map(|t| t.title.clone()).rev().collect()
    } else {
        let q = SONG_QUEUE.read().unwrap();
        q.iter().map(|t| t.title.clone()).collect()
    };

    if ui_mode == 0 {
        ui1::load_banner(song_name, &titles, &mode);
    } else if ui_mode == 1 {
        ui2::load_banner(song_name, &titles, &mode);
    } else if ui_mode == 2 {
        ui3::load_banner(song_name, &titles, &mode);
    }
}

fn add_to_history(track: Track) {
    let mut list = RECENTLY_PLAYED.write().unwrap();
    if let Some(last) = list.back() {
        if last.title == track.title {
            return;
        }
    }
    if list.len() >= HISTORY_LIMIT {
        list.pop_front();
    }
    list.push_back(track);
}

fn get_prev_track() -> Option<Track> {
    let mut list = RECENTLY_PLAYED.write().unwrap();
    list.pop_back()
}

fn queue_add(track: Track) {
    let mut q = SONG_QUEUE.write().unwrap();
    q.push(track);
}

fn queue_add_front(track: Track) {
    let mut q = SONG_QUEUE.write().unwrap();
    q.insert(0, track);
}

fn queue_next() -> Option<Track> {
    let mut q = SONG_QUEUE.write().unwrap();
    if !q.is_empty() {
        Some(q.remove(0))
    } else {
        None
    }
}

fn get_excluded_titles() -> Vec<String> {
    let mut titles = Vec::new();
    let history = RECENTLY_PLAYED.read().unwrap();
    let queue = SONG_QUEUE.read().unwrap();
    titles.extend(history.iter().map(|t| t.title.clone()));
    titles.extend(queue.iter().map(|t| t.title.clone()));
    titles
}

pub async fn queue_auto_add_online(yt: api::YTMusic, id: String) {
    //check if queue is almost over
    let needs_songs = {
        let q = SONG_QUEUE.read().unwrap();
        q.len() < 2
    };

    if needs_songs {
        // check if saved related videoIDs are exhausted
        let cache_empty = {
            let c = RELATED_SONG_LIST.read().unwrap();
            c.is_empty()
        };

        if cache_empty {
            if let Ok(related) = yt.fetch_related_songs(&id, 50).await {
                // println!("\n\nLooking for similar songs...");
                let mut c = RELATED_SONG_LIST.write().unwrap();
                for song in related {
                    c.push((song.title, song.video_id));
                }
            }
        }

        //cannot hold lock during await :(  So extra vec
        let mut to_fetch = Vec::new();
        {
            let mut c = RELATED_SONG_LIST.write().unwrap();
            for _ in 0..5 {
                if let Some(item) = c.pop() {
                    to_fetch.push(item);
                } else {
                    break;
                }
            }
        }

        // Resolve stream URLs and add to queue
        for (title, video_id) in to_fetch {
            if let Ok(url) = yt.fetch_stream_url(&video_id).await {
                queue_add(Track::new(title, url, Some(video_id)));
            }
        }
        refresh_ui(None);
    }
}
