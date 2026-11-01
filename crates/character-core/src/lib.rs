//! Central daemon: owns the character state and orchestrates the other components.
//!
//! Event flow (`ARCHITECTURE.md`, §4): watch → event bus → policy → FSM/UI, with
//! brain, voice and actions involved only when an event needs reasoning.
//! For now only the lifecycle skeleton exists; the pieces are wired in as they land.

use std::future::Future;

use character_types::{Mode, State};

/// The core daemon.
#[derive(Debug)]
pub struct Core {
    mode: Mode,
    state: State,
}

impl Core {
    /// Creates a core in the given privacy mode; the character starts [`State::Hidden`].
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            state: State::default(),
        }
    }

    pub const fn mode(&self) -> Mode {
        self.mode
    }

    pub const fn state(&self) -> State {
        self.state
    }

    /// Runs the core until `shutdown` completes.
    pub async fn run(self, shutdown: impl Future<Output = ()>) {
        tracing::info!(mode = ?self.mode, state = ?self.state, "core started");
        shutdown.await;
        tracing::info!("core stopped");
    }
}

#[cfg(test)]
mod tests {
    use std::future;

    use super::*;

    #[test]
    fn starts_hidden() {
        let core = Core::new(Mode::EventsOnly);

        assert_eq!(core.state(), State::Hidden);
        assert_eq!(core.mode(), Mode::EventsOnly);
    }

    #[tokio::test]
    async fn run_returns_once_shutdown_completes() {
        Core::new(Mode::Off).run(future::ready(())).await;
    }
}
