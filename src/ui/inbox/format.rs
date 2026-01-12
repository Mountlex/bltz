//! Text formatting utilities for inbox rendering.

use aho_corasick::{AhoCorasick, AhoCorasickBuilder};
use ratatui::{style::Style, text::Span};
use std::sync::{Mutex, OnceLock};

static HIGHLIGHT_CACHE: OnceLock<Mutex<Option<(String, AhoCorasick)>>> = OnceLock::new();

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

    let ac = {
        let cache = HIGHLIGHT_CACHE.get_or_init(|| Mutex::new(None));
        let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((cached_query, cached_ac)) = guard.as_ref()
            && cached_query == query
        {
            cached_ac.clone()
        } else {
            let ac = match AhoCorasickBuilder::new()
                .ascii_case_insensitive(true)
                .build([query])
            {
                Ok(ac) => ac,
                Err(_) => return vec![Span::styled(text.to_string(), base_style)],
            };
            *guard = Some((query.to_string(), ac.clone()));
            ac
        }
    };

    let mut spans = Vec::new();
    let mut last_end = 0;

    for mat in ac.find_iter(text) {
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
