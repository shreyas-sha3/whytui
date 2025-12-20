# WhyTUI

A terminal-based YouTube music player written in Rust.  
Search, download, and play songs directly from the terminal.

UI 1             |  UI 2                               | UI 3
:-------------------------:|:-------------------------:|:-------------------------:
<img width="700" height="600" alt="image" src="https://github.com/user-attachments/assets/0e6f7643-ec98-446b-b361-5f3b7a4c77b0" /> |<img width="700" height="700" alt="image" src="https://github.com/user-attachments/assets/88dbb7d5-6c83-4e2e-a6ae-4b79abc90d55" />  | <img width="500" height="600" alt="image" src="https://github.com/user-attachments/assets/6285ef05-387f-4867-9532-216e4cb7347e" />


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

2. Windows (oneliner for admin powershell):

```bash
iwr "https://github.com/shreyas-sha3/whytui/releases/download/Latest/whytui-windows-x86_64.exe" -OutFile "$env:SystemRoot\whytui.exe"
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

  * `RETURN`  → pause the song
  * `n | next`  → play next song in the queue (note: `n3` to skip 3 songs)
  * `p | prev`  → play previous song in the queue
  * `t`  → toggle queue and recently played
  * `< / >`   → seek song in seconds (e.g., `>10` to seek 10 seconds forward)
  * `c | clear`  → clear the queue
  * `L | library` → show ytmusic playlists
  * `l | like` → add currently playing song to liked songs
  * `s | stop`  → stop current song
  * `q | quit`  → quit the application

* Arguments:
  * `--manual` to disable autoplay
  * `--offline` to just play offline songs

* Note: Netscape cookies can be added at `$MusicDir/whytui/config/cookies.txt`

## Requirements

* Rust 
* `mpv` installed and in PATH


## TODO

- [X] pause,seek
- [X] progress bar
- [X] queues
- [X] cache songs/store to disk from memory
- [X] autoplay similar songs
- [X] lyrics
- [ ] reimpliment a fully featured ytmusic api
- [ ] proper tui using ratatui
