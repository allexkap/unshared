use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    DefaultTerminal, Frame,
    widgets::{HighlightSpacing, List, ListState},
};

use crate::fs_tree::{FsTree, FsTreeNodeId};

#[derive(Debug)]
pub struct App {
    fs_tree: FsTree,
    current_node_id: Option<FsTreeNodeId>,
    list_items: Vec<(FsTreeNodeId, String)>,
    list_state: ListState,
    running: bool,
}

impl App {
    pub fn new(fs_tree: FsTree) -> Self {
        Self {
            fs_tree,
            current_node_id: None,
            list_items: Vec::default(),
            list_state: ListState::default(),
            running: false,
        }
    }

    pub fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        self.init();
        while self.running {
            terminal.draw(|frame| self.render(frame))?;
            self.handle_crossterm_events()?;
        }
        Ok(())
    }

    fn init(&mut self) {
        self.update_list_items();
        self.list_state.select_first();
        self.running = true;
    }

    fn render(&mut self, frame: &mut Frame) {
        let list = List::new(self.list_items.iter().map(|i| i.1.clone()))
            .highlight_symbol("> ")
            .highlight_spacing(HighlightSpacing::Always);

        frame.render_stateful_widget(list, frame.area(), &mut self.list_state)
    }

    fn handle_crossterm_events(&mut self) -> Result<()> {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => self.on_key_event(key),
            Event::Mouse(_) => {}
            Event::Resize(_, _) => {}
            _ => {}
        }
        Ok(())
    }

    fn on_key_event(&mut self, key: KeyEvent) {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc | KeyCode::Char('q'))
            | (KeyModifiers::CONTROL, KeyCode::Char('c') | KeyCode::Char('C')) => self.quit(),
            (_, KeyCode::Down) => self.list_state.select_next(),
            (_, KeyCode::Up) => self.list_state.select_previous(),
            (_, KeyCode::Left) => {
                if let Some(node_id) = self.current_node_id {
                    self.current_node_id = self.fs_tree.get_parent(node_id);
                    self.update_list_items();
                }
            }
            (_, KeyCode::Right) => {
                self.current_node_id = Some(self.list_items[self.list_state.selected().unwrap()].0);
                self.update_list_items();
            }
            _ => {}
        }
    }

    fn update_list_items(&mut self) {
        self.list_items = match self.current_node_id {
            Some(id) => self
                .fs_tree
                .get_children(id)
                .into_iter()
                .map(|child_id| (child_id, self.fs_tree.get_name(child_id).to_owned()))
                .collect(),
            None => self.fs_tree.get_roots(),
        };
        self.list_state.select_first();
    }

    fn quit(&mut self) {
        self.running = false;
    }
}
