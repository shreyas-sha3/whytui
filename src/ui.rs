use crate::App;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
}; // We will define App in main.rs

pub fn draw(f: &mut Frame, app: &App) {
    // 1. Split screen into Top (Search) and Bottom (Status)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Top box is 3 lines high
            Constraint::Min(1),    // Bottom box takes the rest
        ])
        .split(f.area());

    // 2. Create the Search Bar
    let search_text = format!("Search: {}", app.input);
    let search_block = Paragraph::new(search_text)
        .block(Block::default().borders(Borders::ALL).title(" 🔍 Search "));

    // 3. Create the Results/Status Area
    let status_text = "Press 'q' to quit. Type to edit text.";
    let status_block = Paragraph::new(status_text)
        .style(Style::default().fg(Color::Yellow))
        .block(Block::default().borders(Borders::ALL).title(" Results "));

    // 4. Render them
    f.render_widget(search_block, chunks[0]);
    f.render_widget(status_block, chunks[1]);
}
