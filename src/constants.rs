//! Application-wide constants for tuning and configuration
//!
//! Centralizes magic numbers to make them discoverable and configurable.

/// Debounce delay for prefetch requests in milliseconds.
/// Prevents flooding the IMAP server during rapid navigation.
pub const PREFETCH_DEBOUNCE_MS: u64 = 150;

/// Number of emails to load per page from cache.
/// Balances memory usage with scroll smoothness.
pub const EMAIL_PAGE_SIZE: usize = 500;

/// IDLE connection timeout in seconds before refresh.
/// RFC 3501 recommends 29 minutes; we use 5 minutes for reliability.
pub const IDLE_TIMEOUT_SECS: u64 = 300;

/// Batch size for flag sync operations.
/// Prevents IMAP command line length limits (~8KB).
pub const FLAG_SYNC_BATCH_SIZE: usize = 500;

/// Maximum retry delay in seconds for connection attempts.
pub const MAX_RETRY_DELAY_SECS: u64 = 30;

/// Maximum number of connection retry attempts.
pub const MAX_RETRIES: u32 = 10;

/// Error message display duration in seconds before auto-dismiss.
pub const ERROR_TTL_SECS: u64 = 5;

/// Delay in seconds before pending deletions are executed.
/// Allows user to undo within this time window.
pub const DELETION_DELAY_SECS: u64 = 10;

/// Minimum terminal width to show split view (list + preview).
/// Below this width, only the email list is shown.
pub const MIN_SPLIT_VIEW_WIDTH: u16 = 80;

/// Fixed width of the folder sidebar pane in columns.
pub const FOLDER_SIDEBAR_WIDTH: u16 = 20;

/// Minimum terminal width to show folder sidebar with split view.
/// Below this width, the sidebar is hidden even if enabled.
pub const MIN_SIDEBAR_VIEW_WIDTH: u16 = 100;

/// Debounce delay for body FTS search in milliseconds.
/// Headers are searched instantly; body FTS runs after this delay.
pub const SEARCH_DEBOUNCE_MS: u64 = 150;

// === UI Constants ===

/// Minimum split ratio percentage for inbox split view.
pub const SPLIT_RATIO_MIN: u16 = 30;

/// Maximum split ratio percentage for inbox split view.
pub const SPLIT_RATIO_MAX: u16 = 70;

/// Target scroll position as fraction of visible area (1/N from top).
/// A value of 4 means the selected item targets 1/4 from the top.
pub const SCROLL_TARGET_FRACTION: usize = 4;

/// Spinner animation frame duration in milliseconds.
pub const SPINNER_FRAME_MS: u128 = 80;

// === Modern Theme Spacing Constants ===

/// Status bar height in lines for modern theme (includes padding).
pub const STATUS_BAR_HEIGHT_MODERN: u16 = 2;

/// Help bar height in lines for modern theme (includes padding).
pub const HELP_BAR_HEIGHT_MODERN: u16 = 2;

/// Horizontal content padding in characters for modern theme.
pub const CONTENT_PADDING_H: u16 = 2;

/// Interval in seconds to check for system theme changes.
pub const THEME_CHECK_INTERVAL_SECS: u64 = 2;
