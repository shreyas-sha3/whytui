mod api;
mod features;
mod flac;
mod offline;
mod player;
mod ui1;
mod ui2;
mod ui3;
mod ui_common;

use crate::api::SongDetails;
use crate::player::clear_temp;
use crate::ui_common::set_status_line;
use crate::{
    flac::fetch_flac_stream_url,
    flac::init_api,
    offline::get_excluded_titles,
    ui1::{show_playlists, show_songs},
};
use colored::*;

use crossterm::{
    event::{self, Event},
    execute,
    terminal::{self, Clear, ClearType},
};
use std::collections::VecDeque;
use std::io::stdout;
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{OnceLock, RwLock, mpsc};
use std::thread;
use std::time::Duration;
use tokio::time;
// -------------------------------------------------------------------
// DATA STRUCTURES
// -------------------------------------------------------------------

#[derive(Debug)]
pub struct AppConfig {
    pub offline_mode: bool,
    pub no_autoplay: bool,
    pub lossless_mode: bool,
    pub game_mode: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub title: String,
    pub duration: String,
    pub artists: Vec<String>,
    pub album: String,
    pub thumbnail_url: Option<String>,
    pub video_id: Option<String>,
    pub url: String,
}

impl Track {
    pub fn new(
        title: String,
        artists: Vec<String>,
        album: String,
        duration: String,
        thumbnail_url: Option<String>,
        video_id: Option<String>,
        url: String,
    ) -> Self {
        Self {
            title,
            artists,
            album,
            duration,
            thumbnail_url,
            video_id,
            url,
        }
    }

    pub fn dummy() -> Self {
        Self::new(
            "Nothing Playing".to_string(), // title
            vec!["~".to_string()],         // artists (Vec<String>)
            "".to_string(),                // album
            "0:00".to_string(),            // duration
            None,                          // thumbnail_url
            None,                          // video_id
            "".to_string(),                // url
        )
    }
}

// ----------------------------------------------------------------------------------
// GLOBAL STATE
// ----------------------------------------------------------------------------------
//
static CONFIG: OnceLock<AppConfig> = OnceLock::new();
static SONG_QUEUE: RwLock<Vec<Track>> = RwLock::new(Vec::new());
// STORES ALL DETAILS OF UPCOMING SONGS
static RELATED_SONG_LIST: RwLock<Vec<api::SongDetails>> = RwLock::new(Vec::new());
static RECENTLY_PLAYED: RwLock<VecDeque<Track>> = RwLock::new(VecDeque::new());
const HISTORY_LIMIT: usize = 50;
//TO KEEP CONSISTENT VOLUME LEVEL ACROSS TRACKS (TO BE READ BY player.rs)
pub static VOLUME: AtomicI64 = AtomicI64::new(70);

pub static IS_PLAYING: AtomicBool = AtomicBool::new(false);
pub static IS_LOSSLESS: AtomicBool = AtomicBool::new(false);
pub static PLAYING_LOSSLESS: AtomicBool = AtomicBool::new(false);
static VIEW_MODE: RwLock<String> = RwLock::new(String::new());
static UI_MODE: AtomicUsize = AtomicUsize::new(0);

//LIST OF SONGS FROM A PLAYLIST
static LIBRARY_SONG_LIST: RwLock<Vec<SongDetails>> = RwLock::new(Vec::new());
//CONTAINS PLAYLIST ID SO AUTOPLAY CAN FETCH FROM THE SAME LIBRARY
static PLAYING_FROM_LIBRARY: RwLock<Option<(String, bool)>> = RwLock::new(None);
pub static LYRIC_OFFSET: AtomicI64 = AtomicI64::new(0);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ----------------------------------------------------------------------------------
    // PART 1 - GET ARGUMENTS, INITIAL GLOBAL (STATIC) VARIABLES
    // ----------------------------------------------------------------------------------
    let args: Vec<String> = std::env::args().collect();

    let app_config = AppConfig {
        offline_mode: args.contains(&"--offline".to_string()),
        no_autoplay: args.contains(&"--manual".to_string()),
        lossless_mode: args.contains(&"--lossless".to_string()),
        game_mode: args.contains(&"--game".to_string()),
    };
    // Set the global OnceLock
    CONFIG.set(app_config).expect("Failed to set config");

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

    clear_temp(&music_dir);
    if config().lossless_mode && !config().offline_mode {
        println!("Finding fastest FLAC server...");
        if let Err(e) = init_api().await {
            println!("FLAC API Init failed: {}", e);
        }
    }
    //
    //
    //
    //
    //

    // ----------------------------------------------------------------------------------
    // PART 2 - SETUP TRANSMITTER, RECIEVER CHANNEL FOR POLLING INPUT
    //         transmitter (sends any input for Search/Command)
    //         receiver (sleeps every 250 ms if not input)
    // ----------------------------------------------------------------------------------

    // let (tx, rx) = mpsc::channel::<String>();
    // thread::spawn(move || {
    //      loop {
    //          let mut s = String::new();
    //          if std::io::stdin().read_line(&mut s).is_ok() {
    //              let _ = tx.send(s.trim().to_string());
    //          }
    //      }
    // });
    let (tx, rx) = mpsc::channel::<String>();
    spawn_input_handler(tx);
    //
    //
    //
    //
    //

    // ----------------------------------------------------------------------------------
    // PART 3 - INITIALIZATION
    // ----------------------------------------------------------------------------------

    // -------------------------------------------------------------------
    // RESTRICTION: ENFORCE MINIMUM TERMINAL SIZE
    // -------------------------------------------------------------------
    let min_width = 52;
    let min_height = 37;

    loop {
        let (cols, rows) = crossterm::terminal::size().unwrap_or((0, 0));

        if cols >= min_width && rows >= min_height {
            break;
            execute!(stdout(), Clear(ClearType::All));
        }
        execute!(stdout(), Clear(ClearType::All));
        println!("Breh Terminal too small!");
        println!("Current: {}x{}", cols, rows);
        println!("Required: {}x{}", min_width, min_height);
        println!("Resize your window >_<");

        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    // ----------------------------------------------------------------------------------
    // CASE 1 : IF OFFLINE MODE INITIAL FETCH RANDOM SONG + POPULATE QUEUE
    // ----------------------------------------------------------------------------------
    if config().offline_mode {
        let exclude = get_excluded_titles();
        {
            let mut q = SONG_QUEUE.write().unwrap();
            offline::populate_queue_offline(&music_dir, &mut q, &exclude);
        }

        if let Some(track) = queue_next() {
            current_track = Some(track.clone()); //to pass it around to functions like next song
            currently_playing = Some(player::play_file(&track.url, &track, &music_dir)?); //object to stop the current song
            refresh_ui(Some(&track));
        } else {
            set_status_line(Some(format!("No local songs found!")));
            refresh_ui(Some(&Track::dummy()));
            // refresh_ui(None);
        }
    }
    // ----------------------------------------------------------------------------------
    // CASE 2 : IF ONLINE MODE TRY TO CONNECT TO API AND FETCH USERNAME
    // ----------------------------------------------------------------------------------
    else {
        let user_status = yt_client
            .fetch_account_name()
            .await
            .unwrap_or("Error".to_string());

        refresh_ui(Some(&Track::dummy()));
        set_status_line(Some(format!("Wassup {}", user_status)));
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
        // if event::poll(Duration::from_millis(0))? {
        //      match event::read()? {
        //          Event::Resize(_, _) => {
        //              execute!(stdout(), Clear(ClearType::All))?;
        //              refresh_ui(None);
        //          }
        //          _ => {}
        //      }
        // }

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
                ui_common::clear_lyrics(); // stop display of lyrics
                if let Some(track) = &current_track {
                    let track_clone = track.clone();
                    let music_dir_clone = music_dir.clone();

                    std::thread::spawn(move || {
                        let file_name =
                            player::the_naming_format_in_which_i_have_saved_the_track_locally(
                                &track_clone.title,
                                &track_clone.artists,
                            );

                        let base_temp = music_dir_clone.join("temp").join(&file_name);
                        let base_full = music_dir_clone.join(&file_name);

                        for ext in ["opus", "flac"] {
                            let temp = base_temp.with_extension(ext);
                            let full = base_full.with_extension(ext);

                            if temp.exists() {
                                let _ = player::apply_metadata(&temp, &track_clone);

                                std::fs::rename(&temp, &full).ok();
                                break;
                            }
                        }
                    });
                    add_to_history(track.clone());
                }

                currently_playing = None;

                // -------------------------------------------------------------------
                // CASE 2 : WHEN NO SONG IS PLAYING CURRENTLY
                // -------------------------------------------------------------------
                if let Some(track) = queue_next() {
                    //Check queue first - If yes play next in queue
                    current_track = Some(track.clone());
                    currently_playing = Some(player::play_file(&track.url, &track, &music_dir)?);

                    // -------------------------------------------------------------------
                    // CASE 2.1 : IF AUTOPLAY IS ENABLED (DEFAULT MODE)
                    // -------------------------------------------------------------------
                    if !config().no_autoplay {
                        // -------------------------------------------------------------------
                        // CASE 2.1.1 : IF USER IS IN OFFLINE MODE (POPULATE FROM OFFLINE.RS)
                        // -------------------------------------------------------------------
                        if config().offline_mode {
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
                    refresh_ui(Some(&track));
                }
                // -------------------------------------------------------------------
                // CASE 2.1 : IF AUTOPLAY IS DISABLED (JUST STOP PLAYBACK)
                // -------------------------------------------------------------------
                else {
                    current_track = None;
                    crate::IS_PLAYING.store(false, Ordering::SeqCst);
                    refresh_ui(Some(&Track::dummy()));
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
        )
        .await
        {
            continue;
        }

        // -------------------------------------------------------------------
        // CASE 3 : IF NONE OF THE ABOVE AND IN OFFLINE MODE CONTINUE
        //         (since search not allowed on offline mode)
        // -------------------------------------------------------------------
        if config().offline_mode {
            set_status_line(Some("Nope not here".to_string()));
            refresh_ui(None);
            continue;
        }

        // -------------------------------------------------------------------
        // CASE 4 : IF NONE OF THE ABOVE AND IN ONLINE MODE,
        //         USE RECIEVED TEXT TO SEARCH CUSTOM API
        // -------------------------------------------------------------------
        let mut songs: Vec<api::SongDetails> = Vec::new();
        songs = match yt_client.search_songs(&input, 5).await {
            Ok(s) => s,
            Err(_) => {
                std::thread::sleep(Duration::from_millis(75));
                set_status_line(Some("Search failed (retry)".to_string()));
                refresh_ui(None);
                continue;
            }
        };

        // -------------------------------------------------------------------
        // CASE 4.1 : IF NO RESULTS SIMPLY REFRESH UI
        // -------------------------------------------------------------------
        if songs.is_empty() {
            set_status_line(Some("Search failed (retry)".to_string()));
            refresh_ui(None);
            continue;
        }

        //
        //
        //
        //

        // -------------------------------------------------------------------
        // PART 6 - IF NONE OF THE ABOVE AND IN ONLINE MODE,
        //         USE RECIEVED TEXT TO SEARCH CUSTOM API
        // -------------------------------------------------------------------
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
                None,
            )
            .await
            .unwrap_or_else(|e| println!("Auto-select error: {}", e));
            //finish this loop
            continue;
        }

        show_songs(&songs);
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
                None,
            )
            .await
            .unwrap_or_else(|e| set_status_line(Some(":( Error playing song".to_string())));
        }
    }
}

use std::path::PathBuf;

async fn handle_global_commands(
    input: &str,
    rx: &std::sync::mpsc::Receiver<String>,
    yt_client: &api::YTMusic,
    current_track: &mut Option<Track>,
    currently_playing: &mut Option<Child>,
    music_dir: &std::path::PathBuf,
) -> bool {
    // let title_ref = current_track.as_ref().map(|t| t.title.as_str()); //now playing song title to pass to refresh_ui
    let current_mode = UI_MODE.load(Ordering::Relaxed); //current ui mode

    // Seek check
    if input.starts_with('>') || input.starts_with('<') {
        if let Ok(s) = input[1..].trim().parse::<i64>() {
            player::seek(if input.starts_with('<') { -s } else { s });
        }
        // refresh_ui(None);
        return true;
    }

    // special commands
    match input {
        "REFRESH_UI" => {
            // execute!(stdout(), Clear(ClearType::All));
            refresh_ui(None);
            set_status_line(None);
            return true;
        }
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
            refresh_ui(None);
            set_status_line(Some(format!("STOPPED SONG")));
            return true;
        }
        "c" | "clear" => {
            SONG_QUEUE.write().unwrap().clear();
            set_status_line(Some(format!("QUEUE CLEARED")));
            return true;
        }
        s if s == "+" || s == "-" => {
            let mut delta: i64 = if s == "+" { 5 } else { -5 };

            while let Ok(next_input) = rx.try_recv() {
                if next_input == "+" {
                    delta += 5;
                } else if next_input == "-" {
                    delta -= 5;
                } else {
                    break;
                }
            }

            let current = VOLUME.load(Ordering::Relaxed);
            let new_vol = (current + delta).clamp(0, 150);
            VOLUME.store(new_vol, Ordering::Relaxed);

            player::vol_change(delta);

            set_status_line(Some(format!("VOLUME {}", new_vol)));

            return true;
        }

        "pause" => {
            if currently_playing.is_some() {
                player::toggle_pause();
                set_status_line(Some(format!("PAUSED/PLAYED")));
            }
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
                                set_status_line(Some("Added to Liked Songs!".to_string()));
                            }
                            Err(e) => {
                                set_status_line(Some(format!(":( Couldn't like song: {}", e)));
                            }
                        }
                    });
                }
            }
            refresh_ui(None);
            return true;
        }
        "a" | "add" => {
            if let Some(track) = current_track {
                if let Some(vid) = &track.video_id {
                    let yt = yt_client.clone();
                    let video_id = vid.clone();

                    if let Ok(playlists) = yt_client.fetch_library_playlists().await {
                        show_playlists(&playlists);

                        if let Ok(sel_str) = rx.recv() {
                            let sel = sel_str.trim().parse::<usize>().unwrap_or(0);
                            if sel >= 1 && sel <= playlists.len() {
                                let selected_playlist_id = playlists[sel - 1].playlist_id.clone();

                                tokio::spawn(async move {
                                    match yt.add_to_playlist(&selected_playlist_id, &video_id).await
                                    {
                                        Ok(_) => {
                                            set_status_line(Some("Added to Playlist!".to_string()))
                                        }
                                        Err(e) => set_status_line(Some(format!(":( Error: {}", e))),
                                    }
                                });
                            }
                        }
                    }
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
            set_status_line(Some(format!("Wassup {}", user_status)));
            return true;
        }
        "t" | "translate" => {
            // ui_common::stop_lyrics();
            ui_common::cycle_lyric_display_mode();
            let mode = ui_common::LYRIC_DISPLAY_MODE.load(Ordering::Relaxed);
            match mode {
                1 => set_status_line(Some("ROMANIZED LYRICS".to_string())),
                2 => set_status_line(Some("TRANSLATED LYRICS".to_string())),
                _ => set_status_line(Some("ORIGINAL LYRICS".to_string())),
            }
            refresh_ui(None);
            return true;
        }
        // "w" | "wrong" => {
        //     ui_common::stop_lyrics();
        //     ui_common::clear_lyrics();
        //     set_status_line(Some(format!("sorry... stopped lyrics")));
        //     refresh_ui(None);
        //     return true;
        // }
        //toggle between the ui modes
        "v" | "view" => {
            ui_common::stop_lyrics();

            let next_ui_mode = (current_mode + 1) % 3;
            UI_MODE.store(next_ui_mode, Ordering::Relaxed);

            execute!(stdout(), Clear(ClearType::All));
            if let Some(track) = current_track {
                refresh_ui(Some(&track));
            }
            return true;
        }
        "r" | "recents" => {
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

        "n" | "next" => {
            ui_common::clear_lyrics();

            if let Some(track) = current_track {
                add_to_history(track.clone());
                player::stop_process(currently_playing, &track.title, music_dir);
            }

            if let Some(track) = queue_next() {
                *current_track = Some(track.clone());
                *currently_playing =
                    Some(player::play_file(&track.url, &track, music_dir).unwrap());

                if !config().no_autoplay {
                    if config().offline_mode {
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

                refresh_ui(Some(&track));
                set_status_line(Some("PLAYING NEXT".into()));
            } else {
                *current_track = None;
                // refresh_ui(None);
            }

            true
        }

        "p" | "previous" => {
            if let Some(track) = current_track {
                if let Some(prev_track) = get_prev_track() {
                    player::stop_process(currently_playing, &track.title, music_dir);
                    queue_add_front(track.clone());

                    *current_track = Some(prev_track.clone());
                    *currently_playing =
                        Some(player::play_file(&prev_track.url, &prev_track, music_dir).unwrap());
                    refresh_ui(Some(&prev_track));
                    set_status_line(Some(format!("PLAYING PREVIOUS")));
                }
            }
            return true;
        }
        "L" | "library" => {
            if config().offline_mode || UI_MODE.load(Ordering::Relaxed) == 2 {
                refresh_ui(None);
                return true;
            }

            let ui_mode = UI_MODE.load(Ordering::Relaxed);
            if ui_mode == 2 {
                refresh_ui(None);
                return true;
            }

            //give rx to library helper
            if let Err(e) =
                handle_library_browsing(rx, yt_client, music_dir, current_track, currently_playing)
                    .await
            {
                set_status_line(Some("Error in Library: {}".to_string()));
            }

            refresh_ui(None);
            return true;
        }
        "g" | "guess" => {
            if currently_playing.is_none() || !config().game_mode {
                set_status_line(Some("NOT NOW!".into()));
                refresh_ui(None);
                return true;
            }

            print!(
                "\n\n\n\r  {}",
                "--- GUESS THE FORMAT --- \n\r  1) OPUS (Lossy)\n\r  2) FLAC (Lossless)"
            );

            if let Ok(guess_input) = rx.recv() {
                let guess = guess_input.trim();
                let is_lossless = PLAYING_LOSSLESS.load(Ordering::SeqCst);

                let correct = match guess {
                    "1" => !is_lossless,
                    "2" => is_lossless,
                    "-" => {
                        refresh_ui(None);
                        set_status_line(Some("Invalid Input".to_string()));
                        return true;
                    }
                    _ => false,
                };

                refresh_ui(None);
                if correct {
                    set_status_line(Some("CORRECT GUESS!".into()));
                } else {
                    let actual = if is_lossless { "FLAC" } else { "OPUS" };
                    set_status_line(Some(format!("WRONG! It was {}", actual)));
                }
            }

            return true;
        }
        s if s == "[" || s == "]" => {
            let mut delta: i64 = if s == "]" { 100 } else { -100 };

            while let Ok(next_input) = rx.try_recv() {
                if next_input == "]" {
                    delta += 100;
                } else if next_input == "[" {
                    delta -= 100;
                } else {
                    break;
                }
            }

            let current = LYRIC_OFFSET.load(Ordering::Relaxed);
            let new_offset = current + delta;
            LYRIC_OFFSET.store(new_offset, Ordering::Relaxed);

            set_status_line(Some(format!("LYRICS OFFSET {}", new_offset)));

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
    playlist_context: Option<(String, bool)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = selection_input.trim();
    let (idx, is_queue) = if input.to_lowercase().starts_with('q') {
        (input[1..].parse::<usize>().unwrap_or(0), true)
    } else {
        (input.parse::<usize>().unwrap_or(0), false)
    };

    if idx >= 1 && idx <= songs_list.len() {
        let selected = &songs_list[idx - 1];
        let safe_title = player::the_naming_format_in_which_i_have_saved_the_track_locally(
            &selected.title,
            &selected.artists,
        );

        let opus = music_dir.join(format!("{}.opus", safe_title));
        let flac = music_dir.join(format!("{}.flac", safe_title));

        let src = if flac.exists() {
            flac.to_string_lossy().to_string()
        } else if opus.exists() {
            opus.to_string_lossy().to_string()
        } else {
            let clean_title =
                if_title_contains_non_english_and_other_language_script_return_only_english_part(
                    &selected.title,
                );
            let query = format!("{} {}", clean_title, selected.artists.join(","));
            let mut final_url = None;

            if config().lossless_mode {
                if !config().game_mode {
                    set_status_line(Some("Trying to fetch lossless".to_string()));
                }
                final_url = fetch_flac_stream_url(&query, &selected.duration).await.ok();
            }
            if final_url.is_none() {
                if !config().game_mode {
                    set_status_line(Some("Fetching from youtube".to_string()));
                }
                final_url = yt_client.fetch_stream_url(&selected.video_id).await.ok();
            }
            match final_url {
                Some(url) => url,
                None => return Ok(()),
            }
        };

        let new_track = Track::new(
            selected.title.clone(),
            selected.artists.clone(),
            selected.album.clone(),
            selected.duration.clone(),
            selected.thumbnail_url.clone(),
            Some(selected.video_id.clone()),
            src,
        );

        if is_queue {
            queue_add_front(new_track);
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
            ui_common::clear_lyrics();
            *current_track = Some(new_track.clone());
            *currently_playing = Some(player::play_file(&new_track.url, &new_track, &music_dir)?);

            if !config().no_autoplay {
                let yt = yt_client.clone();
                let vid = selected.video_id.clone();
                SONG_QUEUE.write().unwrap().clear();
                RELATED_SONG_LIST.write().unwrap().clear();
                tokio::spawn(async move {
                    queue_auto_add_online(yt, vid).await;
                });
            }
            refresh_ui(Some(&new_track));
        }
    } else {
        refresh_ui(None);
    }

    Ok(())
}

use rand::seq::IndexedRandom;
async fn handle_library_browsing(
    rx: &std::sync::mpsc::Receiver<String>,
    yt_client: &api::YTMusic,
    music_dir: &std::path::PathBuf,
    current_track: &mut Option<Track>,
    currently_playing: &mut Option<Child>,
) -> Result<(), Box<dyn std::error::Error>> {
    set_status_line(Some("Fetching Library...".to_string()));

    let playlists = yt_client.fetch_library_playlists().await?;
    show_playlists(&playlists);

    // waiting for playlist selection
    let sel_str = match rx.recv() {
        Ok(s) => s,
        _ => return Ok(()),
    };
    let sel = sel_str.trim().parse::<usize>().unwrap_or(0);

    if sel < 1 || sel > playlists.len() {
        return Ok(());
    }

    let selected_playlist = &playlists[sel - 1];
    set_status_line(Some(format!("Loading '{}'...", selected_playlist.title)));

    let (initial_songs, mut continuation_token) = yt_client
        .fetch_playlist_songs(&selected_playlist.playlist_id, 100)
        .await?;
    *LIBRARY_SONG_LIST.write().unwrap() = initial_songs;

    let mut page: usize = 1;
    const PAGE_SIZE: usize = 5;

    loop {
        refresh_ui(None);

        let list_len = LIBRARY_SONG_LIST.read().unwrap().len();
        let start = (page - 1) * PAGE_SIZE;
        let end = std::cmp::min(start + PAGE_SIZE, list_len);

        println!(" [n]ext | [p]rev | [s]huffle");
        if start < list_len {
            let slice = LIBRARY_SONG_LIST.read().unwrap()[start..end].to_vec();
            ui_common::show_songs(&slice);
            set_status_line(Some(format!("Page {} | Fetched {}", page, list_len)));
        } else {
            println!(" --- End ---");
        }

        if let Ok(input) = rx.recv() {
            match input.trim() {
                "n" => {
                    if page * PAGE_SIZE >= list_len {
                        if let Some(token) = continuation_token.take() {
                            set_status_line(Some("Fetching more...".into()));

                            match yt_client.fetch_continuation(&token).await {
                                Ok((new_songs, next_token)) => {
                                    LIBRARY_SONG_LIST.write().unwrap().extend(new_songs);
                                    continuation_token = next_token;
                                    page += 1;

                                    while rx.try_recv().is_ok() {} //to not increase page when spamming while loading
                                }
                                Err(e) => {
                                    set_status_line(Some(format!("Error: {}", e)));
                                    continuation_token = Some(token);
                                }
                            }
                        } else {
                            set_status_line(Some("No more songs.".into()));
                        }
                    } else {
                        page += 1;
                    }
                }
                "p" => {
                    if page > 1 {
                        page -= 1;
                    }
                }
                // "s" => {
                //     let list = LIBRARY_SONG_LIST.read().unwrap();

                //     if let Some(song) = list.choose(&mut rand::rng()) {
                //         handle_song_selection(
                //             "1".into(),
                //             &[song.clone()],
                //             music_dir,
                //             yt_client,
                //             current_track,
                //             currently_playing,
                //             Some((selected_playlist.playlist_id.clone(), true)),
                //         )
                //         .await?;
                //         break;
                //     }
                // }
                "" => break,

                num_str => {
                    if let Ok(num) = num_str.parse::<usize>() {
                        let song_idx = (page - 1) * PAGE_SIZE + (num - 1);
                        let song = LIBRARY_SONG_LIST.read().unwrap().get(song_idx).cloned();

                        if let Some(s) = song {
                            handle_song_selection(
                                "1".into(),
                                &[s],
                                music_dir,
                                yt_client,
                                current_track,
                                currently_playing,
                                Some((selected_playlist.playlist_id.clone(), false)),
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

fn refresh_ui(track_details: Option<&Track>) {
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
        ui1::load_banner(track_details, &titles, &mode);
    } else if ui_mode == 1 {
        ui2::load_banner(track_details, &titles, &mode);
    } else if ui_mode == 2 {
        ui3::load_banner(track_details, &titles, &mode);
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

fn if_title_contains_non_english_and_other_language_script_return_only_english_part(
    title: &str,
) -> String {
    let ascii_only: String = title.chars().filter(|c| c.is_ascii()).collect();
    ascii_only
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
}

pub async fn queue_auto_add_online(yt: api::YTMusic, id: String) {
    let needs_songs = {
        let q = SONG_QUEUE.read().unwrap();
        q.len() < 2
    };

    if needs_songs {
        // check if saved related songs are exhausted
        let cache_empty = {
            let c = RELATED_SONG_LIST.read().unwrap();
            c.is_empty()
        };

        if cache_empty {
            let (playlist_id, should_suffle) = {
                let guard = PLAYING_FROM_LIBRARY.read().unwrap();
                match &*guard {
                    Some((playlist_id, shuffle_state)) => {
                        (Some(playlist_id.clone()), *shuffle_state)
                    }
                    None => (None, false),
                }
            };

            if let Ok(related) = yt
                .fetch_related_songs(&id, playlist_id.as_deref(), 50, should_suffle)
                .await
            {
                let mut c = RELATED_SONG_LIST.write().unwrap();
                for song in related {
                    c.push(song);
                }
            }
        }

        // Cannot hold lock during await, so move items to a local vec
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
        for details in to_fetch {
            let mut final_url = None;

            // Check lossless if enabled
            if config().lossless_mode {
                let clean_title =
                    if_title_contains_non_english_and_other_language_script_return_only_english_part(
                        &details.title,
                    );
                let query = format!("{} {}", clean_title, details.artists.join(" "));
                final_url = fetch_flac_stream_url(&query, &details.duration).await.ok();
            }

            if final_url.is_none() {
                final_url = yt.fetch_stream_url(&details.video_id).await.ok();
            }

            if let Some(url) = final_url {
                queue_add(Track::new(
                    details.title,
                    details.artists,
                    details.album,
                    details.duration,
                    details.thumbnail_url,
                    Some(details.video_id),
                    url,
                ));
            }
        }

        refresh_ui(None);
        set_status_line(Some("Fetched Similar Songs!".to_string()));
    }
}

pub fn config() -> &'static AppConfig {
    CONFIG.get().expect("Config is not initialized")
}

use crossterm::event::{KeyCode, KeyEventKind};
use std::io::{self, Write};
use std::sync::mpsc::Sender;
pub fn spawn_input_handler(tx: Sender<String>) {
    std::thread::spawn(move || {
        let _ = crossterm::terminal::enable_raw_mode();

        loop {
            if let Ok(true) = event::poll(std::time::Duration::from_millis(100)) {
                if let Ok(ev) = event::read() {
                    match ev {
                        Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                            KeyCode::Char('/') => {
                                execute!(io::stdout(), crossterm::cursor::Show).ok();
                                let mut query = String::new();
                                let prompt = "> ".bright_blue().bold();

                                set_status_line(Some("Search a Song".to_string()));
                                loop {
                                    print!("\r\x1b[2K{} {}█", prompt, query);
                                    io::stdout().flush().unwrap();

                                    if let Ok(Event::Key(k)) = event::read() {
                                        if k.kind != KeyEventKind::Press {
                                            continue;
                                        }

                                        match k.code {
                                            KeyCode::Enter => {
                                                if !query.is_empty() {
                                                    let _ = tx.send(query.clone());
                                                }
                                                break;
                                            }
                                            KeyCode::Esc => {
                                                let _ = tx.send("REFRESH_UI".into());
                                                break;
                                            }
                                            KeyCode::Backspace => {
                                                query.pop();
                                            }

                                            KeyCode::Char(c) => query.push(c),
                                            _ => {}
                                        }
                                    }
                                }
                            }

                            KeyCode::Esc => {
                                let _ = tx.send("".to_string());
                                let _ = tx.send("REFRESH_UI".into());
                            }

                            KeyCode::Char(c) if c.is_ascii_digit() => {
                                let _ = tx.send(c.to_string());
                            }

                            KeyCode::Char('L') => {
                                let _ = tx.send("L".into());
                            }
                            KeyCode::Char('l') => {
                                let _ = tx.send("l".into());
                            }
                            KeyCode::Char('a') => {
                                let _ = tx.send("a".into());
                            }
                            KeyCode::Char('u') => {
                                let _ = tx.send("u".into());
                            }
                            KeyCode::Char(' ') => {
                                let _ = tx.send("pause".into());
                            }
                            KeyCode::Char('p') => {
                                let _ = tx.send("p".into());
                            }
                            KeyCode::Char('n') => {
                                let _ = tx.send("n".into());
                            }
                            KeyCode::Char('v') => {
                                let _ = tx.send("v".into());
                            }
                            KeyCode::Char('t') => {
                                let _ = tx.send("t".into());
                            }
                            KeyCode::Char('r') => {
                                let _ = tx.send("r".into());
                            }
                            KeyCode::Char('s') => {
                                let _ = tx.send("s".into());
                            }
                            KeyCode::Char('g') => {
                                let _ = tx.send("g".into());
                            }

                            KeyCode::Char('+') | KeyCode::Char('=') => {
                                let _ = tx.send("+".into());
                            }
                            KeyCode::Char('-') => {
                                let _ = tx.send("-".into());
                            }
                            KeyCode::Char('[') => {
                                let _ = tx.send("[".into());
                            }
                            KeyCode::Char(']') => {
                                let _ = tx.send("]".into());
                            }
                            KeyCode::Right => {
                                let _ = tx.send(">5".into());
                            }
                            KeyCode::Left => {
                                let _ = tx.send("<5".into());
                            }

                            KeyCode::Char('q') => {
                                let _ = crossterm::terminal::disable_raw_mode();
                                let _ = tx.send("q".into());
                                return;
                            }
                            KeyCode::Char('!') => {
                                let _ = tx.send("q1".into());
                            }
                            KeyCode::Char('@') => {
                                let _ = tx.send("q2".into());
                            }
                            KeyCode::Char('#') => {
                                let _ = tx.send("q3".into());
                            }
                            KeyCode::Char('$') | KeyCode::Char('€') => {
                                let _ = tx.send("q4".into());
                            }
                            KeyCode::Char('%') => {
                                let _ = tx.send("q5".into());
                            }
                            _ => {}
                        },

                        Event::Resize(_, _) => {
                            execute!(stdout(), Clear(ClearType::All));
                            let _ = tx.send("REFRESH_UI".into());
                        }

                        _ => {}
                    }
                }
            }
        }
    });
}
