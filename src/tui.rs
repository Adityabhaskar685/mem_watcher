use std::collections::HashSet;
use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState};

use crate::proc::{
    CpuStats, ProcessEntry, ProcessInfo, get_process_data_realtime, list_all_processes,
    prime_cpu_baseline,
};

const TICK_MS: u64 = 2000;
const CPU_HIGH: f64 = 50.0;
const MEM_HIGH: f64 = 10.0;

enum Screen {
    /// User pick which processes to watch.
    Selector(SelectorState),
    /// Live stats for the chosen PIDs.
    Monitor(MonitorState),
}

struct SelectorState {
    /// Full unfiltered list from /proc
    all: Vec<ProcessEntry>,
    /// Indices into `all` that match the current search query.
    filtered: Vec<usize>,
    /// Current search string typed by the user
    query: String,
    /// Which row in `filtered` the cursor is on.
    cursor: usize,
    /// PIDs the user has ticked with Space.
    selected: HashSet<i32>,
    /// Ratatui table scroll state.
    table_state: TableState,
}

impl SelectorState {
    fn new() -> Self {
        let all = list_all_processes();
        let filtered: Vec<usize> = (0..all.len()).collect();
        let mut s = SelectorState {
            all,
            filtered,
            query: String::new(),
            cursor: 0,
            selected: HashSet::new(),
            table_state: TableState::default(),
        };
        s.table_state.select(Some(0));
        s
    }

    /// Re-build `filtered` to match `query` (case-insensitive substring)
    fn apply_filter(&mut self) {
        let q = self.query.to_lowercase();
        self.filtered = self
            .all
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                q.is_empty() || e.name.to_lowercase().contains(&q) || e.pid.to_string().contains(&q)
            })
            .map(|(i, _)| i)
            .collect();

        self.cursor = 0;
        self.table_state.select(if self.filtered.is_empty() {
            None
        } else {
            Some(0)
        });
    }

    fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.table_state.select(Some(self.cursor));
        }
    }

    fn move_down(&mut self) {
        if !self.filtered.is_empty() && self.cursor < self.filtered.len() - 1 {
            self.cursor += 1;
            self.table_state.select(Some(self.cursor));
        }
    }

    fn toggle_current(&mut self) {
        if let Some(&idx) = self.filtered.get(self.cursor) {
            let pid = self.all[idx].pid;
            if self.selected.contains(&pid) {
                self.selected.remove(&pid);
            } else {
                self.selected.insert(pid);
            }
        }
    }

    fn selected_pids(&self) -> Vec<i32> {
        let mut pids: Vec<i32> = self.selected.iter().copied().collect();
        pids.sort();
        pids
    }
}

struct MonitorState {
    pids: Vec<i32>,
    show_threads: bool,
    cpu_stats: CpuStats,
    /// Latest snapshot - shown while next tick is pending.
    processes: Vec<ProcessInfo>,
    last_tick: Instant,
    elapsed_secs: u64,
    start: Instant,
    table_state: TableState,
    scroll_offset: usize,
}

impl MonitorState {
    fn new(pids: Vec<i32>, show_threads: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let mut cpu_stats = CpuStats::new()?;
        prime_cpu_baseline(&pids, &mut cpu_stats)?;
        // Brief warm-up so first sample has a real denominator
        std::thread::sleep(Duration::from_millis(200));
        let processes =
            get_process_data_realtime(&pids, &mut cpu_stats, show_threads).unwrap_or_default();
        Ok(MonitorState {
            pids,
            show_threads,
            cpu_stats,
            processes,
            last_tick: Instant::now(),
            elapsed_secs: 0,
            start: Instant::now(),
            table_state: TableState::default(),
            scroll_offset: 0,
        })
    }

    fn refresh(&mut self) {
        if let Ok(data) =
            get_process_data_realtime(&self.pids, &mut self.cpu_stats, self.show_threads)
        {
            self.processes = data;
        }
        self.elapsed_secs = self.start.elapsed().as_secs();
        self.last_tick = Instant::now();
    }

    fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    fn scroll_down(&mut self) {
        if self.scroll_offset + 1 < self.processes.len() {
            self.scroll_offset += 1;
        }
    }
}

/// Launch the full TUI. Clean up the terminal correctly.
pub fn run_tui(show_threads: bool) -> Result<(), Box<dyn std::error::Error>> {
    // Set a panic hook that restores the terminal before printing the panic.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        original_hook(info)
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, show_threads);

    restore_terminal()?;
    result
}

fn restore_terminal() -> Result<(), Box<dyn std::error::Error>> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    show_threads: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut screen = Screen::Selector(SelectorState::new());

    loop {
        // Draw
        terminal.draw(|f| match &mut screen {
            Screen::Selector(s) => draw_selector(f, s),
            Screen::Monitor(m) => draw_monitor(f, m),
        })?;

        // Tick Refersh for minitor
        if let Screen::Monitor(ref mut m) = screen {
            if m.last_tick.elapsed() >= Duration::from_millis(TICK_MS) {
                m.refresh();
            }
        }

        // Input (non-blocking, 100 ms poll)
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // Ctrl-c always quits
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Ok(());
                }
                match &mut screen {
                    Screen::Selector(s) => {
                        match key.code {
                            KeyCode::Char('q') => return Ok(()),
                            KeyCode::Up => s.move_up(),
                            KeyCode::Down => s.move_down(),
                            KeyCode::Char(' ') => s.toggle_current(),
                            KeyCode::Enter => {
                                let pids = s.selected_pids();
                                if !pids.is_empty() {
                                    match MonitorState::new(pids, show_threads) {
                                        Ok(m) => screen = Screen::Monitor(m),
                                        Err(e) => {
                                            // show error briefly - stay on selector.
                                            eprintln!("Error starting monitor: {}", e);
                                        }
                                    }
                                }
                            }
                            KeyCode::Backspace => {
                                if let Screen::Selector(ref mut s) = screen {
                                    s.query.pop();
                                    s.apply_filter();
                                }
                            }
                            KeyCode::Char(c) => {
                                if let Screen::Selector(ref mut s) = screen {
                                    s.query.push(c);
                                    s.apply_filter();
                                }
                            }
                            _ => {}
                        }
                    }
                    Screen::Monitor(m) => match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char('s') => {
                            screen = Screen::Selector(SelectorState::new());
                        }
                        KeyCode::Char('t') => {
                            m.show_threads = !m.show_threads;
                            m.refresh();
                        }
                        KeyCode::Up => m.scroll_up(),
                        KeyCode::Down => m.scroll_down(),
                        _ => {}
                    },
                }
            }
        }
    }
}

fn draw_selector(f: &mut ratatui::Frame, s: &mut SelectorState) {
    let area = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Search box
            Constraint::Min(0),    // process list
            Constraint::Length(2), // help bar
        ])
        .split(area);

    // Search box
    let search_text = format!(" Search: {}_", s.query);
    let search = Paragraph::new(search_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" mem_watcher - Select Processes ")
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .style(Style::default().fg(Color::White));
    f.render_widget(search, chunks[0]);

    // Process Table
    let rows: Vec<Row> = s
        .filtered
        .iter()
        .map(|&idx| {
            let entry = &s.all[idx];
            let checked = if s.selected.contains(&entry.pid) {
                "☑"
            } else {
                "☐"
            };
            Row::new(vec![
                Cell::from(checked),
                Cell::from(entry.pid.to_string()),
                Cell::from(entry.name.clone()),
            ])
        })
        .collect();

    let selected_count = s.selected.len();
    let title = format!(
        " Processes ({} shown, {} selected) ",
        s.filtered.len(),
        selected_count
    );

    let table = Table::new(
        rows,
        [
            Constraint::Length(3), // checkbox
            Constraint::Length(8), // PID
            Constraint::Min(20),   // name
        ],
    )
    .header(
        Row::new(vec!["", "PID", "NAME"]).style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::Cyan)),
    )
    .row_highlight_style(
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol("▶ ");

    f.render_stateful_widget(table, chunks[1], &mut s.table_state);

    //Help bar
    let help = Line::from(vec![
        help_key("↑↓", "navigate"),
        help_sep(),
        help_key("Space", "select/deselect"),
        help_sep(),
        help_key("Enter", "start monitoring"),
        help_sep(),
        help_key("q", "quit"),
    ]);
    let help_bar = Paragraph::new(help).style(Style::default().bg(Color::DarkGray));
    f.render_widget(help_bar, chunks[2]);
}

fn draw_monitor(f: &mut ratatui::Frame, m: &mut MonitorState) {
    let area = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(0),    // table
            Constraint::Length(3), // summary
            Constraint::Length(2), // help bar
        ])
        .split(area);

    // ── Header ──
    let next_tick = TICK_MS.saturating_sub(m.last_tick.elapsed().as_millis() as u64);
    let header_text = format!(
        " ⏱  {}s elapsed  │  PIDs: {}  │  next refresh in {}ms ",
        m.elapsed_secs,
        m.pids
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", "),
        next_tick,
    );
    let header = Paragraph::new(header_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" mem_watcher — Live Monitor ")
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .style(Style::default().fg(Color::White));
    f.render_widget(header, chunks[0]);

    // ── Stats table ──
    let col_widths = [
        Constraint::Length(7),  // PID
        Constraint::Length(6),  // PPID
        Constraint::Min(16),    // NAME
        Constraint::Length(5),  // STATE
        Constraint::Length(7),  // %CPU
        Constraint::Length(7),  // %MEM
        Constraint::Length(10), // RSS KB
        Constraint::Length(10), // VSZ KB
        Constraint::Length(8),  // THREADS
        Constraint::Length(6),  // FDS
        Constraint::Length(10), // CPU_TIME
        Constraint::Length(10), // UPTIME
    ];

    let header_row = Row::new(vec![
        "PID", "PPID", "NAME", "ST", "%CPU", "%MEM", "RSS KB", "VSZ KB", "THRD", "FDS", "CPU_TIME",
        "UPTIME",
    ])
    .style(
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = m
        .processes
        .iter()
        .skip(m.scroll_offset)
        .map(|p| {
            let cpu: f64 = p.cpu_percent.parse().unwrap_or(0.0);
            let mem: f64 = p.mem_percent.parse().unwrap_or(0.0);

            let row_style = if cpu >= CPU_HIGH || mem >= MEM_HIGH {
                Style::default().fg(Color::Red)
            } else if p.name.starts_with("|-") {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            };

            Row::new(vec![
                Cell::from(p.pid.clone()),
                Cell::from(p.ppid.clone()),
                Cell::from(p.name.clone()),
                Cell::from(p.state.clone()),
                Cell::from(p.cpu_percent.clone()),
                Cell::from(p.mem_percent.clone()),
                Cell::from(p.rss_kb.clone()),
                Cell::from(p.vsz_kb.clone()),
                Cell::from(p.threads.clone()),
                Cell::from(p.file_descriptors.clone()),
                Cell::from(p.cpu_time.clone()),
                Cell::from(p.uptime.clone()),
            ])
            .style(row_style)
        })
        .collect();

    let table = Table::new(rows, col_widths)
        .header(header_row)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(
                    " {} process(es) {} ",
                    m.processes
                        .iter()
                        .filter(|p| !p.name.starts_with("|-"))
                        .count(),
                    if m.show_threads { "+ threads" } else { "" }
                ))
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .row_highlight_style(Style::default().bg(Color::DarkGray))
        .row_highlight_style(Style::default().add_modifier(Modifier::BOLD));

    f.render_stateful_widget(table, chunks[1], &mut m.table_state);

    // ── Summary bar ──
    let mains: Vec<&ProcessInfo> = m
        .processes
        .iter()
        .filter(|p| !p.name.starts_with("|-"))
        .collect();

    let total = mains.len();
    let avg_cpu = if total > 0 {
        mains
            .iter()
            .filter_map(|p| p.cpu_percent.parse::<f64>().ok())
            .sum::<f64>()
            / total as f64
    } else {
        0.0
    };
    let avg_mem = if total > 0 {
        mains
            .iter()
            .filter_map(|p| p.mem_percent.parse::<f64>().ok())
            .sum::<f64>()
            / total as f64
    } else {
        0.0
    };
    let total_rss: u64 = mains
        .iter()
        .filter_map(|p| p.rss_kb.parse::<u64>().ok())
        .sum();
    let total_fds: u64 = mains
        .iter()
        .filter_map(|p| p.file_descriptors.parse::<u64>().ok())
        .sum();

    let cpu_color = if avg_cpu >= CPU_HIGH {
        Color::Red
    } else {
        Color::Green
    };
    let mem_color = if avg_mem >= MEM_HIGH {
        Color::Red
    } else {
        Color::Green
    };

    let summary = Line::from(vec![
        Span::raw("  📈 "),
        Span::styled(format!("{} proc", total), Style::default().fg(Color::Cyan)),
        Span::raw("  │  CPU: "),
        Span::styled(format!("{:.1}%", avg_cpu), Style::default().fg(cpu_color)),
        Span::raw("  │  MEM: "),
        Span::styled(format!("{:.1}%", avg_mem), Style::default().fg(mem_color)),
        Span::raw(format!("  │  RSS: {} KB", total_rss)),
        Span::raw(format!("  │  FDs: {}", total_fds)),
    ]);
    let summary_widget = Paragraph::new(summary).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(summary_widget, chunks[2]);

    // Help bar
    let help = Line::from(vec![
        help_key("↑↓", "scroll"),
        help_sep(),
        help_key("t", "toggle threads"),
        help_sep(),
        help_key("s", "back to selector"),
        help_sep(),
        help_key("q / Ctrl-C", "quit"),
    ]);
    let help_bar = Paragraph::new(help).style(Style::default().bg(Color::DarkGray));
    f.render_widget(help_bar, chunks[3]);

    // "No data" overlay
    if m.processes.is_empty() {
        let popup = centered_rect(40, 20, area);
        f.render_widget(Clear, popup);
        let msg = Paragraph::new("  No data — processes may have exited.")
            .block(Block::default().borders(Borders::ALL).title(" Warning "))
            .style(Style::default().fg(Color::Yellow));
        f.render_widget(msg, popup);
    }
}

/// Create a centred rectangle of `percent_x` × `percent_y` within `r`.
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn help_key<'a>(key: &'a str, desc: &'a str) -> Span<'a> {
    Span::raw(format!("  [{}] {} ", key, desc))
}

fn help_sep<'a>() -> Span<'a> {
    Span::styled("│", Style::default().fg(Color::DarkGray))
}

