use super::TuiState;
use ratatui::prelude::*;

pub(crate) fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Header data is already in memory. Never discover credentials or read files on a frame.
pub(in crate::tui) fn build_header_sections(
    app: &dyn TuiState,
    width: u16,
) -> (Vec<Line<'static>>, Vec<Line<'static>>) {
    let model = app.provider_model();
    let (mark, mark_color) = crate::tui::provider_logo::badge(&app.provider_name());
    let prefix = "Anastasia · ";
    let mut text = format!("{mark} {model}");
    if let Some(directory) = app.working_dir() {
        text.push_str(" · ");
        text.push_str(&directory);
    }
    let text: String = text
        .chars()
        .take((width as usize).saturating_sub(prefix.chars().count()))
        .collect();
    // Only the provider mark is colored, so the model name keeps its contrast.
    let (mark_text, rest) = text.split_at(mark.len().min(text.len()));
    (
        vec![Line::from(vec![
            Span::raw(prefix),
            Span::styled(
                mark_text.to_string(),
                Style::default().fg(mark_color).bold(),
            ),
            Span::raw(rest.to_string()),
        ])],
        Vec::new(),
    )
}

pub(super) fn build_updates_box_lines(_width: u16, _max_lines: usize) -> Vec<Line<'static>> {
    Vec::new()
}

#[cfg(test)]
pub(crate) fn set_unseen_changelog_entries_override_for_tests(_entries: Option<Vec<String>>) {}
