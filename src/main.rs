mod api;
mod player;
mod ui;

use std::process::Child;
use std::sync::{RwLock, mpsc};
use std::thread;
use std::time::Duration;
/// -------------------------------------------------------------------
/// MAIN APPLICATION
/// -------------------------------------------------------------------

// HOLDS (StreamURL, Title, VideoID) VIDEO ID EMPTY FOR LOCAL SONGS
static SONG_QUEUE: RwLock<Vec<(String, String, String)>> = RwLock::new(Vec::new());
// HOLDS just VIDEO ID of realted songs upto 50 songs  SONG_QUEUE is populated by fetching urls from this list
static RELATED_SONG_LIST: RwLock<Vec<(String, String)>> = RwLock::new(Vec::new());

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    //collect arguements
    let args: Vec<String> = std::env::args().collect();
    let no_autoplay = args.contains(&"--no-autoplay".to_string());

    print!("\x1B[2J\x1B[1;1H\n");

    //create music_dir and temp dir to store currently playing song
    let music_dir = player::prepare_music_dir()?;
    //Custom unofficial api???
    let yt_client = api::YTMusic::new();

    let mut currently_playing: Option<Child> = None;

    let mut current_song_title = String::new(); // to know title so fully buffered songs can be moved from temp-> musicdir

    // transmitter (sends any input for Search/Command)
    // reciever (running every 250 ms.. checks if song ended plays next in queue if yes)
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        loop {
            let mut s = String::new();
            if std::io::stdin().read_line(&mut s).is_ok() {
                let _ = tx.send(s.trim().to_string());
            }
        }
    });

    refresh_ui(Some("Nothing Playing"));

    loop {
        // firstly checks if song is playing
        if let Some(child) = &mut currently_playing {
            if let Ok(Some(_)) = child.try_wait() {
                // if playback of previous song completed
                let temp = music_dir
                    .join("temp")
                    .join(format!("{}.webm", current_song_title));
                let full = music_dir.join(format!("{}.webm", current_song_title));

                // Rename only if the temp(previously played song fully buffered) exists
                if temp.exists() {
                    std::fs::rename(&temp, &full).ok();
                }
                currently_playing = None;

                // Play Next
                if let Some((src, name, video_id)) = queue_next() {
                    current_song_title = name.clone();
                    currently_playing = Some(player::play_file(&src, &name, &music_dir)?);
                    //defauly mode
                    if !no_autoplay {
                        if !video_id.is_empty() {
                            let yt = yt_client.clone();
                            let vid = video_id.clone();
                            tokio::spawn(async move {
                                queue_auto_add(yt, vid).await;
                            });
                        } else {
                            if let Some((path, name)) = shuffle_play(&music_dir)? {
                                queue_add(path, name, String::new());
                                refresh_ui(None);
                            }
                        }
                    }
                    refresh_ui(Some(&name));
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

        // if empty query play local (add to queue if something playing)
        if input.is_empty() {
            if let Some((path, name)) = shuffle_play(&music_dir)? {
                if currently_playing.is_some() {
                    // Local files don't have a video ID
                    queue_add(path, name, String::new());
                    refresh_ui(None);
                } else {
                    current_song_title = name.clone();
                    currently_playing = Some(player::play_file(&path, &name, &music_dir)?);
                    refresh_ui(Some(&name));
                }
            } else {
                println!("No local songs found.");
            }
            continue;
        }

        // special commands
        match input.to_lowercase().as_str() {
            "exit" => {
                player::stop_process(&mut currently_playing, &current_song_title, &music_dir);
                refresh_ui(None);
                break;
            }
            "stop" => {
                player::stop_process(&mut currently_playing, &current_song_title, &music_dir);
                current_song_title.clear();
                refresh_ui(Some("Nothing Playing"));
                continue;
            }
            "c" | "clear" => {
                {
                    let mut q = SONG_QUEUE.write().unwrap();
                    q.clear();
                }
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
            "n" | "next" => {
                player::stop_process(&mut currently_playing, &current_song_title, &music_dir);
                if let Some((p, name, video_id)) = queue_next() {
                    current_song_title = name.clone();
                    currently_playing = Some(player::play_file(&p, &name, &music_dir)?);

                    if !no_autoplay {
                        if !video_id.is_empty() {
                            let yt = yt_client.clone();
                            let vid = video_id.clone();
                            tokio::spawn(async move {
                                queue_auto_add(yt, vid).await;
                            });
                        } else {
                            if let Some((path, name)) = shuffle_play(&music_dir)? {
                                queue_add(path, name, String::new());
                                refresh_ui(None);
                            }
                        }
                    }
                    refresh_ui(Some(&name));
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
        let songs = yt_client.search_songs(&input, 5).await?;

        if songs.is_empty() {
            println!("No results.");
            refresh_ui(None);
            continue;
        }

        ui::show_songs(&songs);

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
                    yt_client.fetch_stream_url(&selected.video_id).await?
                };

                if is_queue {
                    queue_add(src, selected.title.clone(), selected.video_id.clone());
                    refresh_ui(None);
                } else {
                    player::stop_process(&mut currently_playing, &current_song_title, &music_dir);
                    current_song_title = selected.title.clone();
                    currently_playing = Some(player::play_file(&src, &selected.title, &music_dir)?);

                    if !no_autoplay {
                        //add similar songs in background
                        let yt = yt_client.clone();
                        let vid = selected.video_id.clone();
                        SONG_QUEUE.write().unwrap().clear();
                        RELATED_SONG_LIST.write().unwrap().clear();
                        tokio::spawn(async move {
                            queue_auto_add(yt, vid).await;
                        });
                    }
                    refresh_ui(Some(&selected.title));
                }
            } else {
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
    let q = SONG_QUEUE.read().unwrap();
    let queue_titles: Vec<String> = q.iter().map(|(_, t, _)| t.clone()).collect();
    ui::load_banner(song_name, &queue_titles);
}

pub async fn queue_auto_add(yt: api::YTMusic, id: String) {
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
                println!("\n\nLooking for similar songs...");
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
                queue_add(url, title, video_id);
            }
        }
        refresh_ui(None);
    }
}

fn queue_add(src: String, name: String, video_id: String) {
    let mut q = SONG_QUEUE.write().unwrap();
    q.push((src, name, video_id));
}

fn queue_next() -> Option<(String, String, String)> {
    let mut q = SONG_QUEUE.write().unwrap();
    if !q.is_empty() {
        Some(q.remove(0))
    } else {
        None
    }
}

fn shuffle_play(
    dir: &std::path::PathBuf,
) -> Result<Option<(String, String)>, Box<dyn std::error::Error>> {
    use rand::seq::IndexedRandom;
    let entries = std::fs::read_dir(dir)?;
    let songs: Vec<_> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |e| e == "webm"))
        .collect();

    if songs.is_empty() {
        return Ok(None);
    }
    let s = songs.choose(&mut rand::rng()).unwrap();
    let name = s
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    Ok(Some((s.to_string_lossy().to_string(), name)))
}
