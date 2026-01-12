//! Centralized theming for the bltz TUI
//!
//! This module provides a single source of truth for all colors and styles
//! used throughout the application.

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::BorderType;
use std::sync::OnceLock;

use crate::config::ThemeVariant;

/// Global theme variant storage
static THEME_VARIANT: OnceLock<ThemeVariant> = OnceLock::new();

/// Initialize the theme variant (call once at startup)
pub fn init_theme(variant: ThemeVariant) {
    THEME_VARIANT.set(variant).ok();
}

/// Get the current theme variant
pub fn current_theme() -> ThemeVariant {
    THEME_VARIANT.get().copied().unwrap_or_default()
}

/// Check if the terminal supports true color (24-bit RGB)
pub fn supports_true_color() -> bool {
    std::env::var("COLORTERM")
        .map(|v| v == "truecolor" || v == "24bit")
        .unwrap_or(false)
}

/// Catppuccin Mocha color palette for the Modern theme
#[allow(dead_code)]
mod catppuccin {
    use super::Color;

    // Background layers (darkest to lightest)
    pub const BASE: Color = Color::Rgb(30, 30, 46); // #1e1e2e - main background
    pub const MANTLE: Color = Color::Rgb(24, 24, 37); // #181825 - status bar, panels
    pub const SURFACE0: Color = Color::Rgb(49, 50, 68); // #313244 - borders
    pub const SURFACE1: Color = Color::Rgb(69, 71, 90); // #45475a - selection
    pub const SURFACE2: Color = Color::Rgb(88, 91, 112); // #585b70 - active borders

    // Text colors
    pub const TEXT: Color = Color::Rgb(205, 214, 244); // #cdd6f4 - primary
    pub const SUBTEXT1: Color = Color::Rgb(186, 194, 222); // #bac2de - secondary
    pub const SUBTEXT0: Color = Color::Rgb(166, 173, 200); // #a6adc8 - tertiary
    pub const OVERLAY0: Color = Color::Rgb(108, 112, 134); // #6c7086 - muted/disabled

    // Accent colors
    pub const LAVENDER: Color = Color::Rgb(180, 190, 254); // #b4befe - focused borders
    pub const BLUE: Color = Color::Rgb(137, 180, 250); // #89b4fa - links, accent
    pub const TEAL: Color = Color::Rgb(148, 226, 213); // #94e2d5 - secondary accent
    pub const GREEN: Color = Color::Rgb(166, 227, 161); // #a6e3a1 - success, connected
    pub const YELLOW: Color = Color::Rgb(249, 226, 175); // #f9e2af - warnings, stars
    pub const PEACH: Color = Color::Rgb(250, 179, 135); // #fab387 - thread badges
    pub const RED: Color = Color::Rgb(243, 139, 168); // #f38ba8 - errors
    pub const MAUVE: Color = Color::Rgb(203, 166, 247); // #cba6f7 - unread indicator
}

/// Border type helpers for different UI contexts
pub mod borders {
    use super::*;

    /// Border type for popups and modals (rounded in Modern theme)
    pub fn popup() -> BorderType {
        match current_theme() {
            ThemeVariant::Modern => BorderType::Rounded,
            _ => BorderType::Plain,
        }
    }

    /// Border type for focused input fields (rounded in Modern theme)
    pub fn input_focused() -> BorderType {
        match current_theme() {
            ThemeVariant::Modern => BorderType::Rounded,
            _ => BorderType::Plain,
        }
    }

    /// Border type for main panels (always plain for clean look)
    pub fn panel() -> BorderType {
        BorderType::Plain
    }
}

/// Color palette - colors that vary by theme
pub mod colors {
    use super::*;

    // Selection
    pub fn bg_selection() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::SURFACE1,
            ThemeVariant::Dark => Color::Blue,
            ThemeVariant::HighContrast => Color::LightBlue,
        }
    }

    // Status bars
    pub fn bg_status() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::MANTLE,
            ThemeVariant::Dark => Color::DarkGray,
            ThemeVariant::HighContrast => Color::Black,
        }
    }

    pub fn bg_error() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::RED,
            _ => Color::Red,
        }
    }

    // Text colors
    pub fn fg_primary() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::TEXT,
            _ => Color::White,
        }
    }

    pub fn fg_secondary() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::SUBTEXT1,
            ThemeVariant::Dark => Color::Gray,
            ThemeVariant::HighContrast => Color::White,
        }
    }

    pub fn fg_muted() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::OVERLAY0,
            ThemeVariant::Dark => Color::Gray,
            ThemeVariant::HighContrast => Color::Gray,
        }
    }

    pub fn fg_accent() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::BLUE,
            ThemeVariant::Dark => Color::Cyan,
            ThemeVariant::HighContrast => Color::LightCyan,
        }
    }

    pub fn fg_warning() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::YELLOW,
            ThemeVariant::Dark => Color::Yellow,
            ThemeVariant::HighContrast => Color::LightYellow,
        }
    }

    // Semantic
    pub fn unread_indicator() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::MAUVE,
            ThemeVariant::Dark => Color::Magenta,
            ThemeVariant::HighContrast => Color::LightMagenta,
        }
    }

    pub fn thread_badge() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::PEACH,
            ThemeVariant::Dark => Color::Yellow,
            ThemeVariant::HighContrast => Color::LightYellow,
        }
    }

    pub fn border() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::SURFACE0,
            ThemeVariant::Dark => Color::DarkGray,
            ThemeVariant::HighContrast => Color::Gray,
        }
    }

    pub fn border_focused() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::LAVENDER,
            ThemeVariant::Dark => Color::Cyan,
            ThemeVariant::HighContrast => Color::LightCyan,
        }
    }

    // Status colors
    pub fn status_connected() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::GREEN,
            ThemeVariant::Dark => Color::Green,
            ThemeVariant::HighContrast => Color::LightGreen,
        }
    }

    pub fn status_disconnected() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::RED,
            ThemeVariant::Dark => Color::Red,
            ThemeVariant::HighContrast => Color::LightRed,
        }
    }

    pub fn status_syncing() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::YELLOW,
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
    #[allow(dead_code)]
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
