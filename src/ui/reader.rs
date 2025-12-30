use ratatui::{
    layout::{Constraint, Direction, Layout},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use super::app::AppState;
use super::theme::Theme;
use super::widgets::{
    enhanced_status_bar, error_bar, format_date, help_bar, sanitize_text, StatusInfo,
};

pub fn render_reader(frame: &mut Frame, state: &AppState, uid: u32) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Status bar
            Constraint::Length(5), // Headers
            Constraint::Min(0),    // Body
            Constraint::Length(1), // Help bar
        ])
        .split(frame.area());

    // Find the email
    let email = state.emails.iter().find(|e| e.uid == uid);

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
    };
    enhanced_status_bar(frame, chunks[0], &status_info);

    if let Some(email) = email {
        // Headers
        render_headers(frame, chunks[1], email);

        // Body
        render_body(frame, chunks[2], state, uid);
    } else {
        let paragraph = Paragraph::new("Email not found").style(Theme::error_bar());
        frame.render_widget(paragraph, chunks[1]);
    }

    // Help bar or error
    if let Some(ref error) = state.status.error {
        error_bar(frame, chunks[3], error);
    } else {
        // Dynamic hints based on AI summary mode
        let hints: &[(&str, &str)] = if state.reader.show_summary {
            &[
                ("T", "full"),
                ("j/k", "scroll"),
                ("r", "reply"),
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
        help_bar(frame, chunks[3], hints);
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

    let lines = vec![
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
        Line::from(vec![
            Span::styled("Date:    ", label_style),
            Span::styled(format_date(email.date), value_style),
        ]),
        Line::from(vec![
            Span::styled("Subject: ", label_style),
            Span::styled(&email.subject, Theme::text_unread()),
        ]),
    ];

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

fn render_body(frame: &mut Frame, area: ratatui::layout::Rect, state: &AppState, uid: u32) {
    let block = Block::default().borders(Borders::NONE);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let body_text: String = if state.reader.summary_loading {
        "Generating AI summary...".to_string()
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
        "Loading...".to_string()
    } else {
        "[No content]".to_string()
    };

    // Sanitize: remove ANSI sequences and control characters
    let sanitized = sanitize_text(&body_text);

    let text = Text::raw(sanitized);

    let paragraph = Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .scroll((state.reader.scroll as u16, 0));

    frame.render_widget(paragraph, inner);
}
