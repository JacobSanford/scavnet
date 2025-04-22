use ratatui::widgets::{Block, Borders};

pub fn widget() -> Block<'static> {
    Block::new().borders(Borders::TOP).title(format!("scavmainnet v0.1")).clone()
}
