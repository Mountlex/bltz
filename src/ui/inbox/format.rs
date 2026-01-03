//! Text formatting utilities for inbox rendering.

use aho_corasick::AhoCorasick;
use ratatui::{style::Style, text::Span};

/// Highlight query matches in text, returning multiple styled spans.
/// Uses aho-corasick for efficient case-insensitive matching.
pub fn highlight_matches(
    text: &str,
    query: &str,
    base_style: Style,
    highlight_style: Style,
) -> Vec<Span<'static>> {
    if query.is_empty() {
        return vec![Span::styled(text.to_string(), base_style)];
    }

    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();

    // Build aho-corasick automaton for case-insensitive matching
    let ac = match AhoCorasick::new([&query_lower]) {
        Ok(ac) => ac,
        Err(_) => return vec![Span::styled(text.to_string(), base_style)],
    };

    let mut spans = Vec::new();
    let mut last_end = 0;

    for mat in ac.find_iter(&text_lower) {
        // Add non-matching text before this match
        if mat.start() > last_end {
            spans.push(Span::styled(
                text[last_end..mat.start()].to_string(),
                base_style,
            ));
        }
        // Add highlighted match (use original case from text)
        spans.push(Span::styled(
            text[mat.start()..mat.end()].to_string(),
            highlight_style,
        ));
        last_end = mat.end();
    }

    // Add remaining text after last match
    if last_end < text.len() {
        spans.push(Span::styled(text[last_end..].to_string(), base_style));
    }

    if spans.is_empty() {
        vec![Span::styled(text.to_string(), base_style)]
    } else {
        spans
    }
}
