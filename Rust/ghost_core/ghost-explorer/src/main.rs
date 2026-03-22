// ghost-explorer/src/main.rs

use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        Axis, Block, BorderType, Borders, Cell, Chart, Dataset, Gauge, GraphType,
        Paragraph, Row, Table, Tabs,
    },
    Frame, Terminal,
};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use serde_json::Value;

#[derive(Default, Clone)]
struct NodeStats {
    total_tx: u64,
    tips: u64,
    confirmed: u64,
    pending: u64,
    difficulty: usize,
    tps: f64,
    peers: usize,
}

#[derive(Clone)]
struct TxRow {
    tx_id: String,
    sender: String,
    receiver: String,
    amount: String,
    private: bool,
    status: String,
    weight: u64,
}

#[derive(Default, Clone)]
struct AppState {
    stats: NodeStats,
    transactions: Vec<TxRow>,
    connected: bool,
    node_url: String,
    error: Option<String>,
    tps_history: Vec<(f64, f64)>,
    uptime_secs: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let node_url = args.get(1)
        .cloned()
        .unwrap_or_else(|| "ws://127.0.0.1:9000".to_string());

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let state = Arc::new(Mutex::new(AppState {
        node_url: node_url.clone(),
        ..Default::default()
    }));

    let state_ws = Arc::clone(&state);
    tokio::spawn(async move {
        ws_loop(state_ws, node_url).await;
    });

    let tick = Duration::from_millis(500);
    let mut last_tick = Instant::now();
    let mut tab = 0usize;

    loop {
        let state_snap = state.lock().unwrap().clone();

        terminal.draw(|f| draw(f, &state_snap, tab))?;

        let timeout = tick.checked_sub(last_tick.elapsed()).unwrap_or_default();
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Char('q'), _)
                    | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                    (KeyCode::Tab, _) | (KeyCode::Right, _) => {
                        tab = (tab + 1) % 3;
                    }
                    (KeyCode::BackTab, _) | (KeyCode::Left, _) => {
                        tab = tab.saturating_sub(1);
                    }
                    _ => {}
                }
            }
        }

        if last_tick.elapsed() >= tick {
            // Increment uptime
            if let Ok(mut s) = state.lock() {
                if s.connected {
                    s.uptime_secs += 1;
                }
            }
            last_tick = Instant::now();
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

async fn ws_loop(state: Arc<Mutex<AppState>>, url: String) {
    loop {
        match connect_async(&url).await {
            Ok((mut ws, _)) => {
                {
                    let mut s = state.lock().unwrap();
                    s.connected = true;
                    s.error = None;
                }

                loop {
                    // Send explorer request
                    let req = serde_json::json!({
                        "type": "explorer_request",
                        "payload": {},
                        "timestamp": 0.0,
                        "sender": "tui-explorer"
                    });
                    if ws.send(Message::Text(req.to_string())).await.is_err() {
                        break;
                    }

                    // Wait for response
                    match tokio::time::timeout(
                        Duration::from_secs(5),
                        ws.next()
                    ).await {
                        Ok(Some(Ok(Message::Text(text)))) => {
                            if let Ok(msg) = serde_json::from_str::<Value>(&text) {
                                if msg["type"] == "explorer_response" {
                                    update_state(&state, &msg["payload"]);
                                }
                            }
                        }
                        _ => break,
                    }

                    tokio::time::sleep(Duration::from_secs(2)).await;
                }

                let _ = ws.close(None).await;
            }
            Err(e) => {
                let mut s = state.lock().unwrap();
                s.connected = false;
                s.error = Some(format!("Cannot connect: {}", e));
            }
        }

        let mut s = state.lock().unwrap();
        s.connected = false;
        drop(s);
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

fn update_state(state: &Arc<Mutex<AppState>>, payload: &Value) {
    let mut s = state.lock().unwrap();

    if let Some(stats) = payload.get("stats") {
        s.stats.total_tx = stats["total_tx"].as_u64().unwrap_or(0);
        s.stats.tips = stats["tips"].as_u64().unwrap_or(0);
        s.stats.confirmed = stats["confirmed"].as_u64().unwrap_or(0);
        s.stats.pending = stats["pending"].as_u64().unwrap_or(0);
        s.stats.difficulty = stats["difficulty"].as_u64().unwrap_or(2) as usize;
        s.stats.tps = stats["tps"].as_f64().unwrap_or(0.0);
        s.stats.peers = stats["peers"].as_u64().unwrap_or(0) as usize;

        let t = s.tps_history.len() as f64;
        s.tps_history.push((t, s.stats.tps));
        if s.tps_history.len() > 30 {
            s.tps_history.remove(0);
            for (i, point) in s.tps_history.iter_mut().enumerate() {
                point.0 = i as f64;
            }
        }
    }

    if let Some(txs) = payload["transactions"].as_array() {
        s.transactions = txs.iter().map(|tx| TxRow {
            tx_id: tx["tx_id"].as_str().unwrap_or("?").to_string(),
            sender: tx["sender"].as_str().unwrap_or("?").to_string(),
            receiver: tx["receiver"].as_str().unwrap_or("?").to_string(),
            amount: if tx["private"].as_bool().unwrap_or(false) {
                "🔒 PRIVATE".to_string()
            } else {
                format!("{} GHOST", tx["amount"].as_u64().unwrap_or(0))
            },
            private: tx["private"].as_bool().unwrap_or(false),
            status: tx["status"].as_str().unwrap_or("?").to_string(),
            weight: tx["weight"].as_u64().unwrap_or(1),
        }).collect();
    }
}

const ACCENT: Color = Color::Rgb(0, 245, 196);
const ACCENT2: Color = Color::Rgb(0, 136, 255);
const DIM: Color = Color::Rgb(60, 80, 100);
const ORANGE: Color = Color::Rgb(255, 170, 0);
const PURPLE: Color = Color::Rgb(170, 85, 255);
const RED: Color = Color::Rgb(255, 51, 102);
const BG: Color = Color::Rgb(6, 13, 20);

fn draw(f: &mut Frame, state: &AppState, tab: usize) {
    let size = f.area();

    let bg = Block::default().style(Style::default().bg(BG));
    f.render_widget(bg, size);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  
            Constraint::Length(5),  
            Constraint::Length(3),  
            Constraint::Min(0),     
            Constraint::Length(1),  
        ])
        .split(size);

    draw_header(f, state, chunks[0]);
    draw_stats(f, state, chunks[1]);
    draw_tabs(f, tab, chunks[2]);

    match tab {
        0 => draw_transactions(f, state, chunks[3]),
        1 => draw_tps_chart(f, state, chunks[3]),
        2 => draw_dag(f, state, chunks[3]),
        _ => {}
    }

    draw_footer(f, chunks[4]);
}

fn draw_header(f: &mut Frame, state: &AppState, area: Rect) {
    let (status_sym, status_color, status_text) = if state.connected {
        ("●", ACCENT, format!("CONNECTED  {}  uptime {}s",
            state.node_url, state.uptime_secs))
    } else {
        ("●", RED, format!("DISCONNECTED  {}  {}",
            state.node_url,
            state.error.as_deref().unwrap_or("")))
    };

    let line = Line::from(vec![
        Span::styled("  GHOSTLEDGER ", Style::default()
            .fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled("EXPLORER  ", Style::default().fg(DIM)),
        Span::styled(status_sym, Style::default().fg(status_color)),
        Span::styled("  ", Style::default()),
        Span::styled(status_text, Style::default().fg(DIM)),
    ]);

    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(DIM))
        .border_type(BorderType::Plain);

    let p = Paragraph::new(line)
        .block(block)
        .alignment(Alignment::Left);

    f.render_widget(p, area);
}

fn draw_stats(f: &mut Frame, state: &AppState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![Constraint::Ratio(1, 7); 7])
        .split(area);

    let stats = [
        ("TOTAL TX",   state.stats.total_tx.to_string(),  ACCENT),
        ("TIPS",       state.stats.tips.to_string(),       ACCENT2),
        ("CONFIRMED",  state.stats.confirmed.to_string(),  ACCENT),
        ("PENDING",    state.stats.pending.to_string(),    ORANGE),
        ("DIFFICULTY", state.stats.difficulty.to_string(), PURPLE),
        ("TPS",        format!("{:.2}", state.stats.tps),  ACCENT2),
        ("PEERS",      state.stats.peers.to_string(),      ACCENT),
    ];

    for (i, (label, value, color)) in stats.iter().enumerate() {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(Style::default().fg(DIM));

        let content = vec![
            Line::from(Span::styled(
                label.to_string(),
                Style::default().fg(DIM).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                value.to_string(),
                Style::default().fg(*color).add_modifier(Modifier::BOLD),
            )),
        ];

        let p = Paragraph::new(content)
            .block(block)
            .alignment(Alignment::Center);

        f.render_widget(p, chunks[i]);
    }
}

fn draw_tabs(f: &mut Frame, tab: usize, area: Rect) {
    let titles = vec![
        Line::from(Span::styled(" TRANSACTIONS ", Style::default())),
        Line::from(Span::styled(" TPS CHART ", Style::default())),
        Line::from(Span::styled(" DAG ", Style::default())),
    ];

    let tabs = Tabs::new(titles)
        .select(tab)
        .block(Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(DIM)))
        .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .divider(Span::styled(" │ ", Style::default().fg(DIM)));

    f.render_widget(tabs, area);
}

fn draw_transactions(f: &mut Frame, state: &AppState, area: Rect) {
    let header = Row::new(vec![
        Cell::from("TX ID").style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
        Cell::from("FROM").style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
        Cell::from("TO").style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
        Cell::from("AMOUNT").style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
        Cell::from("STATUS").style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
        Cell::from("WEIGHT").style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
    ])
    .height(1)
    .bottom_margin(0);

    let rows: Vec<Row> = if state.transactions.is_empty() {
        vec![Row::new(vec![
            Cell::from(""),
            Cell::from("// NO TRANSACTIONS YET").style(Style::default().fg(DIM)),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
        ])]
    } else {
        state.transactions.iter().map(|tx| {
            let status_color = match tx.status.as_str() {
                "confirmed" => ACCENT,
                "pending"   => ORANGE,
                "conflict"  => RED,
                _           => DIM,
            };
            let amount_color = if tx.private { PURPLE } else { Color::White };

            Row::new(vec![
                Cell::from(format!("{}…", tx.tx_id))
                    .style(Style::default().fg(ACCENT2)),
                Cell::from(format!("{}…", tx.sender))
                    .style(Style::default().fg(DIM)),
                Cell::from(format!("{}…", tx.receiver))
                    .style(Style::default().fg(DIM)),
                Cell::from(tx.amount.clone())
                    .style(Style::default().fg(amount_color)),
                Cell::from(tx.status.to_uppercase())
                    .style(Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
                Cell::from(tx.weight.to_string())
                    .style(Style::default().fg(DIM)),
            ])
        }).collect()
    };

    let title = format!(" TRANSACTIONS ({}) ", state.transactions.len());
    let table = Table::new(
        rows,
        [
            Constraint::Length(18),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(16),
            Constraint::Length(12),
            Constraint::Min(6),
        ],
    )
    .header(header)
    .block(Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(title, Style::default().fg(ACCENT))))
    .row_highlight_style(Style::default().bg(Color::Rgb(10, 20, 30)));

    f.render_widget(table, area);
}

fn draw_tps_chart(f: &mut Frame, state: &AppState, area: Rect) {
    let data: Vec<(f64, f64)> = if state.tps_history.is_empty() {
        vec![(0.0, 0.0)]
    } else {
        state.tps_history.clone()
    };

    let max_tps = data.iter().map(|(_, y)| *y).fold(0.1_f64, f64::max);
    let x_max = data.len() as f64;

    let datasets = vec![
        Dataset::default()
            .name("TPS")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(ACCENT))
            .data(&data),
    ];

    let chart = Chart::new(datasets)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(Style::default().fg(DIM))
            .title(Span::styled(" TPS HISTORY ", Style::default().fg(ACCENT))))
        .x_axis(Axis::default()
            .style(Style::default().fg(DIM))
            .bounds([0.0, x_max])
            .labels(vec![
                Span::styled("0", Style::default().fg(DIM)),
                Span::styled(format!("{:.0}", x_max / 2.0), Style::default().fg(DIM)),
                Span::styled(format!("{:.0}", x_max), Style::default().fg(DIM)),
            ]))
        .y_axis(Axis::default()
            .style(Style::default().fg(DIM))
            .bounds([0.0, max_tps * 1.2])
            .labels(vec![
                Span::styled("0", Style::default().fg(DIM)),
                Span::styled(format!("{:.1}", max_tps / 2.0), Style::default().fg(DIM)),
                Span::styled(format!("{:.1}", max_tps), Style::default().fg(DIM)),
            ]));

    f.render_widget(chart, area);
}

fn draw_dag(f: &mut Frame, state: &AppState, area: Rect) {
    let txs = &state.transactions;

    if txs.is_empty() {
        let p = Paragraph::new("// NO TRANSACTIONS")
            .style(Style::default().fg(DIM))
            .alignment(Alignment::Center)
            .block(Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(DIM))
                .title(Span::styled(" DAG ", Style::default().fg(ACCENT))));
        f.render_widget(p, area);
        return;
    }

    let lines: Vec<Line> = txs.iter().enumerate().map(|(i, tx)| {
        let node_sym = match tx.status.as_str() {
            "confirmed" => "◆",
            "pending"   => "◇",
            "conflict"  => "✕",
            _           => "○",
        };

        let node_color = match tx.status.as_str() {
            "confirmed" => ACCENT,
            "pending"   => ORANGE,
            "conflict"  => RED,
            _           => DIM,
        };

        let connector = if i < txs.len() - 1 { "│" } else { " " };
        let private_tag = if tx.private { " 🔒" } else { "" };

        Line::from(vec![
            Span::styled(format!("  {} ", node_sym), Style::default().fg(node_color)),
            Span::styled(format!("{}…", tx.tx_id), Style::default().fg(ACCENT2)),
            Span::styled(format!("  {}→{}", tx.sender, tx.receiver), Style::default().fg(DIM)),
            Span::styled(format!("  {}", tx.amount), Style::default().fg(
                if tx.private { PURPLE } else { Color::White }
            )),
            Span::styled(private_tag.to_string(), Style::default().fg(PURPLE)),
            Span::styled(format!("  w:{}", tx.weight), Style::default().fg(DIM)),
            Span::styled(format!("\n  {} ", connector), Style::default().fg(DIM)),
        ])
    }).collect();

    let p = Paragraph::new(lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(Style::default().fg(DIM))
            .title(Span::styled(
                format!(" DAG ({} tx) ", txs.len()),
                Style::default().fg(ACCENT),
            )))
        .scroll((0, 0));

    f.render_widget(p, area);
}

fn draw_footer(f: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled("  [TAB]", Style::default().fg(ACCENT)),
        Span::styled(" switch tab  ", Style::default().fg(DIM)),
        Span::styled("[←][→]", Style::default().fg(ACCENT)),
        Span::styled(" navigate  ", Style::default().fg(DIM)),
        Span::styled("[Q]", Style::default().fg(ACCENT)),
        Span::styled(" quit  ", Style::default().fg(DIM)),
        Span::styled("GhostLedger v0.1", Style::default().fg(DIM)),
    ]);

    let p = Paragraph::new(line)
        .style(Style::default().bg(Color::Rgb(4, 8, 12)))
        .alignment(Alignment::Left);

    f.render_widget(p, area);
}