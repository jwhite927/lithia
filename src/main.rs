#[allow(dead_code)]
mod util;

use crate::util::event::{Event, Events};
// use std::{error::Error, io, sync::{Mutex, Arc}};
use sqlx::{Row, mysql::{MySqlPool, MySqlPoolOptions, MySqlRow}};
use std::{
    collections::HashMap,
    error::Error,
    io,
    sync::{Arc, Mutex},
};
use termion::{event::Key, input::MouseTerminal, raw::IntoRawMode, screen::AlternateScreen};
use tui::{
    backend::TermionBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Span, Spans, Text},
    // widgets::{Block, Borders, List, ListItem, Paragraph},
    widgets::{Block, Borders, Paragraph, Wrap, Table, Row as TuiRow},
    Terminal,
};
use unicode_width::UnicodeWidthStr;

#[derive(Debug)]
enum InputMode {
    Normal,
    Connection,
    EditingQuery,
}

#[derive(Debug)]
enum DbStatus {
    Connected,
    Disconnected,
}

#[derive(Debug)]
enum QueryStatus {
    Waiting,
    Complete,
    NotStarted,
}

#[derive(Debug, Clone)]
struct Model {
    shared: Arc<Shared>,
}

impl Model {
    fn new() -> Model {
        Model {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    database_input: "mysql://root:abc123@127.0.0.1:3306/lithia".to_string(),
                    query_input: "SELECT * FROM simple_table;".to_string(),
                    input_mode: InputMode::Normal,
                    connections: Vec::new(),
                    db_status: DbStatus::Disconnected,
                    query_status: QueryStatus::NotStarted,
                    messages: Vec::new(),
                }),
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<State> {
        self.shared.state.lock().unwrap()
    }
}

#[derive(Debug)]
struct Shared {
    state: Mutex<State>,
}

#[derive(Debug)]
struct State {
    database_input: String,
    query_input: String,
    input_mode: InputMode,
    connections: Vec<(String, DbStatus)>,
    db_status: DbStatus,
    query_status: QueryStatus,
    messages: Vec<MySqlRow>,
}

// Acts as View
fn main() -> Result<(), Box<dyn Error>> {
    // Terminal initialization
    let stdout = io::stdout().into_raw_mode()?;
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut events = Events::new();

    let (tx, rx) = std::sync::mpsc::channel();
    let model = Model::new();
    let cloned_model = model.clone();

    std::thread::spawn(move || {
        start_tokio(model, rx).unwrap();
    });

    loop {
        let mut unlocked_model = cloned_model.lock();
        // Draw UI
        terminal.draw(|f| {
            let screen_size = f.size();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(2)
                .constraints(
                    [
                        Constraint::Length(3),
                        Constraint::Length(1),
                        Constraint::Length(3),
                        Constraint::Min(1),
                    ]
                    .as_ref(),
                )
                .split(screen_size);

            let db_input_msg = match unlocked_model.input_mode {
                InputMode::Normal | InputMode::EditingQuery => {
                    vec![
                        Span::raw(" Connection (c) - Status: "),
                        match unlocked_model.db_status {
                            DbStatus::Connected => {
                                Span::styled("Connected ", Style::default().fg(Color::Green))
                            }
                            DbStatus::Disconnected => {
                                Span::styled("Disconnected ", Style::default().fg(Color::Red))
                            }
                        },
                    ]
                }
                InputMode::Connection => (vec![Span::raw(" Editing connection... ")]),
            };

            let db_input = Paragraph::new(unlocked_model.database_input.as_ref())
                .style(Style::default())
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(Spans::from(db_input_msg)),
                );
            f.render_widget(db_input, chunks[0]);

            let conn_width = 2 * screen_size.width / 3;
            let conn_height = 2 * screen_size.height / 3;
            let popup_box = Layout::default()
                .direction(Direction::Vertical)
                .margin(2)
                .constraints([Constraint::Min(3)].as_ref())
                .split(Rect {
                    x: screen_size.width / 2 - conn_width / 2,
                    y: screen_size.height / 2 - conn_height / 2,
                    width: conn_width,
                    height: conn_height,
                });

            if let InputMode::Connection = unlocked_model.input_mode {
                let test_conn = Paragraph::new("Test")
                    .block(Block::default().borders(Borders::ALL).title("Connections"));
                f.render_widget(test_conn, popup_box[0]);
            }

            let (input_msg, style) = match unlocked_model.input_mode {
                InputMode::Normal | InputMode::Connection => (
                    vec![
                        Span::raw("Query (e) Status: "),
                        match unlocked_model.query_status {
                            QueryStatus::Complete => {
                                Span::styled("Complete", Style::default().fg(Color::Green))
                            }
                            QueryStatus::Waiting => {
                                Span::styled("Waiting", Style::default().fg(Color::Yellow))
                            }
                            QueryStatus::NotStarted => Span::styled("Ready", Style::default()),
                        },
                    ],
                    Style::default().add_modifier(Modifier::RAPID_BLINK),
                ),
                InputMode::EditingQuery => (
                    vec![
                        Span::raw("Press "),
                        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(" to stop editing, "),
                        Span::styled("Enter", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(" to record the message"),
                    ],
                    Style::default(),
                ),
            };

            let mut text = Text::from(Spans::from(input_msg));
            text.patch_style(style);
            let help_message = Paragraph::new(text);
            f.render_widget(help_message, chunks[1]);

            let input = Paragraph::new(unlocked_model.query_input.as_ref())
                .style(match unlocked_model.input_mode {
                    InputMode::Normal | InputMode::Connection => Style::default(),
                    InputMode::EditingQuery => Style::default().fg(Color::Yellow),
                })
                .block(Block::default().borders(Borders::ALL).title("Input"));
            f.render_widget(input, chunks[2]);
            match unlocked_model.input_mode {
                InputMode::Normal =>
                    // Hide the cursor. `Frame` does this by default, so we don't need to do anything here
                    {}

                InputMode::EditingQuery => {
                    // Make the cursor visible and ask tui-rs to put it at the specified coordinates after rendering
                    f.set_cursor(
                        // Put cursor past the end of the input text
                        chunks[2].x + unlocked_model.query_input.width() as u16 + 1,
                        // Move one line down, from the border to the input line
                        chunks[2].y + 1,
                    )
                }
                InputMode::Connection => {
                    // Make the cursor visible and ask tui-rs to put it at the specified coordinates after rendering
                    f.set_cursor(
                        // Put cursor past the end of the input text
                        chunks[0].x + unlocked_model.database_input.width() as u16 + 1,
                        // Move one line down, from the border to the input line
                        chunks[0].y + 1,
                    )
                }
            }

            let messages = Table::new(unlocked_model.messages.iter().map(|&row| TuiRow::new(format!("{}", row.get(0)))).collect::<Vec<TuiRow>>())
                .style(Style::default().fg(Color::White))
                .block(Block::default().borders(Borders::ALL).title("Results"));
            f.render_widget(messages, chunks[3]);
        })?;

        // Handle input
        if let Event::Input(input) = events.next()? {
            match unlocked_model.input_mode {
                InputMode::Normal => match input {
                    Key::Char('e') => {
                        unlocked_model.input_mode = InputMode::EditingQuery;
                        events.disable_exit_key();
                    }
                    Key::Char('c') => {
                        unlocked_model.input_mode = InputMode::Connection;
                        events.disable_exit_key();
                    }
                    Key::Char('q') => {
                        break;
                    }
                    _ => {}
                },
                InputMode::EditingQuery => match input {
                    Key::Char('\n') => {
                        unlocked_model.input_mode = InputMode::Normal;
                        unlocked_model.query_status = QueryStatus::Waiting;
                        tx.send(Msg::Query(
                            unlocked_model.query_input.clone(),
                            unlocked_model.database_input.clone(),
                        ))?;
                    }
                    Key::Char(c) => {
                        unlocked_model.query_input.push(c);
                    }
                    Key::Backspace => {
                        unlocked_model.query_input.pop();
                    }
                    Key::Esc => {
                        unlocked_model.input_mode = InputMode::Normal;
                        events.enable_exit_key();
                    }
                    _ => {}
                },
                InputMode::Connection => match input {
                    Key::Char('\n') => {
                        unlocked_model.input_mode = InputMode::Normal;
                        tx.send(Msg::Connect(unlocked_model.database_input.clone()))?;
                    }
                    Key::Char(c) => {
                        unlocked_model.database_input.push(c);
                    }
                    Key::Backspace => {
                        unlocked_model.database_input.pop();
                    }
                    Key::Esc => {
                        unlocked_model.input_mode = InputMode::Normal;
                        events.enable_exit_key();
                    }
                    _ => {}
                },
            }
        }
    }
    Ok(())
}

// Msg defines what can be done with Databases
pub enum Msg {
    Query(String, String),
    Connect(String),
    None,
}

// Acts as Update Function for DB Activities
#[tokio::main]
async fn start_tokio(
    model: Model,
    io_rx: std::sync::mpsc::Receiver<Msg>,
) -> Result<(), Box<dyn Error>> {
    let mut connections = HashMap::new();

    while let Ok(msg) = io_rx.recv() {
        match msg {
            Msg::Query(query, uri) => {
                let db_conn = connections.get(&uri).unwrap();
                let results =  sqlx::query(&query).fetch_all(db_conn).await?;

                let mut model = model.lock();
                model.messages = results;
                model.query_status = QueryStatus::Complete;
                model.connections = update_connections(&connections);
            }
            Msg::Connect(uri) => {
                let pool = MySqlPoolOptions::new().connect(&uri).await?;
                connections.insert(uri.clone(), pool);
                let mut model = model.lock();
                model.db_status = DbStatus::Connected;
            }
            Msg::None => {}
        }
    }
    Ok(())
}

fn update_connections(connections: &HashMap<String, MySqlPool>) -> Vec<(String, DbStatus)> {
    connections
        .iter()
        .map(|(name, conn)| {
            (
                String::from(name),
                if conn.size() > 0 {
                    DbStatus::Connected
                } else {
                    DbStatus::Disconnected
                },
            )
        })
        .collect()
}
