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
            .filter(|p| p.extension().map_or(false, |e| e == "webm" || e == "flac"))
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn path_to_track(path: PathBuf) -> Track {
    let title = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let url = path.to_string_lossy().to_string();
    Track::new(title, url, None)
}
