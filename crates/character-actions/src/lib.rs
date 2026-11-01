//! Allowlist actions (see `ARCHITECTURE.md`, §6).
//!
//! The LLM never executes arbitrary commands: it can only request an action by
//! `id` with arguments that match the JSON schema of its [`ActionManifest`].
//! Arbitrary shell, `sudo` and reading files outside the allowlist are forbidden
//! and therefore have no [`RiskLevel`] at all.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// How dangerous an action is; decides whether confirmation is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RiskLevel {
    /// Level 0: reading (time, battery, network status, current track).
    Read,
    /// Level 1: safe changes (volume, play/pause, DND, open an app from the list).
    Safe,
    /// Level 2: noticeable changes (close a window, toggle Wi-Fi/BT, create a note).
    Notable,
    /// Level 3: dangerous changes (power off, deletion, installing packages).
    Dangerous,
}

impl RiskLevel {
    /// Levels 2 and 3 need an explicit confirmation from the user.
    pub const fn requires_confirmation(self) -> bool {
        matches!(self, Self::Notable | Self::Dangerous)
    }

    /// Level 3 additionally goes through polkit.
    pub const fn requires_polkit(self) -> bool {
        matches!(self, Self::Dangerous)
    }
}

/// Declarative description of a single allowed action (`actions.d/*.toml`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionManifest {
    /// Stable identifier the LLM refers to, e.g. `media.pause`.
    pub id: String,
    pub description: String,
    pub risk: RiskLevel,
    /// JSON Schema of the arguments; no arguments by default.
    #[serde(default = "no_arguments")]
    pub args_schema: Value,
}

impl ActionManifest {
    pub const fn requires_confirmation(&self) -> bool {
        self.risk.requires_confirmation()
    }
}

fn no_arguments() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_policy_follows_the_risk_table() {
        let expected = [
            (RiskLevel::Read, false, false),
            (RiskLevel::Safe, false, false),
            (RiskLevel::Notable, true, false),
            (RiskLevel::Dangerous, true, true),
        ];
        for (level, confirm, polkit) in expected {
            assert_eq!(level.requires_confirmation(), confirm, "{level:?}");
            assert_eq!(level.requires_polkit(), polkit, "{level:?}");
        }
    }

    #[test]
    fn risk_levels_are_ordered_by_danger() {
        assert!(RiskLevel::Read < RiskLevel::Safe);
        assert!(RiskLevel::Safe < RiskLevel::Notable);
        assert!(RiskLevel::Notable < RiskLevel::Dangerous);
    }

    #[test]
    fn manifest_without_arguments_forbids_extra_properties() {
        let manifest: ActionManifest = serde_json::from_value(json!({
            "id": "media.pause",
            "description": "Pause the current media player",
            "risk": "safe",
        }))
        .unwrap();

        assert_eq!(manifest.args_schema["additionalProperties"], json!(false));
        assert!(!manifest.requires_confirmation());
    }

    #[test]
    fn unknown_risk_level_is_rejected() {
        let result = serde_json::from_value::<ActionManifest>(json!({
            "id": "shell.run",
            "description": "Run an arbitrary command",
            "risk": "forbidden",
        }));

        assert!(result.is_err());
    }
}
