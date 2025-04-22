use ratatui::{
    layout::Rect,
    text::{Line, Span},
    style::{Color, Style, Stylize},
    widgets::{Block, Paragraph, Wrap},
    Frame,
};

use crate::scavnet::system::System;

pub fn render(frame: &mut Frame, system: &System, block_default: Block, target_area: Rect) {
    let mut system_detail_text = vec![];

    let system_area_height = target_area.height;
    let system_area_width = target_area.width;

    system_detail_text.push(
        Line::from(vec![
            Span::styled(format!("Logs: {}", system_area_height), Style::new().italic()),
        ])
    );

    let logs = system.get_last_x_logs(system_area_height as usize);
    for log in logs {
        system_detail_text.push(
            Line::from(vec![
                Span::raw(log),
            ])
        );
    }

    let mut system_para = Paragraph::new(system_detail_text)
        .block(block_default.clone().title_top(Line::from("SYSTEM").cyan().bold().centered()))
        .style(Style::default().fg(Color::White))
        .wrap(Wrap { trim: true });

    let actual_line_count = system_para.line_count(system_area_width - 2);
    if actual_line_count > system_area_height.into() {
        system_para = system_para.scroll(((actual_line_count - system_area_height as usize) as u16, 0));
    }

    frame.render_widget(system_para, target_area);
}
