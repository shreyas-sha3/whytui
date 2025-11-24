# WhyTUI

A terminal-based YouTube music player written in Rust.  
Search, download, and play songs directly from the terminal.


<img width="497" height="406" alt="image" src="https://github.com/user-attachments/assets/f732ab9b-1e44-4e73-ae32-718ed0f46015" />


## Features

- Search for songs using YouTube
- Play songs using `ffplay`
- Downloads songs automatically to `~/Music/ytcli-songs`
- Stop or switch songs anytime
- Minimal dependencies and fast startup

## Installation

1. Clone the repository:

```bash
git clone https://github.com/shreyas-sha3/whytui.git
cd whytui
````

2. Build the project with Cargo:

```bash
cargo build --release
```

3. Run the binary:

```bash
cargo run --release
```

## Usage

* Simply press Enter for local playback
* Type the song name and press Enter to search
* Enter the number to play a song (or press Enter again to retry search)
* Commands:

  * `stop` → stop current song
  * `exit` → quit the application

## Requirements

* Rust 
* `ffplay` installed and in PATH
