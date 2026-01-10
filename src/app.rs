pub enum InputMode {
    Normal,
    Editing, // For when typing search queries
}

pub enum CurrentScreen {
    Queue,
    Library,
    Search,
    Lyrics,
}

pub struct App {
    pub input_mode: InputMode,
    pub current_screen: CurrentScreen,
    pub search_input: String,
    pub cursor_position: usize, // For typing
    pub should_quit: bool,
    // You can keep using your Global RWLocks for the heavy data (Songs),
    // but put UI specific state (like selected index) here.
    pub library_selected_index: usize,
}

impl App {
    pub fn new() -> App {
        App {
            input_mode: InputMode::Normal,
            current_screen: CurrentScreen::Queue,
            search_input: String::new(),
            cursor_position: 0,
            should_quit: false,
            library_selected_index: 0,
        }
    }
}
