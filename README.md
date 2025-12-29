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

#### Requirements: `mpv` installed and in PATH


1. Linux:

```bash
curl -L -o whytui https://github.com/shreyas-sha3/whytui/releases/download/Latest/whytui-linux-x86_64 && chmod +x whytui && sudo mv whytui /usr/local/bin/
````

2. MacOS:
```bash
brew install mpv
```
```bash
curl -L -o whytui https://github.com/shreyas-sha3/whytui/releases/download/Latest/whytui-macos-x86_64 && chmod +x whytui && sudo mv whytui /usr/local/bin/
```

3. Windows:
```bash
winget install mpv
```
```bash
curl.exe -L -o whytui.exe https://github.com/shreyas-sha3/whytui/releases/download/Latest/whytui-win-x86_64.exe && move whytui.exe C:\Windows\
```
[oneliner for cmd as administrator]
 
## Usage
- Press `/` and type to search for a song
- Type the song’s number to start playing it
- Hold Shift while typing the number to add it to the queue instead of playing

| Keybind       | Action                                |
|---------------|----------------------------------------|
| `/`           | Search songs                           |
| `SPACEBAR`    | Play/Pause the song                    |
| `n`           | Play next song in the queue            |
| `p`           | Play previous song in the queue        |
| `←`   `→`     | Seek 5 seconds                         |
| `-`   `+`     | Change volume by 5%                    |
| `c`           | Clear the queue                        |
| `r`           | Toggle recently played                 |
| `v`           | Toggle display modes                   |
| `g`           | Take a guess of the quality            |
| `t`           | Toggle romanize/translate              |
| `w`           | Disable lyrics for current song        |
| `L`           | Show YouTube Music libraries           |
| `l`           | Like the currently playing song        |
| `s`           | Stop current song                      |
| `q`           | Quit the application                   |


* Arguments:
  * `--manual` to disable autoplay
  * `--offline` to just play offline songs
  * `--lossless` to attempt fetching lossless audio
  * `--game` try guessing currently playing song quality

* Note: Netscape cookies can be added at `$MusicDir/whytui/config/cookies.txt`


## TODO

- [X] pause,seek
- [X] progress bar
- [X] queues
- [X] cache songs/store to disk from memory
- [X] autoplay similar songs
- [X] lyrics
- [ ] reimpliment a fully featured ytmusic api
- [ ] proper tui using ratatui


## CREDITS

- [lyrics](https://lrclib.net)
- [lossless](https://github.com/uimaxbai/hifi-api)
- Inspirations: [spotify-tui](https://github.com/Rigellute/spotify-tui) [kew](https://github.com/ravachol/kew)
