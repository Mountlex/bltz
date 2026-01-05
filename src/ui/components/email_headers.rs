use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::mail::types::EmailHeader;
use crate::ui::theme::Theme;
use crate::ui::widgets::format_date;

/// Renders email headers (From, To, CC, Date, Subject) in a bordered area.
///
/// # Arguments
/// * `frame` - The frame to render into
/// * `area` - The area to render the headers
/// * `email` - The email header data
/// * `show_attachments` - Whether to show the attachment indicator
/// * `wrap` - Whether to wrap long lines (used in expanded preview)
pub fn render_email_headers(
    frame: &mut Frame,
    area: Rect,
    email: &EmailHeader,
    show_attachments: bool,
    wrap: bool,
) {
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

    // Attachments (if present and requested)
    if show_attachments && email.has_attachments {
        lines.push(Line::from(vec![
            Span::styled("Attach:  ", label_style),
            Span::styled("\u{1F4CE} Has attachments", Theme::text_accent()),
        ]));
    }

    let paragraph = if wrap {
        Paragraph::new(lines).wrap(Wrap { trim: false })
    } else {
        Paragraph::new(lines)
    };

    frame.render_widget(paragraph, inner);
}
