//! RAII terminal lifecycle guard for robust raw mode and alternate screen management.

use crossterm::{
    cursor::{Hide, Show},
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io::{self, Stdout, stdout};

/// RAII guard that manages the terminal state and guarantees restoration on exit or panic.
pub struct TerminalGuard {
    active: bool,
}

impl TerminalGuard {
    /// Initialize raw mode and alternate screen, returning a configured Ratatui terminal and the RAII guard.
    pub fn init() -> io::Result<(Terminal<CrosstermBackend<Stdout>>, Self)> {
        enable_raw_mode()?;
        let mut out = stdout();
        execute!(out, EnterAlternateScreen, EnableMouseCapture, Hide)?;
        let backend = CrosstermBackend::new(out);
        let terminal = Terminal::new(backend)?;

        Ok((terminal, Self { active: true }))
    }

    /// Explicitly restore terminal state before dropping.
    pub fn restore(&mut self) -> io::Result<()> {
        if self.active {
            disable_raw_mode()?;
            execute!(stdout(), LeaveAlternateScreen, DisableMouseCapture, Show)?;
            self.active = false;
        }
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
