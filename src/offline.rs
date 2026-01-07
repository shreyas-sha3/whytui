use crate::Track;
use rand::prelude::SliceRandom;
use std::path::{Path, PathBuf};

pub fn populate_queue_offline(
    music_dir: &PathBuf,
    queue: &mut Vec<Track>,
    exclude_titles: &[String],
) {
    if queue.len() < 3 {
        let new_songs = get_random_batch(music_dir, exclude_titles, 5);
        for song in new_songs {
            queue.push(song);
        }
    }
}

fn get_random_batch(music_dir: &Path, exclude_titles: &[String], count: usize) -> Vec<Track> {
    let mut all = get_all_songs(music_dir);
    let mut rng = rand::rng();

    let candidates: Vec<PathBuf> = all
        .iter()
        .filter(|p| {
            let title = p.file_stem().unwrap_or_default().to_string_lossy();
            !exclude_titles.contains(&title.to_string())
        })
        .cloned()
        .collect();

    let pool = if candidates.is_empty() {
        &mut all
    } else {
        &mut candidates.clone()
    };

    pool.shuffle(&mut rng);

    pool.into_iter()
        .take(count)
        .map(|p| path_to_track(p.clone()))
        .collect()
}

fn get_all_songs(dir: &Path) -> Vec<PathBuf> {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map_or(false, |e| e == "opus" || e == "flac"))
            .collect(),
        Err(_) => Vec::new(),
    }
}

use lofty::prelude::*;

fn path_to_track(path: PathBuf) -> Track {
    let url = path.to_string_lossy().to_string();

    let mut title = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut artists = vec!["Unknown".to_string()];
    let mut album = "Offline Library".to_string();
    let mut duration = "0:00".to_string();

    if let Ok(tagged_file) = lofty::read_from_path(&path) {
        if let Some(tag) = tagged_file.primary_tag() {
            if let Some(t) = tag.title() {
                title = t.to_string();
            }

            if let Some(a) = tag.artist() {
                artists = a.split(", ").map(|s| s.to_string()).collect();
            }

            if let Some(al) = tag.album() {
                album = al.to_string();
            }

            let props = tagged_file.properties();
            let total_seconds = props.duration().as_secs();
            duration = format!("{}:{:02}", total_seconds / 60, total_seconds % 60);
        }
    }

    Track::new(title, artists, album, duration, None, None, url)
}

pub fn get_excluded_titles() -> Vec<String> {
    let mut titles = Vec::new();
    let history = crate::RECENTLY_PLAYED.read().unwrap();
    let queue = crate::SONG_QUEUE.read().unwrap();
    titles.extend(history.iter().map(|t| t.title.clone()));
    titles.extend(queue.iter().map(|t| t.title.clone()));
    titles
}
