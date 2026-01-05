use ratatui::layout::Rect;

/// Creates a centered rect with min/max constraints for width and height.
/// The actual size is clamped between min and max, then constrained to fit within area.
pub fn centered_rect_constrained(
    area: Rect,
    min_width: u16,
    max_width: u16,
    min_height: u16,
    max_height: u16,
) -> Rect {
    // Calculate actual dimensions: prefer max, constrain to available space, respect min
    let w = max_width.min(area.width.saturating_sub(4)).max(min_width);
    let h = max_height
        .min(area.height.saturating_sub(4))
        .max(min_height);

    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}
