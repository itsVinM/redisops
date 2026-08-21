use std::io::stdout;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, Paragraph};
use ratatui::Terminal;

use redisops::server::{Config, Server};
use redisops::stats::{Stats, StatsSnapshot};

fn format_uptime(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{}h {}m {}s", h, m, s)
    } else if m > 0 {
        format!("{}m {}s", m, s)
    } else {
        format!("{}s", s)
    }
}

fn ui(frame: &mut ratatui::Frame, snap: &StatsSnapshot) {
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let title_text = Line::from(vec![
        Span::styled(
            " redis-rs Monitor ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(&snap.addr, Style::default().fg(Color::Yellow)),
        Span::raw("  "),
        Span::styled(
            format_uptime(snap.uptime_secs),
            Style::default().fg(Color::Green),
        ),
    ]);
    let title = Paragraph::new(title_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain),
    );
    frame.render_widget(title, chunks[0]);

    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(chunks[1]);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(4),
            Constraint::Min(0),
        ])
        .split(body_chunks[0]);

    let conn_lines = vec![
        Line::from(format!(" Active:   {:>8}", snap.active_connections)),
        Line::from(format!(" Total:    {:>8}", snap.total_connections)),
        Line::from(""),
        Line::from(format!(" Commands: {:>8}", snap.total_commands)),
    ];
    let conn_block = Paragraph::new(conn_lines).block(
        Block::default()
            .title(" Connections ")
            .borders(Borders::ALL),
    );
    frame.render_widget(conn_block, left_chunks[0]);

    let keys_info = Paragraph::new(vec![Line::from(format!(
        " Stored keys: {}",
        "? (query via KEYS)"
    ))])
    .block(Block::default().title(" Keys ").borders(Borders::ALL));
    frame.render_widget(keys_info, left_chunks[1]);

    let cmds: Vec<ListItem> = snap
        .last_cmds
        .iter()
        .rev()
        .take(32)
        .map(|c| {
            let style = match c.to_lowercase().as_str() {
                "set" | "del" => Style::default().fg(Color::Yellow),
                "get" => Style::default().fg(Color::Green),
                "zadd" | "zrem" | "zscore" | "zquery" => Style::default().fg(Color::Cyan),
                _ => Style::default().fg(Color::White),
            };
            ListItem::new(Line::from(Span::styled(c, style)))
        })
        .collect();

    let cmds_list = List::new(cmds).block(
        Block::default()
            .title(" Recent Commands ")
            .borders(Borders::ALL),
    );
    frame.render_widget(cmds_list, body_chunks[1]);
}

fn run_tui(stats: Arc<Stats>, shutdown_rx: tokio::sync::watch::Receiver<bool>) {
    let res = (|| -> Result<(), Box<dyn std::error::Error>> {
        enable_raw_mode()?;
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        loop {
            terminal.draw(|f| ui(f, &stats.snapshot()))?;

            if *shutdown_rx.borrow() {
                break;
            }

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        _ => {}
                    }
                }
            }
        }

        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;
        Ok(())
    })();

    if let Err(e) = res {
        eprintln!("TUI error: {}", e);
    }
}

#[tokio::main]
async fn main() {
    let stats = Stats::new();
    let cfg = Config::default();
    let srv = Server::new_with_stats(cfg, stats.clone());

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    tokio::spawn(async move {
        if let Err(e) = srv.listen_and_serve().await {
            tracing::error!("server error: {}", e);
            let _ = shutdown_tx.send(true);
        }
    });

    tokio::time::sleep(Duration::from_millis(200)).await;

    run_tui(stats, shutdown_rx);
}
