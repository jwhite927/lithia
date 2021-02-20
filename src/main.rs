#[allow(dead_code)]
mod util;

use crate::util::event::{Event, Events};
use std::{error::Error, io, sync::{Mutex, Arc}, collections::HashMap};
use termion::{event::Key, input::MouseTerminal, raw::IntoRawMode, screen::AlternateScreen};
use tui::{
    backend::TermionBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans, Text},
    // widgets::{Block, Borders, List, ListItem, Paragraph},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use unicode_width::UnicodeWidthStr;
use sqlx::mysql::{MySqlPoolOptions, MySqlPool};

enum InputMode {
    Normal,
    EditingDb,
    EditingQuery,
}

enum DbStatus {
    Connected,
    Disconnected,
}

enum QueryStatus {
    Waiting,
    Complete,
    NotStarted,
}

struct Model {
    database_input: String,
    query_input: String,
    input_mode: InputMode,
    db_status: DbStatus,
    db_connection: Option<MySqlPool>,
    query_status: QueryStatus,
    messages: String,
}

impl Model {
    fn new() -> Model {
        Model {
            // database_input: String::new(),
            // query_input: String::new(),
            database_input: "mysql://root:abc123@127.0.0.1:3307/lithia".to_string(),
            query_input: "SELECT * FROM simple_table;".to_string(),
            input_mode: InputMode::Normal,
            db_status: DbStatus::Disconnected,
            db_connection: None,
            query_status: QueryStatus::NotStarted,
            messages: String::new(),
        }
    }
}



// Acts as View
fn main() -> Result<(), Box<dyn Error>> {
    // Terminal initialization
    let stdout = io::stdout().into_raw_mode()?;
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;


    // Setup event handlers
    let mut events = Events::new();

    let (tx, rx) = std::sync::mpsc::channel();

    // Create default app state
    let model = Arc::new(Mutex::new(Model::new()));

    let cloned_model = Arc::clone(&model);

    std::thread::spawn(move || {
        start_tokio(&model, rx).unwrap();
    });

    loop {
        let mut unlocked_model = cloned_model.lock().unwrap();
        // Draw UI
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(2)
                .constraints(
                    [
                        Constraint::Length(1),
                        Constraint::Length(3),
                        Constraint::Length(1),
                        Constraint::Length(3),
                        Constraint::Min(1),
                    ]
                    .as_ref(),
                )
                .split(f.size());

            let (db_input_msg, db_style) = match unlocked_model.input_mode {
                InputMode::Normal|InputMode::EditingQuery => (
                    vec![
                        Span::raw("Press "),
                        Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(" to exit, "),
                        Span::styled("s", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(" to start editing. Status: "),
                        match unlocked_model.db_status {
                            DbStatus::Connected => Span::styled("Connected", Style::default().fg(Color::Green)),
                            DbStatus::Disconnected => Span::styled("Disconnected", Style::default().fg(Color::Red)),
                        },
                    ],
                    Style::default().add_modifier(Modifier::RAPID_BLINK),
                ),
                InputMode::EditingDb => (
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
            let mut db_help_text = Text::from(Spans::from(db_input_msg));
            db_help_text.patch_style(db_style);
            let help_message2 = Paragraph::new(db_help_text);
            f.render_widget(help_message2, chunks[0]);

            let db_input = Paragraph::new(unlocked_model.database_input.as_ref())
                .style(match unlocked_model.input_mode {
                    InputMode::Normal|InputMode::EditingQuery => Style::default(),
                    InputMode::EditingDb => Style::default().fg(Color::Yellow),
                })
                .block(Block::default().borders(Borders::ALL).title("Input"));
            f.render_widget(db_input, chunks[1]);

            let (input_msg, style) = match unlocked_model.input_mode {
                InputMode::Normal|InputMode::EditingDb => (
                    vec![
                        Span::raw("Press "),
                        Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(" to exit, "),
                        Span::styled("e", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(" to start editing. Status: "),
                        match unlocked_model.query_status {
                            QueryStatus::Complete => Span::styled("Complete", Style::default().fg(Color::Green)),
                            QueryStatus::Waiting => Span::styled("Waiting", Style::default().fg(Color::Yellow)),
                            QueryStatus::NotStarted => Span::styled("Ready", Style::default()),
                        }
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
            f.render_widget(help_message, chunks[2]);

            let input = Paragraph::new(unlocked_model.query_input.as_ref())
                .style(match unlocked_model.input_mode {
                    InputMode::Normal|InputMode::EditingDb => Style::default(),
                    InputMode::EditingQuery => Style::default().fg(Color::Yellow),
                })
                .block(Block::default().borders(Borders::ALL).title("Input"));
            f.render_widget(input, chunks[3]);
            match unlocked_model.input_mode {
                InputMode::Normal =>
                    // Hide the cursor. `Frame` does this by default, so we don't need to do anything here
                    {}

                InputMode::EditingQuery => {
                    // Make the cursor visible and ask tui-rs to put it at the specified coordinates after rendering
                    f.set_cursor(
                        // Put cursor past the end of the input text
                        chunks[3].x + unlocked_model.query_input.width() as u16 + 1,
                        // Move one line down, from the border to the input line
                        chunks[3].y + 1,
                    )
                }
                InputMode::EditingDb => {
                    // Make the cursor visible and ask tui-rs to put it at the specified coordinates after rendering
                    f.set_cursor(
                        // Put cursor past the end of the input text
                        chunks[1].x + unlocked_model.database_input.width() as u16 + 1,
                        // Move one line down, from the border to the input line
                        chunks[1].y + 1,
                    )
                }
            }


            // let messages: Vec<ListItem> = unlocked_model
            //     .messages
            //     .iter()
            //     .enumerate()
            //     .map(|(i, m)| {
            //         let content = vec![Spans::from(Span::raw(format!("{}: {}", i, m)))];
            //         ListItem::new(content)
            //     })
            //     .collect();
            // let messages =
            //     List::new(messages).block(Block::default().borders(Borders::ALL).title("Messages"));
            let messages = Paragraph::new(unlocked_model.messages.clone())
                .block(Block::default().borders(Borders::ALL).title("Results"))
                .wrap(Wrap { trim: false });
            f.render_widget(messages, chunks[4]);
        })?;

        // Handle input
        if let Event::Input(input) = events.next()? {
            match unlocked_model.input_mode {
                InputMode::Normal => match input {
                    Key::Char('e') => {
                        unlocked_model.input_mode = InputMode::EditingQuery;
                        events.disable_exit_key();
                    }
                    Key::Char('s') => {
                        unlocked_model.input_mode = InputMode::EditingDb;
                        events.disable_exit_key();
                    }
                    Key::Char('q') => {
                        break;
                    }
                    _ => {}
                },
                InputMode::EditingQuery => match input {
                    Key::Char('\n') => {
                        tx.send(Msg::Query(unlocked_model.query_input.drain(..).collect(), unlocked_model.database_input.clone()))?;
                        unlocked_model.input_mode = InputMode::Normal;
                        unlocked_model.query_status = QueryStatus::Waiting;
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
                InputMode::EditingDb => match input {
                    Key::Char('\n') => {
                        tx.send(Msg::Connect(unlocked_model.database_input.clone()))?;
                        unlocked_model.input_mode = InputMode::Normal;
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
    None
}


// Acts as Update Function for DB Activities
#[tokio::main]
async fn start_tokio(model: &Arc<Mutex<Model>>, io_rx: std::sync::mpsc::Receiver<Msg>) -> Result<(), sqlx::Error> {

    // let pool = sqlx::mysql::MySqlPoolOptions::new()
    //     .max_connections(5_u32)
    //     .connect("mysql://root:abc123@127.0.0.1:3307/lithia").await?;

    while let Ok(msg) = io_rx.recv() {
        match msg {
            Msg::Query(query, uri) => {
                while let Ok(mut model) = model.lock() {
                    model.messages = "Sending query".to_string();
                }
                let results: String = format!("{:?}", sqlx::query(&query).fetch_one(match model.db_connection {
                    Some(pool) => pool,
                    None => panic!("No pool found"),
                }).await?);

                while let Ok(mut model) = model.lock() {
                    model.messages = results.clone();
                    model.query_status = QueryStatus::Complete;
                }
            }
            Msg::Connect(uri) => {
                let pool = MySqlPoolOptions::new().connect(&uri).await?;
                while let Ok(mut model) = model.lock() {
                    model.db_connection = Some(pool);
                    model.db_status = DbStatus::Connected;
                }
            }
            Msg::None => {}
        }
    }
    Ok(())
}
