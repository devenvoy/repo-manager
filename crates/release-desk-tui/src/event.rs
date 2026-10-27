//! Event reader for crossterm keyboard, resize, and tick intervals.

use crossterm::event::{self, Event as CrosstermEvent, KeyEvent};
use std::io;
use std::time::{Duration, Instant};

/// Events processed by the TUI event loop.
#[derive(Clone, Debug)]
pub enum TuiEvent {
    Key(KeyEvent),
    Resize(u16, u16),
    Tick,
}

/// Helper for polling terminal inputs and generating periodic tick pulses.
pub struct EventHandler {
    tick_rate: Duration,
    last_tick: Instant,
}

impl EventHandler {
    /// Create a new event handler with the specified tick rate.
    pub fn new(tick_rate: Duration) -> Self {
        Self {
            tick_rate,
            last_tick: Instant::now(),
        }
    }

    /// Poll for the next event, respecting the tick rate timeout.
    pub fn next(&mut self) -> io::Result<TuiEvent> {
        let timeout = self
            .tick_rate
            .checked_sub(self.last_tick.elapsed())
            .unwrap_or(Duration::ZERO);

        if event::poll(timeout)? {
            match event::read()? {
                CrosstermEvent::Key(key) => Ok(TuiEvent::Key(key)),
                CrosstermEvent::Resize(w, h) => Ok(TuiEvent::Resize(w, h)),
                _ => Ok(TuiEvent::Tick),
            }
        } else {
            self.last_tick = Instant::now();
            Ok(TuiEvent::Tick)
        }
    }
}
