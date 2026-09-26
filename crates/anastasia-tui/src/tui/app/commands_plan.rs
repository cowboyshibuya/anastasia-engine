use super::commands_improve::{interrupt_and_queue_synthetic_message, start_synthetic_user_turn};
use super::{App, DisplayMessage};

/// A parsed `/plan` command. Planning persists until `/build`; read-only restrictions live in the harness.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PlanCommand {
    pub goal: Option<String>,
}

pub(super) fn parse_plan_command(trimmed: &str) -> Option<PlanCommand> {
    let rest = trimmed.strip_prefix("/plan")?;
    // Only treat `/plan` and `/plan <goal>` as a plan command, not `/planfoo`.
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let goal = rest.trim();
    Some(PlanCommand {
        goal: if goal.is_empty() {
            None
        } else {
            Some(goal.to_string())
        },
    })
}

pub(super) fn build_plan_prompt(goal: Option<&str>) -> String {
    crate::tool::planning_prompt(goal)
}

pub(super) fn plan_launch_notice(goal: Option<&str>, interrupted: bool) -> String {
    let prefix = if interrupted {
        "👉 Interrupting and planning"
    } else {
        "🧭 Planning"
    };
    let notice = match goal.map(str::trim).filter(|goal| !goal.is_empty()) {
        Some(goal) => format!("{} {}... (plan-only; no edits)", prefix, goal),
        None => format!("{}... (plan-only; no edits)", prefix),
    };
    format!(
        "{}\nNote: It is better to talk with your agent until it understands what you mean than to make a plan too early.",
        notice
    )
}

pub(super) fn handle_plan_command_local(app: &mut App, command: PlanCommand) {
    if !app.set_local_planning(true, false, command.goal.clone()) {
        return;
    }
    let prompt = build_plan_prompt(app.session.plan_goal.as_deref());
    if app.is_processing {
        interrupt_and_queue_synthetic_message(
            app,
            prompt,
            "Interrupting for /plan...",
            plan_launch_notice(command.goal.as_deref(), true),
        );
    } else {
        app.push_display_message(DisplayMessage::system(plan_launch_notice(
            command.goal.as_deref(),
            false,
        )));
        start_synthetic_user_turn(app, prompt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plan_accepts_bare_and_goal_forms() {
        assert_eq!(
            parse_plan_command("/plan"),
            Some(PlanCommand { goal: None })
        );
        assert_eq!(
            parse_plan_command("/plan   "),
            Some(PlanCommand { goal: None })
        );
        assert_eq!(
            parse_plan_command("/plan add a compact mode"),
            Some(PlanCommand {
                goal: Some("add a compact mode".to_string())
            })
        );
    }

    #[test]
    fn parse_plan_rejects_other_commands() {
        assert_eq!(parse_plan_command("/planner foo"), None);
        assert_eq!(parse_plan_command("/improve"), None);
        assert_eq!(parse_plan_command("plan ahead"), None);
    }

    #[test]
    fn build_plan_prompt_is_plan_only_and_targets_plan_card() {
        let prompt = build_plan_prompt(Some("ship feature x"));
        assert!(prompt.contains("Goal: ship feature x"));
        assert!(prompt.contains("Do NOT implement anything yet"));
        assert!(prompt.contains("```plan"));
        assert!(prompt.contains("`request_user_input`"));
        assert!(prompt.contains("/build"));

        let bare = build_plan_prompt(None);
        assert!(bare.contains("currently in focus in this session"));
    }

    #[test]
    fn launch_notice_encourages_conversation_before_planning() {
        let notice = plan_launch_notice(Some("ship feature x"), false);
        assert!(notice.contains("Planning ship feature x"));
        assert!(
            notice.contains(
                "It is better to talk with your agent until it understands what you mean"
            )
        );
    }
}
