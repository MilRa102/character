//! Dialog and tool calling, built on the [RIG](https://rig.rs) LLM framework.
//!
//! The brain works with any RIG completion model. The default backend is local
//! (Ollama; a llama.cpp server is reachable through RIG's `llamacpp` provider), and
//! cloud models stay opt-in (`ARCHITECTURE.md`, §1 and §7).
//!
//! Tool calling will be wired to `character-actions`: only allowlisted actions
//! are ever offered to the model.

use rig::{
    Agent, AgentBuilder, DynModel, completion::PromptError, operation::Completion,
    providers::ollama::OllamaConfig,
};

/// Persona used until `persona.toml` is supported.
pub const DEFAULT_PREAMBLE: &str = "You are Character, a small companion living in a drawer at \
the top of the user's COSMIC desktop. Answer briefly and in the language the user speaks.";

/// The model could not produce an answer (the backend is down, the response is malformed, ...).
#[derive(Debug, thiserror::Error)]
#[error("LLM request failed: {0}")]
pub struct BrainError(Box<PromptError>);

impl From<PromptError> for BrainError {
    fn from(error: PromptError) -> Self {
        Self(Box::new(error))
    }
}

/// The LLM side of the companion.
pub struct Brain {
    agent: Agent,
}

impl Brain {
    /// Creates a brain over any RIG completion model.
    pub fn new(model: impl Into<DynModel<Completion>>, preamble: impl Into<String>) -> Self {
        Self {
            agent: AgentBuilder::new(model).preamble(preamble).build(),
        }
    }

    /// Creates a brain on an Ollama daemon (the local one with [`OllamaConfig::new`]).
    ///
    /// Nothing is sent until the first [`reply`](Self::reply).
    pub fn ollama(config: &OllamaConfig, model: &str, preamble: impl Into<String>) -> Self {
        Self::new(config.clone().client().completion(model), preamble)
    }

    /// Answers a single user message.
    ///
    /// An error (for example an unreachable daemon) is not fatal: the companion
    /// keeps working with rule-based reactions.
    pub async fn reply(&self, text: &str) -> Result<String, BrainError> {
        Ok(self.agent.prompt(text).await?.output())
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use rig::test_utils::{MockCompletionModel, MockTurn};

    use super::*;

    #[tokio::test]
    async fn replies_with_the_model_text() {
        let brain = Brain::new(MockCompletionModel::text("Привет!"), DEFAULT_PREAMBLE);

        assert_eq!(brain.reply("hi").await.unwrap(), "Привет!");
    }

    #[tokio::test]
    async fn sends_the_persona_and_the_user_message() {
        let model = MockCompletionModel::text("ok");
        let brain = Brain::new(model.clone(), "You are a fox.");

        brain.reply("pause the music").await.unwrap();

        let requests = model.requests();
        assert_eq!(requests.len(), 1);
        let request = format!("{:?}", requests[0]);
        assert!(request.contains("You are a fox."), "{request}");
        assert!(request.contains("pause the music"), "{request}");
    }

    #[tokio::test]
    async fn model_failure_is_reported_not_panicked() {
        let model = MockCompletionModel::from_turns([MockTurn::error("boom")]);
        let brain = Brain::new(model, DEFAULT_PREAMBLE);

        assert!(brain.reply("hi").await.is_err());
    }

    #[tokio::test]
    async fn unreachable_ollama_is_an_error() {
        let unused_port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let config = OllamaConfig::new().with_base_url(format!("http://127.0.0.1:{unused_port}"));
        let brain = Brain::ollama(&config, "qwen3", DEFAULT_PREAMBLE);

        assert!(brain.reply("hi").await.is_err());
    }
}
