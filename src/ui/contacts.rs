//! Contacts view rendering

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use super::components::centered_rect_constrained;
use super::theme::{Theme, with_selection_bg};
use super::widgets::{error_bar, help_bar, truncate_string};
use crate::app::state::AppState;

pub fn render_contacts(frame: &mut Frame, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Status bar
            Constraint::Min(0),    // Contacts list
            Constraint::Length(1), // Help bar
        ])
        .split(frame.area());

    // Status bar
    render_contacts_status_bar(frame, chunks[0], state);

    // Main contacts list
    render_contacts_list(frame, chunks[1], state);

    // Help bar or error
    if let Some(ref error) = state.status.error {
        error_bar(frame, chunks[2], error);
    } else {
        let hints = if state.contacts.editing.is_some() {
            &[("Enter", "save"), ("Esc", "cancel")][..]
        } else {
            &[
                ("j/k", "nav"),
                ("e", "edit"),
                ("d", "delete"),
                ("Enter", "compose"),
                ("Esc", "back"),
            ][..]
        };
        help_bar(frame, chunks[2], hints);
    }

    // Edit popup overlay (if editing)
    if state.contacts.editing.is_some() {
        render_contact_edit_popup(frame, frame.area(), state);
    }
}

fn render_contacts_status_bar(frame: &mut Frame, area: Rect, state: &AppState) {
    let count = state.contacts.list.len();
    let text = format!(" Contacts ({}) ", count);

    let paragraph = Paragraph::new(text).style(Theme::status_bar());
    frame.render_widget(paragraph, area);
}

fn render_contacts_list(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .title(" Contacts ")
        .borders(Borders::ALL)
        .border_style(Theme::border());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if state.contacts.list.is_empty() {
        let msg =
            Paragraph::new("No contacts yet. Contacts are added when you send or receive emails.")
                .style(Theme::text_muted())
                .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(msg, inner);
        return;
    }

    // Build list items - 2 lines per contact
    let mut items: Vec<ListItem> = Vec::new();
    let mut selected_index: Option<usize> = None;

    for (idx, contact) in state.contacts.list.iter().enumerate() {
        let is_selected = idx == state.contacts.selected;
        if is_selected {
            selected_index = Some(items.len());
        }
        let contact_items = render_contact_item(contact, is_selected, inner.width);
        items.extend(contact_items);
    }

    // Virtual scrolling
    let visible_lines = inner.height as usize;
    let total_items = items.len();

    let scroll_offset = if let Some(sel_idx) = selected_index {
        let target_position = visible_lines / 4;
        sel_idx.saturating_sub(target_position)
    } else {
        0
    };

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

fn render_contact_item(
    contact: &crate::contacts::Contact,
    selected: bool,
    width: u16,
) -> Vec<ListItem<'static>> {
    let width = width as usize;

    // Line 1: Name (or email if no name)
    let display_name = contact.name.as_deref().unwrap_or(&contact.email);
    let has_name = contact.name.is_some();

    // Show contact count on right side
    let count_str = format!("({} interactions)", contact.contact_count);
    let count_width = count_str.len();
    let name_width = width.saturating_sub(count_width + 2);

    let name_display = truncate_string(display_name, name_width);
    let padding = name_width.saturating_sub(name_display.len());

    let base_style = if selected {
        Theme::selected()
    } else {
        ratatui::style::Style::default()
    };

    let name_style = if selected {
        if has_name {
            Theme::selected_bold()
        } else {
            Theme::selected()
        }
    } else if has_name {
        Theme::text_unread()
    } else {
        Theme::text_secondary()
    };

    let count_style = with_selection_bg(Theme::text_muted(), selected);

    let line1 = Line::from(vec![
        Span::styled(name_display, name_style),
        Span::styled(" ".repeat(padding), base_style),
        Span::styled(" ", base_style),
        Span::styled(count_str, count_style),
    ]);

    // Line 2: Email (if name exists) or empty
    let line2 = if has_name {
        let email_display = truncate_string(&contact.email, width);
        let email_padding = width.saturating_sub(email_display.len());

        let email_style = with_selection_bg(Theme::text_muted(), selected);

        Line::from(vec![
            Span::styled("  ", base_style), // indent
            Span::styled(email_display, email_style),
            Span::styled(" ".repeat(email_padding.saturating_sub(2)), base_style),
        ])
    } else {
        // Just show padding for alignment
        Line::from(vec![Span::styled(" ".repeat(width), base_style)])
    };

    vec![ListItem::new(line1), ListItem::new(line2)]
}

fn render_contact_edit_popup(frame: &mut Frame, area: Rect, state: &AppState) {
    let edit_state = match &state.contacts.editing {
        Some(e) => e,
        None => return,
    };

    // Calculate popup size and position
    let popup_area = centered_rect_constrained(area, 30, 50, 7, 7);

    // Clear the area behind the popup
    frame.render_widget(Clear, popup_area);

    // Create the popup block
    let block = Block::default()
        .title(" Edit Contact Name ")
        .borders(Borders::ALL)
        .border_style(Theme::border_focused());

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    // Get the contact being edited
    let contact = state
        .contacts
        .list
        .iter()
        .find(|c| c.id == edit_state.contact_id);

    let email = contact.map(|c| c.email.as_str()).unwrap_or("Unknown");

    // Layout: email label, name input
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Email (read-only)
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Name label
            Constraint::Length(1), // Name input
        ])
        .split(inner);

    // Email (read-only)
    let email_line = Line::from(vec![
        Span::styled("Email: ", Theme::label()),
        Span::styled(email, Theme::text()),
    ]);
    frame.render_widget(Paragraph::new(email_line), sections[0]);

    // Name label
    let name_label = Paragraph::new("Name:").style(Theme::label());
    frame.render_widget(name_label, sections[2]);

    // Name input with cursor
    let input_text = format!("{}│", edit_state.name);
    let input_style = Theme::text().add_modifier(Modifier::UNDERLINED);
    let input = Paragraph::new(input_text).style(input_style);
    frame.render_widget(input, sections[3]);
}
