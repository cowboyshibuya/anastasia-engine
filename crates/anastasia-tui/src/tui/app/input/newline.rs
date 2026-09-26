//! Newline insertion in the composer, including a keymap-independent fallback.
//!
//! Shift+Enter needs the terminal to disambiguate a modified Enter, which in
//! practice means the kitty keyboard protocol: every Enter chord is otherwise
//! the same byte (`0x0d`). Most modern terminals support it and anastasia requests
//! it at startup, so Shift+Enter works out of the box there. `/terminal-setup`
//! fixes the cases that need configuration (tmux, WezTerm) and explains the ones
//! that cannot be fixed (Terminal.app). See `docs/SHIFT_ENTER.md`.
//!
//! Option/Alt+Enter arrives as `ESC` + `CR`, which does not need the protocol,
//! so it works on more terminals but depends on the Option-as-Meta setting.
//!
//! Ctrl+J is the fallback when the terminal cannot distinguish Shift+Enter.

use crossterm::event::{KeyCode, KeyModifiers};

use super::super::App;
use super::insert_input_text;

/// Handles every way an Enter press can insert a newline instead of sending.
///
/// Returns true when the key was consumed as a newline.
pub(in crate::tui::app) fn enter_inserts_newline(
    app: &mut App,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    // Handle the post-paste Enter before suggestions or any send path, in every connection state.
    if code == KeyCode::Enter
        && modifiers.is_empty()
        && super::paste_guard::consume_paste_trailing_enter()
    {
        return true;
    }
    if (code == KeyCode::Char('j') && modifiers == KeyModifiers::CONTROL)
        || (code == KeyCode::Enter && modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT))
    {
        insert_input_text(app, "\n");
        return true;
    }
    false
}
