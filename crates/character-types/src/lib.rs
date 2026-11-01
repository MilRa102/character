//! Shared vocabulary of the Character companion (see `ARCHITECTURE.md`, §5 and §7).
//!
//! The crate is intentionally tiny and dependency-light so that every component
//! (UI, core, brain, ...) can use the same types without depending on each other.

use serde::{Deserialize, Serialize};

/// State of the character FSM.
///
/// Typical path: `hidden` → `peek` → `idle` → `listening` → `thinking` →
/// `speaking` / `acting` → `idle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    /// The drawer is folded away.
    #[default]
    Hidden,
    /// The character peeks out.
    Peek,
    Idle,
    Listening,
    Thinking,
    Speaking,
    Acting,
    /// Waiting for the user to confirm a level 2-3 action.
    Confirming,
    /// Critical event; wins over every other state.
    Alert,
}

/// Emotion overlay shown on top of the current [`State`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Emotion {
    Happy,
    Curious,
    Worried,
    Sleepy,
    Alert,
    Confused,
    Proud,
}

/// Privacy mode of the whole companion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// The companion is disabled.
    Off,
    /// System events only, the microphone is never opened.
    #[default]
    EventsOnly,
    /// Wake-word, STT and TTS are enabled.
    Voice,
}

impl Mode {
    /// Whether the microphone may be opened in this mode.
    pub const fn allows_microphone(self) -> bool {
        matches!(self, Self::Voice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_hidden_and_without_microphone() {
        assert_eq!(State::default(), State::Hidden);
        assert_eq!(Mode::default(), Mode::EventsOnly);
        assert!(!Mode::default().allows_microphone());
    }

    #[test]
    fn only_voice_mode_allows_microphone() {
        assert!(!Mode::Off.allows_microphone());
        assert!(!Mode::EventsOnly.allows_microphone());
        assert!(Mode::Voice.allows_microphone());
    }

    #[test]
    fn serializes_as_kebab_case_names_from_the_docs() {
        assert_eq!(
            serde_json::to_string(&Mode::EventsOnly).unwrap(),
            "\"events-only\""
        );
        assert_eq!(
            serde_json::to_string(&State::Confirming).unwrap(),
            "\"confirming\""
        );
        assert_eq!(
            serde_json::from_str::<Emotion>("\"worried\"").unwrap(),
            Emotion::Worried
        );
    }
}
