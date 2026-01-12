//! Common UI widgets and utilities

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};

use super::theme::{self, Theme};
use crate::constants::CONTENT_PADDING_H;

// Re-export status bar items for backwards compatibility
pub use super::status_bar::{StatusInfo, enhanced_status_bar, spinner_char, status_bar};

pub fn error_bar(frame: &mut Frame, area: Rect, message: &str) {
    let style = Theme::error_bar();
    let paragraph = Paragraph::new(format!(" Error: {} ", message)).style(style);
    frame.render_widget(paragraph, area);
}

pub fn help_bar(frame: &mut Frame, area: Rect, hints: &[(&str, &str)]) {
    use unicode_width::UnicodeWidthStr;

    let use_modern = theme::use_modern_spacing();

    // Fill entire area with background color
    let bg_fill = ratatui::widgets::Block::default().style(Theme::help_bar());
    frame.render_widget(bg_fill, area);

    // Calculate content area (centered vertically if multi-line, with horizontal padding)
    let h_padding = if use_modern { CONTENT_PADDING_H } else { 0 };
    let content_y = if use_modern && area.height >= 2 {
        area.y + (area.height - 1) / 2 // Center the content line
    } else {
        area.y
    };
    let content_area = Rect {
        x: area.x + h_padding,
        y: content_y,
        width: area.width.saturating_sub(h_padding * 2),
        height: 1,
    };

    let available_width = content_area.width as usize;

    // Separator style: " • " with spacing in modern, " │ " in classic
    let sep = if use_modern { "  •  " } else { " │ " };
    let sep_width = sep.width();

    // Calculate total width needed for each hint (including separator)
    let hint_widths: Vec<usize> = hints
        .iter()
        .enumerate()
        .map(|(i, (key, desc))| {
            let base = format!(" {} ", key).width() + desc.to_string().width();
            if i < hints.len() - 1 {
                base + sep_width
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
            spans.push(Span::styled(sep, Theme::text_muted()));
        }
    }
    spans.push(Span::styled(" ", Theme::text_muted())); // trailing space

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, content_area);
}

/// Calculate display width of a string (accounting for Unicode)
pub fn display_width(s: &str) -> usize {
    use unicode_width::UnicodeWidthStr;
    s.width()
}

pub fn truncate_string(s: &str, max_width: usize) -> String {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

    let current_width = s.width();
    if current_width <= max_width {
        return s.to_string();
    }

    if max_width <= 3 {
        // Not enough room for ellipsis, just take what fits
        let mut width = 0;
        let mut result = String::new();
        for c in s.chars() {
            let char_width = c.width().unwrap_or(1);
            if width + char_width > max_width {
                break;
            }
            width += char_width;
            result.push(c);
        }
        return result;
    }

    // Truncate to max_width - 3 (for "...") accounting for display width
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

/// Format date as compact relative time for thread list (2h, 3d, 2w)
pub fn format_relative_date(timestamp: i64) -> String {
    use chrono::{DateTime, Local, Utc};

    let then = DateTime::from_timestamp(timestamp, 0)
        .unwrap_or_else(Utc::now)
        .with_timezone(&Local);
    let now = Local::now();
    let diff = now.signed_duration_since(then);

    if diff.num_seconds() < 60 {
        "now".to_string()
    } else if diff.num_minutes() < 60 {
        format!("{}m", diff.num_minutes())
    } else if diff.num_hours() < 24 {
        format!("{}h", diff.num_hours())
    } else if diff.num_days() < 7 {
        format!("{}d", diff.num_days())
    } else if diff.num_days() < 30 {
        format!("{}w", (diff.num_days() / 7).max(1))
    } else {
        then.format("%b %d").to_string()
    }
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
