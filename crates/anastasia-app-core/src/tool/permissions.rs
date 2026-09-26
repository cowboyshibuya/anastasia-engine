//! User approval gate for tool calls (`/permissions`).
//!
//! - `full`: never asks (the pre-permissions behavior).
//! - `auto`: asks only for bash commands `anastasia_command_risk` flags as
//!   destructive (any level above `Safe`), and for file edits outside the
//!   working directory.
//! - `restricted`: asks before every tool that is not read-only.
//!
//! Approval reuses the `request_user_input` question panel. With no interactive
//! client (headless runs, subagents), a call that needs approval is denied.

use super::ToolContext;
use anastasia_session_types::{UserQuestion, UserQuestionOption};
use anyhow::{Result, anyhow};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionLevel {
    Restricted,
    Auto,
    Full,
}

impl PermissionLevel {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "restricted" | "ask" => Some(Self::Restricted),
            "auto" => Some(Self::Auto),
            "full" | "full-access" | "full_access" | "yolo" => Some(Self::Full),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Restricted => "restricted",
            Self::Auto => "auto",
            Self::Full => "full",
        }
    }

    pub fn configured_default() -> Self {
        Self::parse(&crate::config::config().tools.permissions).unwrap_or(Self::Restricted)
    }
}

#[derive(Default)]
struct SessionPermissions {
    level: Option<PermissionLevel>,
    /// Tools the user approved for the rest of the session.
    always_allowed: HashSet<String>,
}

static SESSIONS: LazyLock<Mutex<HashMap<String, SessionPermissions>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Serializes approval prompts: the question panel holds one batch at a time,
/// and parallel tool calls (e.g. `batch`) would otherwise collide.
static PROMPT_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

pub fn level(session_id: &str) -> PermissionLevel {
    SESSIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(session_id)
        .and_then(|s| s.level)
        .unwrap_or_else(PermissionLevel::configured_default)
}

pub fn set_level(session_id: &str, level: PermissionLevel) {
    SESSIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(session_id.to_string())
        .or_default()
        .level = Some(level);
}

fn always_allowed(session_id: &str, tool: &str) -> bool {
    SESSIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(session_id)
        .is_some_and(|s| s.always_allowed.contains(tool))
}

fn allow_for_session(session_id: &str, tool: &str) {
    SESSIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(session_id.to_string())
        .or_default()
        .always_allowed
        .insert(tool.to_string());
}

const EDIT_TOOLS: &[&str] = &["write", "edit", "multiedit", "patch", "apply_patch"];

/// Tools that never need approval: read-only, or harness bookkeeping.
fn never_asks(tool: &str) -> bool {
    super::planning_tool_allowed(tool) || matches!(tool, "todo" | "invalid" | "anastasia_docs")
}

/// What the call does, for the prompt, when approval is needed; `None` = run.
fn approval_reason(
    level: PermissionLevel,
    tool: &str,
    input: &Value,
    ctx: &ToolContext,
) -> Option<String> {
    if level == PermissionLevel::Full || never_asks(tool) {
        return None;
    }
    let command = || input.get("command").and_then(Value::as_str).unwrap_or("");
    if level == PermissionLevel::Restricted {
        return Some(match tool {
            "bash" => format!("run `{}`", command()),
            _ => match input.get("file_path").and_then(Value::as_str) {
                Some(path) => format!("use `{tool}` on {path}"),
                None => format!("use `{tool}`"),
            },
        });
    }
    // Auto
    match tool {
        "bash" => {
            let risk_ctx = anastasia_command_risk::RiskContext::from_env(ctx.working_dir.clone());
            let assessment = anastasia_command_risk::assess(command(), &risk_ctx);
            // Anything destructive asks, even bounded (`Low`) deletes in the repo.
            // ponytail: command-risk only models destruction; add network/publish
            // patterns (git push, curl | sh) here if auto needs to catch them.
            (assessment.level != anastasia_command_risk::RiskLevel::Safe)
                .then(|| format!("run `{}`", command()))
        }
        t if EDIT_TOOLS.contains(&t) => {
            let path = input.get("file_path").and_then(Value::as_str)?;
            let resolved = ctx.resolve_path(std::path::Path::new(path));
            let inside = ctx
                .working_dir
                .as_ref()
                .is_none_or(|dir| resolved.starts_with(dir));
            (!inside).then(|| format!("use `{tool}` outside the project on {path}"))
        }
        _ => None,
    }
}

const ALLOW_ONCE: &str = "Allow once";
const DENY: &str = "Deny";

/// Ok(()) when the call may run; Err with a message for the model otherwise.
pub(crate) async fn check(tool: &str, input: &Value, ctx: &ToolContext) -> Result<()> {
    if ctx.execution_mode == super::ToolExecutionMode::Direct
        || always_allowed(&ctx.session_id, tool)
    {
        return Ok(());
    }
    let level = level(&ctx.session_id);
    let Some(reason) = approval_reason(level, tool, input, ctx) else {
        return Ok(());
    };
    if !super::request_user_input::has_client(&ctx.session_id) {
        return Err(anyhow!(
            "Permission denied: this call needs user approval ({reason}) but no interactive client is attached. Current level: {}. Use /permissions full to allow it.",
            level.as_str()
        ));
    }

    let _prompt = PROMPT_LOCK.lock().await;
    if always_allowed(&ctx.session_id, tool) {
        return Ok(());
    }
    let session_label = format!("Allow `{tool}` for this session");
    let question = UserQuestion {
        id: "permission".into(),
        header: "Permission".into(),
        question: format!("Allow the agent to {reason}?"),
        options: vec![
            UserQuestionOption {
                label: ALLOW_ONCE.into(),
                description: "Run this call only".into(),
            },
            UserQuestionOption {
                label: session_label.clone(),
                description: "Don't ask again for this tool until the session ends".into(),
            },
            UserQuestionOption {
                label: DENY.into(),
                description: "Refuse; the agent is told you denied it".into(),
            },
        ],
        multi_select: false,
    };
    let answers = super::request_user_input::ask(ctx, vec![question]).await?;
    let choice = answers
        .get("permission")
        .and_then(|values| values.first())
        .map(String::as_str)
        .unwrap_or(DENY);
    if choice == ALLOW_ONCE {
        Ok(())
    } else if choice == session_label {
        allow_for_session(&ctx.session_id, tool);
        Ok(())
    } else if choice == DENY {
        Err(anyhow!("User denied this tool call ({reason})."))
    } else {
        // Free-text answer: treat as a denial carrying the user's instruction.
        Err(anyhow!(
            "User denied this tool call ({reason}) and said: {choice}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> ToolContext {
        ToolContext {
            session_id: "perm-test".into(),
            message_id: "m".into(),
            tool_call_id: "c".into(),
            working_dir: Some("/repo".into()),
            stdin_request_tx: None,
            graceful_shutdown_signal: None,
            execution_mode: super::super::ToolExecutionMode::AgentTurn,
        }
    }

    #[test]
    fn decision_table() {
        use PermissionLevel::*;
        let c = ctx();
        let ls = json!({"command": "ls"});
        let rm = json!({"command": "rm -rf build"});
        let edit_in = json!({"file_path": "src/main.rs"});
        let edit_out = json!({"file_path": "/etc/hosts"});

        for level in [Restricted, Auto, Full] {
            assert!(approval_reason(level, "read", &json!({}), &c).is_none());
        }
        assert!(approval_reason(Full, "bash", &rm, &c).is_none());

        assert!(approval_reason(Auto, "bash", &ls, &c).is_none());
        assert!(approval_reason(Auto, "bash", &rm, &c).is_some());
        assert!(approval_reason(Auto, "edit", &edit_in, &c).is_none());
        assert!(approval_reason(Auto, "edit", &edit_out, &c).is_some());

        assert!(approval_reason(Restricted, "bash", &ls, &c).is_some());
        assert!(approval_reason(Restricted, "edit", &edit_in, &c).is_some());
        assert!(approval_reason(Restricted, "todo", &json!({}), &c).is_none());
    }

    #[tokio::test]
    async fn headless_call_needing_approval_is_denied() {
        let mut c = ctx();
        c.session_id = "perm-headless".into();
        set_level(&c.session_id, PermissionLevel::Restricted);
        let err = check("bash", &json!({"command": "ls"}), &c)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no interactive client"), "{err}");
        set_level(&c.session_id, PermissionLevel::Full);
        assert!(check("bash", &json!({"command": "ls"}), &c).await.is_ok());
    }

    #[test]
    fn parse_levels() {
        assert_eq!(PermissionLevel::parse("Full"), Some(PermissionLevel::Full));
        assert_eq!(
            PermissionLevel::parse("restricted"),
            Some(PermissionLevel::Restricted)
        );
        assert_eq!(PermissionLevel::parse("nope"), None);
    }
}
