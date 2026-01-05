//! Centralized theming for the bltz TUI
//!
//! This module provides a single source of truth for all colors and styles
//! used throughout the application.

use ratatui::style::{Color, Modifier, Style};
use std::sync::OnceLock;

use crate::config::ThemeVariant;

/// Global theme variant storage
static THEME_VARIANT: OnceLock<ThemeVariant> = OnceLock::new();

/// Initialize the theme variant (call once at startup)
pub fn init_theme(variant: ThemeVariant) {
    THEME_VARIANT.set(variant).ok();
}

/// Get the current theme variant
fn current_theme() -> ThemeVariant {
    THEME_VARIANT.get().copied().unwrap_or_default()
}

/// Color palette - colors that vary by theme
pub mod colors {
    use super::*;

    // Selection
    pub fn bg_selection() -> Color {
        match current_theme() {
            ThemeVariant::Dark => Color::Blue,
            ThemeVariant::HighContrast => Color::LightBlue,
        }
    }

    // Status bars
    pub fn bg_status() -> Color {
        match current_theme() {
            ThemeVariant::Dark => Color::DarkGray,
            ThemeVariant::HighContrast => Color::Black,
        }
    }

    pub fn bg_error() -> Color {
        Color::Red
    }

    // Text colors
    pub fn fg_primary() -> Color {
        Color::White
    }

    pub fn fg_secondary() -> Color {
        match current_theme() {
            ThemeVariant::Dark => Color::Gray,
            ThemeVariant::HighContrast => Color::White,
        }
    }

    pub fn fg_muted() -> Color {
        match current_theme() {
            ThemeVariant::Dark => Color::DarkGray,
            ThemeVariant::HighContrast => Color::Gray,
        }
    }

    pub fn fg_accent() -> Color {
        match current_theme() {
            ThemeVariant::Dark => Color::Cyan,
            ThemeVariant::HighContrast => Color::LightCyan,
        }
    }

    pub fn fg_warning() -> Color {
        match current_theme() {
            ThemeVariant::Dark => Color::Yellow,
            ThemeVariant::HighContrast => Color::LightYellow,
        }
    }

    // Semantic
    pub fn unread_indicator() -> Color {
        match current_theme() {
            ThemeVariant::Dark => Color::Magenta,
            ThemeVariant::HighContrast => Color::LightMagenta,
        }
    }

    pub fn thread_badge() -> Color {
        match current_theme() {
            ThemeVariant::Dark => Color::Yellow,
            ThemeVariant::HighContrast => Color::LightYellow,
        }
    }

    pub fn border() -> Color {
        match current_theme() {
            ThemeVariant::Dark => Color::DarkGray,
            ThemeVariant::HighContrast => Color::Gray,
        }
    }

    pub fn border_focused() -> Color {
        match current_theme() {
            ThemeVariant::Dark => Color::Cyan,
            ThemeVariant::HighContrast => Color::LightCyan,
        }
    }

    // Status colors
    pub fn status_connected() -> Color {
        match current_theme() {
            ThemeVariant::Dark => Color::Green,
            ThemeVariant::HighContrast => Color::LightGreen,
        }
    }

    pub fn status_disconnected() -> Color {
        match current_theme() {
            ThemeVariant::Dark => Color::Red,
            ThemeVariant::HighContrast => Color::LightRed,
        }
    }

    pub fn status_syncing() -> Color {
        match current_theme() {
            ThemeVariant::Dark => Color::Yellow,
            ThemeVariant::HighContrast => Color::LightYellow,
        }
    }
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

    // Replied indicator
    pub const REPLIED: &str = "↩";

    // Thread child indent (visual tree lines)
    pub const THREAD_CHILD: &str = "  │ "; // Continuation line
    pub const THREAD_CHILD_MID: &str = "  ├─"; // Middle child
    pub const THREAD_CHILD_LAST: &str = "  └─"; // Last child

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
            .bg(colors::bg_selection())
            .fg(colors::fg_primary())
    }

    /// Style for selected item with bold text
    pub fn selected_bold() -> Style {
        Self::selected().add_modifier(Modifier::BOLD)
    }

    // === Text Styles ===

    /// Normal text
    pub fn text() -> Style {
        Style::default().fg(colors::fg_primary())
    }

    /// Secondary/read text
    pub fn text_secondary() -> Style {
        Style::default().fg(colors::fg_secondary())
    }

    /// Muted/disabled text
    pub fn text_muted() -> Style {
        Style::default().fg(colors::fg_muted())
    }

    /// Unread/bold text
    pub fn text_unread() -> Style {
        Style::default()
            .fg(colors::fg_primary())
            .add_modifier(Modifier::BOLD)
    }

    /// Accent colored text
    pub fn text_accent() -> Style {
        Style::default().fg(colors::fg_accent())
    }

    // === Status Bar ===

    pub fn status_bar() -> Style {
        Style::default()
            .bg(colors::bg_status())
            .fg(colors::fg_primary())
    }

    pub fn error_bar() -> Style {
        Style::default()
            .bg(colors::bg_error())
            .fg(colors::fg_primary())
    }

    // === Help Bar ===

    pub fn help_key() -> Style {
        Style::default()
            .bg(colors::bg_status())
            .fg(colors::fg_warning())
    }

    pub fn help_desc() -> Style {
        Style::default().fg(colors::fg_muted())
    }

    // === Borders ===

    pub fn border() -> Style {
        Style::default().fg(colors::border())
    }

    pub fn border_focused() -> Style {
        Style::default().fg(colors::border_focused())
    }

    // === Indicators ===

    pub fn unread_indicator() -> Style {
        Style::default().fg(colors::unread_indicator())
    }

    /// Unread indicator that preserves selection background
    pub fn unread_indicator_selected() -> Style {
        Style::default()
            .bg(colors::bg_selection())
            .fg(colors::unread_indicator())
    }

    /// Star indicator (yellow)
    pub fn star_indicator() -> Style {
        Style::default().fg(colors::fg_warning())
    }

    /// Star indicator that preserves selection background
    pub fn star_indicator_selected() -> Style {
        Style::default()
            .bg(colors::bg_selection())
            .fg(colors::fg_warning())
    }

    /// Replied indicator (muted)
    pub fn replied_indicator() -> Style {
        Style::default().fg(colors::fg_muted())
    }

    /// Replied indicator that preserves selection background
    pub fn replied_indicator_selected() -> Style {
        Style::default()
            .bg(colors::bg_selection())
            .fg(colors::fg_muted())
    }

    pub fn thread_badge() -> Style {
        Style::default().fg(colors::thread_badge())
    }

    // === Labels ===

    pub fn label() -> Style {
        Style::default()
            .fg(colors::fg_muted())
            .add_modifier(Modifier::BOLD)
    }

    // === Input/Form Styles ===

    /// Highlighted input text (yellow, bold)
    pub fn input_highlight() -> Style {
        Style::default()
            .fg(colors::fg_warning())
            .add_modifier(Modifier::BOLD)
    }

    /// Success/confirmation text (green)
    pub fn text_success() -> Style {
        Style::default().fg(colors::status_connected())
    }

    /// Link/URL text (cyan, underlined)
    pub fn text_link() -> Style {
        Style::default()
            .fg(colors::fg_accent())
            .add_modifier(Modifier::UNDERLINED)
    }

    // === Status Indicators ===

    /// Connected status indicator (green)
    pub fn status_connected() -> Style {
        Style::default()
            .bg(colors::bg_status())
            .fg(colors::status_connected())
    }

    /// Disconnected status indicator (red)
    pub fn status_disconnected() -> Style {
        Style::default()
            .bg(colors::bg_status())
            .fg(colors::status_disconnected())
    }

    /// Syncing/loading status indicator (yellow)
    pub fn status_syncing() -> Style {
        Style::default()
            .bg(colors::bg_status())
            .fg(colors::status_syncing())
    }

    /// Muted status text (low emphasis dividers and separators)
    pub fn status_muted() -> Style {
        Style::default()
            .bg(colors::bg_status())
            .fg(colors::fg_muted())
    }

    /// Status bar info text (readable secondary info like timestamps and memory)
    /// Uses primary color for better contrast on dark status bar background
    pub fn status_info() -> Style {
        Style::default()
            .bg(colors::bg_status())
            .fg(colors::fg_primary())
    }

    // === Account Indicators ===

    /// Account with new mail (cyan dot)
    pub fn account_new_mail() -> Style {
        Style::default()
            .bg(colors::bg_status())
            .fg(colors::unread_indicator())
    }

    /// Account with no new mail (muted)
    pub fn account_no_new() -> Style {
        Style::default()
            .bg(colors::bg_status())
            .fg(colors::fg_muted())
    }

    /// Account with error (red)
    pub fn account_error() -> Style {
        Style::default()
            .bg(colors::bg_status())
            .fg(colors::status_disconnected())
    }

    /// Account name text in status bar
    pub fn account_name() -> Style {
        Style::default()
            .bg(colors::bg_status())
            .fg(colors::fg_secondary())
    }

    /// New mail count badge
    pub fn account_badge() -> Style {
        Style::default()
            .bg(colors::bg_status())
            .fg(colors::thread_badge())
    }
}

/// Merge a style with selection background when selected.
/// This ensures the selection highlight covers the entire row.
pub fn with_selection_bg(style: Style, selected: bool) -> Style {
    if selected {
        style.bg(colors::bg_selection())
    } else {
        style
    }
}
