use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::ui::theme::Theme;

/// Renders text with visual quote bars for email quoted lines.
/// Alternating colors for nested quotes: cyan, yellow, magenta, green.
pub fn render_quoted_text(text: &str) -> Vec<Line<'_>> {
    let quote_colors: [Style; 4] = [
        Theme::text_accent(),      // Cyan - level 1
        Theme::star_indicator(),   // Yellow - level 2
        Theme::unread_indicator(), // Magenta - level 3
        Theme::text_success(),     // Green - level 4
    ];

    text.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with('>') {
                // Count quote depth (number of leading > characters)
                let depth = trimmed.chars().take_while(|&c| c == '>').count().min(4);
                // Build quote bar spans with alternating colors
                let mut spans: Vec<Span> = Vec::with_capacity(depth + 1);
                for i in 0..depth {
                    let color_idx = i % quote_colors.len();
                    spans.push(Span::styled("│ ", quote_colors[color_idx]));
                }
                // Strip leading > characters and spaces
                let content = trimmed.trim_start_matches('>').trim_start();
                spans.push(Span::styled(content, Theme::text_muted()));
                Line::from(spans)
            } else {
                Line::styled(line, Theme::text())
            }
        })
        .collect()
}
