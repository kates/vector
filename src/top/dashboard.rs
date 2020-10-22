use super::{
    events::{Event, Events},
    state::{WidgetsState, TOPOLOGY_HEADERS},
};
use std::io::Stdout;
use termion::event::Key;
use termion::raw::{IntoRawMode, RawTerminal};
use termion::screen::AlternateScreen;
use tui::layout::Rect;
use tui::widgets::Wrap;
use tui::{
    backend::{Backend, TermionBackend},
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Paragraph, Row, Table},
    Frame, Terminal,
};

const INVARIANT: &str =
    "Unable to create terminal session for the Vector top dashboard. Please report this.";

pub struct Widgets {
    constraints: Vec<Constraint>,
    state: WidgetsState,
}

impl Widgets {
    pub fn new(state: WidgetsState) -> Self {
        let constraints = vec![
            Constraint::Length(3),
            Constraint::Max(90),
            Constraint::Length(3),
        ];

        Self { constraints, state }
    }

    /// Renders a title showing 'Vector', and the URL the dashboard is currently connected to
    fn title<B: Backend>(&self, f: &mut Frame<B>, area: Rect) {
        let text = vec![Spans::from(self.state.url())];

        let block = Block::default().borders(Borders::ALL).title(Span::styled(
            "Vector",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
        let w = Paragraph::new(text).block(block).wrap(Wrap { trim: true });

        f.render_widget(w, area);
    }

    /// Renders a topology table, showing sources, transforms and sinks in tabular form, with
    /// statistics pulled from `topology_state`
    fn topology_table<B: Backend>(&self, f: &mut Frame<B>, area: Rect) {
        let items = self.state.topology().load().rows().into_iter().map(|r| {
            Row::StyledData(
                vec![
                    r.name.clone(),
                    r.topology_type.clone(),
                    r.format_events_processed_total(),
                    r.format_errors(),
                    r.format_throughput(),
                ]
                .into_iter(),
                Style::default().fg(Color::White),
            )
        });

        let w = Table::new(TOPOLOGY_HEADERS.iter(), items)
            .block(Block::default().borders(Borders::ALL).title("Topology"))
            .header_gap(1)
            .column_spacing(2)
            .widths(&[
                Constraint::Percentage(20),
                Constraint::Percentage(20),
                Constraint::Percentage(20),
                Constraint::Percentage(20),
                Constraint::Percentage(20),
            ]);

        f.render_widget(w, area);
    }

    /// Renders a box showing instructions on how to exit from `vector top`
    fn quit_box<B: Backend>(&self, f: &mut Frame<B>, area: Rect) {
        let text = vec![Spans::from("To quit, press ESC or 'q'")];

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Gray));
        let w = Paragraph::new(text)
            .block(block)
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Left);

        f.render_widget(w, area);
    }

    fn draw<B: Backend>(&self, f: &mut Frame<B>) {
        let rects = Layout::default()
            .constraints(self.constraints.as_ref())
            .split(f.size());

        self.title(f, rects[0]);
        self.topology_table(f, rects[1]);
        self.quit_box(f, rects[2]);
    }
}

pub struct Dashboard {
    terminal: Terminal<TermionBackend<AlternateScreen<RawTerminal<Stdout>>>>,
}

impl Dashboard {
    /// Create/return a new dashboard. This initializes a new terminal using `AlternateScreen`,
    /// which has the effect of 'overlaying' the existing terminal window to avoid messing with
    /// an existing console session. Exiting from the dashboard removed the window overlay so
    /// the user can return to their previous session
    pub fn new() -> Self {
        let stdout = AlternateScreen::from(std::io::stdout().into_raw_mode().expect(INVARIANT));
        let backend = TermionBackend::new(stdout);
        let terminal = Terminal::new(backend).expect(INVARIANT);

        Self { terminal }
    }

    /// Run the current dashboard by rendering out to the terminal. This will block until the
    /// user exists by pressing `q`
    pub fn run(&mut self, widgets: &Widgets) {
        let events = Events::new();

        loop {
            self.terminal.draw(|f| widgets.draw(f)).expect(INVARIANT);

            if let Event::Input(key) = events.next().unwrap() {
                match key {
                    Key::Char('q') | Key::Esc => {
                        break;
                    }
                    _ => {}
                }
            };
        }
    }
}