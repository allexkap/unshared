use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

pub use fs_tree_panel::FsTreePanel;

mod fs_tree_panel;

pub trait Component {
    fn handle_key_event(&mut self, key: KeyEvent) -> color_eyre::Result<()>;
    fn render(&mut self, frame: &mut Frame, area: Rect);
}
