//! Common UI widgets and utilities

use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::theme::Theme;

// Re-export status bar items for backwards compatibility
pub use super::status_bar::{enhanced_status_bar, status_bar, StatusInfo};

pub fn error_bar(frame: &mut Frame, area: Rect, message: &str) {
    let style = Theme::error_bar();
    let paragraph = Paragraph::new(format!(" Error: {} ", message)).style(style);
    frame.render_widget(paragraph, area);
}

pub fn help_bar(frame: &mut Frame, area: Rect, hints: &[(&str, &str)]) {
    use unicode_width::UnicodeWidthStr;

    let available_width = area.width as usize;

    // Calculate total width needed for each hint (including separator)
    // Format: " key desc │" (separator between hints)
    let hint_widths: Vec<usize> = hints
        .iter()
        .enumerate()
        .map(|(i, (key, desc))| {
            let base = format!(" {} ", key).width() + format!("{}", desc).width();
            if i < hints.len() - 1 {
                base + 3 // " │ " separator
            } else {
                base + 1 // trailing space
            }
        })
        .collect();

    // Find how many hints we can fit
    let mut total_width = 0;
    let mut hints_to_show = 0;
    for width in &hint_widths {
        if total_width + width <= available_width {
            total_width += width;
            hints_to_show += 1;
        } else {
            break;
        }
    }

    // Show at least one hint if possible
    hints_to_show = hints_to_show.max(1).min(hints.len());

    let mut spans: Vec<Span> = Vec::new();
    for (i, (key, desc)) in hints.iter().take(hints_to_show).enumerate() {
        spans.push(Span::styled(format!(" {} ", key), Theme::help_key()));
        spans.push(Span::styled(desc.to_string(), Theme::help_desc()));
        if i < hints_to_show - 1 {
            spans.push(Span::styled(" │ ", Theme::text_muted()));
        }
    }
    spans.push(Span::styled(" ", Theme::text_muted())); // trailing space

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

pub fn truncate_string(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        s.to_string()
    } else if max_len > 3 {
        let truncated: String = s.chars().take(max_len - 3).collect();
        format!("{}...", truncated)
    } else {
        s.chars().take(max_len).collect()
    }
}

pub fn format_date(timestamp: i64) -> String {
    use chrono::{DateTime, Datelike, Local, Utc};

    let dt = DateTime::from_timestamp(timestamp, 0)
        .unwrap_or_else(Utc::now)
        .with_timezone(&Local);

    let now = Local::now();
    let today = now.date_naive();
    let email_date = dt.date_naive();

    if email_date == today {
        dt.format("%H:%M").to_string()
    } else if (today - email_date).num_days() < 7 {
        dt.format("%a %H:%M").to_string()
    } else if email_date.year() == today.year() {
        dt.format("%b %d").to_string()
    } else {
        dt.format("%Y-%m-%d").to_string()
    }
}

/// Format date for display (static format)
pub fn format_relative_date(timestamp: i64) -> String {
    format_date(timestamp)
}

/// Sanitize text for display: remove control characters and ANSI escape sequences
pub fn sanitize_text(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        // Check for ANSI escape sequence (ESC [ ... m)
        if c == '\x1b' {
            // Skip the escape sequence
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                              // Skip until we hit a letter (end of sequence)
                while let Some(&ch) = chars.peek() {
                    chars.next();
                    if ch.is_ascii_alphabetic() {
                        break;
                    }
                }
                continue;
            }
        }
        // Replace other control characters (except newline and tab) with space
        if c.is_control() && c != '\n' && c != '\t' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }

    result
}
