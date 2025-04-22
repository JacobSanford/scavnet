use ratatui::{
    layout::{Constraint, Rect},
    symbols::Marker,
    text::Line,
    style::{Color, Style, Stylize},
    widgets::{Axis, Block, Chart, Dataset, GraphType},
    Frame
};

use crate::scavnet::scanner::Scanner;

pub fn render(frame: &mut Frame, scanner: &Scanner, _block_default: Block, target_area: Rect) {
    let fftdata = scanner.get_fft_data();
    let mut fftchart = vec![];
    for (i, &val) in fftdata.iter().enumerate() {
        fftchart.push((i as f64, val as f64));
    }
    let dataset = Dataset::default()
        .marker(Marker::HalfBlock)
        .style(Style::new().fg(Color::Blue))
        .graph_type(GraphType::Bar)
        .data(&fftchart);

    let fft_chart = Chart::new(vec![dataset])
        .block(Block::bordered().title_top(Line::from("SIGNAL").cyan().bold().centered()))
        .x_axis(
            Axis::default()
                .style(Style::default().gray())
                .bounds([0.0, 256.0])
                .labels(["500Hz".bold(), "1.5KHz".bold(), "2.5KHz".bold(), "3.5KHz".bold()]),
        )
        .y_axis(
            Axis::default()
                .style(Style::default().gray())
                .bounds([0.0, 100.0])
                .labels(["0".bold(), "50".into(), "100".bold()]),
        )
        .hidden_legend_constraints((Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)));
    frame.render_widget(fft_chart, target_area);
}
