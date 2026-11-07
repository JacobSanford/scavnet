//! Terminal front end: owns the terminal, polls keys, and draws the engine's state.

use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::prelude::CrosstermBackend;
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Padding, Paragraph, Sparkline};
use ratatui::{Frame, Terminal};

use crate::audio::spectrum::{MAX_HZ, MIN_HZ};
use crate::engine::{mhz, Engine, Status};

/// Raw mode and the alternate screen, restored on drop and on panic.
struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            default_hook(info);
        }));
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        let terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore();
        let _ = std::panic::take_hook();
    }
}

fn restore() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
}

/// Runs until the user quits with q, Esc or Ctrl-C.
pub fn run(engine: &mut Engine, redraw_hz: u32) -> io::Result<()> {
    let mut guard = TerminalGuard::enter()?;
    let frame_time = Duration::from_secs_f64(1.0 / f64::from(redraw_hz));
    engine.log_info("Connected to antenna. Press q to quit.");
    loop {
        let frame_start = Instant::now();
        engine.tick(frame_start);
        guard.terminal.draw(|f| draw(f, engine))?;
        let deadline = frame_start + frame_time;
        loop {
            let timeout = deadline.saturating_duration_since(Instant::now());
            if !event::poll(timeout)? {
                break;
            }
            if let Event::Key(key) = event::read()? {
                let ctrl_c = key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');
                if key.kind == KeyEventKind::Press && (matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) || ctrl_c)
                {
                    engine.shutdown();
                    return Ok(());
                }
            }
        }
    }
}

fn draw(frame: &mut Frame, engine: &Engine) {
    let [title, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(frame.area());
    let [top, bottom] = Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body);
    let [left, right] = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(top);

    frame.render_widget(
        Block::new()
            .borders(Borders::TOP)
            .title(format!(" scavnet v{} ", env!("CARGO_PKG_VERSION"))),
        title,
    );
    draw_scanner(frame, engine, left);
    draw_system(frame, engine, right);
    draw_signal(frame, engine, bottom);
}

fn panel(title: &'static str) -> Block<'static> {
    Block::bordered()
        .padding(Padding::horizontal(1))
        .title_top(Line::from(title).cyan().bold().centered())
}

fn draw_scanner(frame: &mut Frame, engine: &Engine, area: Rect) {
    let (status, color) = match engine.status() {
        Status::Scanning => ("Scanning...", Color::White),
        Status::Receiving => ("Receiving transmission...", Color::Green),
    };
    let row = |label: &'static str, value: Span<'static>| {
        Line::from(vec![
            Span::styled(format!("{label:<10}"), Style::new().italic()),
            ": ".into(),
            value,
        ])
    };
    let mut lines = vec![
        row("Status", Span::styled(status, Style::new().fg(color))),
        row(
            "TimeCode",
            Span::raw(chrono::Local::now().format("%d-%m-%Y %H:%M:%S").to_string()),
        ),
        row("Frequency", Span::raw(mhz(engine.frequency()))),
        row(
            "Network",
            Span::raw(engine.network().unwrap_or("UNKNOWN").to_uppercase()),
        ),
    ];
    if let Some(caption) = engine.caption() {
        lines.push(row(
            "Caption",
            Span::styled(caption.to_string(), Style::new().fg(Color::Yellow)),
        ));
    }
    frame.render_widget(Paragraph::new(lines).block(panel("SCANNER")), area);
}

fn draw_system(frame: &mut Frame, engine: &Engine, area: Rect) {
    let block = panel("SYSTEM");
    let rows = block.inner(area).height as usize;
    let lines: Vec<Line> = engine.log().tail(rows).map(Line::raw).collect();
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_signal(frame: &mut Frame, engine: &Engine, area: Rect) {
    let block = Block::bordered()
        .title_top(Line::from("SIGNAL").cyan().bold().centered())
        .title_bottom(
            Line::from(format!(" {MIN_HZ:.0} Hz – {:.0} kHz ", MAX_HZ / 1000.0))
                .gray()
                .centered(),
        );
    let width = block.inner(area).width as usize;
    let bands = engine.spectrum();
    // Stretch the bands across the full width so the chart fills the panel at any size.
    let data: Vec<u64> = (0..width)
        .map(|x| u64::from(bands[x * bands.len() / width.max(1)]))
        .collect();
    frame.render_widget(
        Sparkline::default()
            .block(block)
            .data(&data)
            .max(255)
            .style(Style::new().fg(Color::Blue)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::loader::ThreadLoader;
    use crate::audio::output::NullOut;
    use crate::settings::Settings;
    use crate::world::progress::Progress;
    use crate::world::test_support::sample_world;
    use crate::world::World;
    use ratatui::backend::TestBackend;

    #[test]
    fn renders_at_any_terminal_size() {
        let dir = tempfile::tempdir().unwrap();
        let (net, lib) = sample_world(dir.path());
        let world = World::load(&net, &lib).unwrap();
        let loader = ThreadLoader::spawn(dir.path().join("cache")).unwrap();
        let now = Instant::now();
        let mut engine = Engine::new(
            world,
            Settings::default(),
            Progress::default(),
            None,
            Box::new(loader),
            Box::new(NullOut::default()),
            now,
        );
        engine.tick(now);
        for (w, h) in [(0, 0), (1, 1), (3, 2), (12, 6), (40, 10), (200, 60)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| draw(f, &engine)).unwrap();
        }
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| draw(f, &engine)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        for expected in ["SCANNER", "SYSTEM", "SIGNAL", "Scanning...", "0.00100 MHz", "A"] {
            assert!(text.contains(expected), "missing {expected:?}");
        }
    }
}
