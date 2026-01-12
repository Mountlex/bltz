//! Centralized theming for the bltz TUI
//!
//! This module provides a single source of truth for all colors and styles
//! used throughout the application.

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::BorderType;
use std::sync::RwLock;

use crate::config::ThemeVariant;

/// Global theme variant storage (RwLock allows runtime theme switching)
static THEME_VARIANT: RwLock<ThemeVariant> = RwLock::new(ThemeVariant::Modern);

/// Initialize the theme variant (call once at startup)
pub fn init_theme(variant: ThemeVariant) {
    if let Ok(mut guard) = THEME_VARIANT.write() {
        *guard = variant;
    }
}

/// Set the theme at runtime
pub fn set_theme(variant: ThemeVariant) {
    if let Ok(mut guard) = THEME_VARIANT.write() {
        *guard = variant;
    }
}

/// Get the current theme variant
pub fn current_theme() -> ThemeVariant {
    THEME_VARIANT.read().map(|g| *g).unwrap_or_default()
}

/// Get all available theme names for command completion
pub fn available_themes() -> &'static [&'static str] {
    &[
        "modern",
        "dark",
        "high-contrast",
        "solarized-dark",
        "solarized-light",
        "tokyo-night",
        "tokyo-day",
        "rose-pine",
        "rose-pine-dawn",
    ]
}

/// Check if the terminal supports true color (24-bit RGB)
pub fn supports_true_color() -> bool {
    std::env::var("COLORTERM")
        .map(|v| v == "truecolor" || v == "24bit")
        .unwrap_or(false)
}

/// Detect if the system is in dark mode
pub fn detect_system_dark_mode() -> bool {
    // Check BLTZ_COLOR_SCHEME env var first (user override)
    if let Ok(val) = std::env::var("BLTZ_COLOR_SCHEME") {
        return val.to_lowercase() != "light";
    }

    // Try GNOME detection via gsettings
    if let Ok(output) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
        // GNOME color-scheme values: 'default', 'prefer-dark', 'prefer-light'
        if stdout.contains("prefer-light") {
            return false;
        }
        if stdout.contains("prefer-dark") {
            return true;
        }
        // If 'default', check the GTK theme name
        if stdout.contains("default")
            && let Ok(theme_output) = std::process::Command::new("gsettings")
                .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
                .output()
        {
            let theme = String::from_utf8_lossy(&theme_output.stdout).to_lowercase();
            // Themes with "-dark" suffix are dark themes
            // e.g., "Adwaita-dark", "Yaru-dark"
            return theme.contains("-dark") || theme.contains("dark");
        }
    }

    // Try KDE detection via kdeglobals
    if let Some(config_dir) = dirs::config_dir()
        && let Ok(content) = std::fs::read_to_string(config_dir.join("kdeglobals"))
    {
        for line in content.lines() {
            if line.starts_with("ColorScheme=") {
                return line.to_lowercase().contains("dark");
            }
        }
    }

    // Default to dark mode
    true
}

/// Check if modern spacing should be used (extra padding, gaps between threads)
pub fn use_modern_spacing() -> bool {
    matches!(current_theme(), ThemeVariant::Modern)
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

/// Solarized Dark color palette
/// Official colors from https://ethanschoonover.com/solarized/
#[allow(dead_code)]
mod solarized {
    use super::Color;

    // Background layers (dark to light)
    pub const BASE03: Color = Color::Rgb(0, 43, 54); // #002b36 - main background
    pub const BASE02: Color = Color::Rgb(7, 54, 66); // #073642 - secondary bg, selection
    pub const BASE01: Color = Color::Rgb(88, 110, 117); // #586e75 - comments, muted

    // Text colors
    pub const BASE0: Color = Color::Rgb(131, 148, 150); // #839496 - primary text
    pub const BASE1: Color = Color::Rgb(147, 161, 161); // #93a1a1 - secondary text

    // Accent colors
    pub const YELLOW: Color = Color::Rgb(181, 137, 0); // #b58900 - warnings, stars
    pub const ORANGE: Color = Color::Rgb(203, 75, 22); // #cb4b16 - thread badges
    pub const RED: Color = Color::Rgb(220, 50, 47); // #dc322f - errors
    pub const MAGENTA: Color = Color::Rgb(211, 54, 130); // #d33682 - unread indicator
    pub const BLUE: Color = Color::Rgb(38, 139, 210); // #268bd2 - links, accent
    pub const CYAN: Color = Color::Rgb(42, 161, 152); // #2aa198 - borders focused
    pub const GREEN: Color = Color::Rgb(133, 153, 0); // #859900 - success, connected
}

/// Tokyo Night color palette
/// Official colors from https://github.com/folke/tokyonight.nvim
#[allow(dead_code)]
mod tokyo_night {
    use super::Color;

    // Background layers
    pub const BG: Color = Color::Rgb(26, 27, 38); // #1a1b26 - main background
    pub const BG_DARK: Color = Color::Rgb(22, 22, 30); // #16161e - status bar, panels
    pub const BG_HIGHLIGHT: Color = Color::Rgb(41, 46, 66); // #292e42 - selection

    // Text colors
    pub const FG: Color = Color::Rgb(192, 202, 245); // #c0caf5 - primary text
    pub const FG_DARK: Color = Color::Rgb(169, 177, 214); // #a9b1d6 - secondary text
    pub const COMMENT: Color = Color::Rgb(86, 95, 137); // #565f89 - muted/comments

    // Accent colors
    pub const BLUE: Color = Color::Rgb(122, 162, 247); // #7aa2f7 - links, accent
    pub const CYAN: Color = Color::Rgb(125, 207, 255); // #7dcfff - borders focused
    pub const GREEN: Color = Color::Rgb(158, 206, 106); // #9ece6a - success
    pub const YELLOW: Color = Color::Rgb(224, 175, 104); // #e0af68 - warnings, stars
    pub const ORANGE: Color = Color::Rgb(255, 158, 100); // #ff9e64 - thread badges
    pub const RED: Color = Color::Rgb(247, 118, 142); // #f7768e - errors
    pub const MAGENTA: Color = Color::Rgb(187, 154, 247); // #bb9af7 - unread indicator
    pub const BORDER: Color = Color::Rgb(59, 66, 97); // #3b4261 - borders
}

/// Rosé Pine color palette
/// Official colors from https://rosepinetheme.com/palette/
#[allow(dead_code)]
mod rose_pine {
    use super::Color;

    // Background layers
    pub const BASE: Color = Color::Rgb(25, 23, 36); // #191724 - main background
    pub const SURFACE: Color = Color::Rgb(31, 29, 46); // #1f1d2e - status bar, panels
    pub const HIGHLIGHT_MED: Color = Color::Rgb(64, 61, 82); // #403d52 - selection

    // Text colors
    pub const TEXT: Color = Color::Rgb(224, 222, 244); // #e0def4 - primary text
    pub const SUBTLE: Color = Color::Rgb(144, 140, 170); // #908caa - secondary text
    pub const MUTED: Color = Color::Rgb(110, 106, 134); // #6e6a86 - muted/disabled

    // Accent colors
    pub const LOVE: Color = Color::Rgb(235, 111, 146); // #eb6f92 - errors, red
    pub const GOLD: Color = Color::Rgb(246, 193, 119); // #f6c177 - warnings, stars
    pub const ROSE: Color = Color::Rgb(235, 188, 186); // #ebbcba - thread badges
    pub const FOAM: Color = Color::Rgb(156, 207, 216); // #9ccfd8 - success, connected
    pub const IRIS: Color = Color::Rgb(196, 167, 231); // #c4a7e7 - links, unread, accent
}

/// Solarized Light color palette
/// Official colors from https://ethanschoonover.com/solarized/
#[allow(dead_code)]
mod solarized_light {
    use super::Color;

    // Background layers (light to dark)
    pub const BASE3: Color = Color::Rgb(253, 246, 227); // #fdf6e3 - main background
    pub const BASE2: Color = Color::Rgb(238, 232, 213); // #eee8d5 - secondary bg, selection
    pub const BASE1: Color = Color::Rgb(147, 161, 161); // #93a1a1 - comments, muted

    // Text colors (inverted from dark variant)
    pub const BASE00: Color = Color::Rgb(101, 123, 131); // #657b83 - primary text
    pub const BASE01: Color = Color::Rgb(88, 110, 117); // #586e75 - secondary text

    // Accent colors (same as dark variant)
    pub const YELLOW: Color = Color::Rgb(181, 137, 0); // #b58900 - warnings, stars
    pub const ORANGE: Color = Color::Rgb(203, 75, 22); // #cb4b16 - thread badges
    pub const RED: Color = Color::Rgb(220, 50, 47); // #dc322f - errors
    pub const MAGENTA: Color = Color::Rgb(211, 54, 130); // #d33682 - unread indicator
    pub const BLUE: Color = Color::Rgb(38, 139, 210); // #268bd2 - links, accent
    pub const CYAN: Color = Color::Rgb(42, 161, 152); // #2aa198 - borders focused
    pub const GREEN: Color = Color::Rgb(133, 153, 0); // #859900 - success, connected
}

/// Tokyo Night Day color palette
/// Official colors from https://github.com/folke/tokyonight.nvim (day variant)
#[allow(dead_code)]
mod tokyo_day {
    use super::Color;

    // Background layers
    pub const BG: Color = Color::Rgb(228, 231, 235); // #e4e7eb - main background (day)
    pub const BG_DARK: Color = Color::Rgb(214, 217, 224); // #d6d9e0 - status bar, panels
    pub const BG_HIGHLIGHT: Color = Color::Rgb(199, 203, 214); // #c7cbd6 - selection

    // Text colors
    pub const FG: Color = Color::Rgb(59, 66, 97); // #3b4261 - primary text
    pub const FG_DARK: Color = Color::Rgb(107, 112, 137); // #6b7089 - secondary text
    pub const COMMENT: Color = Color::Rgb(143, 150, 178); // #8f96b2 - muted/comments

    // Accent colors (slightly adjusted for light bg)
    pub const BLUE: Color = Color::Rgb(52, 84, 138); // #34548a - links, accent
    pub const CYAN: Color = Color::Rgb(0, 127, 135); // #007f87 - borders focused
    pub const GREEN: Color = Color::Rgb(72, 117, 53); // #487535 - success
    pub const YELLOW: Color = Color::Rgb(143, 95, 32); // #8f5f20 - warnings, stars
    pub const ORANGE: Color = Color::Rgb(181, 84, 58); // #b5543a - thread badges
    pub const RED: Color = Color::Rgb(180, 75, 85); // #b44b55 - errors
    pub const MAGENTA: Color = Color::Rgb(125, 92, 168); // #7d5ca8 - unread indicator
    pub const BORDER: Color = Color::Rgb(179, 183, 194); // #b3b7c2 - borders
}

/// Rosé Pine Dawn color palette
/// Official colors from https://rosepinetheme.com/palette/ (dawn variant)
#[allow(dead_code)]
mod rose_pine_dawn {
    use super::Color;

    // Background layers
    pub const BASE: Color = Color::Rgb(250, 244, 237); // #faf4ed - main background
    pub const SURFACE: Color = Color::Rgb(255, 250, 243); // #fffaf3 - status bar, panels
    pub const HIGHLIGHT_MED: Color = Color::Rgb(223, 218, 210); // #dfdad2 - selection

    // Text colors
    pub const TEXT: Color = Color::Rgb(87, 82, 121); // #575279 - primary text
    pub const SUBTLE: Color = Color::Rgb(121, 117, 147); // #797593 - secondary text
    pub const MUTED: Color = Color::Rgb(152, 147, 165); // #9893a5 - muted/disabled

    // Accent colors (adjusted for light bg)
    pub const LOVE: Color = Color::Rgb(180, 99, 122); // #b4637a - errors, red
    pub const GOLD: Color = Color::Rgb(234, 157, 52); // #ea9d34 - warnings, stars
    pub const ROSE: Color = Color::Rgb(215, 130, 126); // #d7827e - thread badges
    pub const FOAM: Color = Color::Rgb(40, 105, 131); // #286983 - success, connected
    pub const IRIS: Color = Color::Rgb(144, 122, 169); // #907aa9 - links, unread, accent
}

/// Border type helpers for different UI contexts
pub mod borders {
    use super::*;

    /// Border type for popups and modals (rounded for RGB themes)
    pub fn popup() -> BorderType {
        match current_theme() {
            ThemeVariant::Modern
            | ThemeVariant::SolarizedDark
            | ThemeVariant::SolarizedLight
            | ThemeVariant::TokyoNight
            | ThemeVariant::TokyoDay
            | ThemeVariant::RosePine
            | ThemeVariant::RosePineDawn => BorderType::Rounded,
            _ => BorderType::Plain,
        }
    }

    /// Border type for focused input fields (rounded for RGB themes)
    pub fn input_focused() -> BorderType {
        match current_theme() {
            ThemeVariant::Modern
            | ThemeVariant::SolarizedDark
            | ThemeVariant::SolarizedLight
            | ThemeVariant::TokyoNight
            | ThemeVariant::TokyoDay
            | ThemeVariant::RosePine
            | ThemeVariant::RosePineDawn => BorderType::Rounded,
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
            ThemeVariant::SolarizedDark => solarized::BASE02,
            ThemeVariant::SolarizedLight => solarized_light::BASE2,
            ThemeVariant::TokyoNight => tokyo_night::BG_HIGHLIGHT,
            ThemeVariant::TokyoDay => tokyo_day::BG_HIGHLIGHT,
            ThemeVariant::RosePine => rose_pine::HIGHLIGHT_MED,
            ThemeVariant::RosePineDawn => rose_pine_dawn::HIGHLIGHT_MED,
            ThemeVariant::Dark | ThemeVariant::HighContrast => Color::LightBlue,
        }
    }

    // Status bars
    pub fn bg_status() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::MANTLE,
            ThemeVariant::SolarizedDark => solarized::BASE02,
            ThemeVariant::SolarizedLight => solarized_light::BASE2,
            ThemeVariant::TokyoNight => tokyo_night::BG_DARK,
            ThemeVariant::TokyoDay => tokyo_day::BG_DARK,
            ThemeVariant::RosePine => rose_pine::SURFACE,
            ThemeVariant::RosePineDawn => rose_pine_dawn::SURFACE,
            ThemeVariant::Dark => Color::DarkGray,
            ThemeVariant::HighContrast => Color::Black,
        }
    }

    pub fn bg_error() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::RED,
            ThemeVariant::SolarizedDark | ThemeVariant::SolarizedLight => solarized::RED,
            ThemeVariant::TokyoNight => tokyo_night::RED,
            ThemeVariant::TokyoDay => tokyo_day::RED,
            ThemeVariant::RosePine => rose_pine::LOVE,
            ThemeVariant::RosePineDawn => rose_pine_dawn::LOVE,
            _ => Color::Red,
        }
    }

    // Text colors
    pub fn fg_primary() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::TEXT,
            ThemeVariant::SolarizedDark => solarized::BASE0,
            ThemeVariant::SolarizedLight => solarized_light::BASE00,
            ThemeVariant::TokyoNight => tokyo_night::FG,
            ThemeVariant::TokyoDay => tokyo_day::FG,
            ThemeVariant::RosePine => rose_pine::TEXT,
            ThemeVariant::RosePineDawn => rose_pine_dawn::TEXT,
            _ => Color::White,
        }
    }

    pub fn fg_secondary() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::SUBTEXT1,
            ThemeVariant::SolarizedDark => solarized::BASE1,
            ThemeVariant::SolarizedLight => solarized_light::BASE01,
            ThemeVariant::TokyoNight => tokyo_night::FG_DARK,
            ThemeVariant::TokyoDay => tokyo_day::FG_DARK,
            ThemeVariant::RosePine => rose_pine::SUBTLE,
            ThemeVariant::RosePineDawn => rose_pine_dawn::SUBTLE,
            ThemeVariant::Dark => Color::Gray,
            ThemeVariant::HighContrast => Color::White,
        }
    }

    pub fn fg_muted() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::OVERLAY0,
            ThemeVariant::SolarizedDark => solarized::BASE01,
            ThemeVariant::SolarizedLight => solarized_light::BASE1,
            ThemeVariant::TokyoNight => tokyo_night::COMMENT,
            ThemeVariant::TokyoDay => tokyo_day::COMMENT,
            ThemeVariant::RosePine => rose_pine::MUTED,
            ThemeVariant::RosePineDawn => rose_pine_dawn::MUTED,
            ThemeVariant::Dark | ThemeVariant::HighContrast => Color::Gray,
        }
    }

    pub fn fg_accent() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::BLUE,
            ThemeVariant::SolarizedDark | ThemeVariant::SolarizedLight => solarized::BLUE,
            ThemeVariant::TokyoNight => tokyo_night::BLUE,
            ThemeVariant::TokyoDay => tokyo_day::BLUE,
            ThemeVariant::RosePine => rose_pine::IRIS,
            ThemeVariant::RosePineDawn => rose_pine_dawn::IRIS,
            ThemeVariant::Dark => Color::Cyan,
            ThemeVariant::HighContrast => Color::LightCyan,
        }
    }

    pub fn fg_warning() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::YELLOW,
            ThemeVariant::SolarizedDark | ThemeVariant::SolarizedLight => solarized::YELLOW,
            ThemeVariant::TokyoNight => tokyo_night::YELLOW,
            ThemeVariant::TokyoDay => tokyo_day::YELLOW,
            ThemeVariant::RosePine => rose_pine::GOLD,
            ThemeVariant::RosePineDawn => rose_pine_dawn::GOLD,
            ThemeVariant::Dark => Color::Yellow,
            ThemeVariant::HighContrast => Color::LightYellow,
        }
    }

    // Semantic
    pub fn unread_indicator() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::MAUVE,
            ThemeVariant::SolarizedDark | ThemeVariant::SolarizedLight => solarized::MAGENTA,
            ThemeVariant::TokyoNight => tokyo_night::MAGENTA,
            ThemeVariant::TokyoDay => tokyo_day::MAGENTA,
            ThemeVariant::RosePine => rose_pine::IRIS,
            ThemeVariant::RosePineDawn => rose_pine_dawn::IRIS,
            ThemeVariant::Dark => Color::Magenta,
            ThemeVariant::HighContrast => Color::LightMagenta,
        }
    }

    pub fn thread_badge() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::PEACH,
            ThemeVariant::SolarizedDark | ThemeVariant::SolarizedLight => solarized::ORANGE,
            ThemeVariant::TokyoNight => tokyo_night::ORANGE,
            ThemeVariant::TokyoDay => tokyo_day::ORANGE,
            ThemeVariant::RosePine => rose_pine::ROSE,
            ThemeVariant::RosePineDawn => rose_pine_dawn::ROSE,
            ThemeVariant::Dark => Color::Yellow,
            ThemeVariant::HighContrast => Color::LightYellow,
        }
    }

    pub fn border() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::SURFACE0,
            ThemeVariant::SolarizedDark => solarized::BASE01,
            ThemeVariant::SolarizedLight => solarized_light::BASE1,
            ThemeVariant::TokyoNight => tokyo_night::BORDER,
            ThemeVariant::TokyoDay => tokyo_day::BORDER,
            ThemeVariant::RosePine => rose_pine::MUTED,
            ThemeVariant::RosePineDawn => rose_pine_dawn::MUTED,
            ThemeVariant::Dark => Color::DarkGray,
            ThemeVariant::HighContrast => Color::Gray,
        }
    }

    pub fn border_focused() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::LAVENDER,
            ThemeVariant::SolarizedDark | ThemeVariant::SolarizedLight => solarized::CYAN,
            ThemeVariant::TokyoNight => tokyo_night::CYAN,
            ThemeVariant::TokyoDay => tokyo_day::CYAN,
            ThemeVariant::RosePine => rose_pine::IRIS,
            ThemeVariant::RosePineDawn => rose_pine_dawn::IRIS,
            ThemeVariant::Dark => Color::Cyan,
            ThemeVariant::HighContrast => Color::LightCyan,
        }
    }

    // Status colors
    pub fn status_connected() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::GREEN,
            ThemeVariant::SolarizedDark | ThemeVariant::SolarizedLight => solarized::GREEN,
            ThemeVariant::TokyoNight => tokyo_night::GREEN,
            ThemeVariant::TokyoDay => tokyo_day::GREEN,
            ThemeVariant::RosePine => rose_pine::FOAM,
            ThemeVariant::RosePineDawn => rose_pine_dawn::FOAM,
            ThemeVariant::Dark => Color::Green,
            ThemeVariant::HighContrast => Color::LightGreen,
        }
    }

    pub fn status_disconnected() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::RED,
            ThemeVariant::SolarizedDark | ThemeVariant::SolarizedLight => solarized::RED,
            ThemeVariant::TokyoNight => tokyo_night::RED,
            ThemeVariant::TokyoDay => tokyo_day::RED,
            ThemeVariant::RosePine => rose_pine::LOVE,
            ThemeVariant::RosePineDawn => rose_pine_dawn::LOVE,
            ThemeVariant::Dark => Color::Red,
            ThemeVariant::HighContrast => Color::LightRed,
        }
    }

    pub fn status_syncing() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::YELLOW,
            ThemeVariant::SolarizedDark | ThemeVariant::SolarizedLight => solarized::YELLOW,
            ThemeVariant::TokyoNight => tokyo_night::YELLOW,
            ThemeVariant::TokyoDay => tokyo_day::YELLOW,
            ThemeVariant::RosePine => rose_pine::GOLD,
            ThemeVariant::RosePineDawn => rose_pine_dawn::GOLD,
            ThemeVariant::Dark => Color::Yellow,
            ThemeVariant::HighContrast => Color::LightYellow,
        }
    }

    /// Background for help bar (same as status bar)
    pub fn bg_help() -> Color {
        bg_status()
    }

    /// Main background color for the entire UI
    /// This should be used to fill the terminal background for light themes
    pub fn bg_main() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::BASE,
            ThemeVariant::SolarizedDark => solarized::BASE03,
            ThemeVariant::SolarizedLight => solarized_light::BASE3,
            ThemeVariant::TokyoNight => tokyo_night::BG,
            ThemeVariant::TokyoDay => tokyo_day::BG,
            ThemeVariant::RosePine => rose_pine::BASE,
            ThemeVariant::RosePineDawn => rose_pine_dawn::BASE,
            // Basic themes use terminal default
            _ => Color::Reset,
        }
    }

    /// Background for thread gap lines in modern theme
    pub fn bg_thread_gap() -> Color {
        match current_theme() {
            ThemeVariant::Modern => catppuccin::BASE,
            ThemeVariant::SolarizedDark => solarized::BASE03,
            ThemeVariant::SolarizedLight => solarized_light::BASE3,
            ThemeVariant::TokyoNight => tokyo_night::BG,
            ThemeVariant::TokyoDay => tokyo_day::BG,
            ThemeVariant::RosePine => rose_pine::BASE,
            ThemeVariant::RosePineDawn => rose_pine_dawn::BASE,
            _ => Color::Reset,
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

    /// Normal text (with main background for light themes)
    pub fn text() -> Style {
        Style::default()
            .fg(colors::fg_primary())
            .bg(colors::bg_main())
    }

    /// Secondary/read text (with main background for light themes)
    pub fn text_secondary() -> Style {
        Style::default()
            .fg(colors::fg_secondary())
            .bg(colors::bg_main())
    }

    /// Muted/disabled text (with main background for light themes)
    pub fn text_muted() -> Style {
        Style::default()
            .fg(colors::fg_muted())
            .bg(colors::bg_main())
    }

    /// Unread/bold text (with main background for light themes)
    pub fn text_unread() -> Style {
        Style::default()
            .fg(colors::fg_primary())
            .bg(colors::bg_main())
            .add_modifier(Modifier::BOLD)
    }

    /// Accent colored text (with main background for light themes)
    pub fn text_accent() -> Style {
        Style::default()
            .fg(colors::fg_accent())
            .bg(colors::bg_main())
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

    pub fn help_bar() -> Style {
        Style::default()
            .bg(colors::bg_help())
            .fg(colors::fg_primary())
    }

    pub fn help_key() -> Style {
        Style::default()
            .bg(colors::bg_help())
            .fg(colors::fg_warning())
    }

    pub fn help_desc() -> Style {
        Style::default()
            .bg(colors::bg_help())
            .fg(colors::fg_muted())
    }

    // === Thread Gap ===

    /// Style for gap lines between threads in modern theme
    pub fn thread_gap() -> Style {
        Style::default().bg(colors::bg_thread_gap())
    }

    // === Borders ===

    pub fn border() -> Style {
        Style::default().fg(colors::border()).bg(colors::bg_main())
    }

    pub fn border_focused() -> Style {
        Style::default()
            .fg(colors::border_focused())
            .bg(colors::bg_main())
    }

    /// Main background style - use to fill the entire frame for light themes
    pub fn main_bg() -> Style {
        Style::default().bg(colors::bg_main())
    }

    // === Indicators ===

    pub fn unread_indicator() -> Style {
        Style::default()
            .fg(colors::unread_indicator())
            .bg(colors::bg_main())
    }

    /// Unread indicator that preserves selection background
    pub fn unread_indicator_selected() -> Style {
        Style::default()
            .bg(colors::bg_selection())
            .fg(colors::unread_indicator())
    }

    /// Star indicator (yellow)
    pub fn star_indicator() -> Style {
        Style::default()
            .fg(colors::fg_warning())
            .bg(colors::bg_main())
    }

    /// Star indicator that preserves selection background
    pub fn star_indicator_selected() -> Style {
        Style::default()
            .bg(colors::bg_selection())
            .fg(colors::fg_warning())
    }

    /// Replied indicator (muted)
    pub fn replied_indicator() -> Style {
        Style::default()
            .fg(colors::fg_muted())
            .bg(colors::bg_main())
    }

    /// Replied indicator that preserves selection background
    pub fn replied_indicator_selected() -> Style {
        Style::default()
            .bg(colors::bg_selection())
            .fg(colors::fg_muted())
    }

    pub fn thread_badge() -> Style {
        Style::default()
            .fg(colors::thread_badge())
            .bg(colors::bg_main())
    }

    // === Labels ===

    pub fn label() -> Style {
        Style::default()
            .fg(colors::fg_muted())
            .bg(colors::bg_main())
            .add_modifier(Modifier::BOLD)
    }

    // === Input/Form Styles ===

    /// Highlighted input text (yellow, bold)
    pub fn input_highlight() -> Style {
        Style::default()
            .fg(colors::fg_warning())
            .bg(colors::bg_main())
            .add_modifier(Modifier::BOLD)
    }

    /// Success/confirmation text (green)
    pub fn text_success() -> Style {
        Style::default()
            .fg(colors::status_connected())
            .bg(colors::bg_main())
    }

    /// Link/URL text (cyan, underlined)
    pub fn text_link() -> Style {
        Style::default()
            .fg(colors::fg_accent())
            .bg(colors::bg_main())
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

/// Merge a style with selection or main background.
/// This ensures the selection highlight covers the entire row,
/// and non-selected items have the proper background for light themes.
pub fn with_selection_bg(style: Style, selected: bool) -> Style {
    if selected {
        style.bg(colors::bg_selection())
    } else {
        style.bg(colors::bg_main())
    }
}
