//! Shared actor utilities and patterns.

pub mod retry;

pub use retry::{RetryConfig, with_retry};
