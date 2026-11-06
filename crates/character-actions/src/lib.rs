//! Allowlist actions (see `ARCHITECTURE.md`, §6).
//!
//! The LLM never executes arbitrary commands: it can only request a registered
//! action by `id` with arguments matching that action's JSON Schema. Actions
//! above the safe risk level require a separate confirmation before dispatch.
//! Arbitrary shell, `sudo` and reading files outside the allowlist are forbidden.

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use jsonschema::Validator;
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

    /// Level 3 additionally goes through polkit when real actions are added.
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

/// A validated, deterministic collection of actions available for execution.
#[derive(Debug, Default)]
pub struct ActionRegistry {
    actions: BTreeMap<String, RegisteredAction>,
}

#[derive(Debug)]
struct RegisteredAction {
    manifest: ActionManifest,
    args_validator: Validator,
}

impl ActionRegistry {
    /// Validates manifests and rejects duplicate or malformed action IDs/schemas.
    pub fn new(manifests: impl IntoIterator<Item = ActionManifest>) -> Result<Self, ActionError> {
        let mut actions = BTreeMap::new();
        for manifest in manifests {
            if manifest.id.is_empty() || manifest.id.chars().any(char::is_control) {
                return Err(ActionError::InvalidActionId(manifest.id));
            }
            if manifest.description.trim().is_empty() {
                return Err(ActionError::EmptyDescription(manifest.id));
            }
            if actions.contains_key(&manifest.id) {
                return Err(ActionError::DuplicateActionId(manifest.id));
            }

            let args_validator =
                jsonschema::validator_for(&manifest.args_schema).map_err(|error| {
                    ActionError::InvalidSchema {
                        action_id: manifest.id.clone(),
                        message: error.to_string(),
                    }
                })?;
            actions.insert(
                manifest.id.clone(),
                RegisteredAction {
                    manifest,
                    args_validator,
                },
            );
        }
        Ok(Self { actions })
    }

    /// Loads every `*.toml` manifest from a directory in sorted path order.
    pub fn from_dir(directory: impl AsRef<Path>) -> Result<Self, ActionError> {
        let directory = directory.as_ref();
        let entries = fs::read_dir(directory).map_err(|source| ActionError::ReadDirectory {
            path: directory.to_owned(),
            source,
        })?;
        let mut paths = entries
            .map(|entry| {
                entry
                    .map(|entry| entry.path())
                    .map_err(|source| ActionError::ReadDirectoryEntry {
                        path: directory.to_owned(),
                        source,
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        paths.retain(|path| path.extension().is_some_and(|ext| ext == "toml"));
        paths.sort();

        let mut manifests = Vec::with_capacity(paths.len());
        for path in paths {
            let contents =
                fs::read_to_string(&path).map_err(|source| ActionError::ReadManifest {
                    path: path.clone(),
                    source,
                })?;
            let manifest =
                toml::from_str(&contents).map_err(|source| ActionError::ParseManifest {
                    path: path.clone(),
                    source,
                })?;
            manifests.push(manifest);
        }
        Self::new(manifests)
    }

    pub fn get(&self, action_id: &str) -> Option<&ActionManifest> {
        self.actions.get(action_id).map(|action| &action.manifest)
    }

    pub fn manifests(&self) -> impl Iterator<Item = &ActionManifest> {
        self.actions.values().map(|action| &action.manifest)
    }

    fn validate_args(&self, action_id: &str, args: &Value) -> Result<(), Vec<String>> {
        let action = &self.actions[action_id];
        let errors = action
            .args_validator
            .iter_errors(args)
            .map(|error| error.to_string())
            .collect::<Vec<_>>();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// One requested action and its JSON arguments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionCall {
    pub action_id: String,
    pub args: Value,
}

/// An action implementation. Production handlers must dispatch only registered IDs.
pub trait ActionExecutor {
    fn execute(&mut self, call: &ActionCall) -> Result<Value, String>;
}

/// Test/demo executor that records calls without changing system state.
#[derive(Debug, Default)]
pub struct MockActionExecutor {
    calls: Vec<ActionCall>,
}

impl MockActionExecutor {
    pub fn calls(&self) -> &[ActionCall] {
        &self.calls
    }
}

impl ActionExecutor for MockActionExecutor {
    fn execute(&mut self, call: &ActionCall) -> Result<Value, String> {
        self.calls.push(call.clone());
        Ok(json!({ "mock": true }))
    }
}

/// Opaque identifier for one pending user confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct ConfirmationId(uuid::Uuid);

impl std::fmt::Display for ConfirmationId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// The information a trusted UI needs to display a confirmation prompt.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationPrompt {
    pub id: ConfirmationId,
    pub action_id: String,
    pub description: String,
    pub args: Value,
}

/// Result of submitting an action request.
#[derive(Debug, Clone, PartialEq)]
pub enum RequestOutcome {
    Executed(Value),
    ConfirmationRequired(ConfirmationPrompt),
    Cancelled,
    Rejected(RejectReason),
}

/// Why an action request was not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectReason {
    UnknownAction,
    InvalidArguments(Vec<String>),
    KillSwitchActive,
    UnknownConfirmation,
}

/// Audit event for an action request or kill-switch state transition.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum JournalEvent {
    RequestRejected { reason: String },
    ConfirmationRequested { confirmation_id: ConfirmationId },
    ConfirmationAccepted { confirmation_id: ConfirmationId },
    ConfirmationCancelled { confirmation_id: ConfirmationId },
    ExecutionStarted,
    ExecutionSucceeded { result: Value },
    ExecutionFailed { error: String },
    KillSwitchActivated,
    KillSwitchReset,
}

/// One append-only audit record.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JournalEntry {
    pub sequence: u64,
    pub timestamp_unix_ms: u64,
    pub action_id: Option<String>,
    pub args: Option<Value>,
    pub event: JournalEvent,
}

/// Sink for append-only action audit records.
pub trait AuditJournal {
    fn append(&mut self, entry: &JournalEntry) -> io::Result<()>;
}

/// In-memory journal, primarily useful for tests and previews.
#[derive(Debug, Default)]
pub struct MemoryJournal {
    entries: Vec<JournalEntry>,
}

impl MemoryJournal {
    pub fn entries(&self) -> &[JournalEntry] {
        &self.entries
    }
}

impl AuditJournal for MemoryJournal {
    fn append(&mut self, entry: &JournalEntry) -> io::Result<()> {
        self.entries.push(entry.clone());
        Ok(())
    }
}

/// JSON Lines journal that appends and flushes each audit record to disk.
#[derive(Debug)]
pub struct JsonlJournal {
    file: File,
}

impl JsonlJournal {
    /// Opens a journal for append; the parent directory must already exist.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        options.mode(0o600);
        let file = options.open(path)?;
        #[cfg(unix)]
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        Ok(Self { file })
    }
}

impl AuditJournal for JsonlJournal {
    fn append(&mut self, entry: &JournalEntry) -> io::Result<()> {
        serde_json::to_writer(&mut self.file, entry).map_err(io::Error::other)?;
        self.file.write_all(b"\n")?;
        self.file.flush()
    }
}

/// Shared, latched switch that prevents new actions and confirmations.
#[derive(Debug, Clone, Default)]
pub struct KillSwitch {
    active: Arc<AtomicBool>,
}

impl KillSwitch {
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    /// Activates the latch immediately. Use the engine method to also journal and cancel prompts.
    pub fn activate(&self) {
        self.active.store(true, Ordering::SeqCst);
    }
}

struct PendingAction {
    call: ActionCall,
}

/// Validates, gates, journals and dispatches allowlisted actions.
pub struct ActionEngine<E, J> {
    registry: ActionRegistry,
    executor: E,
    journal: J,
    kill_switch: KillSwitch,
    pending: HashMap<ConfirmationId, PendingAction>,
    next_sequence: u64,
}

impl<E: ActionExecutor, J: AuditJournal> ActionEngine<E, J> {
    pub fn new(registry: ActionRegistry, executor: E, journal: J) -> Self {
        Self {
            registry,
            executor,
            journal,
            kill_switch: KillSwitch::default(),
            pending: HashMap::new(),
            next_sequence: 1,
        }
    }

    pub fn kill_switch(&self) -> KillSwitch {
        self.kill_switch.clone()
    }

    pub fn executor(&self) -> &E {
        &self.executor
    }

    pub fn journal(&self) -> &J {
        &self.journal
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Submits an action. Risk levels 2 and 3 are queued until explicitly confirmed.
    pub fn request(&mut self, action_id: &str, args: Value) -> Result<RequestOutcome, ActionError> {
        if self.kill_switch.is_active() {
            self.record(
                None,
                None,
                JournalEvent::RequestRejected {
                    reason: "kill switch active".into(),
                },
            )?;
            return Ok(RequestOutcome::Rejected(RejectReason::KillSwitchActive));
        }

        let Some(action) = self.registry.actions.get(action_id) else {
            self.record(
                Some(action_id.to_owned()),
                Some(args),
                JournalEvent::RequestRejected {
                    reason: "unknown action".into(),
                },
            )?;
            return Ok(RequestOutcome::Rejected(RejectReason::UnknownAction));
        };
        let manifest = &action.manifest;
        if let Err(errors) = self.registry.validate_args(action_id, &args) {
            self.record(
                Some(action_id.to_owned()),
                Some(args),
                JournalEvent::RequestRejected {
                    reason: errors.join("; "),
                },
            )?;
            return Ok(RequestOutcome::Rejected(RejectReason::InvalidArguments(
                errors,
            )));
        }

        let call = ActionCall {
            action_id: action_id.to_owned(),
            args,
        };
        if manifest.requires_confirmation() {
            let id = ConfirmationId(uuid::Uuid::new_v4());
            let prompt = ConfirmationPrompt {
                id,
                action_id: call.action_id.clone(),
                description: manifest.description.clone(),
                args: call.args.clone(),
            };
            self.record(
                Some(call.action_id.clone()),
                Some(call.args.clone()),
                JournalEvent::ConfirmationRequested {
                    confirmation_id: id,
                },
            )?;
            self.pending.insert(id, PendingAction { call });
            Ok(RequestOutcome::ConfirmationRequired(prompt))
        } else {
            self.execute(call)
        }
    }

    /// Confirms exactly the immutable action/arguments shown by the trusted UI.
    pub fn confirm(
        &mut self,
        confirmation_id: ConfirmationId,
    ) -> Result<RequestOutcome, ActionError> {
        if self.kill_switch.is_active() {
            self.cancel_pending(confirmation_id)?;
            return Ok(RequestOutcome::Rejected(RejectReason::KillSwitchActive));
        }
        let Some(pending) = self.pending.remove(&confirmation_id) else {
            self.record(
                None,
                None,
                JournalEvent::RequestRejected {
                    reason: "unknown confirmation".into(),
                },
            )?;
            return Ok(RequestOutcome::Rejected(RejectReason::UnknownConfirmation));
        };
        self.record(
            Some(pending.call.action_id.clone()),
            Some(pending.call.args.clone()),
            JournalEvent::ConfirmationAccepted { confirmation_id },
        )?;
        self.execute(pending.call)
    }

    /// Cancels a pending confirmation without dispatching the action.
    pub fn cancel(
        &mut self,
        confirmation_id: ConfirmationId,
    ) -> Result<RequestOutcome, ActionError> {
        if self.pending.contains_key(&confirmation_id) {
            self.cancel_pending(confirmation_id)?;
            Ok(RequestOutcome::Cancelled)
        } else {
            self.record(
                None,
                None,
                JournalEvent::RequestRejected {
                    reason: "unknown confirmation".into(),
                },
            )?;
            Ok(RequestOutcome::Rejected(RejectReason::UnknownConfirmation))
        }
    }

    /// Activates the switch, cancels all prompts and records each cancellation.
    pub fn activate_kill_switch(&mut self) -> Result<(), ActionError> {
        self.kill_switch.activate();
        self.record(None, None, JournalEvent::KillSwitchActivated)?;
        let ids = self.pending.keys().copied().collect::<Vec<_>>();
        for id in ids {
            self.cancel_pending(id)?;
        }
        Ok(())
    }

    /// Explicitly re-enables actions after a kill-switch activation.
    pub fn reset_kill_switch(&mut self) -> Result<(), ActionError> {
        self.record(None, None, JournalEvent::KillSwitchReset)?;
        self.kill_switch.active.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn cancel_pending(&mut self, confirmation_id: ConfirmationId) -> Result<(), ActionError> {
        if let Some(pending) = self.pending.remove(&confirmation_id) {
            self.record(
                Some(pending.call.action_id),
                Some(pending.call.args),
                JournalEvent::ConfirmationCancelled { confirmation_id },
            )?;
        }
        Ok(())
    }

    fn execute(&mut self, call: ActionCall) -> Result<RequestOutcome, ActionError> {
        if self.kill_switch.is_active() {
            self.record(
                Some(call.action_id),
                Some(call.args),
                JournalEvent::RequestRejected {
                    reason: "kill switch active".into(),
                },
            )?;
            return Ok(RequestOutcome::Rejected(RejectReason::KillSwitchActive));
        }
        self.record(
            Some(call.action_id.clone()),
            Some(call.args.clone()),
            JournalEvent::ExecutionStarted,
        )?;
        match self.executor.execute(&call) {
            Ok(result) => {
                self.record(
                    Some(call.action_id),
                    Some(call.args),
                    JournalEvent::ExecutionSucceeded {
                        result: result.clone(),
                    },
                )?;
                Ok(RequestOutcome::Executed(result))
            }
            Err(error) => {
                self.record(
                    Some(call.action_id),
                    Some(call.args),
                    JournalEvent::ExecutionFailed {
                        error: error.clone(),
                    },
                )?;
                Err(ActionError::ExecutionFailed(error))
            }
        }
    }

    fn record(
        &mut self,
        action_id: Option<String>,
        args: Option<Value>,
        event: JournalEvent,
    ) -> Result<(), ActionError> {
        let timestamp_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u64::MAX.into()) as u64;
        let entry = JournalEntry {
            sequence: self.next_sequence,
            timestamp_unix_ms,
            action_id,
            args,
            event,
        };
        self.journal.append(&entry).map_err(ActionError::Journal)?;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(ActionError::IdentifierExhausted)?;
        Ok(())
    }
}

/// Errors loading configuration, writing the audit journal or dispatching an action.
#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    #[error("invalid action ID `{0}`")]
    InvalidActionId(String),
    #[error("action `{0}` has an empty description")]
    EmptyDescription(String),
    #[error("duplicate action ID `{0}`")]
    DuplicateActionId(String),
    #[error("invalid argument schema for `{action_id}`: {message}")]
    InvalidSchema { action_id: String, message: String },
    #[error("could not read actions directory `{path}`: {source}")]
    ReadDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not read an entry in actions directory `{path}`: {source}")]
    ReadDirectoryEntry {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not read action manifest `{path}`: {source}")]
    ReadManifest {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not parse action manifest `{path}`: {source}")]
    ParseManifest {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("could not append to action journal: {0}")]
    Journal(#[source] io::Error),
    #[error("action execution failed: {0}")]
    ExecutionFailed(String),
    #[error("journal sequence identifier exhausted")]
    IdentifierExhausted,
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    fn manifest(id: &str, risk: RiskLevel) -> ActionManifest {
        ActionManifest {
            id: id.into(),
            description: format!("Test action {id}"),
            risk,
            args_schema: json!({
                "type": "object",
                "properties": { "value": { "type": "string" } },
                "required": ["value"],
                "additionalProperties": false
            }),
        }
    }

    fn engine() -> ActionEngine<MockActionExecutor, MemoryJournal> {
        let registry = ActionRegistry::new([
            manifest("read.test", RiskLevel::Read),
            manifest("safe.test", RiskLevel::Safe),
            manifest("danger.test", RiskLevel::Dangerous),
        ])
        .unwrap();
        ActionEngine::new(
            registry,
            MockActionExecutor::default(),
            MemoryJournal::default(),
        )
    }

    fn temporary_path(extension: &str) -> PathBuf {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "character-actions-{}-{id}.{extension}",
            std::process::id()
        ))
    }

    #[test]
    fn risk_levels_have_expected_confirmation_and_polkit_policy() {
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
    fn safe_action_runs_after_schema_validation_and_is_journaled() {
        let mut engine = engine();
        let result = engine.request("safe.test", json!({"value": "ok"})).unwrap();

        assert_eq!(result, RequestOutcome::Executed(json!({"mock": true})));
        assert_eq!(engine.executor().calls().len(), 1);
        assert!(matches!(
            engine.journal().entries().last().unwrap().event,
            JournalEvent::ExecutionSucceeded { .. }
        ));
    }

    #[test]
    fn invalid_arguments_and_unknown_actions_are_rejected_without_execution() {
        let mut engine = engine();
        assert!(matches!(
            engine.request("safe.test", json!({"value": 4})).unwrap(),
            RequestOutcome::Rejected(RejectReason::InvalidArguments(_))
        ));
        assert_eq!(
            engine.request("missing.action", json!({})).unwrap(),
            RequestOutcome::Rejected(RejectReason::UnknownAction)
        );
        assert!(engine.executor().calls().is_empty());
        assert_eq!(engine.journal().entries().len(), 2);
    }

    #[test]
    fn dangerous_action_waits_for_confirmation_and_uses_unchanged_arguments() {
        let mut engine = engine();
        let RequestOutcome::ConfirmationRequired(prompt) = engine
            .request("danger.test", json!({"value": "original"}))
            .unwrap()
        else {
            panic!("dangerous action must require confirmation");
        };
        assert!(engine.executor().calls().is_empty());
        assert_eq!(engine.pending_count(), 1);
        assert_eq!(
            engine.confirm(prompt.id).unwrap(),
            RequestOutcome::Executed(json!({"mock": true}))
        );
        assert_eq!(
            engine.executor().calls()[0].args,
            json!({"value": "original"})
        );
        assert_eq!(engine.pending_count(), 0);
    }

    #[test]
    fn pending_action_can_be_cancelled_without_dispatch() {
        let mut engine = engine();
        let RequestOutcome::ConfirmationRequired(prompt) = engine
            .request("danger.test", json!({"value": "cancel"}))
            .unwrap()
        else {
            panic!("dangerous action must require confirmation");
        };
        assert_eq!(engine.cancel(prompt.id).unwrap(), RequestOutcome::Cancelled);
        assert!(engine.executor().calls().is_empty());
        assert_eq!(engine.pending_count(), 0);
    }

    #[test]
    fn cancellation_and_kill_switch_prevent_pending_execution() {
        let mut engine = engine();
        let RequestOutcome::ConfirmationRequired(prompt) = engine
            .request("danger.test", json!({"value": "no"}))
            .unwrap()
        else {
            panic!("dangerous action must require confirmation");
        };
        engine.activate_kill_switch().unwrap();
        assert_eq!(engine.pending_count(), 0);
        assert_eq!(
            engine.confirm(prompt.id).unwrap(),
            RequestOutcome::Rejected(RejectReason::KillSwitchActive)
        );
        assert!(engine.executor().calls().is_empty());
        assert!(
            engine
                .journal()
                .entries()
                .iter()
                .any(|entry| matches!(entry.event, JournalEvent::ConfirmationCancelled { .. }))
        );
        assert_eq!(
            engine
                .request("safe.test", json!({"value": "blocked"}))
                .unwrap(),
            RequestOutcome::Rejected(RejectReason::KillSwitchActive)
        );
        engine.reset_kill_switch().unwrap();
        assert!(matches!(
            engine
                .request("safe.test", json!({"value": "allowed"}))
                .unwrap(),
            RequestOutcome::Executed(_)
        ));
    }

    #[test]
    fn manifest_directory_loads_toml_and_ignores_other_files() {
        let directory = temporary_path("dir");
        fs::create_dir(&directory).unwrap();
        fs::write(
            directory.join("sample.toml"),
            r#"
id = "test.sample"
description = "Sample action"
risk = "safe"
"#,
        )
        .unwrap();
        fs::write(directory.join("README"), "ignored").unwrap();

        let registry = ActionRegistry::from_dir(&directory).unwrap();
        assert_eq!(registry.get("test.sample").unwrap().risk, RiskLevel::Safe);

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn invalid_schema_and_duplicate_ids_are_rejected() {
        let mut invalid = manifest("broken.schema", RiskLevel::Safe);
        invalid.args_schema = json!({"type": "not-a-json-schema-type"});
        assert!(matches!(
            ActionRegistry::new([invalid]),
            Err(ActionError::InvalidSchema { .. })
        ));
        assert!(matches!(
            ActionRegistry::new([
                manifest("duplicate", RiskLevel::Read),
                manifest("duplicate", RiskLevel::Safe)
            ]),
            Err(ActionError::DuplicateActionId(_))
        ));
    }

    #[test]
    fn jsonl_journal_appends_parseable_records() {
        let path = temporary_path("jsonl");
        {
            let mut journal = JsonlJournal::open(&path).unwrap();
            let entry = JournalEntry {
                sequence: 1,
                timestamp_unix_ms: 10,
                action_id: Some("safe.test".into()),
                args: Some(json!({"value": "ok"})),
                event: JournalEvent::ExecutionStarted,
            };
            journal.append(&entry).unwrap();
        }
        let contents = fs::read_to_string(&path).unwrap();
        let entry: Value = serde_json::from_str(contents.trim()).unwrap();
        assert_eq!(entry["sequence"], 1);
        fs::remove_file(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn jsonl_journal_restricts_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let path = temporary_path("jsonl");
        let _journal = JsonlJournal::open(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_file(path).unwrap();
    }
}
