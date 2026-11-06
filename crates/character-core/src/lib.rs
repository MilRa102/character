//! Central daemon: owns the character state and orchestrates the other components.
//!
//! Event flow (`ARCHITECTURE.md`, §4): watch → event bus → policy → FSM/UI, with
//! brain, voice and actions involved only when an event needs reasoning.
//! Network connectivity transitions are handled locally; daemon lifecycle and
//! interprocess transport remain future work.

use std::future::Future;

use character_types::{Mode, NetworkStatus, State};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkReaction {
    Disconnected,
    Connected,
}

/// The core daemon.
#[derive(Debug)]
pub struct Core {
    mode: Mode,
    state: State,
    network_status: Option<NetworkStatus>,
}

impl Core {
    /// Creates a core in the given privacy mode; the character starts [`State::Hidden`].
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            state: State::default(),
            network_status: None,
        }
    }

    pub const fn mode(&self) -> Mode {
        self.mode
    }

    pub const fn state(&self) -> State {
        self.state
    }

    /// Updates the connectivity baseline and reacts only to actual transitions.
    pub fn network_status_changed(&mut self, status: NetworkStatus) -> Option<NetworkReaction> {
        let previous = self.network_status.replace(status)?;
        if self.mode == Mode::Off {
            return None;
        }

        match (previous, status) {
            (NetworkStatus::Connected, NetworkStatus::Disconnected) => {
                Some(NetworkReaction::Disconnected)
            }
            (NetworkStatus::Disconnected, NetworkStatus::Connected) => {
                Some(NetworkReaction::Connected)
            }
            _ => None,
        }
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

    #[test]
    fn network_policy_seeds_without_reacting_and_ignores_duplicate_status() {
        let mut core = Core::new(Mode::EventsOnly);

        assert_eq!(core.network_status_changed(NetworkStatus::Connected), None);
        assert_eq!(core.network_status_changed(NetworkStatus::Connected), None);
        assert_eq!(
            core.network_status_changed(NetworkStatus::Disconnected),
            Some(NetworkReaction::Disconnected)
        );
        assert_eq!(
            core.network_status_changed(NetworkStatus::Disconnected),
            None
        );
        assert_eq!(
            core.network_status_changed(NetworkStatus::Connected),
            Some(NetworkReaction::Connected)
        );
    }

    #[test]
    fn disabled_mode_never_reacts_to_network_changes() {
        let mut core = Core::new(Mode::Off);

        assert_eq!(core.network_status_changed(NetworkStatus::Connected), None);
        assert_eq!(
            core.network_status_changed(NetworkStatus::Disconnected),
            None
        );
    }
}
