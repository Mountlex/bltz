use aho_corasick::AhoCorasick;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

use super::app::{AppState, MatchType};
use super::status_bar::spinner_char;
use super::theme::{Theme, symbols, with_selection_bg};
use super::widgets::{
    StatusInfo, enhanced_status_bar, error_bar, format_date, format_relative_date, help_bar,
    sanitize_text, truncate_string,
};
use crate::command::{CommandHelp, CommandResult};
use crate::constants::{
    MIN_SPLIT_VIEW_WIDTH, SCROLL_TARGET_FRACTION, SPLIT_RATIO_MAX, SPLIT_RATIO_MIN,
};
use crate::input::KeybindingEntry;
use crate::mail::EmailThread;
use crate::mail::types::EmailHeader;

/// Highlight query matches in text, returning multiple styled spans
/// Uses aho-corasick for efficient case-insensitive matching
fn highlight_matches(
    text: &str,
    query: &str,
    base_style: Style,
    highlight_style: Style,
) -> Vec<Span<'static>> {
    if query.is_empty() {
        return vec![Span::styled(text.to_string(), base_style)];
    }

    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();

    // Build aho-corasick automaton for case-insensitive matching
    let ac = match AhoCorasick::new([&query_lower]) {
        Ok(ac) => ac,
        Err(_) => return vec![Span::styled(text.to_string(), base_style)],
    };

    let mut spans = Vec::new();
    let mut last_end = 0;

    for mat in ac.find_iter(&text_lower) {
        // Add non-matching text before this match
        if mat.start() > last_end {
            spans.push(Span::styled(
                text[last_end..mat.start()].to_string(),
                base_style,
            ));
        }
        // Add highlighted match (use original case from text)
        spans.push(Span::styled(
            text[mat.start()..mat.end()].to_string(),
            highlight_style,
        ));
        last_end = mat.end();
    }

    // Add remaining text after last match
    if last_end < text.len() {
        spans.push(Span::styled(text[last_end..].to_string(), base_style));
    }

    if spans.is_empty() {
        vec![Span::styled(text.to_string(), base_style)]
    } else {
        spans
    }
}

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
    let visible_count = state.visible_threads().len();
    let folder_name = if state.folder.current.is_empty() {
        "INBOX"
    } else {
        &state.folder.current
    };

    let status_info = StatusInfo {
        folder: folder_name,
        unread: state.unread_count,
        total: state.total_count,
        connected: state.connection.connected,
        loading: state.status.loading,
        last_sync: state.connection.last_sync,
        account: if state.connection.account_name.is_empty() {
            "Not connected"
        } else {
            &state.connection.account_name
        },
        search_query: if state.search.query.is_empty() {
            None
        } else {
            Some(&state.search.query)
        },
        search_results: visible_count,
        status_message: if state.status.message.is_empty() {
            None
        } else {
            Some(&state.status.message)
        },
        other_accounts: &state.connection.other_accounts,
        starred_view: state.is_starred_view(),
    };
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

    // Help popup (rendered last so it appears on top)
    if let super::app::ModalState::Help {
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

fn render_command_bar(frame: &mut Frame, area: Rect, state: &AppState) {
    // Show confirmation prompt, result, or input
    if state.modal.pending_confirmation().is_some() {
        // Confirmation mode - show the prompt from command_result
        if let Some(CommandResult::Success(msg)) = state.modal.command_result() {
            let input = state.modal.command_input().unwrap_or("");
            let text = format!(" :{} {} ", input, msg);
            let paragraph = Paragraph::new(text).style(Theme::status_bar());
            frame.render_widget(paragraph, area);
        }
        return;
    }

    if let Some(result) = state.modal.command_result() {
        let (text, style) = match result {
            CommandResult::Success(msg) => (msg.clone(), Theme::text()),
            CommandResult::Error(msg) => (msg.clone(), Theme::error_bar()),
        };
        let paragraph = Paragraph::new(format!(" {} ", text)).style(style);
        frame.render_widget(paragraph, area);
        return;
    }

    // Normal input mode
    let cursor = if state.modal.is_command() { "│" } else { "" };
    let style = if state.modal.is_command() {
        Theme::status_bar()
    } else {
        Theme::text_muted()
    };

    let input = state.modal.command_input().unwrap_or("");
    let text = format!(" :{}{} ", input, cursor);
    let paragraph = Paragraph::new(text).style(style);
    frame.render_widget(paragraph, area);
}

fn render_thread_list(frame: &mut Frame, area: Rect, state: &AppState, show_border: bool) {
    let inner = if show_border {
        let block = Block::default()
            .borders(Borders::RIGHT)
            .border_style(Theme::border());
        let inner = block.inner(area);
        frame.render_widget(block, area);
        inner
    } else {
        area
    };

    let visible = state.visible_threads();

    if visible.is_empty() {
        let msg = if state.status.loading {
            "Loading emails..."
        } else if !state.search.query.is_empty() {
            "No matching emails. Press Esc to clear search."
        } else if state.is_starred_view() {
            "No starred emails. Press s on any email to star it."
        } else {
            "No emails. Press c to compose."
        };
        let paragraph = Paragraph::new(msg)
            .style(Theme::text_muted())
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(paragraph, inner);
        return;
    }

    // Build all visible items with their selection state
    let mut items: Vec<ListItem> = Vec::new();
    let mut selected_index: Option<usize> = None;

    for (thread_idx, thread) in visible.iter().enumerate() {
        let is_current_thread = thread_idx == state.thread.selected;
        let is_expanded = state.is_thread_expanded(&thread.id);

        if is_expanded {
            // Render thread header (collapsed style) as first item
            let is_header_selected = is_current_thread && state.thread.selected_in_thread == 0;
            if is_header_selected {
                selected_index = Some(items.len());
            }
            let latest_email = thread.latest(&state.emails);
            let match_type = state.get_match_type(latest_email.uid);
            let header_items = render_thread_header(
                thread,
                &state.emails,
                is_header_selected,
                inner.width,
                true,
                &state.search.query,
                match_type,
            );
            items.extend(header_items);

            // Render each email in thread
            for (email_idx, email) in thread.emails(&state.emails).enumerate() {
                let is_email_selected =
                    is_current_thread && state.thread.selected_in_thread == email_idx + 1;
                if is_email_selected {
                    selected_index = Some(items.len());
                }
                let email_match_type = state.get_match_type(email.uid);
                let email_items = render_thread_email(
                    email,
                    is_email_selected,
                    inner.width,
                    &state.search.query,
                    email_match_type,
                );
                items.extend(email_items);
            }
        } else {
            // Collapsed thread
            let is_selected = is_current_thread;
            if is_selected {
                selected_index = Some(items.len());
            }
            let latest_email = thread.latest(&state.emails);
            let match_type = state.get_match_type(latest_email.uid);
            let thread_items = render_thread_header(
                thread,
                &state.emails,
                is_selected,
                inner.width,
                false,
                &state.search.query,
                match_type,
            );
            items.extend(thread_items);
        }
    }

    // Virtual scrolling based on selected item
    let visible_lines = inner.height as usize;
    let total_items = items.len();

    // Calculate scroll offset to keep selected item visible
    // Since we can't persist scroll state, center selection in viewport
    let scroll_offset = if let Some(sel_idx) = selected_index {
        // Each thread/email takes 2 lines, so selected item spans sel_idx to sel_idx+1
        // Try to keep selection near the top third of the visible area for better context
        let target_position = visible_lines / SCROLL_TARGET_FRACTION;
        sel_idx.saturating_sub(target_position)
    } else {
        0
    };

    // Ensure we don't scroll past the end
    let scroll_offset = scroll_offset.min(total_items.saturating_sub(visible_lines));

    let end = (scroll_offset + visible_lines).min(total_items);
    let visible_items: Vec<ListItem> = items
        .into_iter()
        .skip(scroll_offset)
        .take(end.saturating_sub(scroll_offset))
        .collect();

    let list = List::new(visible_items);
    frame.render_widget(list, inner);
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

        // Helper to calculate lines needed for a field value
        let lines_for_field = |text: &str| -> u16 {
            if text.is_empty() || value_width == 0 {
                1
            } else {
                text.len().div_ceil(value_width).max(1) as u16
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

            (from_lines + to_lines + cc_lines + date_lines + subject_lines)
                .min(inner.height.saturating_sub(5))
        } else {
            // Collapsed: 1 line per field
            4 + if has_cc { 1 } else { 0 }
        };

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(header_lines + 1), // Headers + border
                Constraint::Min(0),                   // Body
            ])
            .split(inner);

        // Render headers
        render_email_headers(frame, sections[0], email, expanded);

        // Render body
        render_email_body(frame, sections[1], state);
    } else {
        let paragraph = Paragraph::new("No email selected")
            .style(Theme::text_muted())
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(paragraph, inner);
    }
}

fn render_email_headers(frame: &mut Frame, area: Rect, email: &EmailHeader, expanded: bool) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Theme::border());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let label_style = Theme::label();
    let value_style = Theme::text();

    let from_display = if let Some(ref name) = email.from_name {
        format!("{} <{}>", name, email.from_addr)
    } else {
        email.from_addr.clone()
    };

    let mut lines: Vec<Line> = Vec::new();

    // From
    lines.push(Line::from(vec![
        Span::styled("From:    ", label_style),
        Span::styled(from_display, value_style),
    ]));

    // To
    lines.push(Line::from(vec![
        Span::styled("To:      ", label_style),
        Span::styled(email.to_addr.as_deref().unwrap_or(""), value_style),
    ]));

    // CC (if present and non-empty)
    if let Some(ref cc) = email.cc_addr
        && !cc.trim().is_empty()
    {
        lines.push(Line::from(vec![
            Span::styled("Cc:      ", label_style),
            Span::styled(cc.as_str(), value_style),
        ]));
    }

    // Date
    lines.push(Line::from(vec![
        Span::styled("Date:    ", label_style),
        Span::styled(format_date(email.date), value_style),
    ]));

    // Subject
    lines.push(Line::from(vec![
        Span::styled("Subject: ", label_style),
        Span::styled(&email.subject, Theme::text_unread()),
    ]));

    // Use Paragraph with Wrap when expanded to allow natural wrapping
    let paragraph = if expanded {
        Paragraph::new(lines).wrap(Wrap { trim: false })
    } else {
        Paragraph::new(lines)
    };

    frame.render_widget(paragraph, inner);
}

fn render_email_body(frame: &mut Frame, area: Rect, state: &AppState) {
    // Clear the area first to prevent rendering artifacts when content changes
    frame.render_widget(Clear, area);

    let body_text: String = if let Some(ref body) = state.reader.body {
        body.display_text()
    } else if state.status.loading {
        format!("{} Loading...", spinner_char())
    } else {
        // Show preview if body not loaded
        if let Some(email) = state.current_email_from_thread() {
            if let Some(ref preview) = email.preview {
                preview.clone()
            } else {
                "[Press Enter to load full content]".to_string()
            }
        } else {
            String::new()
        }
    };

    // Sanitize text: remove ANSI sequences and control characters
    let sanitized = sanitize_text(&body_text);

    let text = Text::raw(sanitized);

    let paragraph = Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .scroll((state.reader.scroll as u16, 0));

    frame.render_widget(paragraph, area);
}

/// Render a collapsed thread header (2 lines)
fn render_thread_header(
    thread: &EmailThread,
    emails: &[EmailHeader],
    selected: bool,
    width: u16,
    expanded: bool,
    search_query: &str,
    match_type: MatchType,
) -> Vec<ListItem<'static>> {
    let email = thread.latest(emails);
    let width = width as usize;

    // Line 1: ▶ Alice Smith                                    Dec 27
    let from = email.display_from();
    let date = format_relative_date(thread.latest_date);

    let date_width = date.len().max(10);
    let indicator_width = 2; // "▶ " or "▼ " or "  "
    let from_width = width.saturating_sub(indicator_width + date_width + 1);

    let from_display = truncate_string(from, from_width);
    let padding = from_width.saturating_sub(from_display.len());

    // Base style - used for padding to ensure full row highlight
    let base_style = if selected {
        Theme::selected()
    } else {
        Style::default()
    };

    // From name style - bold if unread, dimmed if read
    let from_style = if selected {
        if thread.has_unread() {
            Theme::selected_bold()
        } else {
            Theme::selected()
        }
    } else if thread.has_unread() {
        Theme::text_unread()
    } else {
        Theme::text_secondary()
    };

    // Highlight style for search matches - add bold to base from_style
    let from_highlight_style = from_style.add_modifier(Modifier::BOLD);

    let expand_indicator = if thread.total_count > 1 {
        if expanded {
            symbols::THREAD_EXPANDED
        } else {
            symbols::THREAD_COLLAPSED
        }
    } else {
        symbols::THREAD_SINGLE
    };

    // All spans must inherit selection background when selected
    let indicator_style = with_selection_bg(Theme::text_muted(), selected);
    let date_style = with_selection_bg(Theme::text_muted(), selected);

    // Build from display with highlighting
    let from_spans = highlight_matches(
        &from_display,
        search_query,
        from_style,
        from_highlight_style,
    );

    let mut line1_spans = vec![Span::styled(expand_indicator, indicator_style)];
    line1_spans.extend(from_spans);
    line1_spans.push(Span::styled(" ".repeat(padding), base_style));
    line1_spans.push(Span::styled(
        format!("{:>width$}", date, width = date_width),
        date_style,
    ));

    let line1 = Line::from(line1_spans);

    // Line 2: ●+★ Subject...                              [3]
    let unread_indicator = if thread.has_unread() {
        symbols::UNREAD
    } else {
        symbols::READ
    };
    let attachment_indicator = if thread.has_attachments {
        symbols::ATTACHMENT
    } else {
        symbols::NO_ATTACHMENT
    };
    let has_starred = thread.emails(emails).any(|e| e.is_flagged());
    let star_indicator = if has_starred { symbols::STARRED } else { " " };

    let badge = if thread.total_count > 1 {
        format!("[{}]", thread.total_count)
    } else {
        String::new()
    };
    let badge_width = badge.len();

    // Indent: 2 (indicator) + 1 (unread) + 1 (attach) + 1 (star) + 1 (space)
    let indent = 6;
    let subject_width = width.saturating_sub(indent + badge_width + 1);

    let _subject_display = truncate_string(&email.subject, subject_width);
    let _subject_padding = subject_width.saturating_sub(_subject_display.len());

    // Unread indicator - cyan when unread, must have selection bg when selected
    let unread_style = if selected {
        if thread.has_unread() {
            Theme::unread_indicator_selected()
        } else {
            Theme::selected()
        }
    } else if thread.has_unread() {
        Theme::unread_indicator()
    } else {
        Theme::text_muted()
    };

    // Attachment indicator - must have selection bg when selected
    let attach_style = with_selection_bg(
        if thread.has_attachments {
            Theme::text()
        } else {
            Style::default()
        },
        selected,
    );

    // Star indicator - yellow when starred, must have selection bg when selected
    let star_style = if selected {
        if has_starred {
            Theme::star_indicator_selected()
        } else {
            Theme::selected()
        }
    } else if has_starred {
        Theme::star_indicator()
    } else {
        Style::default()
    };

    // Subject style
    let subject_style = if selected {
        Theme::selected()
    } else if thread.has_unread() {
        Theme::text()
    } else {
        Theme::text_muted()
    };

    // Highlight style for search matches - add bold to base subject_style
    let subject_highlight_style = subject_style.add_modifier(Modifier::BOLD);

    // Badge style - inherit selection bg when selected
    let badge_style = with_selection_bg(Theme::thread_badge(), selected);

    // [body] indicator for body-only matches
    let body_indicator = matches!(match_type, MatchType::Body);
    let body_indicator_str = if body_indicator { " [body]" } else { "" };
    let body_indicator_width = body_indicator_str.len();

    // Recalculate subject width accounting for [body] indicator
    let actual_subject_width = subject_width.saturating_sub(body_indicator_width);
    let actual_subject_display = truncate_string(&email.subject, actual_subject_width);
    let actual_subject_padding = actual_subject_width.saturating_sub(actual_subject_display.len());

    // Build subject with highlighting
    let subject_spans = highlight_matches(
        &actual_subject_display,
        search_query,
        subject_style,
        subject_highlight_style,
    );

    let mut line2_spans = vec![
        Span::styled("  ", base_style), // indent to match indicator
        Span::styled(unread_indicator, unread_style),
        Span::styled(attachment_indicator, attach_style),
        Span::styled(star_indicator, star_style),
        Span::styled(" ", base_style),
    ];
    line2_spans.extend(subject_spans);

    // Add [body] indicator if this is a body-only match
    if body_indicator {
        let body_style = with_selection_bg(Style::default().fg(Color::DarkGray), selected);
        line2_spans.push(Span::styled(body_indicator_str, body_style));
    }

    line2_spans.push(Span::styled(" ".repeat(actual_subject_padding), base_style));
    line2_spans.push(Span::styled(badge, badge_style));

    let line2 = Line::from(line2_spans);

    vec![ListItem::new(line1), ListItem::new(line2)]
}

/// Render an individual email within an expanded thread (2 lines, indented)
fn render_thread_email(
    email: &EmailHeader,
    selected: bool,
    width: u16,
    search_query: &str,
    match_type: MatchType,
) -> Vec<ListItem<'static>> {
    let width = width as usize;

    let from = email.display_from();
    let date = format_relative_date(email.date);

    let indent_width = 4; // extra indent for thread children
    let date_width = date.len().max(10);
    let from_width = width.saturating_sub(indent_width + date_width + 1);

    let from_display = truncate_string(from, from_width);
    let padding = from_width.saturating_sub(from_display.len());

    // Base style - used for padding to ensure full row highlight
    let base_style = if selected {
        Theme::selected()
    } else {
        Style::default()
    };

    // From name style - bold if unread, dimmed if read
    let from_style = if selected {
        if !email.is_seen() {
            Theme::selected_bold()
        } else {
            Theme::selected()
        }
    } else if !email.is_seen() {
        Theme::text_unread()
    } else {
        Theme::text_secondary()
    };

    // Highlight style for search matches - add bold to base from_style
    let from_highlight_style = from_style.add_modifier(Modifier::BOLD);

    // Date style - must have selection bg when selected
    let date_style = with_selection_bg(Theme::text_muted(), selected);

    // Build from display with highlighting
    let from_spans = highlight_matches(
        &from_display,
        search_query,
        from_style,
        from_highlight_style,
    );

    let mut line1_spans = vec![Span::styled(
        symbols::THREAD_CHILD,
        with_selection_bg(Theme::border(), selected),
    )];
    line1_spans.extend(from_spans);
    line1_spans.push(Span::styled(" ".repeat(padding), base_style));
    line1_spans.push(Span::styled(
        format!("{:>width$}", date, width = date_width),
        date_style,
    ));

    let line1 = Line::from(line1_spans);

    // Line 2: subject
    let unread_indicator = if !email.is_seen() {
        symbols::UNREAD
    } else {
        symbols::READ
    };
    let attachment_indicator = if email.has_attachments {
        symbols::ATTACHMENT
    } else {
        symbols::NO_ATTACHMENT
    };
    let is_starred = email.is_flagged();
    let star_indicator = if is_starred { symbols::STARRED } else { " " };
    let is_replied = email.is_answered();
    let replied_indicator = if is_replied { symbols::REPLIED } else { " " };

    let inner_indent = indent_width + 5; // indent + unread + attach + star + replied + space
    let subject_width = width.saturating_sub(inner_indent);

    let _subject_display = truncate_string(&email.subject, subject_width);
    let _subject_padding = subject_width.saturating_sub(_subject_display.len());

    // Unread indicator - cyan when unread, must have selection bg when selected
    let unread_style = if selected {
        if !email.is_seen() {
            Theme::unread_indicator_selected()
        } else {
            Theme::selected()
        }
    } else if !email.is_seen() {
        Theme::unread_indicator()
    } else {
        Theme::text_muted()
    };

    // Attachment indicator - must have selection bg when selected
    let attach_style = with_selection_bg(
        if email.has_attachments {
            Theme::text()
        } else {
            Style::default()
        },
        selected,
    );

    // Star indicator - yellow when starred, must have selection bg when selected
    let star_style = if selected {
        if is_starred {
            Theme::star_indicator_selected()
        } else {
            Theme::selected()
        }
    } else if is_starred {
        Theme::star_indicator()
    } else {
        Style::default()
    };

    // Replied indicator - muted when replied, must have selection bg when selected
    let replied_style = if selected {
        if is_replied {
            Theme::replied_indicator_selected()
        } else {
            Theme::selected()
        }
    } else if is_replied {
        Theme::replied_indicator()
    } else {
        Style::default()
    };

    // Subject style
    let subject_style = if selected {
        Theme::selected()
    } else if !email.is_seen() {
        Theme::text()
    } else {
        Theme::text_muted()
    };

    // Highlight style for search matches - add bold to base subject_style
    let subject_highlight_style = subject_style.add_modifier(Modifier::BOLD);

    // [body] indicator for body-only matches
    let body_indicator = matches!(match_type, MatchType::Body);
    let body_indicator_str = if body_indicator { " [body]" } else { "" };
    let body_indicator_width = body_indicator_str.len();

    // Recalculate subject width accounting for [body] indicator
    let actual_subject_width = subject_width.saturating_sub(body_indicator_width);
    let actual_subject_display = truncate_string(&email.subject, actual_subject_width);
    let actual_subject_padding = actual_subject_width.saturating_sub(actual_subject_display.len());

    // Build subject with highlighting
    let subject_spans = highlight_matches(
        &actual_subject_display,
        search_query,
        subject_style,
        subject_highlight_style,
    );

    let mut line2_spans = vec![
        Span::styled(
            symbols::THREAD_CHILD,
            with_selection_bg(Theme::border(), selected),
        ),
        Span::styled(unread_indicator, unread_style),
        Span::styled(attachment_indicator, attach_style),
        Span::styled(star_indicator, star_style),
        Span::styled(replied_indicator, replied_style),
        Span::styled(" ", base_style),
    ];
    line2_spans.extend(subject_spans);

    // Add [body] indicator if this is a body-only match
    if body_indicator {
        let body_style = with_selection_bg(Style::default().fg(Color::DarkGray), selected);
        line2_spans.push(Span::styled(body_indicator_str, body_style));
    }

    line2_spans.push(Span::styled(" ".repeat(actual_subject_padding), base_style)); // fill to full width

    let line2 = Line::from(line2_spans);

    vec![ListItem::new(line1), ListItem::new(line2)]
}

/// Render a folder picker overlay
fn render_folder_picker(frame: &mut Frame, area: Rect, state: &AppState) {
    // Calculate popup size and position (min 10 chars wide for usability)
    let popup_width = 30.min(area.width.saturating_sub(4)).max(10);
    let popup_height = (state.folder.list.len() as u16 + 2)
        .min(area.height.saturating_sub(4))
        .max(3);

    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear the area behind the popup
    frame.render_widget(Clear, popup_area);

    // Create the popup block
    let block = Block::default()
        .title(" Folders ")
        .borders(Borders::ALL)
        .border_style(Theme::border_focused());

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    if state.folder.list.is_empty() {
        let msg = if state.status.loading {
            "Loading..."
        } else {
            "No folders"
        };
        let paragraph = Paragraph::new(msg)
            .style(Theme::text_muted())
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(paragraph, inner);
        return;
    }

    // Build folder list items
    let items: Vec<ListItem> = state
        .folder
        .list
        .iter()
        .enumerate()
        .map(|(idx, folder)| {
            let is_selected = idx == state.folder.selected;
            let is_current = folder == &state.folder.current;

            let style = if is_selected {
                Theme::selected()
            } else if is_current {
                Theme::text_accent().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let prefix = if is_current {
                format!("{} ", symbols::CURRENT_FOLDER)
            } else {
                "  ".to_string()
            };
            ListItem::new(format!("{}{}", prefix, folder)).style(style)
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, inner);
}

/// Render the unified help popup (keybindings + commands)
fn render_unified_help_popup(
    frame: &mut Frame,
    area: Rect,
    keys: &[KeybindingEntry],
    commands: &[CommandHelp],
    scroll: usize,
) {
    // Count unique categories to calculate height
    let mut categories: Vec<&str> = Vec::new();
    for key in keys {
        if categories.last() != Some(&key.category) {
            categories.push(key.category);
        }
    }
    let keybinding_category_count = categories.len();

    // Calculate popup size - keybindings + commands section
    let keybinding_lines = keys.len() + keybinding_category_count * 2;
    let command_lines = commands.len() + 2; // header + blank line + entries
    let content_height = keybinding_lines + command_lines + 1; // +1 for blank line separator

    let popup_width = 50.min(area.width.saturating_sub(4)).max(36);
    let popup_height = (content_height as u16 + 2)
        .min(area.height.saturating_sub(4))
        .max(10);

    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear the area behind the popup
    frame.render_widget(Clear, popup_area);

    // Create the popup block
    let block = Block::default()
        .title(" Help ")
        .title_bottom(" j/k scroll │ . or Esc close ")
        .borders(Borders::ALL)
        .border_style(Theme::border_focused());

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    // Build combined list items
    let mut items: Vec<ListItem> = Vec::new();
    let mut current_category: Option<&str> = None;
    let key_width = 12;

    // Add keybindings grouped by category
    for entry in keys {
        // Add category header if this is a new category
        if current_category != Some(entry.category) {
            // Add blank line before category (except first)
            if current_category.is_some() {
                items.push(ListItem::new(Line::from("")));
            }

            // Category header
            let header_line = Line::from(vec![
                Span::styled(
                    format!("── {} ", entry.category),
                    Theme::text_secondary().add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "─"
                        .repeat(
                            inner.width.saturating_sub(entry.category.len() as u16 + 4) as usize
                        ),
                    Theme::border(),
                ),
            ]);
            items.push(ListItem::new(header_line));
            current_category = Some(entry.category);
        }

        // Keybinding entry
        let char_count = entry.key.chars().count();
        let key_display = if char_count > key_width {
            entry.key.chars().take(key_width).collect::<String>()
        } else {
            format!("{:width$}", entry.key, width = key_width)
        };

        let line = Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(key_display, Theme::text_accent()),
            Span::styled(&entry.description, Theme::text()),
        ]);

        items.push(ListItem::new(line));
    }

    // Add commands section
    items.push(ListItem::new(Line::from("")));
    let commands_header = Line::from(vec![
        Span::styled(
            "── Commands ",
            Theme::text_secondary().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "─".repeat(inner.width.saturating_sub(13) as usize),
            Theme::border(),
        ),
    ]);
    items.push(ListItem::new(commands_header));

    let cmd_width = 14;
    for cmd in commands {
        let cmd_display = format!(":{:<width$}", cmd.name, width = cmd_width - 1);
        let line = Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(cmd_display, Theme::text_accent()),
            Span::styled(cmd.description, Theme::text()),
        ]);
        items.push(ListItem::new(line));
    }

    // Apply scroll offset
    let visible_items: Vec<ListItem> = items.into_iter().skip(scroll).collect();

    let list = List::new(visible_items);
    frame.render_widget(list, inner);
}
