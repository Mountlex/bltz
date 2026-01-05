//! AI actor for async processing of AI requests

use std::time::Duration;
use tokio::sync::mpsc;

use super::client::OpenRouterClient;
use super::prompts;
use crate::actor::{RetryConfig, with_retry};
use crate::mail::ThreadId;

/// Default retry configuration for AI operations
fn default_retry_config() -> RetryConfig {
    RetryConfig::new(3, Duration::from_millis(500), Duration::from_secs(10))
}

/// Commands that can be sent to the AI actor
#[derive(Debug)]
#[allow(dead_code)]
pub enum AiCommand {
    /// Summarize a single email
    SummarizeEmail {
        uid: u32,
        subject: String,
        body: String,
    },
    /// Summarize an entire email thread
    SummarizeThread {
        thread_id: ThreadId,
        /// Vec of (from, subject, body) for each email in thread
        emails: Vec<(String, String, String)>,
    },
    /// Polish/improve writing
    Polish { original: String },
    /// Shutdown the actor
    Shutdown,
}

/// Events emitted by the AI actor
#[derive(Debug, Clone)]
pub enum AiEvent {
    /// Single email summary completed
    EmailSummary { uid: u32, summary: String },
    /// Thread summary completed
    ThreadSummary {
        thread_id: ThreadId,
        summary: String,
    },
    /// Polished text ready
    Polished { original: String, polished: String },
    /// Error occurred during AI processing
    Error(String),
}

/// Handle for communicating with the AI actor
pub struct AiActorHandle {
    pub cmd_tx: mpsc::Sender<AiCommand>,
    pub event_rx: mpsc::Receiver<AiEvent>,
}

/// Spawn the AI actor task
pub fn spawn_ai_actor(
    client: OpenRouterClient,
    summary_max_tokens: u32,
    polish_max_tokens: u32,
) -> AiActorHandle {
    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    let (event_tx, event_rx) = mpsc::channel(32);

    tokio::spawn(ai_actor_loop(
        client,
        summary_max_tokens,
        polish_max_tokens,
        cmd_rx,
        event_tx,
    ));

    AiActorHandle { cmd_tx, event_rx }
}

async fn ai_actor_loop(
    client: OpenRouterClient,
    summary_max_tokens: u32,
    polish_max_tokens: u32,
    mut cmd_rx: mpsc::Receiver<AiCommand>,
    event_tx: mpsc::Sender<AiEvent>,
) {
    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            AiCommand::SummarizeEmail { uid, subject, body } => {
                let user_content = format!("Subject: {}\n\n{}", subject, body);
                let retry_config = default_retry_config();
                let result = with_retry(&retry_config, || {
                    client.complete(
                        prompts::EMAIL_SUMMARY_SYSTEM,
                        &user_content,
                        summary_max_tokens,
                    )
                })
                .await;

                let event = match result {
                    Ok(summary) => AiEvent::EmailSummary { uid, summary },
                    Err(e) => AiEvent::Error(format!("Summary failed: {}", e)),
                };
                if event_tx.send(event).await.is_err() {
                    tracing::warn!("AI actor: event receiver dropped");
                    break;
                }
            }

            AiCommand::SummarizeThread { thread_id, emails } => {
                let thread_content = emails
                    .iter()
                    .map(|(from, subject, body)| {
                        format!("From: {}\nSubject: {}\n\n{}\n---", from, subject, body)
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n");

                let retry_config = default_retry_config();
                let result = with_retry(&retry_config, || {
                    client.complete(
                        prompts::THREAD_SUMMARY_SYSTEM,
                        &thread_content,
                        summary_max_tokens,
                    )
                })
                .await;

                let event = match result {
                    Ok(summary) => AiEvent::ThreadSummary { thread_id, summary },
                    Err(e) => AiEvent::Error(format!("Thread summary failed: {}", e)),
                };
                if event_tx.send(event).await.is_err() {
                    tracing::warn!("AI actor: event receiver dropped");
                    break;
                }
            }

            AiCommand::Polish { original } => {
                let retry_config = default_retry_config();
                let result = with_retry(&retry_config, || {
                    client.complete(prompts::POLISH_SYSTEM, &original, polish_max_tokens)
                })
                .await;

                let event = match result {
                    Ok(polished) => AiEvent::Polished {
                        original: original.clone(),
                        polished,
                    },
                    Err(e) => AiEvent::Error(format!("Polish failed: {}", e)),
                };
                if event_tx.send(event).await.is_err() {
                    tracing::warn!("AI actor: event receiver dropped");
                    break;
                }
            }

            AiCommand::Shutdown => {
                break;
            }
        }
    }
}
