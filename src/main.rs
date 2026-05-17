mod app;
mod config;
mod codeword;
mod project;
mod session;
mod llm;
mod skill;
mod ui;

use std::io;
use std::time::Duration;

use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;

use app::{App, AppEvent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = config::Config::load()?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (tx, mut rx) = mpsc::channel::<AppEvent>(256);

    // Terminal events live in a blocking thread — crossterm::event::poll is blocking
    let tx_keys = tx.clone();
    tokio::task::spawn_blocking(move || loop {
        if event::poll(Duration::from_millis(50)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                if tx_keys.blocking_send(AppEvent::Key(key)).is_err() {
                    break;
                }
            }
        }
    });

    let mut app = App::new(config, tx)?;

    loop {
        terminal.draw(|f| ui::render(f, &app))?;

        // Drain all pending events without blocking the render loop
        loop {
            match rx.try_recv() {
                Ok(event) => app.handle_event(event).await,
                Err(_) => break,
            }
        }

        if app.should_quit {
            break;
        }

        tokio::time::sleep(Duration::from_millis(16)).await; // ~60 fps
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
