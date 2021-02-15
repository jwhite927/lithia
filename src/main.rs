#[allow(dead_code)]
mod util;

use crate::util::event::{Event, Events};
use std::{error::Error, io, sync::{Mutex, Arc, mpsc::Sender}};
use termion::{event::Key, input::MouseTerminal, raw::IntoRawMode, screen::AlternateScreen};
use tui::{
    backend::TermionBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Terminal,
};
use unicode_width::UnicodeWidthStr;
use mysql_async::prelude::*;

enum InputMode {
    Normal,
    Editing,
}

/// Model holds the state of the application
struct Model {
    /// Transmitting part of a channel
    tx: Sender<Msg>,
    /// Current value of the input box
    input: String,
    /// Current input mode
    input_mode: InputMode,
    /// History of recorded messages
    messages: Vec<String>,

}

impl Model {
    fn new(tx: Sender<Msg>) -> Model {
        Model {
            tx: tx,
            input: String::new(),
            input_mode: InputMode::Normal,
            messages: Vec::new(),
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
    let mut model = Arc::new(Mutex::new(Model::new(tx)));

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
                        Constraint::Min(1),
                    ]
                    .as_ref(),
                )
                .split(f.size());

            let (input_msg, style) = match unlocked_model.input_mode {
                InputMode::Normal => (
                    vec![
                        Span::raw("Press "),
                        Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(" to exit, "),
                        Span::styled("e", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(" to start editing."),
                    ],
                    Style::default().add_modifier(Modifier::RAPID_BLINK),
                ),
                InputMode::Editing => (
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
            f.render_widget(help_message, chunks[0]);

            let input = Paragraph::new(unlocked_model.input.as_ref())
                .style(match unlocked_model.input_mode {
                    InputMode::Normal => Style::default(),
                    InputMode::Editing => Style::default().fg(Color::Yellow),
                })
                .block(Block::default().borders(Borders::ALL).title("Input"));
            f.render_widget(input, chunks[1]);
            match unlocked_model.input_mode {
                InputMode::Normal =>
                    // Hide the cursor. `Frame` does this by default, so we don't need to do anything here
                    {}

                InputMode::Editing => {
                    // Make the cursor visible and ask tui-rs to put it at the specified coordinates after rendering
                    f.set_cursor(
                        // Put cursor past the end of the input text
                        chunks[1].x + unlocked_model.input.width() as u16 + 1,
                        // Move one line down, from the border to the input line
                        chunks[1].y + 1,
                    )
                }
            }

            // let messages: Vec<ListItem> = results
            //     .iter()
            //     .map(|i| {
            //         let content = vec![Spans::from(Span::raw(format!("{}", i)))];
            //         ListItem::new(content)
            //     })
            //     .collect();
            let messages: Vec<ListItem> = unlocked_model
                .messages
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    let content = vec![Spans::from(Span::raw(format!("{}: {}", i, m)))];
                    ListItem::new(content)
                })
                .collect();
            let messages =
                List::new(messages).block(Block::default().borders(Borders::ALL).title("Messages"));
            f.render_widget(messages, chunks[2]);
        })?;

        // Handle input
        if let Event::Input(input) = events.next()? {
            match unlocked_model.input_mode {
                InputMode::Normal => match input {
                    Key::Char('e') => {
                        unlocked_model.input_mode = InputMode::Editing;
                        events.disable_exit_key();
                    }
                    Key::Char('w') => {
                        unlocked_model.tx.send(Msg::GetQuery).unwrap();
                    }
                    Key::Char('q') => {
                        break;
                    }
                    _ => {}
                },
                InputMode::Editing => match input {
                    Key::Char('\n') => {
                        // unlocked_model.messages.push(unlocked_model.input.drain(..).collect());
                    }
                    Key::Char(_c) => {
                        // unlocked_model.input.push(c);
                    }
                    Key::Backspace => {
                        // unlocked_model.input.pop();
                    }
                    Key::Esc => {
                        // unlocked_model.input_mode = InputMode::Normal;
                        events.enable_exit_key();
                    }
                    _ => {}
                },
            }
        }
    }
    Ok(())
}

pub enum Msg {
    GetQuery,
    None
}


// Acts as Update
#[tokio::main]
async fn start_tokio(model: &Arc<Mutex<Model>>, io_rx: std::sync::mpsc::Receiver<Msg>) -> Result<(), mysql_async::Error> {
    let database_opts = mysql_async::OptsBuilder::default()
        .ip_or_hostname("localhost")
        .db_name(Some("mig_dest"))
        .tcp_port(3307)
        .user(Some("root"))
        .pass(Some("abc123"));

    let pool = mysql_async::Pool::new(database_opts);
    let mut conn = pool.get_conn().await?;

    while let Ok(msg) = io_rx.recv() {
        match msg {
            Msg::GetQuery => {
                // let results = conn.query("SELECT * FROM tbl_asset LIMIT 3").await;

                while let Ok(mut model) = model.lock() {
                    model.messages = vec!["Ok".to_string()];
                }
            }
            Msg::None => {}
        }
    }
    Ok(())
}
