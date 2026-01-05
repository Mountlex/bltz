//! Thread list rendering with virtual scrolling.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::app::state::{AppState, MatchType};
use crate::constants::SCROLL_TARGET_FRACTION;
use crate::mail::EmailThread;
use crate::mail::types::EmailHeader;

use super::super::theme::{Theme, symbols, with_selection_bg};
use super::super::widgets::{display_width, format_relative_date, truncate_string};
use super::format::highlight_matches;

pub fn render_thread_list(frame: &mut Frame, area: Rect, state: &AppState, show_border: bool) {
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

    let visible_lines = inner.height as usize;

    // Phase 1: Calculate line counts and find selected position (no allocations)
    // Each thread/email header takes 2 lines
    let mut total_lines = 0usize;
    let mut selected_line = 0usize;
    let mut thread_line_offsets: Vec<usize> = Vec::with_capacity(visible.len());

    for (thread_idx, thread) in visible.iter().enumerate() {
        thread_line_offsets.push(total_lines);
        let is_current_thread = thread_idx == state.thread.selected;
        let is_expanded = state.is_thread_expanded(&thread.id);

        if is_expanded {
            // Header takes 2 lines
            if is_current_thread && state.thread.selected_in_thread == 0 {
                selected_line = total_lines;
            }
            total_lines += 2;

            // Each email takes 2 lines
            let email_count = thread.email_indices.len();
            if is_current_thread && state.thread.selected_in_thread > 0 {
                // selected_in_thread is 1-indexed for emails in expanded thread
                let email_offset = state.thread.selected_in_thread.saturating_sub(1);
                selected_line = total_lines + (email_offset * 2);
            }
            total_lines += email_count * 2;
        } else {
            // Collapsed thread takes 2 lines
            if is_current_thread {
                selected_line = total_lines;
            }
            total_lines += 2;
        }
    }

    // Phase 2: Calculate scroll offset based on selection
    let target_position = visible_lines / SCROLL_TARGET_FRACTION;
    let scroll_offset = selected_line.saturating_sub(target_position);
    let scroll_offset = scroll_offset.min(total_lines.saturating_sub(visible_lines));
    let scroll_end = scroll_offset + visible_lines;

    // Phase 3: Find which threads fall in the visible range
    let first_visible_thread = thread_line_offsets
        .iter()
        .rposition(|&offset| offset <= scroll_offset)
        .unwrap_or(0);

    // Phase 4: Build items ONLY for visible threads
    let mut items: Vec<ListItem> = Vec::with_capacity(visible_lines + 4);
    let mut current_line = thread_line_offsets[first_visible_thread];
    let mut first_added_line: Option<usize> = None;

    for (thread_idx, thread) in visible.iter().enumerate().skip(first_visible_thread) {
        // Stop if we've gone past the visible area
        if current_line >= scroll_end {
            break;
        }

        let is_current_thread = thread_idx == state.thread.selected;
        let is_expanded = state.is_thread_expanded(&thread.id);

        if is_expanded {
            // Render thread header
            let is_header_selected = is_current_thread && state.thread.selected_in_thread == 0;
            let latest_email = thread.latest(&state.emails);
            let match_type = state.get_match_type(latest_email.uid);

            // Only render if visible
            if current_line + 2 > scroll_offset {
                if first_added_line.is_none() {
                    first_added_line = Some(current_line);
                }
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
            }
            current_line += 2;

            // Render each email in thread
            let email_count = thread.email_indices.len();
            for (email_idx, email) in thread.emails(&state.emails).enumerate() {
                if current_line >= scroll_end {
                    break;
                }
                if current_line + 2 > scroll_offset {
                    if first_added_line.is_none() {
                        first_added_line = Some(current_line);
                    }
                    let is_email_selected =
                        is_current_thread && state.thread.selected_in_thread == email_idx + 1;
                    let email_match_type = state.get_match_type(email.uid);
                    let is_last = email_idx == email_count - 1;
                    let email_items = render_thread_email(
                        email,
                        is_email_selected,
                        inner.width,
                        &state.search.query,
                        email_match_type,
                        is_last,
                    );
                    items.extend(email_items);
                }
                current_line += 2;
            }
        } else {
            // Collapsed thread - 2 lines
            if current_line + 2 > scroll_offset {
                if first_added_line.is_none() {
                    first_added_line = Some(current_line);
                }
                let is_selected = is_current_thread;
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
            current_line += 2;
        }
    }

    // Skip items before scroll_offset (for partially visible threads at top)
    // Use first_added_line instead of thread start to avoid over-skipping when
    // a thread's header is above the viewport but its emails are visible
    let skip_lines = first_added_line
        .map(|first| scroll_offset.saturating_sub(first))
        .unwrap_or(0);
    let visible_items: Vec<ListItem> = items
        .into_iter()
        .skip(skip_lines)
        .take(visible_lines)
        .collect();

    let list = List::new(visible_items);
    frame.render_widget(list, inner);
}

/// Render a collapsed thread header (2 lines)
pub fn render_thread_header(
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
    // Show "→ [recipient]" for sent emails, otherwise show sender
    let from = if email.is_sent() {
        let recipient = email.to_addr.as_deref().unwrap_or("(unknown)");
        let first_recipient = recipient.split(',').next().unwrap_or(recipient).trim();
        format!("→ {}", first_recipient)
    } else {
        email.display_from().to_string()
    };
    let date = format_relative_date(thread.latest_date);

    let date_width = date.len().max(10);
    let indicator_width = 2; // "▶ " or "▼ " or "  "
    let from_width = width.saturating_sub(indicator_width + date_width + 1);

    let from_display = truncate_string(&from, from_width);
    let padding = from_width.saturating_sub(display_width(&from_display));

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

    // Indent: 2 (indicator) + 1 (unread) + 1 (attach) + 1 (star) + 1 (space) + 1 (padding)
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

    // Subject style - bold for unread, muted for read
    let subject_style = if selected {
        if thread.has_unread() {
            Theme::selected_bold()
        } else {
            Theme::selected()
        }
    } else if thread.has_unread() {
        Theme::text_unread()
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
    let actual_subject_padding =
        actual_subject_width.saturating_sub(display_width(&actual_subject_display));

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
pub fn render_thread_email(
    email: &EmailHeader,
    selected: bool,
    width: u16,
    search_query: &str,
    match_type: MatchType,
    is_last: bool,
) -> Vec<ListItem<'static>> {
    let width = width as usize;

    // Show "→ [recipient]" for sent emails, otherwise show sender
    let from = if email.is_sent() {
        let recipient = email.to_addr.as_deref().unwrap_or("(unknown)");
        // Extract first recipient name if multiple
        let first_recipient = recipient.split(',').next().unwrap_or(recipient).trim();
        format!("→ {}", first_recipient)
    } else {
        email.display_from().to_string()
    };
    let date = format_relative_date(email.date);

    let indent_width = 4; // extra indent for thread children
    let date_width = date.len().max(10);
    let from_width = width.saturating_sub(indent_width + date_width + 1);

    let from_display = truncate_string(&from, from_width);
    let padding = from_width.saturating_sub(display_width(&from_display));

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

    // Use tree connector: ├─ for middle items, └─ for last item
    let tree_connector = if is_last {
        symbols::THREAD_CHILD_LAST
    } else {
        symbols::THREAD_CHILD_MID
    };
    let mut line1_spans = vec![Span::styled(
        tree_connector,
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
    let subject_width = width.saturating_sub(inner_indent + 1); // +1 for padding before divider

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

    // Subject style - bold for unread, muted for read
    let subject_style = if selected {
        if !email.is_seen() {
            Theme::selected_bold()
        } else {
            Theme::selected()
        }
    } else if !email.is_seen() {
        Theme::text_unread()
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
    let actual_subject_padding =
        actual_subject_width.saturating_sub(display_width(&actual_subject_display));

    // Build subject with highlighting
    let subject_spans = highlight_matches(
        &actual_subject_display,
        search_query,
        subject_style,
        subject_highlight_style,
    );

    // For line 2: use continuation │ for non-last, spaces for last
    let line2_prefix = if is_last {
        "    "
    } else {
        symbols::THREAD_CHILD
    };
    let mut line2_spans = vec![
        Span::styled(line2_prefix, with_selection_bg(Theme::border(), selected)),
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
