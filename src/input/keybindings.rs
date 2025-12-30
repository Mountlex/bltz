use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;

use crate::config::KeybindingMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    // Navigation
    Up,
    Down,
    Left,
    Right,
    Top,
    Bottom,
    PageUp,
    PageDown,

    // Account switching
    NextAccount,
    PrevAccount,

    // Actions
    Open,
    Back,
    Quit,
    Reply,
    ReplyAll,
    Forward,
    Compose,
    Delete,
    ToggleRead,
    ToggleStar,
    ViewStarred,
    Refresh,
    ToggleThread,
    Search,
    FolderPicker,

    // Contacts
    OpenContacts,
    EditContact,

    // Command mode
    Command,
    ExecuteCommand,
    ConfirmCommand,
    CancelCommand,

    // Composer
    NextField,
    PrevField,
    Send,
    Cancel,
    CycleSendAccount,

    // Autocomplete (composer)
    AutocompleteUp,
    AutocompleteDown,
    AutocompleteSelect,
    AutocompleteClose,

    // Add account wizard
    WizardNext,
    WizardBack,
    WizardConfirm,

    // Undo
    Undo,

    // AI features
    ToggleSummary,   // Toggle AI summary in reader
    SummarizeThread, // Summarize entire thread
    Polish,          // Polish/improve writing in composer
    AcceptPolish,    // Accept polished text
    RejectPolish,    // Reject polished text

    // Help
    Help, // Toggle help view
}

pub struct KeyBindings {
    bindings: HashMap<KeyEvent, Action>,
}

/// A displayable keybinding entry
#[derive(Debug, Clone)]
pub struct KeybindingEntry {
    pub key: String,
    pub description: String,
    pub category: &'static str,
}

impl KeyBindings {
    pub fn new(mode: &KeybindingMode) -> Self {
        let bindings = match mode {
            KeybindingMode::Vim => Self::vim_bindings(),
            KeybindingMode::Arrows => Self::arrow_bindings(),
        };
        Self { bindings }
    }

    pub fn get(&self, event: &KeyEvent) -> Option<Action> {
        self.bindings.get(event).copied()
    }

    /// Get all keybindings as displayable entries grouped by category
    pub fn all_bindings(&self) -> Vec<KeybindingEntry> {
        let mut entries: Vec<_> = self
            .bindings
            .iter()
            .map(|(event, action)| KeybindingEntry {
                key: format_key_event(event),
                description: action_description(action),
                category: action_category(action),
            })
            .collect();

        // Sort by category first, then by description
        entries.sort_by(|a, b| {
            let cat_order = category_order(a.category).cmp(&category_order(b.category));
            if cat_order == std::cmp::Ordering::Equal {
                a.description.cmp(&b.description)
            } else {
                cat_order
            }
        });
        entries
    }

    fn vim_bindings() -> HashMap<KeyEvent, Action> {
        let mut map = HashMap::new();

        // Navigation
        map.insert(key('j'), Action::Down);
        map.insert(key('k'), Action::Up);
        map.insert(key('h'), Action::Left);
        map.insert(key('l'), Action::Right);
        map.insert(key('g'), Action::Top);
        map.insert(shift_key('G'), Action::Bottom);
        map.insert(ctrl_key('d'), Action::PageDown);
        map.insert(ctrl_key('u'), Action::PageUp);

        // Actions
        map.insert(key_code(KeyCode::Enter), Action::Open);
        map.insert(key('q'), Action::Quit);
        map.insert(key_code(KeyCode::Esc), Action::Back);
        map.insert(key('r'), Action::Reply);
        map.insert(key('a'), Action::ReplyAll);
        map.insert(key('f'), Action::Forward);
        map.insert(key('c'), Action::Compose);
        map.insert(key('d'), Action::Delete);
        map.insert(key('m'), Action::ToggleRead);
        map.insert(key('s'), Action::ToggleStar);
        map.insert(shift_key('S'), Action::ViewStarred);
        map.insert(ctrl_key('r'), Action::Refresh);
        map.insert(key_code(KeyCode::Tab), Action::ToggleThread);
        map.insert(key(' '), Action::ToggleThread);
        map.insert(key('/'), Action::Search);
        map.insert(key('b'), Action::FolderPicker);
        map.insert(key(':'), Action::Command);

        // Account switching (] = next, [ = prev)
        map.insert(key(']'), Action::NextAccount);
        map.insert(key('['), Action::PrevAccount);

        // Contacts
        map.insert(shift_key('B'), Action::OpenContacts);

        // Composer (Tab→NextField handled in handler.rs for composer context only)
        map.insert(shift_key_code(KeyCode::BackTab), Action::PrevField);
        map.insert(ctrl_key('s'), Action::Send);
        map.insert(ctrl_key('a'), Action::CycleSendAccount);

        // Undo
        map.insert(key('u'), Action::Undo);

        // AI features
        map.insert(shift_key('T'), Action::ToggleSummary);
        map.insert(ctrl_key('t'), Action::SummarizeThread);
        map.insert(ctrl_key('p'), Action::Polish);

        // Help
        map.insert(key('.'), Action::Help);

        map
    }

    fn arrow_bindings() -> HashMap<KeyEvent, Action> {
        let mut map = HashMap::new();

        // Navigation
        map.insert(key_code(KeyCode::Down), Action::Down);
        map.insert(key_code(KeyCode::Up), Action::Up);
        map.insert(key_code(KeyCode::Left), Action::Left);
        map.insert(key_code(KeyCode::Right), Action::Right);
        map.insert(key_code(KeyCode::Home), Action::Top);
        map.insert(key_code(KeyCode::End), Action::Bottom);
        map.insert(key_code(KeyCode::PageDown), Action::PageDown);
        map.insert(key_code(KeyCode::PageUp), Action::PageUp);

        // Actions
        map.insert(key_code(KeyCode::Enter), Action::Open);
        map.insert(key_code(KeyCode::Esc), Action::Back);
        map.insert(key_code(KeyCode::Backspace), Action::Back);
        map.insert(ctrl_key('q'), Action::Quit);
        map.insert(ctrl_key('r'), Action::Reply);
        map.insert(shift_key('A'), Action::ReplyAll);
        map.insert(ctrl_key('f'), Action::Forward);
        map.insert(ctrl_key('n'), Action::Compose);
        map.insert(key_code(KeyCode::Delete), Action::Delete);
        map.insert(ctrl_key('u'), Action::ToggleRead);
        map.insert(ctrl_key('s'), Action::ToggleStar);
        map.insert(shift_key('S'), Action::ViewStarred);
        map.insert(key_code(KeyCode::F(6)), Action::ViewStarred);
        map.insert(key_code(KeyCode::F(5)), Action::Refresh);
        map.insert(key_code(KeyCode::Tab), Action::ToggleThread);
        map.insert(key(' '), Action::ToggleThread);
        map.insert(key_code(KeyCode::F(3)), Action::Search);
        map.insert(key('/'), Action::Search);
        map.insert(key('p'), Action::FolderPicker);
        map.insert(key_code(KeyCode::F(2)), Action::FolderPicker);
        map.insert(key(':'), Action::Command);

        // Account switching (] = next, [ = prev, or Ctrl+Right/Left)
        map.insert(key(']'), Action::NextAccount);
        map.insert(key('['), Action::PrevAccount);
        map.insert(ctrl_key_code(KeyCode::Right), Action::NextAccount);
        map.insert(ctrl_key_code(KeyCode::Left), Action::PrevAccount);

        // Contacts
        map.insert(shift_key('C'), Action::OpenContacts);

        // Composer (Tab→NextField handled in handler.rs for composer context only)
        map.insert(shift_key_code(KeyCode::BackTab), Action::PrevField);
        map.insert(ctrl_key('s'), Action::Send);
        map.insert(ctrl_key('c'), Action::Cancel);
        map.insert(key_code(KeyCode::F(4)), Action::CycleSendAccount);

        // Undo
        map.insert(ctrl_key('z'), Action::Undo);

        // AI features
        map.insert(key_code(KeyCode::F(7)), Action::ToggleSummary);
        map.insert(key_code(KeyCode::F(8)), Action::SummarizeThread);
        map.insert(ctrl_key('p'), Action::Polish);

        // Help
        map.insert(key('.'), Action::Help);

        map
    }
}

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn shift_key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT)
}

fn ctrl_key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn key_code(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn shift_key_code(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::SHIFT)
}

fn ctrl_key_code(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::CONTROL)
}

/// Format a KeyEvent for display
fn format_key_event(event: &KeyEvent) -> String {
    let mut parts = Vec::new();

    if event.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("Ctrl+");
    }
    if event.modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("Shift+");
    }
    if event.modifiers.contains(KeyModifiers::ALT) {
        parts.push("Alt+");
    }

    let key_str = match event.code {
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => "Shift+Tab".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Delete => "Delete".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PgUp".to_string(),
        KeyCode::PageDown => "PgDn".to_string(),
        KeyCode::F(n) => format!("F{}", n),
        _ => format!("{:?}", event.code),
    };

    format!("{}{}", parts.join(""), key_str)
}

/// Get a human-readable description for an action
fn action_description(action: &Action) -> String {
    match action {
        Action::Up => "Move up".to_string(),
        Action::Down => "Move down".to_string(),
        Action::Left => "Move left / collapse".to_string(),
        Action::Right => "Move right / expand".to_string(),
        Action::Top => "Go to top".to_string(),
        Action::Bottom => "Go to bottom".to_string(),
        Action::PageUp => "Page up".to_string(),
        Action::PageDown => "Page down".to_string(),
        Action::NextAccount => "Next account".to_string(),
        Action::PrevAccount => "Previous account".to_string(),
        Action::Open => "Open / select".to_string(),
        Action::Back => "Go back / close".to_string(),
        Action::Quit => "Quit".to_string(),
        Action::Reply => "Reply to email".to_string(),
        Action::ReplyAll => "Reply all".to_string(),
        Action::Forward => "Forward email".to_string(),
        Action::Compose => "Compose new email".to_string(),
        Action::Delete => "Delete email".to_string(),
        Action::ToggleRead => "Toggle read/unread".to_string(),
        Action::ToggleStar => "Toggle star".to_string(),
        Action::ViewStarred => "View starred emails".to_string(),
        Action::Refresh => "Refresh / sync".to_string(),
        Action::ToggleThread => "Toggle thread expansion".to_string(),
        Action::Search => "Search emails".to_string(),
        Action::FolderPicker => "Open folder picker".to_string(),
        Action::OpenContacts => "Open contacts".to_string(),
        Action::EditContact => "Edit contact".to_string(),
        Action::Command => "Enter command mode".to_string(),
        Action::ExecuteCommand => "Execute command".to_string(),
        Action::ConfirmCommand => "Confirm command".to_string(),
        Action::CancelCommand => "Cancel command".to_string(),
        Action::NextField => "Next field".to_string(),
        Action::PrevField => "Previous field".to_string(),
        Action::Send => "Send email".to_string(),
        Action::Cancel => "Cancel".to_string(),
        Action::CycleSendAccount => "Cycle send account".to_string(),
        Action::AutocompleteUp => "Autocomplete: previous".to_string(),
        Action::AutocompleteDown => "Autocomplete: next".to_string(),
        Action::AutocompleteSelect => "Autocomplete: select".to_string(),
        Action::AutocompleteClose => "Autocomplete: close".to_string(),
        Action::WizardNext => "Wizard: next step".to_string(),
        Action::WizardBack => "Wizard: go back".to_string(),
        Action::WizardConfirm => "Wizard: confirm".to_string(),
        Action::Undo => "Undo last action".to_string(),
        Action::ToggleSummary => "Toggle AI summary".to_string(),
        Action::SummarizeThread => "Summarize thread (AI)".to_string(),
        Action::Polish => "Polish writing (AI)".to_string(),
        Action::AcceptPolish => "Accept polished text".to_string(),
        Action::RejectPolish => "Reject polished text".to_string(),
        Action::Help => "Toggle help".to_string(),
    }
}

/// Get the category for an action
fn action_category(action: &Action) -> &'static str {
    match action {
        Action::Up
        | Action::Down
        | Action::Left
        | Action::Right
        | Action::Top
        | Action::Bottom
        | Action::PageUp
        | Action::PageDown => "Navigation",

        Action::NextAccount | Action::PrevAccount => "Accounts",

        Action::Open
        | Action::Back
        | Action::Quit
        | Action::Reply
        | Action::ReplyAll
        | Action::Forward
        | Action::Compose
        | Action::Delete
        | Action::ToggleRead
        | Action::ToggleStar
        | Action::ViewStarred
        | Action::Refresh
        | Action::ToggleThread
        | Action::Search
        | Action::FolderPicker
        | Action::Undo
        | Action::OpenContacts => "Actions",

        Action::EditContact => "Contacts",

        Action::Command
        | Action::ExecuteCommand
        | Action::ConfirmCommand
        | Action::CancelCommand => "Commands",

        Action::NextField
        | Action::PrevField
        | Action::Send
        | Action::Cancel
        | Action::CycleSendAccount
        | Action::AutocompleteUp
        | Action::AutocompleteDown
        | Action::AutocompleteSelect
        | Action::AutocompleteClose => "Composer",

        Action::WizardNext | Action::WizardBack | Action::WizardConfirm => "Wizard",

        Action::ToggleSummary
        | Action::SummarizeThread
        | Action::Polish
        | Action::AcceptPolish
        | Action::RejectPolish => "AI",

        Action::Help => "Help",
    }
}

/// Get sort order for categories
fn category_order(category: &str) -> u8 {
    match category {
        "Navigation" => 0,
        "Actions" => 1,
        "Accounts" => 2,
        "Contacts" => 3,
        "AI" => 4,
        "Commands" => 5,
        "Composer" => 6,
        "Wizard" => 7,
        "Help" => 8,
        _ => 99,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vim_bindings() {
        let bindings = KeyBindings::new(&KeybindingMode::Vim);

        assert_eq!(bindings.get(&key('j')), Some(Action::Down));
        assert_eq!(bindings.get(&key('k')), Some(Action::Up));
        assert_eq!(bindings.get(&key('q')), Some(Action::Quit));
    }

    #[test]
    fn test_arrow_bindings() {
        let bindings = KeyBindings::new(&KeybindingMode::Arrows);

        assert_eq!(bindings.get(&key_code(KeyCode::Down)), Some(Action::Down));
        assert_eq!(bindings.get(&key_code(KeyCode::Up)), Some(Action::Up));
        assert_eq!(bindings.get(&ctrl_key('q')), Some(Action::Quit));
    }
}
