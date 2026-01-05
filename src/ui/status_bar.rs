//! Status bar rendering with connection indicators and account badges

use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use super::theme::{Theme, symbols};
use crate::app::state::OtherAccountInfo;
use crate::constants::SPINNER_FRAME_MS;

/// Get current process memory usage (RSS) in bytes
/// Returns None if unable to read memory info
#[cfg(unix)]
fn get_memory_usage() -> Option<u64> {
    use std::fs;
    // Read /proc/self/status and find VmRSS line
    let status = fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if line.starts_with("VmRSS:") {
            // Format: "VmRSS:     12345 kB"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let kb: u64 = parts[1].parse().ok()?;
                return Some(kb * 1024); // Convert to bytes
            }
        }
    }
    None
}

#[cfg(not(unix))]
fn get_memory_usage() -> Option<u64> {
    None
}

/// Format bytes as human-readable string (e.g., "12.3 MB")
fn format_memory(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Simple status bar with left and right text
pub fn status_bar(frame: &mut Frame, area: Rect, left: &str, right: &str) {
    let style = Theme::status_bar();

    let left_span = Span::styled(format!(" {} ", left), style);
    let right_span = Span::styled(format!(" {} ", right), style);

    let available = area
        .width
        .saturating_sub(left.len() as u16 + right.len() as u16 + 4);
    let padding = " ".repeat(available as usize);

    let line = Line::from(vec![left_span, Span::styled(padding, style), right_span]);

    let paragraph = Paragraph::new(line).style(style);
    frame.render_widget(paragraph, area);
}

/// Status bar info for rendering
pub struct StatusInfo<'a> {
    pub folder: &'a str,
    pub unread: usize,
    pub total: usize,
    pub connected: bool,
    pub loading: bool,
    pub last_sync: Option<i64>,
    pub account: &'a str,
    pub search_query: Option<&'a str>,
    pub search_results: usize,
    pub status_message: Option<&'a str>,
    /// Other accounts for status bar indicators (empty if single account)
    pub other_accounts: &'a [OtherAccountInfo],
    /// Whether currently showing starred emails view
    pub starred_view: bool,
    /// Whether conversation mode is enabled (show sent in threads)
    pub conversation_mode: bool,
    /// Whether there's an unacknowledged error (show indicator)
    pub has_error: bool,
}

/// Calculate display width of a string (accounting for Unicode)
fn display_width(s: &str) -> usize {
    use unicode_width::UnicodeWidthStr;
    s.width()
}

/// Truncate string to fit display width
fn truncate_to_width(s: &str, max_width: usize) -> String {
    use unicode_width::UnicodeWidthChar;

    if max_width < 4 {
        return s.chars().take(max_width).collect();
    }

    let mut width = 0;
    let mut result = String::new();

    for c in s.chars() {
        let char_width = c.width().unwrap_or(1);
        if width + char_width > max_width - 3 {
            result.push_str("...");
            return result;
        }
        width += char_width;
        result.push(c);
    }
    result
}

/// Enhanced status bar with connection indicator and more info
pub fn enhanced_status_bar(frame: &mut Frame, area: Rect, info: &StatusInfo) {
    let style = Theme::status_bar();
    let width = area.width as usize;

    // Build left side content
    let conn_indicator = if info.loading {
        format!(" {} ", spinner_char())
    } else if info.connected {
        format!(" {} ", symbols::CONNECTED)
    } else {
        format!(" {} ", symbols::DISCONNECTED)
    };

    // Error indicator (red !)
    let error_indicator = if info.has_error { "! " } else { "" };

    // Build folder info as spans (unread count is bold)
    let unread_style = style.add_modifier(ratatui::style::Modifier::BOLD);
    let folder_info_spans: Vec<(String, Style)> = if let Some(query) = info.search_query {
        if !query.is_empty() {
            vec![(
                format!("\"{}\" ({} results)", query, info.search_results),
                style,
            )]
        } else if info.starred_view {
            vec![
                (
                    format!("{} {} [Starred] ", symbols::STARRED, info.folder),
                    style,
                ),
                (info.unread.to_string(), unread_style),
                (format!(" / {}", info.total), style),
            ]
        } else {
            vec![
                (format!("{} ", info.folder), style),
                (info.unread.to_string(), unread_style),
                (format!(" / {}", info.total), style),
            ]
        }
    } else if info.starred_view {
        vec![
            (
                format!("{} {} [Starred] ", symbols::STARRED, info.folder),
                style,
            ),
            (info.unread.to_string(), unread_style),
            (format!(" / {}", info.total), style),
        ]
    } else {
        vec![
            (format!("{} ", info.folder), style),
            (info.unread.to_string(), unread_style),
            (format!(" / {}", info.total), style),
        ]
    };

    // Add conversation mode indicator if enabled (only show in INBOX)
    let mut folder_info_spans = folder_info_spans;
    if info.conversation_mode && info.folder == "INBOX" {
        folder_info_spans.push((" [Conv]".to_string(), style));
    }

    let folder_info_width: usize = folder_info_spans
        .iter()
        .map(|(s, _)| display_width(s))
        .sum();

    // Build right side content
    let status_msg = if let Some(msg) = info.status_message {
        if !msg.is_empty() {
            format!("{} │ ", msg)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let memory_info = if let Some(bytes) = get_memory_usage() {
        format!("{} │ ", format_memory(bytes))
    } else {
        String::new()
    };

    let sync_info = if let Some(ts) = info.last_sync {
        format!("{} │ ", format_relative_time(ts))
    } else {
        String::new()
    };

    // Build account indicators for other accounts
    let account_indicators = build_account_indicators(info.other_accounts);
    let indicators_width = display_width(
        &account_indicators
            .iter()
            .map(|(s, _)| s.as_str())
            .collect::<Vec<_>>()
            .join(""),
    );

    // Calculate widths to determine available space for account
    let left_width =
        display_width(&conn_indicator) + display_width(error_indicator) + folder_info_width;
    let fixed_right_width = display_width(&status_msg)
        + display_width(&memory_info)
        + display_width(&sync_info)
        + indicators_width
        + 2; // +2 for account padding
    let min_padding = 2; // Minimum spacing between left and right

    let available_for_account = width.saturating_sub(left_width + fixed_right_width + min_padding);
    let account = if display_width(info.account) <= available_for_account {
        info.account.to_string()
    } else {
        truncate_to_width(info.account, available_for_account.max(10))
    };

    let right_width = display_width(&status_msg)
        + display_width(&memory_info)
        + display_width(&sync_info)
        + display_width(&account)
        + indicators_width
        + 1;
    let padding_width = width.saturating_sub(left_width + right_width);
    let padding = " ".repeat(padding_width);

    // Build spans with proper styling
    let conn_style = if info.loading {
        Theme::status_syncing()
    } else if info.connected {
        Theme::status_connected()
    } else {
        Theme::status_disconnected()
    };

    let mut spans = vec![Span::styled(conn_indicator, conn_style)];
    // Add error indicator if present
    if info.has_error {
        spans.push(Span::styled(error_indicator, Theme::status_disconnected()));
    }
    // Add folder info spans (with bold unread count)
    for (text, span_style) in folder_info_spans {
        spans.push(Span::styled(text, span_style));
    }
    spans.extend([
        Span::styled(padding, style),
        Span::styled(status_msg, Theme::status_info()),
        Span::styled(memory_info, Theme::status_info()),
        Span::styled(sync_info, Theme::status_info()),
        Span::styled(format!("{} ", account), style),
    ]);

    // Add account indicator spans
    for (text, indicator_style) in account_indicators {
        spans.push(Span::styled(text, indicator_style));
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).style(style);
    frame.render_widget(paragraph, area);
}

/// Build account indicator spans for the status bar
/// Returns a list of (text, style) pairs
fn build_account_indicators(accounts: &[OtherAccountInfo]) -> Vec<(String, Style)> {
    if accounts.is_empty() {
        return Vec::new();
    }

    let mut spans = Vec::new();
    spans.push(("│ ".to_string(), Theme::status_muted()));

    for (i, account) in accounts.iter().enumerate() {
        if i > 0 {
            spans.push((" ".to_string(), Theme::status_bar()));
        }

        // Indicator symbol
        let (indicator, indicator_style) = if account.has_error {
            (symbols::ACCOUNT_ERROR, Theme::account_error())
        } else if account.has_new_mail {
            (symbols::ACCOUNT_NEW_MAIL, Theme::account_new_mail())
        } else if account.connected {
            (symbols::ACCOUNT_NO_NEW, Theme::account_no_new())
        } else {
            (symbols::DISCONNECTED, Theme::account_error())
        };

        spans.push((indicator.to_string(), indicator_style));

        // Account name (short)
        spans.push((account.name.clone(), Theme::account_name()));

        // New mail count badge if there's new mail
        if account.has_new_mail && account.new_count > 0 {
            spans.push((format!("({})", account.new_count), Theme::account_badge()));
        }
    }

    spans.push((" ".to_string(), Theme::status_bar()));
    spans
}

/// Format a timestamp as relative time (e.g., "2m ago", "1h ago", "Yesterday")
pub fn format_relative_time(timestamp: i64) -> String {
    use chrono::{DateTime, Local, Utc};

    let then = DateTime::from_timestamp(timestamp, 0)
        .unwrap_or_else(Utc::now)
        .with_timezone(&Local);
    let now = Local::now();
    let diff = now.signed_duration_since(then);

    if diff.num_seconds() < 60 {
        "just now".to_string()
    } else if diff.num_minutes() < 60 {
        format!("{}m ago", diff.num_minutes())
    } else if diff.num_hours() < 24 {
        format!("{}h ago", diff.num_hours())
    } else if diff.num_days() == 1 {
        "yesterday".to_string()
    } else if diff.num_days() < 7 {
        format!("{}d ago", diff.num_days())
    } else {
        then.format("%b %d").to_string()
    }
}

/// Get an animated spinner character for loading states
pub fn spinner_char() -> char {
    let spinner = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";
    let idx = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
        / SPINNER_FRAME_MS) as usize
        % spinner.chars().count();

    spinner.chars().nth(idx).unwrap_or('*')
}
