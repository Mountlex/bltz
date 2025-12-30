use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

use super::app::{AppState, ComposerField, PolishPreview};
use super::theme::Theme;
use super::widgets::{error_bar, help_bar, status_bar};
use crate::mail::types::ComposeEmail;

/// Composer layout areas computed based on account count
struct ComposerLayout {
    status_area: Rect,
    from_area: Option<Rect>,
    to_area: Rect,
    cc_area: Rect,
    subject_area: Rect,
    body_area: Rect,
    help_area: Rect,
}

fn compute_layout(area: Rect, has_multiple_accounts: bool) -> ComposerLayout {
    if has_multiple_accounts {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Status bar
                Constraint::Length(3), // From field
                Constraint::Length(3), // To field
                Constraint::Length(3), // Cc field
                Constraint::Length(3), // Subject field
                Constraint::Min(0),    // Body
                Constraint::Length(1), // Help bar
            ])
            .split(area);

        ComposerLayout {
            status_area: chunks[0],
            from_area: Some(chunks[1]),
            to_area: chunks[2],
            cc_area: chunks[3],
            subject_area: chunks[4],
            body_area: chunks[5],
            help_area: chunks[6],
        }
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Status bar
                Constraint::Length(3), // To field
                Constraint::Length(3), // Cc field
                Constraint::Length(3), // Subject field
                Constraint::Min(0),    // Body
                Constraint::Length(1), // Help bar
            ])
            .split(area);

        ComposerLayout {
            status_area: chunks[0],
            from_area: None,
            to_area: chunks[1],
            cc_area: chunks[2],
            subject_area: chunks[3],
            body_area: chunks[4],
            help_area: chunks[5],
        }
    }
}

pub fn render_composer(
    frame: &mut Frame,
    state: &AppState,
    email: &ComposeEmail,
    field: ComposerField,
) {
    let has_multiple_accounts = state.connection.account_names.len() > 1;
    let layout = compute_layout(frame.area(), has_multiple_accounts);

    // Status bar
    let status = if email.in_reply_to.is_some() {
        "Reply"
    } else if email.subject.to_lowercase().starts_with("fwd:") {
        "Forward"
    } else {
        "New Email"
    };
    status_bar(frame, layout.status_area, status, "");

    // From field (multi-account only)
    if let Some(from_area) = layout.from_area {
        let from_account_index = email
            .from_account_index
            .unwrap_or(state.connection.account_index);
        let from_account_name = state
            .connection
            .account_names
            .get(from_account_index)
            .map(|s| s.as_str())
            .unwrap_or("Unknown");
        render_from_field(frame, from_area, from_account_name);
    }

    // To field
    render_field(
        frame,
        layout.to_area,
        "To",
        &email.to,
        field == ComposerField::To,
    );

    // Cc field
    render_field(
        frame,
        layout.cc_area,
        "Cc",
        &email.cc,
        field == ComposerField::Cc,
    );

    // Subject field
    render_field(
        frame,
        layout.subject_area,
        "Subject",
        &email.subject,
        field == ComposerField::Subject,
    );

    // Body
    render_body_field(
        frame,
        layout.body_area,
        &email.body,
        field == ComposerField::Body,
    );

    // Help bar or error
    if let Some(ref error) = state.status.error {
        error_bar(frame, layout.help_area, error);
    } else {
        let hints: &[(&str, &str)] = if state.autocomplete.visible {
            &[("Tab", "select"), ("↑/↓", "nav"), ("Esc", "close")]
        } else if has_multiple_accounts {
            if state.polish.enabled {
                &[
                    ("Tab", "next"),
                    ("Ctrl+A", "account"),
                    ("Ctrl+P", "polish"),
                    ("Ctrl+S", "send"),
                    ("Esc", "cancel"),
                ]
            } else {
                &[
                    ("Tab", "next"),
                    ("Ctrl+A", "account"),
                    ("Ctrl+S", "send"),
                    ("Esc", "cancel"),
                ]
            }
        } else if state.polish.enabled {
            &[
                ("Tab", "next"),
                ("Ctrl+P", "polish"),
                ("Ctrl+S", "send"),
                ("Esc", "cancel"),
            ]
        } else {
            &[("Tab", "next"), ("Ctrl+S", "send"), ("Esc", "cancel")]
        };
        help_bar(frame, layout.help_area, hints);
    }

    // Autocomplete dropdown (rendered last, on top)
    if state.autocomplete.visible && !state.autocomplete.suggestions.is_empty() {
        let dropdown_area = match field {
            ComposerField::To => layout.to_area,
            ComposerField::Cc => layout.cc_area,
            _ => return,
        };
        render_autocomplete_dropdown(frame, dropdown_area, state);
    }

    // Polish preview modal (rendered on top of everything)
    if let Some(ref preview) = state.polish.preview {
        render_polish_preview(frame, preview);
    }
}

fn render_autocomplete_dropdown(frame: &mut Frame, field_area: Rect, state: &AppState) {
    let max_suggestions = 5.min(state.autocomplete.suggestions.len());
    let dropdown_height = (max_suggestions as u16) + 2; // +2 for borders

    let dropdown_area = Rect {
        x: field_area.x,
        y: field_area.y + field_area.height,
        width: field_area.width,
        height: dropdown_height,
    };

    frame.render_widget(Clear, dropdown_area);

    let items: Vec<ListItem> = state
        .autocomplete
        .suggestions
        .iter()
        .take(max_suggestions)
        .enumerate()
        .map(|(idx, contact)| {
            let style = if idx == state.autocomplete.selected {
                Theme::selected()
            } else {
                Theme::text()
            };

            let text = if let Some(ref name) = contact.name {
                format!("{} <{}>", name, contact.email)
            } else {
                contact.email.clone()
            };

            ListItem::new(text).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Theme::border_focused()),
    );

    frame.render_widget(list, dropdown_area);
}

fn render_from_field(frame: &mut Frame, area: Rect, account_name: &str) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Theme::border())
        .title(" From ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let line = Line::from(vec![
        Span::styled(account_name, Theme::text_accent()),
        Span::styled("  (Ctrl+A to change)", Theme::text_muted()),
    ]);

    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, inner);
}

fn render_field(frame: &mut Frame, area: Rect, label: &str, value: &str, focused: bool) {
    let border_style = if focused {
        Theme::border_focused()
    } else {
        Theme::border()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(format!(" {} ", label));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let style = if focused {
        Theme::text()
    } else {
        Theme::text_secondary()
    };

    let text = if focused {
        format!("{}│", value)
    } else {
        value.to_string()
    };

    let paragraph = Paragraph::new(text).style(style);
    frame.render_widget(paragraph, inner);
}

fn render_body_field(frame: &mut Frame, area: Rect, body: &str, focused: bool) {
    let border_style = if focused {
        Theme::border_focused()
    } else {
        Theme::border()
    };

    let char_count = body.chars().count();
    let title = format!(" Body ({} chars) ", char_count);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let style = if focused {
        Theme::text()
    } else {
        Theme::text_secondary()
    };

    let text = if focused {
        format!("{}│", body)
    } else {
        body.to_string()
    };

    let paragraph = Paragraph::new(text).style(style).wrap(Wrap { trim: false });

    frame.render_widget(paragraph, inner);
}

/// Render the AI polish preview modal (side-by-side diff)
fn render_polish_preview(frame: &mut Frame, preview: &PolishPreview) {
    // Create a centered modal taking 80% of the screen
    let area = centered_rect(80, 80, frame.area());

    // Clear the background
    frame.render_widget(Clear, area);

    // Create layout for the modal
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Title
            Constraint::Min(0),    // Content (side-by-side)
            Constraint::Length(1), // Help bar
        ])
        .split(area);

    // Title bar
    let title = if preview.loading {
        " AI Polish (Loading...) "
    } else {
        " AI Polish Preview "
    };
    let title_block = Block::default()
        .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
        .border_style(Theme::border_focused())
        .title(title);
    frame.render_widget(title_block, chunks[0]);

    // Side-by-side content
    let content_area = chunks[1];
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(content_area);

    // Original text (left pane)
    let original_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Theme::border())
        .title(" Original ");
    let original_text = Paragraph::new(preview.original.as_str())
        .wrap(Wrap { trim: false })
        .block(original_block);
    frame.render_widget(original_text, panes[0]);

    // Polished text (right pane)
    let polished_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Theme::border_focused())
        .title(" Polished ");
    let polished_content = if preview.loading {
        "Generating polished text..."
    } else if preview.polished.is_empty() {
        "[No changes suggested]"
    } else {
        preview.polished.as_str()
    };
    let polished_text = Paragraph::new(polished_content)
        .wrap(Wrap { trim: false })
        .block(polished_block);
    frame.render_widget(polished_text, panes[1]);

    // Help bar
    let help_block = Block::default()
        .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
        .border_style(Theme::border_focused());
    frame.render_widget(help_block, chunks[2]);

    if !preview.loading {
        let help_text = Line::from(vec![
            Span::styled("Enter", Theme::text_accent()),
            Span::styled(" Accept  ", Theme::text_secondary()),
            Span::styled("Esc", Theme::text_accent()),
            Span::styled(" Reject", Theme::text_secondary()),
        ]);
        let help_para = Paragraph::new(help_text);
        let inner_help = Rect {
            x: chunks[2].x + 1,
            y: chunks[2].y,
            width: chunks[2].width.saturating_sub(2),
            height: 1,
        };
        frame.render_widget(help_para, inner_help);
    }
}

/// Create a centered rectangle with given percentage of parent area
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
