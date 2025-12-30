//! AI features module for email summarization and writing polish
//!
//! This module provides optional AI-powered features using the OpenRouter API:
//! - Email summarization (single emails and threads)
//! - Grammar and writing polish for composed emails

mod actor;
mod client;
mod prompts;

pub use actor::{spawn_ai_actor, AiActorHandle, AiCommand, AiEvent};
pub use client::OpenRouterClient;
