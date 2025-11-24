# WhyTUI

A terminal-based YouTube music player written in Rust.  
Search, download, and play songs directly from the terminal.

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

* Type the song name and press Enter to search
* Enter the number to play a song
* Commands:

  * `stop` → stop current song
  * `exit` → quit the application

## Requirements

* Rust 
* `ffplay` installed and in PATH
