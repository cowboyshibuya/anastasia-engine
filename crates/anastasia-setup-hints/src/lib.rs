//! Platform setup hints shown on startup.
//!
//! - Windows: suggest native Alt launch hotkeys plus Copilot-key setup and terminal setup.
//! - macOS: if the user is on the default built-in Terminal.app, show a one-time
//!   notice that it renders anastasia poorly and suggest a modern terminal (Ghostty).
//! - Linux: no global launcher setup; terminal keybinding hints still apply.
//!
//! Each nudge can be dismissed permanently with "Don't ask again".
//! State is persisted in `~/.anastasia-cli/setup_hints.json`.

// Several launch-hotkey helpers are gated `#[cfg(any(test, target_os = "macos"))]`
// because the unit tests exercise the macOS launch-hotkey notice logic on every
// platform. In a non-macOS *test* build their only production callers (the
// `#[cfg(target_os = "macos")]` notice/install paths) are compiled out, so the
// helpers the tests don't call directly look dead. They are real macOS code, so
// silence dead_code only for that specific build shape instead of deleting them.
#![cfg_attr(all(test, not(target_os = "macos")), allow(dead_code))]

use anastasia_storage as storage;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, IsTerminal};
use std::path::PathBuf;

pub mod keymap;

#[cfg(any(test, windows))]
mod windows_hotkeys;
#[cfg(windows)]
mod windows_setup;
#[cfg(windows)]
use windows_setup::{
    create_windows_desktop_shortcut, maybe_show_windows_setup_hints, run_setup_hotkey_windows,
    run_windows_hotkey_listener, uninstall_windows_hotkey_listener,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SetupHintsState {
    pub launch_count: u64,
    pub hotkey_configured: bool,
    pub hotkey_dismissed: bool,
    #[serde(alias = "wezterm_configured")]
    pub alacritty_configured: bool,
    #[serde(alias = "wezterm_dismissed")]
    pub alacritty_dismissed: bool,
    #[serde(default)]
    pub desktop_shortcut_created: bool,
    #[serde(default = "default_true")]
    pub startup_spawn_hint_dismissed: bool,
    pub mac_ghostty_guided: bool,
    pub mac_ghostty_dismissed: bool,
    /// Number of times we have shown the terminal/setup nudge prompt to the user
    /// (across all platforms). Used to cap the total number of nudges so we never
    /// pester someone forever if they keep choosing "Not now".
    #[serde(default)]
    pub terminal_nudge_count: u64,
    /// Version of the installed macOS Cmd+; hotkey listener. Bumped when the
    /// listener implementation changes in a way that requires reinstalling the
    /// LaunchAgent for already-configured users (e.g. the run-loop fix that made
    /// the hotkey actually fire). `0` = legacy/unknown.
    #[serde(default)]
    pub hotkey_listener_version: u32,
    /// Version of the cross-platform launch command metadata. Bumped when
    /// generated macOS/Linux/Windows launchers need refreshing so successful
    /// shortcut use continues feeding the learned-keybinding state.
    #[serde(default)]
    pub launch_hotkey_tracking_version: u32,
    /// Canonical signature of the keybinding conflicts we last warned the user
    /// about (sorted, joined chord+field pairs). Empty means "no conflicts known
    /// / never warned". We only re-show the startup conflict notice when this
    /// signature changes, so users are warned once per distinct conflict set and
    /// never nagged about the same conflicts on every launch.
    #[serde(default)]
    pub keymap_conflict_signature: String,
    /// Whether we've shown the one-time "glyph-safe mode is active" disclosure
    /// for fragile-glyph terminals (macOS VS Code integrated terminal / Apple
    /// Terminal). We surface the tradeoff once per install so the user knows
    /// colors are quantized to 256 to avoid the terminal's glyph corruption.
    #[serde(default)]
    pub glyph_safe_notice_shown: bool,
    /// Counts successful launches by canonical launch-hotkey chord. Used to stop
    /// showing already-learned repo hotkey hints.
    #[serde(default)]
    pub launch_hotkey_usage: HashMap<String, u64>,
    /// Last time a launch-shortcut reminder was shown for each external CLI.
    /// Keys are stable source ids such as `claude` and `codex`; values are Unix
    /// timestamps in seconds.
    #[serde(default)]
    pub cli_launch_hint_last_shown: HashMap<String, u64>,
    /// Lifetime reminder count per external CLI. The native SessionStart hooks
    /// may fire on every launch, but the reminder intentionally stops after a
    /// small number of spaced repetitions.
    #[serde(default)]
    pub cli_launch_hint_shown_count: HashMap<String, u64>,
}

/// Serde default helper: fields documented as "true by default".
fn default_true() -> bool {
    true
}

impl Default for SetupHintsState {
    fn default() -> Self {
        Self {
            launch_count: 0,
            hotkey_configured: false,
            hotkey_dismissed: false,
            alacritty_configured: false,
            alacritty_dismissed: false,
            desktop_shortcut_created: false,
            // Dismissed by default: the system-wide launch-hotkey spawn notice is
            // opt-in noise, so new state starts with it suppressed.
            startup_spawn_hint_dismissed: true,
            mac_ghostty_guided: false,
            mac_ghostty_dismissed: false,
            terminal_nudge_count: 0,
            hotkey_listener_version: 0,
            launch_hotkey_tracking_version: 0,
            keymap_conflict_signature: String::new(),
            glyph_safe_notice_shown: false,
            launch_hotkey_usage: HashMap::new(),
            cli_launch_hint_last_shown: HashMap::new(),
            cli_launch_hint_shown_count: HashMap::new(),
        }
    }
}

/// Current macOS hotkey listener implementation version.
///
/// Increment this whenever the listener needs to be reinstalled for existing
/// users on update. History:
/// - 1: pump the Core Foundation run loop on the main thread so Cmd+; fires
///   (previously the listener blocked and never delivered events).
/// - 2: promote the launchd process to a UIElement app (`TransformProcessType`)
///   and run the Carbon application event loop, so a faceless background
///   process is actually eligible to receive `RegisterEventHotKey` events.
///   Version 1 still never fired because the process had no window-server
///   connection.
/// - 3: register three launch hotkeys instead of one. `Cmd+;` opens anastasia in
///   `$HOME`, `Cmd+'` opens it in the last project directory, and `Cmd+Shift+'`
///   opens a self-dev session in the last anastasia repo. Existing users are
///   migrated so the extra scripts/registrations are installed on update.
/// - 4: hotkeys are config-driven. The installer resolves `[launch_hotkeys]`
///   from config (empty -> the same three built-ins) into per-entry scripts and
///   a `plan.json`; the listener registers chords from that plan. Existing users
///   migrate so the plan file and per-entry scripts are written, enabling the
///   baked per-repo hotkeys auto-import can add.
/// - 5: the listener launches configured repos directly through
///   `anastasia-terminal-launch`, avoiding the generated shell-script hop on hotkey
///   press. Scripts/plan are still written for compatibility and diagnostics.
/// - 6: direct launches pass `--spawn-hotkey` into the new Anastasia process so
///   global shortcut proficiency is recorded by the same cross-platform path.
#[cfg(any(test, target_os = "macos"))]
pub const HOTKEY_LISTENER_VERSION: u32 = 6;

/// Maximum number of times we will ever show the terminal/setup nudge prompt
/// to a user (across all launches and platforms). After this many nudges we stop
/// asking, even if the user never explicitly picked "Don't ask again".
pub const MAX_TERMINAL_NUDGES: u64 = 5;

#[derive(Debug, Clone, Default)]
pub struct StartupHints {
    pub auto_send_message: Option<String>,
    pub status_notice: Option<String>,
    pub display_message: Option<(String, String)>,
}

impl StartupHints {
    fn with_status_and_display(
        status_notice: String,
        title: impl Into<String>,
        display_message: String,
    ) -> Self {
        Self {
            auto_send_message: None,
            status_notice: Some(status_notice),
            display_message: Some((title.into(), display_message)),
        }
    }
}

impl SetupHintsState {
    fn path() -> Result<PathBuf> {
        Ok(storage::anastasia_dir()?.join("setup_hints.json"))
    }

    pub fn load() -> Self {
        let Ok(path) = Self::path() else {
            return Self::default();
        };
        Self::load_from(&path)
    }

    /// Load state from `path`, falling back to its `.bak` sibling.
    ///
    /// The atomic writer keeps the previous version at `.bak`. If the primary
    /// file is missing or unreadable (deleted, interrupted swap), fall back to
    /// it instead of silently resetting state like `launch_count`, which
    /// downstream heuristics (e.g. first-run onboarding) rely on.
    fn load_from(path: &std::path::Path) -> Self {
        if let Ok(state) = storage::read_json(path) {
            return state;
        }
        let bak = path.with_extension("bak");
        storage::read_json(&bak).unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        // Best-effort UI state (launch counter + one-time hint/nudge flags).
        // This is written on every interactive launch and is not durability
        // critical: losing the most recent update on a power cut just re-shows a
        // hint or under-counts a launch. Use the non-fsync fast write so we do
        // not pay macOS's `F_FULLFSYNC` (full disk-platter flush, ~8ms here)
        // twice on the startup critical path. The atomic rename still protects
        // against torn/partial writes, and load() falls back to `.bak`.
        storage::write_json_fast(&path, self)
    }
}

#[cfg(any(test, target_os = "macos", target_os = "linux", windows))]
fn mac_hotkey_support_dir() -> Result<PathBuf> {
    Ok(storage::anastasia_dir()?.join("hotkey"))
}

/// File holding the last project directory anastasia was launched from. The `Cmd+'`
/// global hotkey reads this at fire time to reopen anastasia there.
#[cfg(any(test, target_os = "macos", target_os = "linux", windows))]
fn mac_hotkey_last_dir_file() -> Result<PathBuf> {
    Ok(mac_hotkey_support_dir()?.join("last_dir"))
}

/// File holding the last anastasia *repository* directory the user worked in. The
/// `Cmd+Shift+'` global hotkey reads this to open a self-dev session there.
#[cfg(any(test, target_os = "macos", target_os = "linux", windows))]
fn mac_hotkey_last_repo_file() -> Result<PathBuf> {
    Ok(mac_hotkey_support_dir()?.join("last_repo"))
}

/// Record the directories the global launch hotkeys should reopen.
///
/// Called once per interactive launch with the process's working directory.
/// `$HOME` launches are ignored for the "last project" file so the `Cmd+'`
/// hotkey keeps pointing at a real project rather than home (which already has
/// its own `Cmd+;` hotkey). When `dir` is inside a anastasia repo, the repo root is
/// recorded for the self-dev hotkey.
///
/// Best-effort and side-effect-only: failures are logged, never propagated, so
/// this can be dropped onto the startup path without risk.
pub fn record_launch_dirs(dir: &std::path::Path, repo_dir: Option<&std::path::Path>) {
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    {
        if let Err(err) = record_launch_dirs_inner(dir, repo_dir) {
            anastasia_logging::warn(&format!("failed to record launch dirs for hotkeys: {err}"));
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        let _ = (dir, repo_dir);
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
fn record_launch_dirs_inner(
    dir: &std::path::Path,
    repo_dir: Option<&std::path::Path>,
) -> Result<()> {
    let support_dir = mac_hotkey_support_dir()?;
    std::fs::create_dir_all(&support_dir)?;

    if should_record_last_dir(dir, dirs::home_dir().as_deref()) {
        std::fs::write(mac_hotkey_last_dir_file()?, format!("{}\n", dir.display()))?;
    }

    if let Some(repo) = repo_dir {
        std::fs::write(
            mac_hotkey_last_repo_file()?,
            format!("{}\n", repo.display()),
        )?;
    }

    Ok(())
}

/// Whether `dir` should be recorded as the "last project" directory for the
/// `Cmd+'` hotkey. Home is skipped because it already has its own `Cmd+;`
/// hotkey, so recording it would make `Cmd+'` redundant with `Cmd+;`.
#[cfg(any(test, target_os = "macos", target_os = "linux", windows))]
fn should_record_last_dir(dir: &std::path::Path, home: Option<&std::path::Path>) -> bool {
    home != Some(dir)
}

/// Read a single-character choice from the user.
#[cfg(windows)]
fn read_choice() -> String {
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
    input.trim().to_lowercase()
}

/// Manual `anastasia setup-hotkey` command.
///
/// Runs the full interactive setup flow regardless of launch count.
#[cfg_attr(
    target_os = "linux",
    allow(
        clippy::needless_return,
        reason = "explicit return ends a cfg-gated block"
    )
)]
pub fn run_setup_hotkey(
    _listen_macos_hotkey: bool,
    _listen_windows_hotkey: bool,
    _uninstall: bool,
    _notify_cli_launch: Option<&str>,
) -> Result<()> {
    anyhow::bail!("Global launchers are not part of Anastasia CLI")
}

/// Run the macOS global-hotkey listener on the current (main) thread.
///
/// This must be called from `main()` before any tokio runtime is created, so
/// that the Core Foundation run loop driving Carbon hotkey events lives on the
/// real main thread. On non-macOS platforms this is a no-op that returns `Ok`.
pub fn run_macos_hotkey_listener_main_thread() -> Result<()> {
    anyhow::bail!("Global launchers are not part of Anastasia CLI")
}

/// Record one successful global launch-hotkey use. Launchers pass the canonical
/// chord through the hidden `--spawn-hotkey` argument; canonicalizing again here
/// keeps persisted learning state stable even if an older launcher passes a
/// differently ordered spelling.
pub fn record_launch_hotkey_use(chord: &str) {
    let Some(chord) = keymap::KeyChord::parse(chord).map(|chord| chord.canonical()) else {
        anastasia_logging::warn(&format!(
            "ignored invalid launch hotkey usage chord: {chord}"
        ));
        return;
    };
    let mut state = SetupHintsState::load();
    let uses = state.launch_hotkey_usage.entry(chord.clone()).or_insert(0);
    *uses = uses.saturating_add(1);
    if let Err(err) = state.save() {
        anastasia_logging::warn(&format!(
            "failed to record launch hotkey usage for {chord}: {err}"
        ));
    }
}

/// Decide what macOS hotkey listener action a launch should take, given the
/// persisted setup state. Extracted as a pure function so the upgrade/install
/// gating can be unit-tested without touching launchd.
#[cfg(any(test, target_os = "macos"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacHotkeyAction {
    /// First-time install (never configured, never dismissed).
    Install,
    /// Reinstall because the configured listener predates the current version.
    Migrate,
    /// Config opted out (`[launch_hotkeys] enabled = false`): unload and
    /// remove any installed LaunchAgent instead of (re)installing it.
    Disable,
    /// Nothing to do.
    None,
}

/// Main entry point: check if we should show setup hints.
///
/// Called early in startup, before the TUI is initialized.
/// Returns optional structured startup hints for the TUI.
///
/// - Windows: On every 3rd launch, can show hotkey + Alacritty nudges.
/// - macOS: On every 3rd launch, can suggest Ghostty and optionally hand off
///   to AI-guided setup by returning a prebuilt prompt.
pub fn maybe_show_setup_hints() -> Option<StartupHints> {
    None
}

/// Pure debounce decision for the keybinding-conflict notice.
///
/// Given the freshly-computed conflict `signature` and the `previous` signature
/// we last stored, decide what to do. Separated from I/O so the
/// warn-once-per-change policy can be unit-tested without touching the machine
/// or the filesystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConflictHintDecision {
    /// Nothing changed since last time; stay silent and leave state untouched.
    Unchanged,
    /// The conflict set changed but is now empty (resolved); update the stored
    /// signature but show nothing.
    ResolvedSilently,
    /// New or changed conflicts; update the stored signature and show a notice.
    Warn,
}

pub(crate) fn conflict_hint_decision(signature: &str, previous: &str) -> ConflictHintDecision {
    if signature == previous {
        ConflictHintDecision::Unchanged
    } else if signature.is_empty() {
        ConflictHintDecision::ResolvedSilently
    } else {
        ConflictHintDecision::Warn
    }
}

/// Check whether anastasia's keybindings conflict with shortcuts owned by the
/// terminal or the OS, and return a one-time startup notice when the set of
/// conflicts has changed since we last warned.
///
/// This is config-aware (the caller passes the user's live keybindings) and
/// debounced via a stored signature: a user is warned once per distinct
/// conflict set and never nagged about the same conflicts on subsequent
/// launches. Returns `None` when there are no conflicts, when nothing changed,
/// or when input is not a real TTY.
///
/// The actual diagnostics are always available on demand via the `/keys`
/// command; this only surfaces the proactive heads-up.
pub fn maybe_show_keymap_conflict_hint(
    keybindings: &anastasia_config_types::KeybindingsConfig,
) -> Option<StartupHints> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return None;
    }

    let snapshot = keymap::snapshot_cached_or_refresh();
    let mut state = SetupHintsState::load();
    let (hint, changed) = keymap_conflict_hint_for(keybindings, &snapshot, &mut state);
    if changed {
        let _ = state.save();
    }
    hint
}

/// Core of [`maybe_show_keymap_conflict_hint`], separated from TTY detection and
/// disk I/O so the full decision + state-update path is unit-testable.
///
/// Returns the optional notice and whether `state` was mutated (and therefore
/// should be persisted by the caller).
pub(crate) fn keymap_conflict_hint_for(
    keybindings: &anastasia_config_types::KeybindingsConfig,
    snapshot: &keymap::KeymapSnapshot,
    state: &mut SetupHintsState,
) -> (Option<StartupHints>, bool) {
    let conflicts = keymap::detect_conflicts(keybindings, snapshot);
    let signature = keymap::conflict_signature(&conflicts);

    match conflict_hint_decision(&signature, &state.keymap_conflict_signature) {
        ConflictHintDecision::Unchanged => (None, false),
        ConflictHintDecision::ResolvedSilently => {
            state.keymap_conflict_signature = signature;
            (None, true)
        }
        ConflictHintDecision::Warn => {
            state.keymap_conflict_signature = signature;
            let hint = keymap::render_status_line(keybindings, snapshot).map(|status| {
                let display = keymap::render_report(keybindings, snapshot);
                StartupHints::with_status_and_display(status, "Keybindings", display)
            });
            (hint, true)
        }
    }
}

/// Whether the current terminal triggers anastasia's glyph-safe color quantization
/// (macOS VS Code integrated terminal / Apple Terminal). Mirrors the detection
/// in `anastasia-tui-style`'s color module and `anastasia-app-core::perf` so the
/// disclosure fires exactly when the behavior is active. Overridable with
/// `ANASTASIA_CLI_GLYPH_SAFE_MODE=on|off`.
fn glyph_safe_mode_active() -> bool {
    if let Ok(raw) = std::env::var("ANASTASIA_CLI_GLYPH_SAFE_MODE") {
        match raw.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => return true,
            "0" | "false" | "no" | "off" => return false,
            _ => {}
        }
    }
    if !cfg!(target_os = "macos") {
        return false;
    }
    match std::env::var("TERM_PROGRAM") {
        Ok(tp) => {
            let tp = tp.to_ascii_lowercase();
            tp == "vscode" || tp == "apple_terminal"
        }
        Err(_) => false,
    }
}

/// One-time disclosure that glyph-safe mode (256-color quantization) is active,
/// shown the first time anastasia launches in a fragile-glyph terminal. Discloses
/// the tradeoff (slightly reduced color fidelity) and how to opt out.
pub fn maybe_show_glyph_safe_notice() -> Option<StartupHints> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return None;
    }
    let mut state = SetupHintsState::load();
    let (hint, changed) = glyph_safe_notice_for(glyph_safe_mode_active(), &mut state);
    if changed {
        let _ = state.save();
    }
    hint
}

/// Core of [`maybe_show_glyph_safe_notice`], split out for unit testing.
/// Returns the optional notice and whether `state` was mutated.
pub(crate) fn glyph_safe_notice_for(
    active: bool,
    state: &mut SetupHintsState,
) -> (Option<StartupHints>, bool) {
    if !active || state.glyph_safe_notice_shown {
        return (None, false);
    }
    state.glyph_safe_notice_shown = true;
    let status =
        "Glyph-safe mode: colors quantized to 256 to avoid this terminal's glyph corruption."
            .to_string();
    let display = "This terminal (VS Code integrated terminal / Apple Terminal on macOS) corrupts \
its glyph cache under anastasia's full-color animations, rendering letters as boxes. \
anastasia automatically quantizes colors to the 256-palette here to keep text readable; \
the only tradeoff is slightly reduced color fidelity. Animations still run. \
For full color, use Ghostty, iTerm2, kitty, or WezTerm, or set ANASTASIA_CLI_GLYPH_SAFE_MODE=off."
        .to_string();
    (
        Some(StartupHints::with_status_and_display(
            status, "Display", display,
        )),
        true,
    )
}

/// Manual `anastasia setup-launcher` command.
pub fn run_setup_launcher() -> Result<()> {
    anyhow::bail!("Global launchers are not part of Anastasia CLI")
}

/// Reinstall the launch hotkeys after the `[launch_hotkeys]` config changed
/// (e.g. auto-import baked a per-repo mapping).
///
/// Re-resolves config into platform launch bindings so new chords take effect
/// immediately. An explicit `enabled = false` remains an opt-out. Best-effort:
/// errors are logged, never propagated, so this is safe on the startup path.
pub fn reinstall_launch_hotkeys_after_config_change() {}

#[cfg(test)]
#[path = "setup_hints_tests.rs"]
mod setup_hints_tests;
