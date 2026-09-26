//! Platform setup hints shown on startup.
//!
//! - Windows: suggest native Alt launch hotkeys plus Copilot-key setup and terminal setup.
//! - macOS: if the user is on the default built-in Terminal.app, show a one-time
//!   notice that it renders anastasia poorly and suggest a modern terminal (Ghostty).
//! - Linux: create a .desktop launcher file.
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

#[cfg(any(test, target_os = "linux"))]
mod linux_env;
#[cfg(any(test, target_os = "linux"))]
mod linux_niri;
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

// ===========================================================================
// Linux global launch hotkeys (niri, Hyprland/omarchy, sway, i3)
//
// Wayland clients cannot grab system-wide hotkeys, so on Linux we bind the
// keys in the compositor's own config. niri and Hyprland hot-reload their
// configs on save; sway/i3 get an explicit `reload` IPC call.
// ===========================================================================

/// Detect the running compositor/window manager from the session environment.
#[cfg(target_os = "linux")]
fn detect_linux_compositor() -> Option<linux_env::LinuxCompositor> {
    linux_env::detect_compositor_from(&|key| std::env::var(key).ok())
}

/// Config file anastasia manages for a flat (`#`-commented) compositor config.
/// For i3 the legacy `~/.i3/config` location is honored when the XDG path is
/// missing. GNOME/KDE do not use a spliceable config file and return `None`.
#[cfg(target_os = "linux")]
fn flat_compositor_config_path(comp: linux_env::LinuxCompositor) -> Option<PathBuf> {
    use linux_env::LinuxCompositor;
    let base = xdg_config_home()?;
    match comp {
        LinuxCompositor::Niri => niri_config_path(),
        LinuxCompositor::Hyprland => Some(base.join("hypr").join("hyprland.conf")),
        LinuxCompositor::Sway => Some(base.join("sway").join("config")),
        LinuxCompositor::Bspwm => Some(base.join("sxhkd").join("sxhkdrc")),
        LinuxCompositor::I3 => {
            let xdg = base.join("i3").join("config");
            if xdg.exists() {
                return Some(xdg);
            }
            let legacy = dirs::home_dir()?.join(".i3").join("config");
            if legacy.exists() {
                Some(legacy)
            } else {
                Some(xdg)
            }
        }
        LinuxCompositor::Gnome
        | LinuxCompositor::Kde
        | LinuxCompositor::Cinnamon
        | LinuxCompositor::Mate
        | LinuxCompositor::Xfce => None,
    }
}

/// KDE's global-shortcuts registry file.
#[cfg(target_os = "linux")]
fn kde_globalshortcutsrc_path() -> Option<PathBuf> {
    Some(xdg_config_home()?.join("kglobalshortcutsrc"))
}

/// Directory for anastasia's hidden KDE launcher desktop files.
#[cfg(target_os = "linux")]
fn kde_applications_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".local").join("share")))?;
    Some(base.join("applications"))
}

/// The config file anastasia would manage for the *current* session's compositor.
#[cfg(target_os = "linux")]
fn linux_hotkey_config_path(comp: linux_env::LinuxCompositor) -> Option<PathBuf> {
    match comp {
        linux_env::LinuxCompositor::Niri => niri_config_path(),
        linux_env::LinuxCompositor::Kde => kde_globalshortcutsrc_path(),
        other => flat_compositor_config_path(other),
    }
}

/// Human description of where the binds land, for the startup notice footer.
#[cfg(target_os = "linux")]
fn linux_hotkey_target_description(comp: linux_env::LinuxCompositor) -> String {
    use linux_env::LinuxCompositor;
    match comp {
        LinuxCompositor::Gnome => "GNOME custom shortcuts (via dconf)".to_string(),
        LinuxCompositor::Kde => "KDE global shortcuts (kglobalshortcutsrc)".to_string(),
        LinuxCompositor::Cinnamon => "Cinnamon custom shortcuts (via dconf)".to_string(),
        LinuxCompositor::Mate => "MATE custom shortcuts (via dconf)".to_string(),
        LinuxCompositor::Xfce => "XFCE keyboard shortcuts (via xfconf)".to_string(),
        other => {
            let path = linux_hotkey_config_path(other)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "its config".to_string());
            format!("your {} config ({})", other.name(), path)
        }
    }
}

/// The sentinel that marks anastasia's managed region in `path` for `comp`.
#[cfg(target_os = "linux")]
fn linux_hotkey_sentinel(comp: linux_env::LinuxCompositor) -> &'static str {
    match comp {
        linux_env::LinuxCompositor::Niri => linux_niri::NIRI_BLOCK_BEGIN,
        _ => linux_env::HASH_BLOCK_BEGIN,
    }
}

/// Whether anastasia's launch hotkeys are already installed for `comp`.
#[cfg(target_os = "linux")]
fn linux_hotkeys_installed(comp: linux_env::LinuxCompositor) -> bool {
    use linux_env::LinuxCompositor;
    match comp {
        LinuxCompositor::Gnome => gnome_keybinding_list().contains("/anastasia-launch-"),
        LinuxCompositor::Cinnamon => dconf_read("/org/cinnamon/desktop/keybindings/custom-list")
            .contains("anastasia-launch-"),
        LinuxCompositor::Mate => {
            dconf_list("/org/mate/desktop/keybindings/").contains("anastasia-launch-")
        }
        LinuxCompositor::Xfce => xfce_shortcut_commands_text().contains("/launch_anastasia_"),
        LinuxCompositor::Kde => kde_globalshortcutsrc_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|text| text.contains("[services][anastasia-launch-"))
            .unwrap_or(false),
        other => linux_hotkey_config_path(other)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|text| text.contains(linux_hotkey_sentinel(other)))
            .unwrap_or(false),
    }
}

/// Whether `chord` can be expressed as a binding for `comp` (used to filter
/// the startup notice down to hotkeys that would actually install).
#[cfg(target_os = "linux")]
fn linux_chord_expressible(comp: linux_env::LinuxCompositor, chord: &keymap::KeyChord) -> bool {
    match comp {
        linux_env::LinuxCompositor::Niri => linux_niri::chord_to_niri_bind(chord).is_some(),
        linux_env::LinuxCompositor::Kde => linux_env::kde_shortcut(chord).is_some(),
        linux_env::LinuxCompositor::Gnome
        | linux_env::LinuxCompositor::Cinnamon
        | linux_env::LinuxCompositor::Mate
        | linux_env::LinuxCompositor::Xfce => linux_env::gnome_binding(chord).is_some(),
        _ => linux_env::xkb_key_name(&chord.key).is_some(),
    }
}

/// Install (or refresh) the launch hotkeys for the detected compositor.
/// Returns `Ok(true)` if anything was changed.
#[cfg(target_os = "linux")]
fn install_linux_launch_hotkeys(comp: linux_env::LinuxCompositor) -> Result<bool> {
    use linux_env::LinuxCompositor;
    match comp {
        LinuxCompositor::Niri => install_niri_launch_hotkeys(),
        LinuxCompositor::Gnome => install_gnome_launch_hotkeys(),
        LinuxCompositor::Kde => install_kde_launch_hotkeys(),
        LinuxCompositor::Cinnamon => install_cinnamon_launch_hotkeys(),
        LinuxCompositor::Mate => install_mate_launch_hotkeys(),
        LinuxCompositor::Xfce => install_xfce_launch_hotkeys(),
        other => install_flat_launch_hotkeys(other),
    }
}

/// Refuse to run the installer for an uninstall request. Linux hotkeys are
/// written through several compositor-specific stores, and no safe common
/// removal operation exists yet.
#[cfg(target_os = "linux")]
fn uninstall_linux_launch_hotkeys() -> Result<()> {
    anyhow::bail!(
        "automatic launch-hotkey removal is not supported for this Linux desktop; no changes were made"
    )
}

/// Install (or refresh) the launch-hotkey binds for a flat `#`-commented
/// compositor config (Hyprland/omarchy, sway, i3). Bind lines execute launch
/// scripts written under `~/.anastasia-cli/hotkey/`, so the config never embeds shell
/// one-liners. Writes a timestamped backup before modifying; no-op when the
/// managed block already matches. Returns `Ok(true)` if the config changed.
#[cfg(target_os = "linux")]
fn install_flat_launch_hotkeys(comp: linux_env::LinuxCompositor) -> Result<bool> {
    let Some(config_path) = flat_compositor_config_path(comp) else {
        anyhow::bail!("could not locate {} config path", comp.name());
    };
    if !config_path.exists() {
        anyhow::bail!(
            "{} config not found at {}",
            comp.name(),
            config_path.display()
        );
    }

    let binds = write_linux_launch_scripts()?;
    let block = match comp {
        linux_env::LinuxCompositor::Hyprland => linux_env::render_hyprland_block(&binds),
        linux_env::LinuxCompositor::Sway | linux_env::LinuxCompositor::I3 => {
            linux_env::render_sway_block(&binds)
        }
        linux_env::LinuxCompositor::Bspwm => linux_env::render_sxhkd_block(&binds),
        linux_env::LinuxCompositor::Niri
        | linux_env::LinuxCompositor::Gnome
        | linux_env::LinuxCompositor::Kde
        | linux_env::LinuxCompositor::Cinnamon
        | linux_env::LinuxCompositor::Mate
        | linux_env::LinuxCompositor::Xfce => {
            unreachable!("handled by dedicated install paths")
        }
    };
    let Some(block) = block else {
        anyhow::bail!("no installable launch hotkeys for {}", comp.name());
    };

    let current = std::fs::read_to_string(&config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    let result = linux_env::splice_flat_managed_block(&current, &block);
    if !result.changed {
        return Ok(false);
    }

    backup_compositor_config(&config_path);

    storage::write_bytes(&config_path, result.text.as_bytes())
        .with_context(|| format!("writing {}", config_path.display()))?;
    anastasia_logging::info(&format!(
        "installed {} {} launch hotkey(s) into {}",
        binds.len(),
        comp.name(),
        config_path.display()
    ));

    reload_compositor_config(comp);
    Ok(true)
}

/// Read one dconf key's textual value. Empty string on any failure (missing
/// dconf, unset key, etc.).
#[cfg(target_os = "linux")]
fn dconf_read(path: &str) -> String {
    std::process::Command::new("dconf")
        .args(["read", path])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// List a dconf directory's children. Empty string on failure.
#[cfg(target_os = "linux")]
fn dconf_list(dir: &str) -> String {
    std::process::Command::new("dconf")
        .args(["list", dir])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

/// Read GNOME's custom-keybindings list via dconf.
#[cfg(target_os = "linux")]
fn gnome_keybinding_list() -> String {
    dconf_read("/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings")
}

/// Write one dconf key. Errors bubble up so a missing dconf binary fails the
/// install with a clear message. dconf is used instead of gsettings because it
/// does not require the settings-daemon schemas to be installed in the running
/// environment, and the desktops' media-keys plugins read dconf directly.
#[cfg(target_os = "linux")]
fn dconf_write(path: &str, value: &str) -> Result<()> {
    let status = std::process::Command::new("dconf")
        .args(["write", path, value])
        .status()
        .context("failed to run dconf (is this a GNOME-family session?)")?;
    if !status.success() {
        anyhow::bail!("dconf write {path} failed with {status}");
    }
    Ok(())
}

/// Write one dconf key only when its value differs; reports whether a write
/// happened so installers can distinguish "installed" from "already up to
/// date".
#[cfg(target_os = "linux")]
fn dconf_write_checked(path: &str, value: &str) -> Result<bool> {
    if dconf_read(path) == value {
        return Ok(false);
    }
    dconf_write(path, value)?;
    Ok(true)
}

/// Quote a string as a GVariant string literal for `dconf write`.
#[cfg(target_os = "linux")]
fn gvariant_string(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// Install (or refresh) the launch hotkeys as GNOME custom keybindings via
/// dconf. Slot-stable paths make re-installs overwrite in place, and the
/// custom-keybindings list merge preserves the user's own entries. GNOME
/// applies dconf changes immediately; no reload needed.
#[cfg(target_os = "linux")]
fn install_gnome_launch_hotkeys() -> Result<bool> {
    let binds = write_linux_launch_scripts()?;
    let keybindings = linux_env::gnome_keybindings(&binds);
    if keybindings.is_empty() {
        anyhow::bail!("no installable launch hotkeys for GNOME");
    }

    // Point the custom-keybindings list at our slots (plus everything the
    // user already had).
    let list_path = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings";
    let current = gnome_keybinding_list();
    let ours: Vec<String> = keybindings.iter().map(|kb| kb.path.clone()).collect();
    let merged = linux_env::merge_gnome_keybinding_list(&current, &ours);
    let mut changed = dconf_write_checked(list_path, &merged)?;

    for kb in &keybindings {
        changed |= dconf_write_checked(&format!("{}name", kb.path), &gvariant_string(&kb.name))?;
        changed |= dconf_write_checked(
            &format!("{}command", kb.path),
            &gvariant_string(&kb.command),
        )?;
        changed |= dconf_write_checked(
            &format!("{}binding", kb.path),
            &gvariant_string(&kb.binding),
        )?;
    }

    anastasia_logging::info(&format!(
        "installed {} GNOME launch hotkey(s) via dconf",
        keybindings.len()
    ));
    Ok(changed)
}

/// Install (or refresh) the launch hotkeys as Cinnamon custom keybindings.
/// Same dconf-backed shape as GNOME but under `/org/cinnamon/`, with slot
/// names (not paths) in `custom-list` and array-typed bindings.
#[cfg(target_os = "linux")]
fn install_cinnamon_launch_hotkeys() -> Result<bool> {
    let binds = write_linux_launch_scripts()?;
    let keybindings = linux_env::dconf_keybindings(&binds);
    if keybindings.is_empty() {
        anyhow::bail!("no installable launch hotkeys for Cinnamon");
    }

    let list_path = "/org/cinnamon/desktop/keybindings/custom-list";
    let current = dconf_read(list_path);
    let ours: Vec<String> = keybindings.iter().map(|kb| kb.slot.clone()).collect();
    let merged = linux_env::merge_gnome_keybinding_list(&current, &ours);
    let mut changed = dconf_write_checked(list_path, &merged)?;

    for kb in &keybindings {
        let base = format!(
            "/org/cinnamon/desktop/keybindings/custom-keybindings/{}/",
            kb.slot
        );
        changed |= dconf_write_checked(&format!("{base}name"), &gvariant_string(&kb.name))?;
        changed |= dconf_write_checked(&format!("{base}command"), &gvariant_string(&kb.command))?;
        // Cinnamon bindings are arrays of accelerator strings.
        changed |= dconf_write_checked(
            &format!("{base}binding"),
            &format!("[{}]", gvariant_string(&kb.binding)),
        )?;
    }

    anastasia_logging::info(&format!(
        "installed {} Cinnamon launch hotkey(s) via dconf",
        keybindings.len()
    ));
    Ok(changed)
}

/// Install (or refresh) the launch hotkeys as MATE custom keybindings under
/// `/org/mate/desktop/keybindings/`. MATE discovers slots from the dconf tree
/// itself, so there is no master list to merge.
#[cfg(target_os = "linux")]
fn install_mate_launch_hotkeys() -> Result<bool> {
    let binds = write_linux_launch_scripts()?;
    let keybindings = linux_env::dconf_keybindings(&binds);
    if keybindings.is_empty() {
        anyhow::bail!("no installable launch hotkeys for MATE");
    }

    let mut changed = false;
    for kb in &keybindings {
        let base = format!("/org/mate/desktop/keybindings/{}/", kb.slot);
        changed |= dconf_write_checked(&format!("{base}name"), &gvariant_string(&kb.name))?;
        // MATE calls the command key `action`.
        changed |= dconf_write_checked(&format!("{base}action"), &gvariant_string(&kb.command))?;
        changed |= dconf_write_checked(&format!("{base}binding"), &gvariant_string(&kb.binding))?;
    }

    anastasia_logging::info(&format!(
        "installed {} MATE launch hotkey(s) via dconf",
        keybindings.len()
    ));
    Ok(changed)
}

/// All `property value` lines from XFCE's keyboard-shortcuts channel. Empty
/// on failure (missing xfconf-query, not XFCE).
#[cfg(target_os = "linux")]
fn xfce_shortcut_commands_text() -> String {
    std::process::Command::new("xfconf-query")
        .args(["-c", "xfce4-keyboard-shortcuts", "-l", "-v"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

/// Install (or refresh) the launch hotkeys as XFCE keyboard shortcuts via
/// xfconf-query. Stale anastasia entries bound to accelerators we no longer use
/// are removed so a re-baked chord layout never leaves orphaned bindings.
#[cfg(target_os = "linux")]
fn install_xfce_launch_hotkeys() -> Result<bool> {
    let binds = write_linux_launch_scripts()?;

    let mut wanted: Vec<(String, String)> = Vec::new();
    for bind in &binds {
        if let Some(accel) = linux_env::gnome_binding(&bind.chord) {
            wanted.push((format!("/commands/custom/{accel}"), bind.script.clone()));
        }
    }
    if wanted.is_empty() {
        anyhow::bail!("no installable launch hotkeys for XFCE");
    }

    let existing = xfce_shortcut_commands_text();
    let mut changed = false;

    // Remove stale anastasia entries bound to accelerators we no longer use.
    for line in existing.lines() {
        let Some((prop, value)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        if value.trim().contains("/launch_anastasia_") && !wanted.iter().any(|(p, _)| p == prop) {
            let _ = std::process::Command::new("xfconf-query")
                .args(["-c", "xfce4-keyboard-shortcuts", "-p", prop, "-r"])
                .status();
            changed = true;
        }
    }

    for (prop, script) in &wanted {
        let already = existing.lines().any(|line| {
            line.split_once(char::is_whitespace)
                .is_some_and(|(p, v)| p == *prop && v.trim() == script)
        });
        if already {
            continue;
        }
        let status = std::process::Command::new("xfconf-query")
            .args([
                "-c",
                "xfce4-keyboard-shortcuts",
                "-p",
                prop,
                "-n",
                "-t",
                "string",
                "-s",
                script,
            ])
            .status()
            .context("failed to run xfconf-query (is this an XFCE session?)")?;
        if !status.success() {
            anyhow::bail!("xfconf-query set {prop} failed with {status}");
        }
        changed = true;
    }

    anastasia_logging::info(&format!(
        "installed {} XFCE launch hotkey(s) via xfconf",
        wanted.len()
    ));
    Ok(changed)
}

/// Install (or refresh) the launch hotkeys as KDE global shortcuts: one hidden
/// desktop file per hotkey plus a `_launch=` binding in `kglobalshortcutsrc`.
/// kglobalaccel watches for desktop-file changes; a re-login may be needed on
/// older Plasma versions.
#[cfg(target_os = "linux")]
fn install_kde_launch_hotkeys() -> Result<bool> {
    let binds = write_linux_launch_scripts()?;
    let shortcuts = linux_env::kde_shortcuts(&binds);
    if shortcuts.is_empty() {
        anyhow::bail!("no installable launch hotkeys for KDE");
    }

    let apps_dir = kde_applications_dir()
        .ok_or_else(|| anyhow::anyhow!("could not locate ~/.local/share/applications"))?;
    std::fs::create_dir_all(&apps_dir)?;
    for sc in &shortcuts {
        std::fs::write(apps_dir.join(&sc.desktop_file_name), &sc.desktop_file_body)?;
    }

    let rc_path = kde_globalshortcutsrc_path()
        .ok_or_else(|| anyhow::anyhow!("could not locate kglobalshortcutsrc"))?;
    let current = std::fs::read_to_string(&rc_path).unwrap_or_default();
    let updated = linux_env::upsert_kde_shortcut_sections(&current, &shortcuts);
    let changed = updated != current;
    if changed {
        if rc_path.exists() {
            backup_compositor_config(&rc_path);
        }
        storage::write_bytes(&rc_path, updated.as_bytes())
            .with_context(|| format!("writing {}", rc_path.display()))?;
    }

    // Nudge kglobalaccel to re-read its config. Best-effort: the shortcuts
    // still land after the next login if the daemon isn't reachable.
    let _ = std::process::Command::new("kquitapp6")
        .arg("kglobalaccel")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    anastasia_logging::info(&format!(
        "installed {} KDE launch hotkey(s) ({} + desktop files)",
        shortcuts.len(),
        rc_path.display()
    ));
    Ok(changed)
}

/// Timestamped backup matching the `.bak-anastasia-*` convention, taken before
/// modifying a user's compositor config. Best-effort.
#[cfg(target_os = "linux")]
fn backup_compositor_config(config_path: &std::path::Path) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let file_name = config_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".to_string());
    let backup = config_path.with_file_name(format!("{file_name}.bak-anastasia-hotkeys-{ts}"));
    if let Err(err) = std::fs::copy(config_path, &backup) {
        anastasia_logging::warn(&format!(
            "failed to back up compositor config before hotkey install: {err}"
        ));
    }
}

/// Ask the compositor to reload its config so new binds take effect without a
/// re-login. niri and Hyprland watch their config files, so only sway/i3 and
/// sxhkd need an explicit poke. Best-effort.
#[cfg(target_os = "linux")]
fn reload_compositor_config(comp: linux_env::LinuxCompositor) {
    let cmd: &[&str] = match comp {
        linux_env::LinuxCompositor::Sway => &["swaymsg", "reload"],
        linux_env::LinuxCompositor::I3 => &["i3-msg", "reload"],
        // sxhkd re-reads its config on SIGUSR1.
        linux_env::LinuxCompositor::Bspwm => &["pkill", "-USR1", "-x", "sxhkd"],
        _ => return,
    };
    match std::process::Command::new(cmd[0])
        .args(&cmd[1..])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => anastasia_logging::warn(&format!(
            "{} exited with {status} while reloading hotkey binds",
            cmd[0]
        )),
        Err(err) => anastasia_logging::warn(&format!("failed to run {} reload: {err}", cmd[0])),
    }
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
