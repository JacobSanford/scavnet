
use ratatui::{
    layout::{Constraint, Direction, Layout},
    Frame
};

use crate::scavnet::interfaces::components::scanner;
use crate::scavnet::interfaces::components::signal;
use crate::scavnet::interfaces::components::system;
use crate::scavnet::interfaces::components::titlebar;
use crate::scavnet::scanner::Scanner;
use crate::scavnet::system::System;

use super::super::{
    interface::{
        InterfaceRegion,
        InterfaceRegionState,
        InterfaceNavigationState,
    },
};

pub fn ui(frame: &mut Frame, scanner: &Scanner, system: &System) {
    let state = super::super::interface::NAVIGATION_STATE.lock().clone();
    let block_default = super::super::interface::BLOCK_DEFAULT.clone();

    // Main Layout
    let main_layout = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ],
    )
    .split(frame.area());
    frame.render_widget(
        titlebar::widget(),
        main_layout[0],
    );

    let vertical_layout = Layout::new(
        Direction::Vertical,
        [Constraint::Percentage(50), Constraint::Percentage(50)],
    )
    .split(main_layout[1]);

    let top_horiz_layout = Layout::new(
        Direction::Horizontal,
        [Constraint::Percentage(50), Constraint::Percentage(50)],
    )
    .split(vertical_layout[0]);

    // Widgets
    scanner::render(frame, scanner, block_default.clone(), top_horiz_layout[0]);
    system::render(frame, system, block_default.clone(), top_horiz_layout[1]);
    signal::render(frame, scanner, block_default.clone(), vertical_layout[1]);

    // Region based comps.
    let mut loop1_title = format!(" [Loop1] {:.2} BPM | ", "");
    match state.region {
        InterfaceRegion::Loop1 => {
            loop1_title += "(a) add, (ESC) exit ";
        }
        _ => {
            loop1_title += "(F1) select ";
        }
    }
}
