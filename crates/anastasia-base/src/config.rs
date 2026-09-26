//! Configuration file support for anastasia
//!
//! Config is loaded from `~/.anastasia-cli/config.toml` (or `$ANASTASIA_CLI_HOME/config.toml`)
//! Environment variables override config file settings.

pub use anastasia_config_types::{
    AgentsConfig, AmbientConfig, AuthConfig, AutoJudgeConfig, AutoReviewConfig, CompactionConfig,
    CompactionMode, CrossProviderFailoverMode, DiagramDisplayMode, DiagramPanePosition,
    DiffDisplayMode, DisplayConfig, FeatureConfig, GatewayConfig, HookCommands, HooksConfig,
    KeybindingsConfig, LatexRenderingMode, LaunchHotkeyEntry, LaunchHotkeysConfig,
    MarkdownSpacingMode, NamedProviderAuth, NamedProviderConfig, NamedProviderModelConfig,
    NamedProviderType, NativeScrollbarConfig, NotificationsConfig, OverscrollStatusMode,
    PowerConfig, ProviderConfig, ReasoningDisplayMode, SafetyConfig, SessionPickerResumeAction,
    SwarmSpawnMode, SwarmStripLayout, TerminalConfig, UpdateChannel,
    WebSearchConfig, WebSearchEngine,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{LazyLock, RwLock};
use std::time::{Duration, Instant, SystemTime};

const CONFIG_CACHE_CHECK_INTERVAL: Duration = if cfg!(test) {
    Duration::ZERO
} else {
    Duration::from_millis(500)
};

const CONFIG_ENV_KEYS: &[&str] = &[
    "HOME",
    "ANASTASIA_CLI_ACP_PROFILE",
    "ANASTASIA_CLI_ACP_TOOL_PROFILE",
    "ANASTASIA_CLI_ACTIVE_SESSIONS_MANAGER",
    "ANASTASIA_CLI_EXTERNAL_SESSIONS",
    "ANASTASIA_CLI_AMBIENT_ENABLED",
    "ANASTASIA_CLI_AMBIENT_MAX_INTERVAL",
    "ANASTASIA_CLI_AMBIENT_MIN_INTERVAL",
    "ANASTASIA_CLI_AMBIENT_MODEL",
    "ANASTASIA_CLI_AMBIENT_PROACTIVE",
    "ANASTASIA_CLI_AMBIENT_PROVIDER",
    "ANASTASIA_CLI_AMBIENT_VISIBLE",
    "ANASTASIA_CLI_ANIMATION_FPS",
    "ANASTASIA_CLI_AUTO_POKE",
    "ANASTASIA_CLI_AUTOJUDGE_ENABLED",
    "ANASTASIA_CLI_AUTOJUDGE_MODEL",
    "ANASTASIA_CLI_AUTOREVIEW_ENABLED",
    "ANASTASIA_CLI_AUTOREVIEW_MODEL",
    "ANASTASIA_CLI_AUTO_POKE",
    "ANASTASIA_CLI_AUTO_SERVER_RELOAD",
    "ANASTASIA_CLI_CHECK_UPDATES",
    "ANASTASIA_CLI_BING_API_KEY",
    "ANASTASIA_CLI_BING_API_KEY_ENV",
    "ANASTASIA_CLI_BING_MARKET",
    "ANASTASIA_CLI_CENTERED_TOGGLE_KEY",
    "ANASTASIA_CLI_CHAT_NATIVE_SCROLLBAR",
    "ANASTASIA_CLI_COMPACT_NOTIFICATIONS",
    "ANASTASIA_CLI_COMPACTION_MAX_TOKENS",
    "ANASTASIA_CLI_COPY_BADGE_ALT_LABEL",
    "ANASTASIA_CLI_COPY_SELECTION_TOGGLE_KEY",
    "ANASTASIA_CLI_COPILOT_PREMIUM",
    "ANASTASIA_CLI_CROSS_PROVIDER_FAILOVER",
    "ANASTASIA_CLI_DEBUG_SOCKET",
    "ANASTASIA_CLI_DEFAULT_REASONING_DISPLAY",
    "ANASTASIA_CLI_DICTATION_COMMAND",
    "ANASTASIA_CLI_DICTATION_KEY",
    "ANASTASIA_CLI_DICTATION_MODE",
    "ANASTASIA_CLI_DICTATION_TIMEOUT_SECS",
    "ANASTASIA_CLI_DIFF_LINE_WRAP",
    "ANASTASIA_CLI_DIFF_MODE",
    "ANASTASIA_CLI_DIFF_MODE_CYCLE_KEY",
    "ANASTASIA_CLI_DIAGRAM_PANE_TOGGLE_KEY",
    "ANASTASIA_CLI_DISABLE_BASE_TOOLS",
    "ANASTASIA_CLI_DISABLED_ANIMATIONS",
    "ANASTASIA_CLI_DISABLED_TOOLS",
    "ANASTASIA_CLI_DISCORD_BOT_TOKEN",
    "ANASTASIA_CLI_DISCORD_BOT_USER_ID",
    "ANASTASIA_CLI_DISCORD_CHANNEL_ID",
    "ANASTASIA_CLI_DISCORD_REPLY_ENABLED",
    "ANASTASIA_CLI_DISPLAY_CENTERED",
    "ANASTASIA_CLI_EFFORT_DECREASE_KEY",
    "ANASTASIA_CLI_EFFORT_INCREASE_KEY",
    "ANASTASIA_CLI_EMAIL_REPLY_ENABLED",
    "ANASTASIA_CLI_EMAIL_TO",
    "ANASTASIA_CLI_FOCUS_HOOK",
    "ANASTASIA_CLI_GATEWAY_BIND_ADDR",
    "ANASTASIA_CLI_GATEWAY_ENABLED",
    "ANASTASIA_CLI_GATEWAY_PORT",
    "ANASTASIA_CLI_HOME",
    "ANASTASIA_CLI_HOOK_PRE_TOOL",
    "ANASTASIA_CLI_HOOK_PRE_TOOL_TIMEOUT_MS",
    "ANASTASIA_CLI_HOOK_POST_TOOL",
    "ANASTASIA_CLI_HOOK_SESSION_END",
    "ANASTASIA_CLI_HOOK_SESSION_START",
    "ANASTASIA_CLI_HOOK_TURN_END",
    "ANASTASIA_CLI_HOOK_TURN_START",
    "ANASTASIA_CLI_IDLE_ANIMATION",
    "ANASTASIA_CLI_IMAP_HOST",
    "ANASTASIA_CLI_INFO_WIDGET_TOGGLE_KEY",
    "ANASTASIA_CLI_JADE_RELAY_API_BASE",
    "ANASTASIA_CLI_JADE_RELAY_ENABLED",
    "ANASTASIA_CLI_JADE_RELAY_LAUNCH_ENABLED",
    "ANASTASIA_CLI_JADE_RELAY_LAUNCH_WORKING_DIR",
    "ANASTASIA_CLI_JADE_RELAY_REPLY_ENABLED",
    "ANASTASIA_CLI_JADE_RELAY_SESSION_ID",
    "ANASTASIA_CLI_JADE_RELAY_TOKEN",
    "ANASTASIA_CLI_JADE_RELAY_TOKEN_ID",
    "ANASTASIA_CLI_JADE_RELAY_USER_ID",
    "ANASTASIA_CLI_KV_CACHE_MISS_NOTICES",
    "ANASTASIA_CLI_LATEX_RENDERING",
    "ANASTASIA_CLI_MARKDOWN_SPACING",
    "ANASTASIA_CLI_MEMORY_EMBEDDING_BACKEND",
    "ANASTASIA_CLI_MEMORY_EMBEDDING_BASE_URL",
    "ANASTASIA_CLI_MEMORY_EMBEDDING_DIM",
    "ANASTASIA_CLI_MEMORY_EMBEDDING_MODEL",
    "ANASTASIA_CLI_MEMORY_ENABLED",
    "ANASTASIA_CLI_ENABLE_MERMAID",
    "ANASTASIA_CLI_MEMORY_MODEL",
    "ANASTASIA_CLI_MEMORY_SIDECAR_ENABLED",
    "ANASTASIA_CLI_PERSIST_MEMORY_INJECTIONS",
    "ANASTASIA_CLI_MESSAGE_TIMESTAMPS",
    "ANASTASIA_CLI_MODEL",
    "ANASTASIA_CLI_MODEL_SWITCH_KEY",
    "ANASTASIA_CLI_MODEL_SWITCH_PREV_KEY",
    "ANASTASIA_CLI_MOUSE_CAPTURE",
    "ANASTASIA_CLI_NEW_TERMINAL_KEY",
    "ANASTASIA_CLI_NO_EMOJI",
    "ANASTASIA_CLI_NTFY_SERVER",
    "ANASTASIA_CLI_NTFY_TOPIC",
    "ANASTASIA_CLI_OPENAI_NATIVE_COMPACTION_MODE",
    "ANASTASIA_CLI_OPENAI_NATIVE_COMPACTION_THRESHOLD_TOKENS",
    "ANASTASIA_CLI_OPENAI_REASONING_EFFORT",
    "ANASTASIA_CLI_OPENAI_SERVICE_TIER",
    "ANASTASIA_CLI_OPENAI_TRANSPORT",
    "ANASTASIA_CLI_ANTHROPIC_REASONING_EFFORT",
    "ANASTASIA_CLI_PRESERVE_REASONING_CONTEXT",
    "ANASTASIA_CLI_PERFORMANCE",
    "ANASTASIA_CLI_PERMISSIONS",
    "ANASTASIA_CLI_PIN_IMAGES",
    "ANASTASIA_CLI_PIN_TODOS",
    "ANASTASIA_CLI_PREVENT_SLEEP_WHILE_STREAMING",
    "ANASTASIA_CLI_PROVIDER",
    "ANASTASIA_CLI_PROMPT_ENTRY_ANIMATION",
    "ANASTASIA_CLI_QUEUE_MODE",
    "ANASTASIA_CLI_REASONING_DISPLAY",
    "ANASTASIA_CLI_REDRAW_FPS",
    "ANASTASIA_CLI_SAME_PROVIDER_ACCOUNT_FAILOVER",
    "ANASTASIA_CLI_SCROLL_BOOKMARK_KEY",
    "ANASTASIA_CLI_SCROLL_DOWN_FALLBACK_KEY",
    "ANASTASIA_CLI_SCROLL_DOWN_KEY",
    "ANASTASIA_CLI_SCROLL_PAGE_DOWN_KEY",
    "ANASTASIA_CLI_SCROLL_PAGE_UP_KEY",
    "ANASTASIA_CLI_SCROLL_PROMPT_DOWN_KEY",
    "ANASTASIA_CLI_SCROLL_PROMPT_UP_KEY",
    "ANASTASIA_CLI_SCROLL_UP_FALLBACK_KEY",
    "ANASTASIA_CLI_SCROLL_UP_KEY",
    "ANASTASIA_CLI_SEARXNG_URL",
    "ANASTASIA_CLI_SHOW_AGENTGREP_OUTPUT",
    "ANASTASIA_CLI_SHOW_BASH_OUTPUT",
    "ANASTASIA_CLI_SHOW_DIFFS",
    "ANASTASIA_CLI_SHOW_THINKING",
    "ANASTASIA_CLI_SIDE_PANEL_TOGGLE_KEY",
    "ANASTASIA_CLI_SIDE_PANEL_NATIVE_SCROLLBAR",
    "ANASTASIA_CLI_SMTP_PASSWORD",
    "ANASTASIA_CLI_SPAWN_HOOK",
    "ANASTASIA_CLI_STREAM_IDLE_TIMEOUT_SECS",
    "ANASTASIA_CLI_MAX_RETRIES",
    "ANASTASIA_CLI_MCP_TOOLS",
    "ANASTASIA_CLI_MCP_TOOLS_TOKEN_THRESHOLD",
    "ANASTASIA_CLI_RETRY_BACKOFF_CAP_SECS",
    "ANASTASIA_CLI_SWARM_ENABLED",
    "ANASTASIA_CLI_SWARM_EFFORT",
    "ANASTASIA_CLI_SWARM_MODEL",
    "ANASTASIA_CLI_SWARM_MAX_CONCURRENT_AGENTS",
    "ANASTASIA_CLI_SWARM_SPAWN_MODE",
    "ANASTASIA_CLI_SWARM_STRIP_LAYOUT",
    "ANASTASIA_CLI_TELEGRAM_BOT_TOKEN",
    "ANASTASIA_CLI_TELEGRAM_CHAT_ID",
    "ANASTASIA_CLI_TELEGRAM_REPLY_ENABLED",
    "ANASTASIA_CLI_TOOL_CALL_DETAILS",
    "ANASTASIA_CLI_TOOL_PROFILE",
    "ANASTASIA_CLI_TOOLS",
    "ANASTASIA_CLI_TRUSTED_EXTERNAL_AUTH_SOURCES",
    "ANASTASIA_CLI_TYPING_SCROLL_LOCK_TOGGLE_KEY",
    "ANASTASIA_CLI_UPDATE_CHANNEL",
    "ANASTASIA_CLI_WEBSEARCH_ENGINE",
    "ANASTASIA_CLI_WEBSEARCH_FALLBACK_ENGINES",
    "ANASTASIA_CLI_WORKSPACE_DOWN_KEY",
    "ANASTASIA_CLI_WORKSPACE_LEFT_KEY",
    "ANASTASIA_CLI_WORKSPACE_RIGHT_KEY",
    "ANASTASIA_CLI_WORKSPACE_UP_KEY",
    "XDG_CONFIG_HOME",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConfigCacheFingerprint {
    path: Option<PathBuf>,
    modified: Option<SystemTime>,
    len: Option<u64>,
    env: Vec<(String, String)>,
}

impl ConfigCacheFingerprint {
    fn current() -> Self {
        let path = Config::path();
        let metadata = path.as_ref().and_then(|path| std::fs::metadata(path).ok());
        Self {
            path,
            modified: metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok()),
            len: metadata.as_ref().map(std::fs::Metadata::len),
            env: config_env_fingerprint(),
        }
    }
}

struct ConfigCache {
    config: &'static Config,
    fingerprint: ConfigCacheFingerprint,
    last_checked: Instant,
    force_reload: bool,
}

static CONFIG_CACHE: LazyLock<RwLock<ConfigCache>> = LazyLock::new(|| {
    let config = leak_config(Config::load());
    // Fingerprint after the load: applying env overrides may set env vars
    // (e.g. copilot_premium -> ANASTASIA_CLI_COPILOT_PREMIUM), and fingerprinting
    // first would guarantee a spurious full reload on the next check.
    let fingerprint = ConfigCacheFingerprint::current();
    // Seed the global context-limit cache from named provider configs on first
    // load so every codepath (TUI info widget, compaction budget, model
    // switching) sees user-configured `context_window` values from the start.
    // Read from the loaded config directly to avoid recursing into config(),
    // which would deadlock on the still-initializing CONFIG_CACHE.
    populate_context_limits_from_config_ref(config);
    RwLock::new(ConfigCache {
        config,
        fingerprint,
        last_checked: Instant::now(),
        force_reload: false,
    })
});

fn leak_config(config: Config) -> &'static Config {
    Box::leak(Box::new(config))
}

/// Seed the global context-limit cache from a config reference directly.
///
/// Used during CONFIG_CACHE initialization (where calling config() would
/// deadlock) and shares its logic with
/// `crate::provider::populate_context_limits_from_config`.
fn populate_context_limits_from_config_ref(cfg: &Config) {
    crate::provider::populate_context_limits_from_config_value(cfg);
}

/// Get the global config instance.
///
/// The returned reference is backed by a reloadable process cache. Calls check
/// the config file path/metadata and relevant environment overrides on a short
/// throttle, not every frame. When those inputs change, the next checked call
/// reloads config.toml and invalidates dependent auth/model caches. Older
/// references remain valid for the duration of any in-flight operation.
pub fn config() -> &'static Config {
    let now = Instant::now();
    if let Ok(cache) = CONFIG_CACHE.read()
        && !cache.force_reload
        && now.duration_since(cache.last_checked) < CONFIG_CACHE_CHECK_INTERVAL
    {
        return cache.config;
    }

    let mut reload_reason = None;
    let config = {
        let mut cache = CONFIG_CACHE
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let now = Instant::now();
        if !cache.force_reload
            && now.duration_since(cache.last_checked) < CONFIG_CACHE_CHECK_INTERVAL
        {
            return cache.config;
        }

        let fingerprint = ConfigCacheFingerprint::current();
        cache.last_checked = now;
        if cache.force_reload || cache.fingerprint != fingerprint {
            reload_reason = Some(describe_config_reload(
                cache.force_reload,
                &cache.fingerprint,
                &fingerprint,
            ));
            cache.config = leak_config(Config::load());
            // Loading applies env overrides that can themselves set env vars
            // (e.g. copilot_premium propagates config -> ANASTASIA_CLI_COPILOT_PREMIUM).
            // Re-fingerprint after the load so those self-inflicted env changes
            // don't trigger a guaranteed second reload on the next check.
            cache.fingerprint = ConfigCacheFingerprint::current();
            cache.force_reload = false;
        }
        cache.config
    };

    if let Some(reason) = reload_reason {
        crate::logging::info(&format!("CONFIG_RELOAD {}", reason));
        // A config reload can change config-derived system prompt sections,
        // which legitimately invalidates the
        // KV cache prefix of warm sessions. Document it so a subsequent
        // harness-attributed cache miss is surfaced with this cause instead of
        // as an unexplained prompt mutation.
        crate::cache_invalidation::record("config reload", &reason);
        notify_config_reloaded();
        // Re-seed the global context-limit cache so user edits to named
        // provider `context_window` values take effect without a restart.
        crate::provider::populate_context_limits_from_config();
    }

    config
}

fn describe_config_reload(
    forced: bool,
    previous: &ConfigCacheFingerprint,
    next: &ConfigCacheFingerprint,
) -> String {
    let mut parts = Vec::new();
    if forced {
        parts.push("forced=true".to_string());
    }
    if previous.path != next.path {
        parts.push(format!(
            "path={:?}->{:?}",
            previous.path.as_ref().map(|p| p.display().to_string()),
            next.path.as_ref().map(|p| p.display().to_string())
        ));
    }
    if previous.modified != next.modified {
        parts.push("modified_changed=true".to_string());
    }
    if previous.len != next.len {
        parts.push(format!("len={:?}->{:?}", previous.len, next.len));
    }
    let env_changes = describe_env_changes(&previous.env, &next.env);
    if !env_changes.is_empty() {
        parts.push(format!("env=[{}]", env_changes.join(", ")));
    }
    if parts.is_empty() {
        "unchanged".to_string()
    } else {
        parts.join(" ")
    }
}

fn describe_env_changes(previous: &[(String, String)], next: &[(String, String)]) -> Vec<String> {
    let previous_map: BTreeMap<&str, &str> = previous
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let next_map: BTreeMap<&str, &str> = next
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let keys: BTreeSet<&str> = previous_map
        .keys()
        .chain(next_map.keys())
        .copied()
        .collect();

    keys.into_iter()
        .filter_map(|key| match (previous_map.get(key), next_map.get(key)) {
            (Some(previous), Some(next)) if previous != next => Some(format!(
                "{}:changed({}->{})",
                key,
                env_value_fingerprint(previous),
                env_value_fingerprint(next)
            )),
            (None, Some(next)) => Some(format!("{}:added({})", key, env_value_fingerprint(next))),
            (Some(previous), None) => Some(format!(
                "{}:removed({})",
                key,
                env_value_fingerprint(previous)
            )),
            _ => None,
        })
        .collect()
}

fn env_value_fingerprint(value: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    format!("len:{} hash:{:016x}", value.len(), hasher.finish())
}

fn config_env_fingerprint() -> Vec<(String, String)> {
    let mut values = std::env::vars_os()
        .filter_map(|(key, value)| {
            let key = key.to_string_lossy().to_string();
            if CONFIG_ENV_KEYS.contains(&key.as_str()) {
                Some((key, value.to_string_lossy().to_string()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    values.sort_by(|left, right| left.0.cmp(&right.0));
    values
}

pub fn invalidate_config_cache() {
    let mut cache = CONFIG_CACHE
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.force_reload = true;
    drop(cache);
    notify_config_reloaded();
}

fn notify_config_reloaded() {
    CONFIG_RELOAD_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    for listener in CONFIG_RELOAD_LISTENERS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
    {
        listener();
    }
}

/// Monotonic counter bumped every time the config cache reloads.
///
/// Callers that snapshot config-derived state (e.g. the TUI's parsed
/// keybindings) can poll this cheaply and re-derive their snapshot when the
/// generation changes, giving instant hot-reload of config edits without a
/// restart.
static CONFIG_RELOAD_GENERATION: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Current config reload generation. Increments after every cache reload.
pub fn config_reload_generation() -> u64 {
    CONFIG_RELOAD_GENERATION.load(std::sync::atomic::Ordering::Relaxed)
}

/// Listeners invoked after the config cache reloads.
///
/// Config is a foundational module, so instead of reaching up into higher-level
/// subsystems (auth cache, event bus) on reload, those subsystems register a
/// reaction here at startup. This keeps config free of upward dependencies and
/// breaks the config -> auth / config -> bus cycle edges.
/// Type of a config reload listener callback.
type ConfigReloadListener = fn();

static CONFIG_RELOAD_LISTENERS: LazyLock<RwLock<Vec<ConfigReloadListener>>> =
    LazyLock::new(|| RwLock::new(Vec::new()));

/// Register a callback to run after the config cache reloads.
///
/// Callbacks must be cheap and non-blocking; they run on whichever thread
/// triggers the reload. Intended to be called once per subsystem during
/// process startup.
pub fn on_config_reloaded(listener: fn()) {
    CONFIG_RELOAD_LISTENERS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(listener);
}

/// Main configuration struct
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    /// Daemon behavior for autonomous wake requests.
    pub server: ServerConfig,

    /// Keybinding configuration
    pub keybindings: KeybindingsConfig,

    /// External dictation / speech-to-text integration
    pub dictation: DictationConfig,

    /// Display/UI configuration
    pub display: DisplayConfig,

    /// Feature toggles
    pub features: FeatureConfig,

    /// Web search tool configuration
    pub websearch: WebSearchConfig,

    /// Built-in tool exposure configuration
    pub tools: ToolConfig,

    /// Agent Client Protocol adapter configuration
    pub acp: AcpConfig,

    /// Auth trust / consent configuration
    pub auth: AuthConfig,

    /// Provider configuration
    pub provider: ProviderConfig,

    /// Named provider profiles, keyed by profile name.
    ///
    /// Example:
    /// [providers.my-gateway]
    /// type = "openai-compatible"
    /// base_url = "https://llm.example.com/v1"
    /// api_key_env = "MY_GATEWAY_API_KEY"
    pub providers: BTreeMap<String, NamedProviderConfig>,

    /// Agent-specific model defaults
    pub agents: AgentsConfig,

    /// Terminal window/pane spawning configuration
    pub terminal: TerminalConfig,

    /// Lifecycle hooks (external commands at turn/session/tool boundaries)
    pub hooks: HooksConfig,

    /// Ambient mode configuration
    pub ambient: AmbientConfig,

    /// Safety / notification configuration
    pub safety: SafetyConfig,

    /// Desktop notifications for interactive sessions (e.g. turn completion)
    pub notifications: NotificationsConfig,

    /// WebSocket gateway configuration for remote clients.
    pub gateway: GatewayConfig,

    /// Compaction configuration
    pub compaction: CompactionConfig,

    /// Power-management configuration (prevent sleep while streaming)
    pub power: PowerConfig,

    /// Auto-review configuration
    pub autoreview: AutoReviewConfig,

    /// Auto-judge configuration
    pub autojudge: AutoJudgeConfig,

    /// Global "launch a new anastasia" hotkeys (macOS). Baked once by auto-import.
    pub launch_hotkeys: LaunchHotkeysConfig,
}

/// Controls who owns autonomous wake execution.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WakeMode {
    /// The daemon starts idle turns and interrupts running turns itself.
    #[default]
    Internal,
    /// The daemon emits a wake request and leaves turn scheduling to its operator.
    External,
}

impl WakeMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "internal" => Some(Self::Internal),
            "external" => Some(Self::External),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Ownership model for autonomous wake requests.
    pub wake_mode: WakeMode,
}

/// Agent Client Protocol adapter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AcpConfig {
    /// Client compatibility profile: "standard" (default), "extended", or "full".
    pub profile: String,
    /// Tool profile to request when `anastasia acp` starts a daemon itself.
    pub tool_profile: String,
}

impl Default for AcpConfig {
    fn default() -> Self {
        Self {
            profile: "standard".to_string(),
            tool_profile: "acp".to_string(),
        }
    }
}

/// Controls how MCP server tools are exposed to the model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpToolsMode {
    /// Expose individual tools until their serialized definitions exceed the
    /// configured threshold, then use the fixed search/call surface.
    #[default]
    Auto,
    /// Always expose every MCP server tool as a top-level tool definition.
    Eager,
    /// Expose only the fixed `mcp_search` and `mcp_call` tools.
    Deferred,
}

impl McpToolsMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Eager => "eager",
            Self::Deferred => "deferred",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "eager" => Some(Self::Eager),
            "deferred" => Some(Self::Deferred),
            _ => None,
        }
    }
}

/// Controls which tools are sent to the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolConfig {
    /// Tool profile: "full" (default), "acp", "minimal"/"lite", or "none".
    pub profile: String,
    /// Explicit allow-list. When set, only these tools are exposed.
    /// Use "*" or "all" to expose all tools without an allow-list.
    pub enabled: Vec<String>,
    /// Tools to remove after applying profile/enabled.
    pub disabled: Vec<String>,
    /// Disable all built-in tools unless `enabled` is provided.
    pub disable_base_tools: bool,
    /// MCP tool exposure mode: auto (default), eager, or deferred.
    pub mcp_tools: McpToolsMode,
    /// In auto mode, defer MCP tools when their definitions exceed this token estimate.
    #[serde(
        alias = "mcp_tools_threshold",
        alias = "mcp_tools_auto_threshold",
        alias = "mcp_tools_auto_threshold_tokens"
    )]
    pub mcp_tools_token_threshold: usize,
    /// Default tool permission level: "auto" (default), "restricted" or "full".
    /// Changed per session with /permissions.
    pub permissions: String,
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            profile: String::new(),
            enabled: Vec::new(),
            disabled: Vec::new(),
            disable_base_tools: false,
            mcp_tools: McpToolsMode::Auto,
            mcp_tools_token_threshold: 8_000,
            permissions: "auto".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolSelection {
    pub allowed_tools: Option<HashSet<String>>,
    pub disabled_tools: HashSet<String>,
}

impl ToolConfig {
    pub fn selection(&self) -> ToolSelection {
        let mut allowed_tools = self.base_allowed_tools();
        let disabled_tools: HashSet<String> = self
            .disabled
            .iter()
            .map(|name| normalize_tool_name(name))
            .filter(|name| !name.is_empty())
            .collect();

        if let Some(allowed) = allowed_tools.as_mut() {
            for name in &disabled_tools {
                allowed.remove(name);
            }
        }

        ToolSelection {
            allowed_tools,
            disabled_tools,
        }
    }

    pub fn allowed_tools(&self) -> Option<HashSet<String>> {
        self.selection().allowed_tools
    }

    pub fn apply_to_allowed_set(&self, allowed: &mut HashSet<String>) {
        let selection = self.selection();
        if let Some(global_allowed) = selection.allowed_tools {
            allowed.retain(|name| global_allowed.contains(name));
        }
        for disabled in selection.disabled_tools {
            allowed.remove(&disabled);
        }
    }

    fn base_allowed_tools(&self) -> Option<HashSet<String>> {
        let (explicit, enables_all_tools) = self.normalized_enabled_tools();

        let profile = self.profile.trim().to_ascii_lowercase();
        if enables_all_tools {
            None
        } else if !explicit.is_empty() {
            Some(explicit)
        } else if self.disable_base_tools || matches!(profile.as_str(), "none" | "off" | "disabled")
        {
            Some(HashSet::new())
        } else if matches!(profile.as_str(), "acp") {
            Some(
                [
                    "bash",
                    "read",
                    "write",
                    "edit",
                    "multiedit",
                    "apply_patch",
                    "patch",
                    "agentgrep",
                    "ls",
                    "batch",
                    "mcp",
                ]
                .into_iter()
                .map(|name| name.to_string())
                .collect(),
            )
        } else if matches!(profile.as_str(), "minimal" | "lite" | "small") {
            Some(
                [
                    "bash",
                    "read",
                    "write",
                    "edit",
                    "multiedit",
                    "apply_patch",
                    "patch",
                    "agentgrep",
                    "ls",
                ]
                .into_iter()
                .map(|name| name.to_string())
                .collect(),
            )
        } else {
            None
        }
    }

    fn normalized_enabled_tools(&self) -> (HashSet<String>, bool) {
        let mut enabled = HashSet::new();
        let mut enables_all_tools = false;

        for name in &self.enabled {
            let normalized = normalize_tool_name(name);
            if normalized.is_empty() {
                continue;
            }
            if normalized == "*" || normalized.eq_ignore_ascii_case("all") {
                enables_all_tools = true;
            } else {
                enabled.insert(normalized);
            }
        }

        (enabled, enables_all_tools)
    }
}

fn normalize_tool_name(name: &str) -> String {
    let trimmed = name.trim().trim_matches('"');
    anastasia_tool_types::resolve_tool_name(trimmed).to_string()
}

/// External dictation / speech-to-text integration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DictationConfig {
    /// Shell command to run. Must print the transcript to stdout.
    pub command: String,
    /// How to apply the resulting transcript.
    pub mode: crate::protocol::TranscriptMode,
    /// Optional in-app hotkey to trigger dictation.
    pub key: String,
    /// Maximum time to wait for the command to finish (0 = no timeout).
    pub timeout_secs: u64,
}

impl Default for DictationConfig {
    fn default() -> Self {
        Self {
            command: String::new(),
            mode: crate::protocol::TranscriptMode::Send,
            key: "off".to_string(),
            timeout_secs: 90,
        }
    }
}

pub mod change_report;
mod config_file;
mod default_file;
mod display_summary;
mod env_overrides;

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "config_color_tests.rs"]
mod color_tests;
