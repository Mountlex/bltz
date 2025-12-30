//! Centralized theming for the bltz TUI
//!
//! This module provides a single source of truth for all colors and styles
//! used throughout the application.

use ratatui::style::{Modifier, Style};

/// Color palette - all colors defined in one place
pub mod colors {
    use ratatui::style::Color;

    // Selection
    pub const BG_SELECTION: Color = Color::Blue;

    // Status bars
    pub const BG_STATUS: Color = Color::DarkGray;
    pub const BG_ERROR: Color = Color::Red;

    // Text colors
    pub const FG_PRIMARY: Color = Color::White;
    pub const FG_SECONDARY: Color = Color::Gray;
    pub const FG_MUTED: Color = Color::DarkGray;
    pub const FG_ACCENT: Color = Color::Cyan;
    pub const FG_WARNING: Color = Color::Yellow;

    // Semantic
    pub const UNREAD_INDICATOR: Color = Color::Cyan;
    pub const THREAD_BADGE: Color = Color::Yellow;
    pub const BORDER: Color = Color::DarkGray;
    pub const BORDER_FOCUSED: Color = Color::Cyan;

    // Status colors
    pub const STATUS_CONNECTED: Color = Color::Green;
    pub const STATUS_DISCONNECTED: Color = Color::Red;
    pub const STATUS_SYNCING: Color = Color::Yellow;
}

/// UI symbols - centralized for consistency
pub mod symbols {
    pub const UNREAD: &str = "●";
    pub const READ: &str = " ";
    pub const ATTACHMENT: &str = "+";
    pub const NO_ATTACHMENT: &str = " ";
    pub const THREAD_EXPANDED: &str = "▼ ";
    pub const THREAD_COLLAPSED: &str = "▶ ";
    pub const THREAD_SINGLE: &str = "  ";
    pub const CURRENT_FOLDER: &str = "●";

    // Star indicator
    pub const STARRED: &str = "★";

    // Status indicators
    pub const CONNECTED: &str = "●";
    pub const DISCONNECTED: &str = "○";

    // Account indicators
    pub const ACCOUNT_NEW_MAIL: &str = "●";
    pub const ACCOUNT_NO_NEW: &str = "○";
    pub const ACCOUNT_ERROR: &str = "!";
}

/// Pre-composed styles for common UI elements
pub struct Theme;

impl Theme {
    // === Selection Styles ===

    /// Base style for selected items
    pub fn selected() -> Style {
        Style::default()
            .bg(colors::BG_SELECTION)
            .fg(colors::FG_PRIMARY)
    }

    /// Style for selected item with bold text
    pub fn selected_bold() -> Style {
        Self::selected().add_modifier(Modifier::BOLD)
    }

    // === Text Styles ===

    /// Normal text
    pub fn text() -> Style {
        Style::default().fg(colors::FG_PRIMARY)
    }

    /// Secondary/read text
    pub fn text_secondary() -> Style {
        Style::default().fg(colors::FG_SECONDARY)
    }

    /// Muted/disabled text
    pub fn text_muted() -> Style {
        Style::default().fg(colors::FG_MUTED)
    }

    /// Unread/bold text
    pub fn text_unread() -> Style {
        Style::default()
            .fg(colors::FG_PRIMARY)
            .add_modifier(Modifier::BOLD)
    }

    /// Accent colored text
    pub fn text_accent() -> Style {
        Style::default().fg(colors::FG_ACCENT)
    }

    // === Status Bar ===

    pub fn status_bar() -> Style {
        Style::default()
            .bg(colors::BG_STATUS)
            .fg(colors::FG_PRIMARY)
    }

    pub fn error_bar() -> Style {
        Style::default().bg(colors::BG_ERROR).fg(colors::FG_PRIMARY)
    }

    // === Help Bar ===

    pub fn help_key() -> Style {
        Style::default()
            .bg(colors::BG_STATUS)
            .fg(colors::FG_WARNING)
    }

    pub fn help_desc() -> Style {
        Style::default().fg(colors::FG_MUTED)
    }

    // === Borders ===

    pub fn border() -> Style {
        Style::default().fg(colors::BORDER)
    }

    pub fn border_focused() -> Style {
        Style::default().fg(colors::BORDER_FOCUSED)
    }

    // === Indicators ===

    pub fn unread_indicator() -> Style {
        Style::default().fg(colors::UNREAD_INDICATOR)
    }

    /// Unread indicator that preserves selection background
    pub fn unread_indicator_selected() -> Style {
        Style::default()
            .bg(colors::BG_SELECTION)
            .fg(colors::UNREAD_INDICATOR)
    }

    /// Star indicator (yellow)
    pub fn star_indicator() -> Style {
        Style::default().fg(colors::FG_WARNING)
    }

    /// Star indicator that preserves selection background
    pub fn star_indicator_selected() -> Style {
        Style::default()
            .bg(colors::BG_SELECTION)
            .fg(colors::FG_WARNING)
    }

    pub fn thread_badge() -> Style {
        Style::default().fg(colors::THREAD_BADGE)
    }

    // === Labels ===

    pub fn label() -> Style {
        Style::default()
            .fg(colors::FG_MUTED)
            .add_modifier(Modifier::BOLD)
    }

    // === Input/Form Styles ===

    /// Highlighted input text (yellow, bold)
    pub fn input_highlight() -> Style {
        Style::default()
            .fg(colors::FG_WARNING)
            .add_modifier(Modifier::BOLD)
    }

    /// Success/confirmation text (green)
    pub fn text_success() -> Style {
        Style::default().fg(colors::STATUS_CONNECTED)
    }

    /// Link/URL text (cyan, underlined)
    pub fn text_link() -> Style {
        Style::default()
            .fg(colors::FG_ACCENT)
            .add_modifier(Modifier::UNDERLINED)
    }

    // === Status Indicators ===

    /// Connected status indicator (green)
    pub fn status_connected() -> Style {
        Style::default()
            .bg(colors::BG_STATUS)
            .fg(colors::STATUS_CONNECTED)
    }

    /// Disconnected status indicator (red)
    pub fn status_disconnected() -> Style {
        Style::default()
            .bg(colors::BG_STATUS)
            .fg(colors::STATUS_DISCONNECTED)
    }

    /// Syncing/loading status indicator (yellow)
    pub fn status_syncing() -> Style {
        Style::default()
            .bg(colors::BG_STATUS)
            .fg(colors::STATUS_SYNCING)
    }

    /// Muted status text (uses secondary gray for visibility on dark status bar)
    pub fn status_muted() -> Style {
        Style::default()
            .bg(colors::BG_STATUS)
            .fg(colors::FG_SECONDARY)
    }

    // === Account Indicators ===

    /// Account with new mail (cyan dot)
    pub fn account_new_mail() -> Style {
        Style::default()
            .bg(colors::BG_STATUS)
            .fg(colors::UNREAD_INDICATOR)
    }

    /// Account with no new mail (muted)
    pub fn account_no_new() -> Style {
        Style::default().bg(colors::BG_STATUS).fg(colors::FG_MUTED)
    }

    /// Account with error (red)
    pub fn account_error() -> Style {
        Style::default()
            .bg(colors::BG_STATUS)
            .fg(colors::STATUS_DISCONNECTED)
    }

    /// Account name text in status bar
    pub fn account_name() -> Style {
        Style::default()
            .bg(colors::BG_STATUS)
            .fg(colors::FG_SECONDARY)
    }

    /// New mail count badge
    pub fn account_badge() -> Style {
        Style::default()
            .bg(colors::BG_STATUS)
            .fg(colors::THREAD_BADGE)
    }
}

/// Merge a style with selection background when selected.
/// This ensures the selection highlight covers the entire row.
pub fn with_selection_bg(style: Style, selected: bool) -> Style {
    if selected {
        style.bg(colors::BG_SELECTION)
    } else {
        style
    }
}
