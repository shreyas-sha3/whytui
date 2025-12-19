mod api;
mod offline;
mod player;
mod ui1;
mod ui2;
mod ui3;
mod ui_common;
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{self, Clear, ClearType},
};
use std::collections::VecDeque;
use std::io::stdout;
use std::process::Child;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{RwLock, mpsc};
use std::thread;
use std::time::Duration;

use crate::api::SongDetails;
use crate::ui1::{show_playlists, show_songs};
// -------------------------------------------------------------------
// DATA STRUCTURES
// -------------------------------------------------------------------

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

// ----------------------------------------------------------------------------------
// GLOBAL STATE
// ----------------------------------------------------------------------------------

static SONG_QUEUE: RwLock<Vec<Track>> = RwLock::new(Vec::new());
// HOLDS just VIDEO ID of related songs upto 50 songs. SONG_QUEUE is populated by fetching urls from this list
static RELATED_SONG_LIST: RwLock<Vec<(String, String)>> = RwLock::new(Vec::new());
static RECENTLY_PLAYED: RwLock<VecDeque<Track>> = RwLock::new(VecDeque::new());
const HISTORY_LIMIT: usize = 50;

static VIEW_MODE: RwLock<String> = RwLock::new(String::new());
static UI_MODE: AtomicUsize = AtomicUsize::new(0);

//LIST OF SONGS FROM A PLAYLIST
static LIBRARY_SONG_LIST: RwLock<Vec<SongDetails>> = RwLock::new(Vec::new());
//CONTAINS PLAYLIST ID SO AUTOPLAY CAN FETCH FROM THE SAME LIBRARY
static PLAYING_FROM_LIBRARY: RwLock<Option<String>> = RwLock::new(None);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ----------------------------------------------------------------------------------
    // PART 1 - GET ARGUMENTS, INITIAL GLOBAL (STATIC) VARIABLES
    // ----------------------------------------------------------------------------------
    let args: Vec<String> = std::env::args().collect();
    let offline_mode = args.contains(&"--offline".to_string());
    let no_autoplay = args.contains(&"--manual".to_string());

    //set default view mode to queue
    *VIEW_MODE.write().unwrap() = "queue".to_string();
    //create music_dir and temp dir to store currently playing song
    let music_dir = player::prepare_music_dir()?;
    //set cookie path
    let cookies_path = music_dir.join("config/cookies.txt");
    //Custom unofficial apiz ( call with cookies if available)
    let yt_client = api::YTMusic::new_with_cookies(cookies_path.to_str().unwrap()).unwrap();
    //mpv handle to extract child and stop songs if needed
    let mut currently_playing: Option<Child> = None;
    //contains song details (including vid_id for online songs)
    let mut current_track: Option<Track> = None;
    // CLEAR SCREEN BEFORE STARTING THE REAL SHIT
    execute!(stdout(), Clear(ClearType::All));
    //
    //
    //
    //
    //

    // ----------------------------------------------------------------------------------
    // PART 2 - SETUP TRANSMITTER, RECIEVER CHANNEL FOR POLLING INPUT
    //          transmitter (sends any input for Search/Command)
    //          receiver (sleeps every 250 ms if not input)
    // ----------------------------------------------------------------------------------

    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        loop {
            let mut s = String::new();
            if std::io::stdin().read_line(&mut s).is_ok() {
                let _ = tx.send(s.trim().to_string());
            }
        }
    });

    //
    //
    //
    //
    //

    // ----------------------------------------------------------------------------------
    // PART 3 - INITIALIZATION
    // ----------------------------------------------------------------------------------

    // ----------------------------------------------------------------------------------
    // CASE 1 : IF OFFLINE MODE INITIAL FETCH RANDOM SONG + POPULATE QUEUE
    // ----------------------------------------------------------------------------------
    if offline_mode {
        let exclude = get_excluded_titles();
        {
            let mut q = SONG_QUEUE.write().unwrap();
            offline::populate_queue_offline(&music_dir, &mut q, &exclude);
        }

        if let Some(track) = queue_next() {
            current_track = Some(track.clone()); //to pass it around to functions like next song
            currently_playing = Some(player::play_file(&track.url, &track.title, &music_dir)?); //object to stop the current song
            refresh_ui(Some(&track.title));
        } else {
            println!("No local songs found in {:?}", music_dir);
            refresh_ui(Some("Nothing Playing"));
        }
    }
    // ----------------------------------------------------------------------------------
    // CASE 2 : IF ONLINCE MODE TRY TO CONNECT TO API AND FETCH USERNAME
    // ----------------------------------------------------------------------------------
    else {
        let user_status = yt_client
            .fetch_account_name()
            .await
            .unwrap_or("Error".to_string());
        let login_message = format!("\n Hello {}! | Nothing Playing ", user_status);
        refresh_ui(Some(&login_message));
    }

    //
    //
    //
    //
    //

    // -------------------------------------------------------------------
    // START OF GAME LOOP (BOTH ONLINE,OFFLINE)
    // -------------------------------------------------------------------
    loop {
        if event::poll(Duration::from_millis(0))? {
            match event::read()? {
                Event::Resize(_, _) => {
                    execute!(stdout(), Clear(ClearType::All))?;
                    refresh_ui(None);
                }
                _ => {}
            }
        }

        // -------------------------------------------------------------------
        // PART 4 - CHECK IF A SONG IS PLAYING ALREADY
        // -------------------------------------------------------------------
        if let Some(child) = &mut currently_playing {
            // Reaping previous song instance thus mutable reference needed ( also to check if finished naturally)
            // if finished fully move song from /temp folder to music_dir
            //
            if let Ok(Some(_)) = child.try_wait() {
                // -------------------------------------------------------------------
                // CASE 1 : A SONG IS CURRENTLY BEING PLAYED
                // -------------------------------------------------------------------
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

                // -------------------------------------------------------------------
                // CASE 2 : WHEN NO SONG IS PLAYING CURRENTLY
                // -------------------------------------------------------------------
                if let Some(track) = queue_next() {
                    //Check queue first - If yes play next in queue
                    current_track = Some(track.clone());
                    currently_playing =
                        Some(player::play_file(&track.url, &track.title, &music_dir)?);

                    // -------------------------------------------------------------------
                    // CASE 2.1 : IF AUTOPLAY IS ENABLED (DEFAULT MODE)
                    // -------------------------------------------------------------------
                    if !no_autoplay {
                        // -------------------------------------------------------------------
                        // CASE 2.1.1 : IF USER IS IN OFFLINE MODE (POPULATE FROM OFFLINE.RS)
                        // -------------------------------------------------------------------
                        if offline_mode {
                            let exclude = get_excluded_titles();
                            {
                                let mut q = SONG_QUEUE.write().unwrap();
                                offline::populate_queue_offline(&music_dir, &mut q, &exclude);
                            }
                        }
                        // -------------------------------------------------------------------
                        // CASE 2.1.2 : IF USER IS IN ONLINE MODE (CALL AUTO ADD FUNCTION)
                        // -------------------------------------------------------------------
                        else if let Some(vid) = &track.video_id {
                            let yt = yt_client.clone();
                            let v = vid.clone();
                            tokio::spawn(async move {
                                queue_auto_add_online(yt, v).await;
                            });
                        }
                    }
                    refresh_ui(Some(&track.title));
                }
                // -------------------------------------------------------------------
                // CASE 2.1 : IF AUTOPLAY IS DISABLED (JUST STOP PLAYBACK)
                // -------------------------------------------------------------------
                else {
                    current_track = None;
                    refresh_ui(Some("Nothing Playing"));
                }
            }
        }

        //
        //
        //
        //
        //

        // -------------------------------------------------------------------
        // PART 5 - CHECK FOR ANY INPUT FROM USER VIA RX
        // -------------------------------------------------------------------
        let input = match rx.try_recv() {
            Ok(s) => s,
            Err(_) => {
                // if nothing sleep for 250 ms for cpu relief (not the best idea)
                thread::sleep(Duration::from_millis(250));
                continue;
            }
        };

        // -------------------------------------------------------------------
        // CASE 1 : IF USER SIMLPY PRESSED ENTER REFRESH UI TO FIX ANY SCROLL
        // -------------------------------------------------------------------
        if input.is_empty() {
            execute!(stdout(), Clear(ClearType::All));
            refresh_ui(None);
            continue;
        }

        // -------------------------------------------------------------------
        // CASE 2 : CHECK IF USER HAS GIVEN ANY SPECIAL COMMAND
        // -------------------------------------------------------------------
        if handle_global_commands(
            &input,
            &rx, // Passed RX so library can use it
            &yt_client,
            &mut current_track,
            &mut currently_playing,
            &music_dir,
            offline_mode,
            no_autoplay,
        )
        .await
        {
            continue;
        }

        // -------------------------------------------------------------------
        // CASE 3 : IF NONE OF THE ABOVE AND IN OFFLINE MODE CONTINUE
        //          (since search not allowed on offline mode)
        // -------------------------------------------------------------------
        if offline_mode {
            refresh_ui(None);
            continue;
        }

        // -------------------------------------------------------------------
        // CASE 4 : IF NONE OF THE ABOVE AND IN ONLINE MODE,
        //          USE RECIEVED TEXT TO SEARCH CUSTOM API
        // -------------------------------------------------------------------
        let mut songs: Vec<api::SongDetails> = Vec::new();
        songs = match yt_client.search_songs(&input, 5).await {
            Ok(s) => s,
            Err(_) => {
                println!("Search failed.");
                continue;
            }
        };

        // -------------------------------------------------------------------
        // CASE 4.1 : IF NO RESULTS SIMPLY REFRESH UI
        // -------------------------------------------------------------------
        if songs.is_empty() {
            println!("No results.");
            refresh_ui(None);
            continue;
        }

        //
        //
        //
        //

        // -------------------------------------------------------------------
        // PART 6 - IF NONE OF THE ABOVE AND IN ONLINE MODE,
        //          USE RECIEVED TEXT TO SEARCH CUSTOM API
        // -------------------------------------------------------------------
        show_songs(&songs);
        // -------------------------------------------------------------------
        // CASE 1 : IF USING MINIMAL UI SIMULATE AUTO SELECTING FIRST RESULT
        // -------------------------------------------------------------------

        // if we are searching specify that we are not in a playlist anymore
        {
            *PLAYING_FROM_LIBRARY.write().unwrap() = None;
        }
        if UI_MODE.load(Ordering::Relaxed) == 2 {
            // simulate selecting the first result
            handle_song_selection(
                "1".to_string(),
                &songs,
                &music_dir,
                &yt_client,
                &mut current_track,
                &mut currently_playing,
                no_autoplay,
                None,
            )
            .await
            .unwrap_or_else(|e| println!("Auto-select error: {}", e));
            //finish this loop
            continue;
        }

        // -------------------------------------------------------------------
        // CASE 2 : IF IN OTHER UI MODE TAKE INPUT FROM USER FOR SELECTION
        // -------------------------------------------------------------------
        if let Ok(sel_str) = rx.recv() {
            handle_song_selection(
                sel_str,
                &songs,
                &music_dir,
                &yt_client,
                &mut current_track,
                &mut currently_playing,
                no_autoplay,
                None,
            )
            .await
            .unwrap_or_else(|e| println!("Error playing song: {}", e));
        }
    }
}

use std::path::PathBuf;
async fn handle_global_commands(
    input: &str,
    rx: &std::sync::mpsc::Receiver<String>, //to give to library
    yt_client: &api::YTMusic,
    current_track: &mut Option<Track>,
    currently_playing: &mut Option<Child>,
    music_dir: &std::path::PathBuf,
    offline_mode: bool,
    no_autoplay: bool,
) -> bool {
    // Seek check
    if input.starts_with('>') || input.starts_with('<') {
        if let Ok(s) = input[1..].trim().parse::<i64>() {
            player::seek(if input.starts_with('<') { -s } else { s });
        }
        refresh_ui(None);
        return true;
    }

    // special commands
    match input {
        "q" | "quit" => {
            if let Some(track) = current_track {
                add_to_history(track.clone());
                player::stop_process(currently_playing, &track.title, music_dir);
            }
            refresh_ui(None);
            std::process::exit(0);
        }
        "s" | "stop" => {
            if let Some(track) = current_track {
                add_to_history(track.clone());
                player::stop_process(currently_playing, &track.title, music_dir);
            }
            *current_track = None;
            refresh_ui(Some("Nothing Playing"));
            return true;
        }
        "c" | "clear" => {
            SONG_QUEUE.write().unwrap().clear();
            refresh_ui(None);
            return true;
        }
        s if s.chars().all(|c| c == '-' || c == '+') => {
            let delta: i64 = s.chars().map(|c| if c == '+' { 1 } else { -1 }).sum();
            player::vol_change(delta);
            refresh_ui(None);
            return true;
        }
        "p" | "pause" => {
            if currently_playing.is_some() {
                player::toggle_pause();
            }
            refresh_ui(None);
            return true;
        }
        "l" | "like" => {
            // If track is playing,playing track has a vid_id
            if let Some(track) = current_track {
                if let Some(vid) = &track.video_id {
                    let yt = yt_client.clone();
                    let video_id = vid.clone();
                    let title = track.title.clone();

                    tokio::spawn(async move {
                        match yt.like_song(&video_id).await {
                            Ok(_) => {
                                println!("'{}' Added to Liked Songs", title);
                            }
                            Err(e) => eprintln!("Error liking song: {}", e),
                        }
                    });
                }
            }
            refresh_ui(None);
            return true;
        }
        "u" | "user" => {
            let user_status = yt_client
                .fetch_account_name()
                .await
                .unwrap_or("Error".to_string());
            println!("Status: {}", user_status);
            return true;
        }
        //toggle between the ui modes
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

            execute!(stdout(), Clear(ClearType::All));

            let title_ref = current_track.as_ref().map(|t| t.title.as_str());
            refresh_ui(title_ref);

            return true;
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
            return true;
        }
        //next song in queue can also be done as n#
        s if s == "n"
            || s == "next"
            || (s.starts_with('n') && s[1..].chars().all(|c| c.is_ascii_digit())) =>
        {
            let steps = if s == "n" || s == "next" {
                1
            } else {
                s[1..].parse::<usize>().unwrap_or(1)
            };

            for _ in 0..steps {
                if let Some(track) = current_track {
                    add_to_history(track.clone());
                    player::stop_process(currently_playing, &track.title, music_dir);
                }

                if let Some(track) = queue_next() {
                    *current_track = Some(track.clone());
                    *currently_playing =
                        Some(player::play_file(&track.url, &track.title, music_dir).unwrap());

                    if !no_autoplay {
                        if offline_mode {
                            let exclude = get_excluded_titles();
                            let mut q = SONG_QUEUE.write().unwrap();
                            offline::populate_queue_offline(music_dir, &mut q, &exclude);
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
                    *current_track = None;
                    refresh_ui(Some("Nothing Playing"));
                    break;
                }

                continue;
            }
            return true;
        }
        "b" | "back" => {
            if let Some(track) = current_track {
                player::stop_process(currently_playing, &track.title, music_dir);
                queue_add_front(track.clone());
            }

            if let Some(prev_track) = get_prev_track() {
                *current_track = Some(prev_track.clone());
                *currently_playing =
                    Some(player::play_file(&prev_track.url, &prev_track.title, music_dir).unwrap());
                refresh_ui(Some(&prev_track.title));
            } else {
                refresh_ui(None);
            }
            return true;
        }
        "L" | "library" => {
            if offline_mode {
                refresh_ui(None);
                return true;
            }
            //give rx to library helper
            if let Err(e) = handle_library_browsing(
                rx,
                yt_client,
                music_dir,
                current_track,
                currently_playing,
                no_autoplay,
            )
            .await
            {
                println!("Error in Library: {}", e);
            }

            refresh_ui(None);
            return true;
        }
        _ => return false,
    }
}

pub async fn handle_song_selection(
    selection_input: String,
    songs_list: &[api::SongDetails],
    music_dir: &PathBuf,
    yt_client: &api::YTMusic,
    current_track: &mut Option<Track>,
    currently_playing: &mut Option<Child>,
    no_autoplay: bool,
    playlist_context: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = selection_input.trim();
    let (idx, is_queue) = if input.to_lowercase().starts_with('q') {
        (input[1..].parse::<usize>().unwrap_or(0), true)
    } else {
        (input.parse::<usize>().unwrap_or(0), false)
    };

    if idx >= 1 && idx <= songs_list.len() {
        let selected = &songs_list[idx - 1];
        let path = music_dir.join(format!("{}.webm", selected.title));

        let src = if path.exists() {
            path.to_string_lossy().to_string()
        } else {
            match yt_client.fetch_stream_url(&selected.video_id).await {
                Ok(u) => u,
                Err(_) => return Ok(()),
            }
        };

        let new_track = Track::new(selected.title.clone(), src, Some(selected.video_id.clone()));

        if is_queue {
            queue_add(new_track);
            refresh_ui(None);
        } else {
            if let Some(track) = current_track {
                add_to_history(track.clone());
                player::stop_process(currently_playing, &track.title, music_dir);
            }

            {
                let mut guard = PLAYING_FROM_LIBRARY.write().unwrap();
                *guard = playlist_context;
            }

            *current_track = Some(new_track.clone());
            *currently_playing = Some(player::play_file(
                &new_track.url,
                &new_track.title,
                music_dir,
            )?);

            if !no_autoplay {
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
        refresh_ui(None);
    }

    Ok(())
}

async fn handle_library_browsing(
    rx: &std::sync::mpsc::Receiver<String>,
    yt_client: &api::YTMusic,
    music_dir: &std::path::PathBuf,
    current_track: &mut Option<Track>,
    currently_playing: &mut Option<Child>,
    no_autoplay: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Fetching Library...");
    let playlists = yt_client.fetch_library_playlists().await?;
    show_playlists(&playlists);

    if let Ok(sel_str) = rx.recv() {
        let sel = sel_str.trim().parse::<usize>().unwrap_or(0);
        if sel >= 1 && sel <= playlists.len() {
            let selected_playlist = &playlists[sel - 1];
            println!("Loading '{}'...", selected_playlist.title);

            {
                println!("Fetching first 100 songs...");
                let fetched_songs = yt_client
                    .fetch_playlist_songs(&selected_playlist.playlist_id, 100)
                    .await?;
                *LIBRARY_SONG_LIST.write().unwrap() = fetched_songs;
            }

            let mut page: usize = 1;

            loop {
                refresh_ui(None);
                println!(" [n] Next | [p] Prev");

                {
                    let songs = LIBRARY_SONG_LIST.read().unwrap();
                    let start = (page - 1) * 5;
                    if start < songs.len() {
                        let limit = std::cmp::min(start + 5, songs.len());
                        ui1::show_songs(&songs[start..limit].to_vec());
                    } else {
                        println!("--- End of Playlist ---");

                        if page > 1 {
                            page -= 1;
                        }
                    }
                }

                if let Ok(input) = rx.recv() {
                    let what_to_do_now = input.as_str().trim();
                    if what_to_do_now == "n" {
                        page += 1;
                    } else if what_to_do_now == "p" {
                        if page > 1 {
                            page -= 1;
                        }
                    } else if what_to_do_now == "b" {
                        break;
                    } else if let Ok(num) = what_to_do_now.parse::<usize>() {
                        let selected_song = {
                            let songs = LIBRARY_SONG_LIST.read().unwrap();
                            let actual_idx = (page - 1) * 5 + (num - 1);
                            if actual_idx < songs.len() {
                                Some(songs[actual_idx].clone())
                            } else {
                                None
                            }
                        };

                        if let Some(song) = selected_song {
                            handle_song_selection(
                                "1".to_string(),
                                &[song],
                                music_dir,
                                yt_client,
                                current_track,
                                currently_playing,
                                no_autoplay,
                                Some(selected_playlist.playlist_id.clone()),
                            )
                            .await?;

                            break;
                        }
                    }
                }
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
            let playlist_context = {
                let guard = PLAYING_FROM_LIBRARY.read().unwrap();
                guard.clone()
            };
            if let Ok(related) = yt
                .fetch_related_songs(&id, playlist_context.as_deref(), 50)
                .await
            {
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
                if !c.is_empty() {
                    let item = c.remove(0);
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
