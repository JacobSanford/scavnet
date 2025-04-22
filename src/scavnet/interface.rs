use std::{
    collections::HashMap,
    io::{
        stdout,
        Stdout
    }
};

use crossterm::{
    terminal::{
        disable_raw_mode,
        enable_raw_mode,
        EnterAlternateScreen,
        LeaveAlternateScreen
    },
    ExecutableCommand,
    event::{
        self,
        KeyCode,
        KeyEvent,
        KeyModifiers,
    }
};

use lazy_static::lazy_static;
use parking_lot::Mutex;
use ratatui::{
  prelude::{
    Color,
    CrosstermBackend,
    Style,
    Terminal,
  },
  Frame,
  widgets::{
    Block, Borders, Padding
  },
};

use crate::scavnet::scanner::Scanner;
use crate::scavnet::system::System;

use super::{
    interfaces::{
        main,
    },
};

lazy_static! {
    pub static ref NAVIGATION_STATE: Mutex<InterfaceNavigationState> = Mutex::new(
      InterfaceNavigationState{
        window: InterfaceWindow::Main,
        region: InterfaceRegion::None
      }
    );
    static ref SIG_EXIT: Mutex<bool> = Mutex::new(false);

    pub static ref STATE_COLORS: HashMap<InterfaceRegionState, Color> = {
        let mut m = HashMap::new();
        m.insert(InterfaceRegionState::Active, Color::Yellow);
        m.insert(InterfaceRegionState::Inactive, Color::White);
        m
    };

    pub static ref BLOCK_DEFAULT: Block<'static> = Block::default()
        .borders(Borders::ALL)
        .padding(Padding::new(1, 1, 1, 1))
        .border_style(Style::default()
        .fg(Color::White)
    );

    pub static ref SEARCH_INPUT_BUFFER: Mutex<String> = Mutex::new(String::new());
}

#[derive(Clone, Hash, Eq, PartialEq)]
pub enum InterfaceRegionState {
    Active,
    Inactive,
}

#[derive(Clone, PartialEq)]
pub struct InterfaceNavigationState {
    pub region: InterfaceRegion,
    pub window: InterfaceWindow,
}

#[derive(Clone, PartialEq)]
pub enum InterfaceWindow {
    Main,
}

#[derive(Clone, PartialEq)]
pub enum InterfaceRegion {
    Drop1,
    Loop1,
    Loop2,
    Mutations,
    None,
}

pub struct MainInterface {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl InterfaceNavigationState {
    pub fn get_region_state(&self, region: InterfaceRegion) -> InterfaceRegionState {
        if NAVIGATION_STATE.lock().region == region {
            InterfaceRegionState::Active
        } else {
            InterfaceRegionState::Inactive
        }
    }

    pub fn get_region_color(&self, region: InterfaceRegion) -> Color {
        STATE_COLORS.get(&self.get_region_state(region)).unwrap().clone()
    }
}

impl MainInterface {
    pub fn new() -> Self {
        let _ = enable_raw_mode();
        let _ = stdout().execute(EnterAlternateScreen);
        let terminal = Terminal::new(CrosstermBackend::new(stdout())).unwrap();

        Self { 
            terminal
        }
    }

    pub fn draw(&mut self, scanner: &Scanner, system: &System) {
        let state = NAVIGATION_STATE.lock().clone();
        match state.window {
            InterfaceWindow::Main => {
                self.render(main::ui, scanner, system);
            }
            // InterfaceWindow::Search => {
            //     self.render(search::ui);
            // }
        }
    }

    pub fn cleanup(&mut self) {
        let _ = disable_raw_mode();
        let _ = stdout().execute(LeaveAlternateScreen);
    }

    pub fn get_exit(&self) -> bool{
        SIG_EXIT.lock().clone()
    }

    fn render(&mut self, render_callback: fn(&mut Frame, &Scanner, &System), scanner: &Scanner, system: &System) {
        let _ = self.terminal.draw(|frame| {
            render_callback(frame, scanner, system);
        });
    }

    pub fn react_to_key_events(&mut self) {
        let key = {
            let key_event = super::super::LAST_KEYEVENT.lock();
            if key_event.code == KeyCode::Null && key_event.modifiers == KeyModifiers::NONE {
                return;
            }
            key_event.clone()
        };

        *super::super::LAST_KEYEVENT.lock() = KeyEvent::new(KeyCode::Null, KeyModifiers::NONE);

        let mut navigation_state = NAVIGATION_STATE.lock();

        if key.kind == event::KeyEventKind::Press {
            match navigation_state.window {
                InterfaceWindow::Main => {
                    match navigation_state.region {
                        InterfaceRegion::Loop1 => {
                            if let KeyCode::Char('a') = key.code {
                                // navigation_state.window = InterfaceWindow::Search;
                            }
                        },
                        InterfaceRegion::None => {
                            if let KeyCode::Char('q') = key.code {
                                *SIG_EXIT.lock() = true;
                            }
                        },
                        _ => {},
                    }

                    match key.code {
                        KeyCode::Esc => {
                            navigation_state.region = InterfaceRegion::None;
                        },
                        KeyCode::Char(c) if ('1'..='9').contains(&c) => {
                            let trigger = c.to_string().parse::<usize>().unwrap();
                        },
                        _ => {},
                    }
                },
                _ => {},
            }
        }
    }

}
