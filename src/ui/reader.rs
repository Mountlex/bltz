use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::Modifier,
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use super::theme::Theme;
use super::widgets::{
    StatusInfo, enhanced_status_bar, error_bar, format_date, help_bar, sanitize_text, spinner_char,
};
use crate::app::state::AppState;
use crate::mail::types::Attachment;

pub fn render_reader(frame: &mut Frame, state: &AppState, uid: u32) {
    // Find the email first to determine header height
    let email = state.emails.iter().find(|e| e.uid == uid);
    // Check if CC has actual content (not empty)
    let has_cc = email
        .and_then(|e| e.cc_addr.as_ref())
        .is_some_and(|cc| !cc.trim().is_empty());
    let header_lines = if has_cc { 6 } else { 5 };

    // Determine attachment panel height
    let show_attachments = state.reader.show_attachments && !state.reader.attachments.is_empty();
    let attachment_height = if show_attachments {
        // Show up to 5 attachments + 1 line for title/border
        (state.reader.attachments.len().min(5) + 2) as u16
    } else if email.is_some_and(|e| e.has_attachments) {
        // Show attachment indicator line
        1
    } else {
        0
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),                 // Status bar
            Constraint::Length(header_lines),      // Headers
            Constraint::Length(attachment_height), // Attachments (if any)
            Constraint::Min(0),                    // Body
            Constraint::Length(1),                 // Help bar
        ])
        .split(frame.area());

    // Enhanced status bar (same as inbox)
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
        search_query: None, // Reader doesn't show search
        search_results: 0,
        status_message: if state.status.message.is_empty() {
            None
        } else {
            Some(&state.status.message)
        },
        other_accounts: &state.connection.other_accounts,
        starred_view: state.is_starred_view(),
        conversation_mode: state.conversation_mode,
    };
    enhanced_status_bar(frame, chunks[0], &status_info);

    if let Some(email) = email {
        // Headers
        render_headers(frame, chunks[1], email);

        // Attachments
        if show_attachments {
            render_attachments(
                frame,
                chunks[2],
                &state.reader.attachments,
                state.reader.attachment_selected,
                true,
            );
        } else if email.has_attachments {
            render_attachment_indicator(frame, chunks[2], state.reader.attachments.len());
        }

        // Body
        render_body(frame, chunks[3], state, uid);
    } else {
        let paragraph = Paragraph::new("Email not found").style(Theme::error_bar());
        frame.render_widget(paragraph, chunks[1]);
    }

    // Help bar or error
    if let Some(ref error) = state.status.error {
        error_bar(frame, chunks[4], error);
    } else {
        // Dynamic hints based on context
        let hints: &[(&str, &str)] = if state.reader.show_attachments {
            // Attachment mode hints
            &[
                ("j/k", "select"),
                ("Enter", "open"),
                ("s", "save"),
                ("Esc", "back"),
            ]
        } else if state.reader.show_summary {
            &[
                ("T", "full"),
                ("j/k", "scroll"),
                ("r", "reply"),
                ("Esc", "back"),
            ]
        } else if email.is_some_and(|e| e.has_attachments) {
            &[
                ("A", "attach"),
                ("j/k", "scroll"),
                ("r", "reply"),
                ("f", "fwd"),
                ("Esc", "back"),
            ]
        } else {
            &[
                ("T", "summary"),
                ("j/k", "scroll"),
                ("r", "reply"),
                ("f", "fwd"),
                ("d", "del"),
                ("Esc", "back"),
            ]
        };
        help_bar(frame, chunks[4], hints);
    }
}

fn render_headers(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    email: &crate::mail::types::EmailHeader,
) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Theme::border());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let label_style = Theme::label();
    let value_style = Theme::text();

    let mut lines = vec![
        Line::from(vec![
            Span::styled("From:    ", label_style),
            Span::styled(
                format!(
                    "{} <{}>",
                    email.from_name.as_deref().unwrap_or(""),
                    email.from_addr
                ),
                value_style,
            ),
        ]),
        Line::from(vec![
            Span::styled("To:      ", label_style),
            Span::styled(email.to_addr.as_deref().unwrap_or(""), value_style),
        ]),
    ];

    // Add CC line if present and non-empty
    if let Some(ref cc) = email.cc_addr
        && !cc.trim().is_empty()
    {
        lines.push(Line::from(vec![
            Span::styled("Cc:      ", label_style),
            Span::styled(cc.as_str(), value_style),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled("Date:    ", label_style),
        Span::styled(format_date(email.date), value_style),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Subject: ", label_style),
        Span::styled(&email.subject, Theme::text_unread()),
    ]));

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

fn render_attachment_indicator(frame: &mut Frame, area: ratatui::layout::Rect, count: usize) {
    let text = if count > 0 {
        format!(" {} {} - press A to view", "📎", count)
    } else {
        " 📎 Has attachments - press A to load".to_string()
    };

    let paragraph = Paragraph::new(text).style(Theme::text_muted());
    frame.render_widget(paragraph, area);
}

fn render_attachments(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    attachments: &[Attachment],
    selected: usize,
    focused: bool,
) {
    let block = Block::default()
        .title(" Attachments ")
        .borders(Borders::TOP)
        .border_style(if focused {
            Theme::border_focused()
        } else {
            Theme::border()
        });

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let items: Vec<ListItem> = attachments
        .iter()
        .enumerate()
        .map(|(i, att)| {
            let line = format!("{} {} ({})", att.icon(), att.filename, att.formatted_size());

            let style = if focused && i == selected {
                Theme::selected().add_modifier(Modifier::BOLD)
            } else {
                Theme::text()
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, inner);
}

fn render_body(frame: &mut Frame, area: ratatui::layout::Rect, state: &AppState, uid: u32) {
    let block = Block::default().borders(Borders::NONE);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let body_text: String = if state.reader.summary_loading {
        format!("{} Generating AI summary...", spinner_char())
    } else if state.reader.show_summary {
        // Show AI summary if available
        if let Some((cached_uid, ref summary)) = state.reader.cached_summary {
            if cached_uid == uid {
                format!("[AI Summary]\n\n{}", summary)
            } else {
                // Show thread summary if available
                if let Some((ref thread_id, ref summary)) = state.reader.cached_thread_summary {
                    if state
                        .current_thread()
                        .map(|t| &t.id == thread_id)
                        .unwrap_or(false)
                    {
                        format!("[AI Thread Summary]\n\n{}", summary)
                    } else {
                        "[Press T to generate summary]".to_string()
                    }
                } else {
                    "[Press T to generate summary]".to_string()
                }
            }
        } else if let Some((ref thread_id, ref summary)) = state.reader.cached_thread_summary {
            if state
                .current_thread()
                .map(|t| &t.id == thread_id)
                .unwrap_or(false)
            {
                format!("[AI Thread Summary]\n\n{}", summary)
            } else {
                "[Press T to generate summary]".to_string()
            }
        } else {
            "[Press T to generate summary]".to_string()
        }
    } else if let Some(ref body) = state.reader.body {
        body.display_text()
    } else if state.status.loading {
        format!("{} Loading...", spinner_char())
    } else {
        "[No content]".to_string()
    };

    // Sanitize: remove ANSI sequences and control characters
    let sanitized = sanitize_text(&body_text);

    // Build styled text with dimmed quoted lines
    let lines: Vec<Line> = sanitized
        .lines()
        .map(|line| {
            if line.trim_start().starts_with('>') {
                Line::styled(line, Theme::text_muted())
            } else {
                Line::raw(line)
            }
        })
        .collect();

    let text = Text::from(lines);

    let paragraph = Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .scroll((state.reader.scroll as u16, 0));

    frame.render_widget(paragraph, inner);
}
