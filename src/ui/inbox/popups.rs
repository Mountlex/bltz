//! Modal popup overlays for the inbox view.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::app::state::AppState;
use crate::command::{CommandHelp, CommandResult};
use crate::input::KeybindingEntry;

use super::super::theme::{Theme, symbols};

pub fn render_command_bar(frame: &mut Frame, area: Rect, state: &AppState) {
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

/// Render a folder picker overlay
pub fn render_folder_picker(frame: &mut Frame, area: Rect, state: &AppState) {
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
pub fn render_unified_help_popup(
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
