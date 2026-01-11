//! Inbox view rendering.
//!
//! This module is split into:
//! - `mod.rs` - Main render_inbox, layout, preview, headers, body
//! - `thread.rs` - Thread list rendering with virtual scrolling
//! - `format.rs` - Text formatting utilities (highlight_matches)
//! - `popups.rs` - Modal overlays (folder picker, help popup, command bar)

mod format;
mod popups;
mod thread;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span, Text},
    widgets::{Clear, Paragraph, Wrap},
};

use crate::app::state::{AppState, ModalState};
use crate::constants::{MIN_SPLIT_VIEW_WIDTH, SPLIT_RATIO_MAX, SPLIT_RATIO_MIN};

use super::components::{render_email_headers, render_quoted_text};
use super::status_bar::spinner_char;
use super::theme::Theme;
use super::widgets::{StatusInfo, enhanced_status_bar, error_bar, help_bar, sanitize_text};

use popups::{
    render_command_bar, render_confirm_modal, render_folder_picker, render_unified_help_popup,
};
use thread::render_thread_list;

pub fn render_inbox(frame: &mut Frame, state: &AppState) {
    let show_search_bar = state.modal.is_search() || !state.search.query.is_empty();
    let show_command_bar = state.modal.is_command() || state.modal.command_result().is_some();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(if show_command_bar {
            vec![
                Constraint::Length(1), // Status bar
                Constraint::Min(0),    // Main content (split view)
                Constraint::Length(1), // Command bar (replaces help bar when active)
            ]
        } else if show_search_bar {
            vec![
                Constraint::Length(1), // Status bar
                Constraint::Length(1), // Search bar
                Constraint::Min(0),    // Main content (split view)
                Constraint::Length(1), // Help bar or error
            ]
        } else {
            vec![
                Constraint::Length(1), // Status bar
                Constraint::Min(0),    // Main content (split view)
                Constraint::Length(1), // Help bar or error
            ]
        })
        .split(frame.area());

    let (status_area, search_area, main_area, help_area) = if show_command_bar {
        (chunks[0], None, chunks[1], chunks[2])
    } else if show_search_bar {
        (chunks[0], Some(chunks[1]), chunks[2], chunks[3])
    } else {
        (chunks[0], None, chunks[1], chunks[2])
    };

    // Status bar
    let visible_count = state.visible_thread_count();
    let status_info = StatusInfo::from_state(state, Some(visible_count));
    enhanced_status_bar(frame, status_area, &status_info);

    // Search bar (if active)
    if let Some(area) = search_area {
        render_search_bar(frame, area, state);
    }

    // Split view: thread list on left, content on right (if wide enough)
    let show_preview = main_area.width >= MIN_SPLIT_VIEW_WIDTH;

    if show_preview {
        let ratio = state.split_ratio.clamp(SPLIT_RATIO_MIN, SPLIT_RATIO_MAX);
        let split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(ratio),       // Thread list
                Constraint::Percentage(100 - ratio), // Content preview
            ])
            .split(main_area);

        // Thread list (left pane)
        render_thread_list(frame, split[0], state, true);

        // Content preview (right pane)
        render_preview(frame, split[1], state);
    } else {
        // Narrow terminal: only show thread list (no border)
        render_thread_list(frame, main_area, state, false);
    }

    // Command bar or error (keybindings footer removed - use :keys command instead)
    if show_command_bar {
        render_command_bar(frame, help_area, state);
    } else if let Some(ref error) = state.status.error {
        error_bar(frame, help_area, error);
    } else if state.modal.is_folder_picker() {
        let hints = &[("j/k", "nav"), ("Enter", "select"), ("Esc", "close")];
        help_bar(frame, help_area, hints);
    } else if state.modal.is_search() {
        let hints = &[("Type", "search"), ("Enter/Esc", "done")];
        help_bar(frame, help_area, hints);
    } else {
        // Default help bar for discoverability
        let hints = &[("j/k", "nav"), ("Enter", "open"), (".", "help")];
        help_bar(frame, help_area, hints);
    }

    // Folder picker overlay (rendered last so it appears on top)
    if state.modal.is_folder_picker() {
        render_folder_picker(frame, frame.area(), state);
    }

    // Confirmation modal for destructive commands
    if let Some(pending) = state.modal.pending_confirmation() {
        render_confirm_modal(frame, frame.area(), pending);
    }

    // Help popup (rendered last so it appears on top)
    if let ModalState::Help {
        ref keybindings,
        ref commands,
        scroll,
    } = state.modal
    {
        render_unified_help_popup(frame, frame.area(), keybindings, commands, scroll);
    }
}

fn render_search_bar(frame: &mut Frame, area: Rect, state: &AppState) {
    let style = if state.modal.is_search() {
        Theme::status_bar()
    } else if !state.search.query.is_empty() {
        // Show query more visibly when inactive but has content
        Theme::text_secondary()
    } else {
        Theme::text_muted()
    };

    let cursor = if state.modal.is_search() { "│" } else { "" };
    let text = format!(" / {}{} ", state.search.query, cursor);

    let paragraph = Paragraph::new(text).style(style);
    frame.render_widget(paragraph, area);
}

fn render_preview(frame: &mut Frame, area: Rect, state: &AppState) {
    // Add 1-char left padding for visual separation from border
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    let inner = chunks[1];

    // Get the currently selected email
    let email = state.current_email_from_thread();

    if let Some(email) = email {
        let expanded = state.reader.headers_expanded;

        // Check if CC has actual content (not empty)
        let has_cc = email
            .cc_addr
            .as_ref()
            .is_some_and(|cc| !cc.trim().is_empty());

        // Calculate header height based on actual content
        // Label width is 9 chars ("Subject: "), so value area is width - 9
        let value_width = inner.width.saturating_sub(9) as usize;

        use unicode_width::UnicodeWidthStr;

        // Helper to calculate lines needed for a field value
        let lines_for_field = |text: &str| -> u16 {
            if text.is_empty() || value_width == 0 {
                1
            } else {
                let text_width = text.width();
                text_width.div_ceil(value_width).max(1) as u16
            }
        };

        let from_display = if let Some(ref name) = email.from_name {
            format!("{} <{}>", name, email.from_addr)
        } else {
            email.from_addr.clone()
        };

        let header_lines = if expanded {
            // Calculate actual lines needed for each field when wrapped
            let from_lines = lines_for_field(&from_display);
            let to_lines = lines_for_field(email.to_addr.as_deref().unwrap_or(""));
            let cc_lines = if has_cc {
                lines_for_field(email.cc_addr.as_deref().unwrap_or(""))
            } else {
                0
            };
            let date_lines = 1; // Date is always short
            let subject_lines = lines_for_field(&email.subject);
            let attach_lines = if email.has_attachments { 1 } else { 0 };

            (from_lines + to_lines + cc_lines + date_lines + subject_lines + attach_lines)
                .min(inner.height.saturating_sub(5))
        } else {
            // Collapsed: 1 line per field (From, To, Date, Subject + optional CC + optional Attach)
            4 + if has_cc { 1 } else { 0 } + if email.has_attachments { 1 } else { 0 }
        };

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(header_lines + 1), // Headers + border
                Constraint::Min(0),                   // Body
            ])
            .split(inner);

        // Render headers
        render_email_headers(frame, sections[0], email, true, expanded);

        // Render body
        render_email_body(frame, sections[1], state);
    } else {
        // Show helpful hints when no email is selected
        let hint_lines = vec![
            Line::from(Span::styled("No email selected", Theme::text_secondary())),
            Line::from(""),
            Line::from(Span::styled("j/k to navigate", Theme::text_muted())),
            Line::from(Span::styled("Enter to read", Theme::text_muted())),
            Line::from(Span::styled(". for help", Theme::text_muted())),
        ];
        let paragraph = Paragraph::new(hint_lines).alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(paragraph, inner);
    }
}

fn render_email_body(frame: &mut Frame, area: Rect, state: &AppState) {
    // Clear the area first to prevent rendering artifacts when content changes
    frame.render_widget(Clear, area);

    // Get sanitized body text - uses cache when body is loaded, otherwise show loading/preview
    let sanitized = if state.reader.body.is_some() {
        // Use cached sanitized body (computed once per body change)
        state.reader.sanitized_body(sanitize_text)
    } else if state.status.loading {
        format!("{} Loading...", spinner_char())
    } else {
        // Show preview if body not loaded
        if let Some(email) = state.current_email_from_thread() {
            if let Some(ref preview) = email.preview {
                sanitize_text(preview)
            } else {
                "[Press Enter to load full content]".to_string()
            }
        } else {
            String::new()
        }
    };

    // Build styled text with visual quote bars for quoted lines
    let lines = render_quoted_text(&sanitized);
    let text = Text::from(lines);

    let scroll = state.reader.scroll.min(u16::MAX as usize) as u16;
    let paragraph = Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    frame.render_widget(paragraph, area);
}
