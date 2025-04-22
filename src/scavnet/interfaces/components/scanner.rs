use ratatui::{
    layout::Rect,
    text::{Line, Span},
    style::{Color, Style, Stylize},
    widgets::{Block, Paragraph},
    Frame,
};

use crate::scavnet::scanner::Scanner;

pub fn render(frame: &mut Frame, scanner: &Scanner, block_default: Block, target_area: Rect) {
    let mut scanner_detail_text = vec![];
    let scanner_status_text = format!("{}", scanner.status());
    let scanner_status_color = match scanner_status_text.as_str() {
        "Scanning..." => Color::White,
        "Paused" => Color::Yellow,
        _ => Color::Green,
    };

    // Status
    scanner_detail_text.push(
        Line::from(vec![
            Span::styled("Status", Style::new().italic()),
            "    : ".into(),
            Span::styled(scanner_status_text, Style::new().fg(scanner_status_color)),
        ])
    );

    // Timecode
    let cur_time_text = format!("{}", chrono::Utc::now().format("%d-%m-%Y %H:%M:%S"));
    scanner_detail_text.push(
        Line::from(vec![
            Span::styled("TimeCode", Style::new().italic()),
            "  : ".into(),
            Span::raw(cur_time_text),
        ])
    );

    // Frequency
    let scanner_freq = scanner.cur_freq_display();
    let scanner_freq_text = format!("{}", scanner_freq); 
    scanner_detail_text.push(
        Line::from(vec![
            Span::styled("Frequency", Style::new().italic()),
            " : ".into(),
            Span::raw(scanner_freq_text),
        ])
    );
    
    // Cur Network Name
    let cur_network_name = scanner.cur_network_name().to_uppercase();
    scanner_detail_text.push(
        Line::from(vec![
            Span::styled("Network", Style::new().italic()),
            "   : ".into(),
            Span::raw(cur_network_name),
        ])
    );

    let scanner_para = Paragraph::new(scanner_detail_text)
        .block(block_default.clone().title_top(Line::from("SCANNER").cyan().bold().centered()))
        .style(Style::default().fg(Color::White));
    frame.render_widget(scanner_para, target_area);
}