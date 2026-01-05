//! Retry utilities for actor operations with exponential backoff.

use std::future::Future;
use std::time::Duration;

/// Configuration for retry behavior
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Initial delay before first retry
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
        }
    }
}

impl RetryConfig {
    /// Create a new retry config with specified parameters
    pub fn new(max_retries: u32, initial_delay: Duration, max_delay: Duration) -> Self {
        Self {
            max_retries,
            initial_delay,
            max_delay,
        }
    }
}

/// Execute an async operation with exponential backoff retry.
///
/// The operation is retried up to `config.max_retries` times, with exponentially
/// increasing delays between attempts (capped at `config.max_delay`).
///
/// Returns the result of the first successful attempt, or the last error if all
/// retries are exhausted.
///
/// # Example
/// ```ignore
/// let result = with_retry(&config, || async {
///     some_fallible_operation().await
/// }).await;
/// ```
pub async fn with_retry<F, Fut, T, E>(config: &RetryConfig, mut operation: F) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut attempts = 0;
    let mut delay = config.initial_delay;

    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                attempts += 1;
                if attempts > config.max_retries {
                    return Err(e);
                }

                tracing::warn!(
                    "Operation failed (attempt {}/{}): {}. Retrying in {:?}...",
                    attempts,
                    config.max_retries + 1,
                    e,
                    delay
                );

                tokio::time::sleep(delay).await;

                // Exponential backoff with cap
                delay = (delay * 2).min(config.max_delay);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn test_retry_success_first_attempt() {
        let config = RetryConfig::new(3, Duration::from_millis(10), Duration::from_millis(100));
        let attempts = AtomicU32::new(0);

        let result: Result<i32, &str> = with_retry(&config, || {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { Ok(42) }
        })
        .await;

        assert_eq!(result, Ok(42));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retry_success_after_failures() {
        let config = RetryConfig::new(3, Duration::from_millis(10), Duration::from_millis(100));
        let attempts = AtomicU32::new(0);

        let result: Result<i32, &str> = with_retry(&config, || {
            let count = attempts.fetch_add(1, Ordering::SeqCst) + 1;
            async move {
                if count < 3 {
                    Err("temporary failure")
                } else {
                    Ok(42)
                }
            }
        })
        .await;

        assert_eq!(result, Ok(42));
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_exhausted() {
        let config = RetryConfig::new(2, Duration::from_millis(10), Duration::from_millis(100));
        let attempts = AtomicU32::new(0);

        let result: Result<i32, &str> = with_retry(&config, || {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { Err("permanent failure") }
        })
        .await;

        assert_eq!(result, Err("permanent failure"));
        assert_eq!(attempts.load(Ordering::SeqCst), 3); // 1 initial + 2 retries
    }
}
