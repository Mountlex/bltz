use crossterm::event::{Event, KeyCode, KeyEvent};

use super::keybindings::{Action, KeyBindings};
use crate::app::state::{AppState, View};

pub enum InputResult {
    Continue,
    Quit,
    Action(Action),
    Char(char),
    Backspace,
}

pub fn handle_input(event: Event, state: &AppState, bindings: &KeyBindings) -> InputResult {
    match event {
        Event::Key(key_event) => handle_key(key_event, state, bindings),
        _ => InputResult::Continue,
    }
}

fn handle_key(key: KeyEvent, state: &AppState, bindings: &KeyBindings) -> InputResult {
    // Check if we're in AI polish preview mode (modal)
    if is_polish_preview_mode(state) {
        return handle_polish_preview_input(key);
    }

    // Check if we're in add account wizard mode
    if is_add_account_mode(state) {
        return handle_add_account_input(key, state);
    }

    // Check if we're in contacts edit mode
    if is_contacts_edit_mode(state) {
        return handle_contacts_edit_input(key);
    }

    // Check if we're in contacts view
    if is_contacts_mode(state) {
        return handle_contacts_input(key, bindings);
    }

    // Check if we're in attachment view (reader with attachments focused)
    if is_attachment_mode(state) {
        return handle_attachment_input(key, bindings);
    }

    // Check if we're in help mode
    if is_help_mode(state) {
        return handle_help_input(key, bindings);
    }

    // Check if we're in folder picker mode
    if is_picker_mode(state) {
        return handle_folder_picker(key, bindings);
    }

    // Check if autocomplete is visible in composer
    if is_autocomplete_mode(state) {
        return handle_autocomplete_input(key, bindings);
    }

    // Check if we're in text input mode
    if is_text_input_mode(state) {
        return handle_text_input(key, state, bindings);
    }

    // Check for mapped action
    if let Some(action) = bindings.get(&key) {
        if action == Action::Quit {
            return InputResult::Quit;
        }
        return InputResult::Action(action);
    }

    InputResult::Continue
}

fn is_polish_preview_mode(state: &AppState) -> bool {
    state.polish.preview.is_some()
}

fn handle_polish_preview_input(key: KeyEvent) -> InputResult {
    // In polish preview modal: Enter accepts, Esc rejects
    match key.code {
        KeyCode::Enter => InputResult::Action(Action::AcceptPolish),
        KeyCode::Esc => InputResult::Action(Action::RejectPolish),
        _ => InputResult::Continue,
    }
}

fn handle_folder_picker(key: KeyEvent, bindings: &KeyBindings) -> InputResult {
    // In folder picker, j/k navigate, Enter selects, Esc/` closes
    if let Some(action) = bindings.get(&key) {
        match action {
            Action::Up | Action::Down => return InputResult::Action(action),
            Action::Open => return InputResult::Action(Action::Open),
            _ => {}
        }
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('`') => InputResult::Action(Action::Back),
        KeyCode::Enter => InputResult::Action(Action::Open),
        _ => InputResult::Continue,
    }
}

fn is_help_mode(state: &AppState) -> bool {
    state.modal.is_help()
}

fn handle_help_input(key: KeyEvent, bindings: &KeyBindings) -> InputResult {
    // In help modal: j/k scroll, Esc or "." closes
    if let Some(action) = bindings.get(&key) {
        match action {
            Action::Help => return InputResult::Action(Action::Help),
            Action::Up => return InputResult::Action(Action::Up),
            Action::Down => return InputResult::Action(Action::Down),
            _ => {}
        }
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('.') => InputResult::Action(Action::Help),
        KeyCode::Up | KeyCode::Char('k') => InputResult::Action(Action::Up),
        KeyCode::Down | KeyCode::Char('j') => InputResult::Action(Action::Down),
        _ => InputResult::Continue,
    }
}

fn is_text_input_mode(state: &AppState) -> bool {
    matches!(state.view, View::Composer { .. })
        || state.modal.is_search()
        || state.modal.is_command()
}

fn is_picker_mode(state: &AppState) -> bool {
    state.modal.is_folder_picker()
}

fn handle_text_input(key: KeyEvent, state: &AppState, bindings: &KeyBindings) -> InputResult {
    // Special handling for command mode
    if state.modal.is_command() {
        return handle_command_input(key, state);
    }

    // Special handling for search mode
    if state.modal.is_search() {
        return handle_search_input(key);
    }

    // Check for control actions first (composer)
    if let Some(action) = bindings.get(&key) {
        match action {
            Action::Send
            | Action::Cancel
            | Action::NextField
            | Action::PrevField
            | Action::CycleSendAccount
            | Action::Polish => {
                return InputResult::Action(action);
            }
            _ => {}
        }
    }

    // Handle text input
    match key.code {
        KeyCode::Char(c) => InputResult::Char(c),
        KeyCode::Backspace => InputResult::Backspace,
        KeyCode::Enter => InputResult::Char('\n'),
        KeyCode::Tab => InputResult::Action(Action::NextField),
        KeyCode::Esc => InputResult::Action(Action::Cancel),
        _ => InputResult::Continue,
    }
}

fn handle_command_input(key: KeyEvent, state: &AppState) -> InputResult {
    // If awaiting confirmation (e.g., for :clear)
    if state.modal.pending_confirmation().is_some() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                return InputResult::Action(Action::ConfirmCommand);
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                return InputResult::Action(Action::CancelCommand);
            }
            _ => return InputResult::Continue,
        }
    }

    // Normal command input
    match key.code {
        KeyCode::Char(c) => InputResult::Char(c),
        KeyCode::Backspace => InputResult::Backspace,
        KeyCode::Enter => InputResult::Action(Action::ExecuteCommand),
        KeyCode::Esc => InputResult::Action(Action::Back),
        _ => InputResult::Continue,
    }
}

fn handle_search_input(key: KeyEvent) -> InputResult {
    match key.code {
        KeyCode::Char(c) => InputResult::Char(c),
        KeyCode::Backspace => InputResult::Backspace,
        KeyCode::Enter | KeyCode::Esc => InputResult::Action(Action::Back), // Exit search mode
        _ => InputResult::Continue,
    }
}

fn is_add_account_mode(state: &AppState) -> bool {
    matches!(state.view, View::AddAccount { .. })
}

fn is_contacts_mode(state: &AppState) -> bool {
    matches!(state.view, View::Contacts)
}

fn is_attachment_mode(state: &AppState) -> bool {
    matches!(state.view, View::Reader { .. }) && state.reader.show_attachments
}

fn handle_attachment_input(key: KeyEvent, bindings: &KeyBindings) -> InputResult {
    // In attachment view: j/k navigate, Enter opens, s saves, A/Esc exits
    if let Some(action) = bindings.get(&key) {
        match action {
            Action::Up | Action::Down => return InputResult::Action(action),
            Action::ToggleAttachments => return InputResult::Action(action),
            _ => {}
        }
    }

    match key.code {
        KeyCode::Char('s') => InputResult::Action(Action::SaveAttachment),
        KeyCode::Enter => InputResult::Action(Action::OpenAttachment),
        KeyCode::Esc => InputResult::Action(Action::ToggleAttachments), // Close attachment view
        _ => InputResult::Continue,
    }
}

fn is_contacts_edit_mode(state: &AppState) -> bool {
    matches!(state.view, View::Contacts) && state.contacts.editing.is_some()
}

fn handle_contacts_input(key: KeyEvent, bindings: &KeyBindings) -> InputResult {
    // In contacts view: j/k navigate, e edit, d delete, Enter compose, Esc back
    if let Some(action) = bindings.get(&key) {
        match action {
            Action::Up | Action::Down => return InputResult::Action(action),
            Action::Delete => return InputResult::Action(action),
            _ => {}
        }
    }

    match key.code {
        KeyCode::Char('e') => InputResult::Action(Action::EditContact),
        KeyCode::Char('d') => InputResult::Action(Action::Delete),
        KeyCode::Enter => InputResult::Action(Action::Open),
        KeyCode::Esc | KeyCode::Char('q') => InputResult::Action(Action::Back),
        _ => InputResult::Continue,
    }
}

fn handle_contacts_edit_input(key: KeyEvent) -> InputResult {
    // Text input for contact name editing
    match key.code {
        KeyCode::Char(c) => InputResult::Char(c),
        KeyCode::Backspace => InputResult::Backspace,
        KeyCode::Enter => InputResult::Action(Action::Open), // Save
        KeyCode::Esc => InputResult::Action(Action::Back),   // Cancel
        _ => InputResult::Continue,
    }
}

fn is_autocomplete_mode(state: &AppState) -> bool {
    if let View::Composer { field, .. } = state.view {
        use crate::app::state::ComposerField;
        return (field == ComposerField::To || field == ComposerField::Cc)
            && state.autocomplete.visible;
    }
    false
}

fn handle_autocomplete_input(key: KeyEvent, bindings: &KeyBindings) -> InputResult {
    // When autocomplete is visible: Tab selects, Up/Down navigate, Esc closes
    // Other keys fall through to normal text input

    if let Some(action) = bindings.get(&key) {
        match action {
            Action::Up => return InputResult::Action(Action::AutocompleteUp),
            Action::Down => return InputResult::Action(Action::AutocompleteDown),
            _ => {}
        }
    }

    match key.code {
        KeyCode::Tab => InputResult::Action(Action::AutocompleteSelect),
        KeyCode::Up => InputResult::Action(Action::AutocompleteUp),
        KeyCode::Down => InputResult::Action(Action::AutocompleteDown),
        KeyCode::Esc => InputResult::Action(Action::AutocompleteClose),
        // Allow normal text input to continue
        KeyCode::Char(c) => InputResult::Char(c),
        KeyCode::Backspace => InputResult::Backspace,
        KeyCode::Enter => InputResult::Action(Action::NextField), // Move to next field
        _ => InputResult::Continue,
    }
}

fn handle_add_account_input(key: KeyEvent, state: &AppState) -> InputResult {
    use crate::app::state::AddAccountStep;

    // Get the current step
    let step = if let View::AddAccount { step, .. } = &state.view {
        step
    } else {
        return InputResult::Continue;
    };

    match step {
        AddAccountStep::ChooseAuthMethod => {
            // Up/down to select, Enter to confirm, Esc to cancel
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => InputResult::Action(Action::Up),
                KeyCode::Down | KeyCode::Char('j') => InputResult::Action(Action::Down),
                KeyCode::Enter => InputResult::Action(Action::WizardNext),
                KeyCode::Esc => InputResult::Action(Action::Back),
                _ => InputResult::Continue,
            }
        }
        AddAccountStep::EnterEmail
        | AddAccountStep::EnterPassword
        | AddAccountStep::EnterImapServer
        | AddAccountStep::EnterSmtpServer => {
            // Text input mode
            match key.code {
                KeyCode::Char(c) => InputResult::Char(c),
                KeyCode::Backspace => InputResult::Backspace,
                KeyCode::Enter => InputResult::Action(Action::WizardNext),
                KeyCode::Esc => InputResult::Action(Action::WizardBack),
                _ => InputResult::Continue,
            }
        }
        AddAccountStep::OAuth2Flow => {
            // Just Esc to cancel
            match key.code {
                KeyCode::Esc => InputResult::Action(Action::WizardBack),
                _ => InputResult::Continue,
            }
        }
        AddAccountStep::Confirm => {
            // Enter to confirm, Esc to cancel
            match key.code {
                KeyCode::Enter => InputResult::Action(Action::WizardConfirm),
                KeyCode::Esc => InputResult::Action(Action::WizardBack),
                _ => InputResult::Continue,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::KeybindingMode;

    #[test]
    fn test_quit_action() {
        let bindings = KeyBindings::new(&KeybindingMode::Vim);
        let state = AppState::default();

        let key = KeyEvent::new(KeyCode::Char('q'), crossterm::event::KeyModifiers::NONE);
        let result = handle_key(key, &state, &bindings);

        assert!(matches!(result, InputResult::Quit));
    }
}
