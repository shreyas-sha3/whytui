# WhyTUI

A terminal-based YouTube music player written in Rust.  
Search, download, and play songs directly from the terminal.


<img width="497" height="406" alt="image" src="https://github.com/user-attachments/assets/f732ab9b-1e44-4e73-ae32-718ed0f46015" />


## Features

- Search for songs using YouTubeMusic
- Play songs using `mpv`
- Caches songs automatically to `~/Music/whytui`
- Auto adds related songs to queue
- Can play directly from Cache
- Minimal dependencies and fast startup

## Installation

1. Linux:

```bash
curl -L -o whytui https://github.com/shreyas-sha3/whytui/releases/download/Latest/whytui-linux-x86_64 && chmod +x whytui && sudo mv whytui /usr/local/bin/
````

2. MacOS:

```bash
curl -L -o whytui https://github.com/shreyas-sha3/whytui/releases/download/Latest/whytui-macos-x86_64 && chmod +x whytui && sudo mv whytui /usr/local/bin/
```

 
## Usage

### At `Search / Command:`
- Press Enter: Play a random local song / add it to queue  
- Type a song name: Search for a song.  
- Type a command (`pause`, `next`) and press Enter.

### At `Select (1-5):`
- Enter a number: Play the selected song immediately.  
- Enter `q` followed by a number (e.g., `q2`): Add the selected song to the queue.  
- Press Enter without a number: Retry or cancel the search.

* Commands:

  * `p | pause`  → pause the song
  * `n | next`  → play next song in the queue
  * `c | clear`  → clear the queue
  * `< / >`   → seek song in seconds (e.g., `>10` to seek 10 seconds forward)
  * `stop`  → stop current song
  * `exit`  → quit the application

## Requirements

* Rust 
* `mpv` installed and in PATH


## TODO

- [X] pause,seek
- [X] progress bar
- [X] queues
- [X] cache songs/store to disk from memory
- [X] autoplay similar songs
- [ ] reimpliment ytmusic-api for rust
- [ ] proper tui using ratatui
