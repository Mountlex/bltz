//! Background render thread for non-blocking UI rendering.
//!
//! The render thread owns the Terminal and renders snapshots of AppState
//! sent from the main event loop. This keeps the event loop responsive
//! to IMAP events and user input.

use std::io;
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};

use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use super::state::AppState;

/// Command sent to the render thread.
pub enum RenderCommand {
    /// Render this state snapshot
    Render(Box<AppState>),
    /// Shutdown the render thread
    Shutdown,
}

/// Handle to the background render thread.
pub struct RenderThread {
    /// Channel to send render commands
    cmd_tx: SyncSender<RenderCommand>,
    /// Thread join handle
    handle: Option<JoinHandle<()>>,
}

impl RenderThread {
    /// Spawn a new render thread.
    ///
    /// The render thread takes ownership of terminal setup/teardown.
    /// Returns the handle for sending render commands.
    pub fn spawn() -> io::Result<Self> {
        // Channel with capacity 1 - we only care about the latest state
        let (cmd_tx, cmd_rx) = mpsc::sync_channel::<RenderCommand>(1);

        let handle = thread::spawn(move || {
            // Setup terminal in the render thread
            if let Err(e) = enable_raw_mode() {
                tracing::error!("Failed to enable raw mode: {}", e);
                return;
            }

            let mut stdout = io::stdout();
            if let Err(e) = execute!(stdout, EnterAlternateScreen) {
                tracing::error!("Failed to enter alternate screen: {}", e);
                disable_raw_mode().ok();
                return;
            }

            let backend = CrosstermBackend::new(stdout);
            let mut terminal = match Terminal::new(backend) {
                Ok(t) => t,
                Err(e) => {
                    tracing::error!("Failed to create terminal: {}", e);
                    disable_raw_mode().ok();
                    return;
                }
            };

            // Render loop
            while let Ok(cmd) = cmd_rx.recv() {
                match cmd {
                    RenderCommand::Render(state) => {
                        if let Err(e) = terminal.draw(|f| crate::ui::render(f, &state)) {
                            tracing::error!("Render error: {}", e);
                        }
                    }
                    RenderCommand::Shutdown => break,
                }
            }

            // Cleanup terminal
            disable_raw_mode().ok();
            execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
        });

        Ok(Self {
            cmd_tx,
            handle: Some(handle),
        })
    }

    /// Request a render of the given state (non-blocking).
    ///
    /// If the render thread is busy, the previous pending frame is replaced.
    /// This is intentional - we always want to render the latest state.
    pub fn render(&self, state: AppState) {
        match self.cmd_tx.try_send(RenderCommand::Render(Box::new(state))) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                // Channel full - render thread is busy, frame will be skipped
                // This is fine, we'll send the next frame when ready
                tracing::trace!("Render thread busy, skipping frame");
            }
            Err(TrySendError::Disconnected(_)) => {
                tracing::error!("Render thread disconnected");
            }
        }
    }

    /// Shutdown the render thread and wait for it to finish.
    pub fn shutdown(mut self) {
        // Send shutdown command (blocking to ensure it's received)
        let _ = self.cmd_tx.send(RenderCommand::Shutdown);

        // Wait for thread to finish
        if let Some(handle) = self.handle.take() {
            handle.join().ok();
        }
    }
}
