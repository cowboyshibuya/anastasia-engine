use super::{App, DisplayMessage};
use crate::protocol::ServerEvent;
use anastasia_session_types::{UserAnswers, UserQuestion, validate_user_answers};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    style::{Modifier, Style},
    text::Line,
};

pub(super) struct QuestionPanel {
    pub session_id: String,
    pub request_id: String,
    questions: Vec<UserQuestion>,
    index: usize,
    focus: usize,
    answers: UserAnswers,
    custom: Vec<String>,
    cursor: usize,
    error: Option<String>,
}
impl QuestionPanel {
    fn new(session_id: String, request_id: String, questions: Vec<UserQuestion>) -> Self {
        let custom = vec![String::new(); questions.len()];
        Self {
            session_id,
            request_id,
            questions,
            index: 0,
            focus: 0,
            answers: UserAnswers::new(),
            custom,
            cursor: 0,
            error: None,
        }
    }
    fn advance(&mut self) {
        self.index += 1;
        self.focus = 0;
        self.cursor = 0;
        self.error = None;
    }
    /// None keeps editing; Some(None) cancels; Some(Some(...)) submits.
    fn key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        text: Option<&str>,
    ) -> Option<Option<UserAnswers>> {
        let review = self.index == self.questions.len();
        if code == KeyCode::Enter && modifiers.contains(KeyModifiers::SHIFT) {
            if !review && self.focus == self.questions[self.index].options.len() {
                self.insert("\n");
            }
            return None;
        }
        match code {
            KeyCode::Esc => {
                if self.index == 0 {
                    return Some(None);
                }
                self.index -= 1;
                self.focus = 0;
                self.cursor = 0;
                self.error = None;
                return None;
            }
            KeyCode::BackTab => {
                self.index = self.index.saturating_sub(1);
                self.focus = 0;
                self.cursor = 0;
                return None;
            }
            KeyCode::Tab => {
                if !review {
                    self.advance();
                }
                return None;
            }
            KeyCode::Enter if review => {
                match validate_user_answers(&self.questions, &self.answers) {
                    Ok(()) => return Some(Some(self.answers.clone())),
                    Err(e) => {
                        self.error = Some(e);
                        return None;
                    }
                }
            }
            _ if review => return None,
            _ => {}
        }
        let q = &self.questions[self.index];
        let custom = self.focus == q.options.len();
        match code {
            KeyCode::Up if !custom => self.focus = self.focus.saturating_sub(1),
            KeyCode::Down if !custom => {
                self.focus = (self.focus + 1).min(q.options.len());
                self.cursor = self.custom[self.index].chars().count();
            }
            KeyCode::Up if custom => self.focus = self.focus.saturating_sub(1),
            KeyCode::Char(' ') if !custom && q.multi_select => {
                let values = self.answers.entry(q.id.clone()).or_default();
                let label = &q.options[self.focus].label;
                if let Some(i) = values.iter().position(|v| v == label) {
                    values.remove(i);
                } else {
                    values.push(label.clone());
                }
            }
            KeyCode::Enter if modifiers.contains(KeyModifiers::SHIFT) && custom => {
                self.insert("\n")
            }
            KeyCode::Char('j') if modifiers.contains(KeyModifiers::CONTROL) && custom => {
                self.insert("\n")
            }
            KeyCode::Enter => {
                if custom {
                    let value = self.custom[self.index].trim().to_owned();
                    if value.is_empty() {
                        self.error = Some("Write an answer first".into());
                        return None;
                    }
                    // A custom answer is an alternative to the offered choices.
                    self.answers.insert(q.id.clone(), vec![value]);
                } else if !q.multi_select {
                    self.answers
                        .insert(q.id.clone(), vec![q.options[self.focus].label.clone()]);
                }
                if self.answers.get(&q.id).is_none_or(|v| v.is_empty()) {
                    self.error = Some("Select at least one answer with Space".into());
                } else {
                    self.advance();
                }
            }
            KeyCode::Left if custom => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right if custom => {
                self.cursor = (self.cursor + 1).min(self.custom[self.index].chars().count())
            }
            KeyCode::Home if custom => self.cursor = 0,
            KeyCode::End if custom => self.cursor = self.custom[self.index].chars().count(),
            KeyCode::Backspace if custom => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    let a = self.byte_cursor();
                    let b = self.custom[self.index][a..]
                        .chars()
                        .next()
                        .unwrap()
                        .len_utf8()
                        + a;
                    self.custom[self.index].replace_range(a..b, "");
                }
            }
            KeyCode::Delete if custom => {
                let a = self.byte_cursor();
                if let Some(ch) = self.custom[self.index][a..].chars().next() {
                    self.custom[self.index].replace_range(a..a + ch.len_utf8(), "");
                }
            }
            KeyCode::Char(ch)
                if custom && !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert(text.unwrap_or(&ch.to_string()))
            }
            _ => {}
        }
        None
    }
    pub(super) fn paste(&mut self, text: &str) {
        if self.index < self.questions.len()
            && self.focus == self.questions[self.index].options.len()
        {
            self.insert(&text.replace("\r\n", "\n").replace('\r', "\n"));
        }
    }
    fn byte_cursor(&self) -> usize {
        self.custom[self.index]
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.custom[self.index].len())
    }
    fn insert(&mut self, text: &str) {
        if self.custom[self.index].len() + text.len() > 16384 {
            self.error = Some("Answer exceeds 16 KiB".into());
            return;
        }
        let at = self.byte_cursor();
        self.custom[self.index].insert_str(at, text);
        self.cursor += text.chars().count();
        self.error = None;
    }
    pub(super) fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut rows = Vec::new();
        let mut add = |text: String, focused: bool| {
            let style = if focused {
                Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default()
            };
            // Wrap by terminal cell width, including Unicode custom answers.
            let mut row = String::new();
            let mut cells = 0;
            for ch in text.chars() {
                let n = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if ch == '\n' || cells + n > usize::from(width.max(1)) {
                    rows.push(Line::styled(std::mem::take(&mut row), style));
                    cells = 0;
                }
                if ch != '\n' {
                    row.push(ch);
                    cells += n;
                }
            }
            rows.push(Line::styled(row, style));
        };
        if self.index == self.questions.len() {
            add(
                "Review answers · Enter submits · Shift+Tab edits · Esc goes back".into(),
                true,
            );
            for q in &self.questions {
                add(
                    format!(
                        "{}: {}",
                        q.header,
                        self.answers
                            .get(&q.id)
                            .map(|v| v.join(", "))
                            .unwrap_or_else(|| "Unanswered".into())
                    ),
                    false,
                );
            }
        } else {
            let q = &self.questions[self.index];
            add(
                format!(
                    "Question {}/{} · {}",
                    self.index + 1,
                    self.questions.len(),
                    q.header
                ),
                true,
            );
            add(q.question.clone(), false);
            for (i, option) in q.options.iter().enumerate() {
                let checked = self
                    .answers
                    .get(&q.id)
                    .is_some_and(|v| v.contains(&option.label));
                add(
                    format!(
                        "{} {}{}",
                        if self.focus == i { ">" } else { " " },
                        if checked {
                            "[x] "
                        } else if q.multi_select {
                            "[ ] "
                        } else {
                            ""
                        },
                        option.label
                    ),
                    self.focus == i,
                );
                if !option.description.is_empty() {
                    add(format!("  {}", option.description), false);
                }
            }
            let mut text = self.custom[self.index].clone();
            if self.focus == q.options.len() {
                text.insert(self.byte_cursor(), '▏');
            }
            add(
                format!(
                    "{} Custom answer: {}",
                    if self.focus == q.options.len() {
                        ">"
                    } else {
                        " "
                    },
                    text
                ),
                self.focus == q.options.len(),
            );
            add("↑/↓ select · Space toggles · Enter next · Tab/Shift+Tab questions · Esc back/cancel".into(), false);
        }
        if let Some(error) = &self.error {
            add(error.clone(), true);
        }
        rows
    }
}

impl App {
    pub(super) fn ensure_local_question_connection(&mut self) {
        if self
            .question_connection
            .as_ref()
            .is_none_or(|c| c.session_id != self.session.id)
        {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            self.question_connection = Some(crate::tool::request_user_input::connect(
                &self.session.id,
                tx,
            ));
            self.question_events = Some(rx);
            self.planning_policy = Some(crate::tool::register_session_tool_policy(
                &self.session.id,
                None,
                Default::default(),
                self.session.planning,
            ));
        }
    }
    pub(super) fn poll_local_questions(&mut self) -> bool {
        let mut changed = false;
        while let Some(event) = self
            .question_events
            .as_mut()
            .and_then(|rx| rx.try_recv().ok())
        {
            changed |= self.apply_question_event(event);
        }
        changed
    }
    pub(super) fn apply_question_event(&mut self, event: ServerEvent) -> bool {
        match event {
            ServerEvent::QuestionRequest {
                session_id,
                request_id,
                questions,
                ..
            } => {
                let current = self
                    .remote_session_id
                    .as_deref()
                    .unwrap_or(&self.session.id);
                if session_id != current {
                    return false;
                }
                self.question_panel = Some(QuestionPanel::new(session_id, request_id, questions));
                self.request_full_redraw();
                true
            }
            ServerEvent::QuestionClosed { request_id, .. } => {
                if self
                    .question_panel
                    .as_ref()
                    .is_some_and(|p| p.request_id == request_id)
                {
                    self.question_panel = None;
                    self.request_full_redraw();
                    return true;
                }
                false
            }
            _ => false,
        }
    }
    pub(super) fn handle_question_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        text: Option<&str>,
    ) -> bool {
        let Some(panel) = &mut self.question_panel else {
            return false;
        };
        if let Some(answers) = panel.key(code, modifiers, text) {
            let response = (panel.session_id.clone(), panel.request_id.clone(), answers);
            if self.is_remote {
                self.pending_question_response = Some(response);
            } else if let Some(connection) = &self.question_connection {
                if let Err(e) =
                    crate::tool::request_user_input::respond(connection, &response.1, response.2)
                {
                    self.push_display_message(DisplayMessage::error(e.to_string()));
                }
            }
            self.question_panel = None;
        }
        self.request_full_redraw();
        true
    }
    pub(super) fn set_local_planning(
        &mut self,
        planning: bool,
        auto: bool,
        goal: Option<String>,
    ) -> bool {
        if planning && self.provider.handles_tools_internally() {
            self.push_display_message(DisplayMessage::error("This provider executes tools outside the harness; use an API provider for enforced planning"));
            return false;
        }
        let previous = (
            self.session.planning,
            self.session.auto_mode,
            self.session.plan_goal.clone(),
        );
        self.session.planning = planning;
        self.session.auto_mode = auto && !planning;
        self.session.plan_goal = if planning {
            goal.or_else(|| self.session.plan_goal.clone())
        } else {
            None
        };
        if let Err(e) = self.session.save() {
            (
                self.session.planning,
                self.session.auto_mode,
                self.session.plan_goal,
            ) = previous;
            self.push_display_message(DisplayMessage::error(e.to_string()));
            return false;
        }
        self.planning_policy = Some(crate::tool::register_session_tool_policy(
            &self.session.id,
            None,
            Default::default(),
            planning,
        ));
        self.request_full_redraw();
        true
    }

    /// Shift+Tab (local): Build → Plan → Auto → Build.
    pub(super) fn cycle_local_agent_mode(&mut self) {
        let (planning, auto) = next_agent_mode(self.session.planning, self.session.auto_mode);
        if self.set_local_planning(planning, auto, None) {
            self.set_status_notice(agent_mode_notice(planning, auto));
        }
    }
}

/// Build (neither flag) → Plan → Auto → Build.
pub(super) fn next_agent_mode(planning: bool, auto: bool) -> (bool, bool) {
    match (planning, auto) {
        (false, false) => (true, false),
        (true, _) => (false, true),
        (false, true) => (false, false),
    }
}

pub(super) fn agent_mode_notice(planning: bool, auto: bool) -> &'static str {
    if planning {
        "Plan mode · read-only · Shift+Tab to switch"
    } else if auto {
        "Auto mode · plan → build → verify · Shift+Tab to switch"
    } else {
        "Build mode · Shift+Tab to switch"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn panel(multi: bool) -> QuestionPanel {
        QuestionPanel::new(
            "s".into(),
            "r".into(),
            vec![UserQuestion {
                id: "scope".into(),
                header: "Scope".into(),
                question: "Choose".into(),
                options: vec![anastasia_session_types::UserQuestionOption {
                    label: "CLI".into(),
                    description: "Terminal".into(),
                }],
                multi_select: multi,
            }],
        )
    }
    #[test]
    fn selection_review_custom_and_cancel() {
        let mut p = panel(true);
        p.key(KeyCode::Enter, KeyModifiers::NONE, None);
        assert_eq!(p.index, 0);
        p.key(KeyCode::Char(' '), KeyModifiers::NONE, None);
        p.key(KeyCode::Enter, KeyModifiers::NONE, None);
        assert_eq!(p.index, 1);
        assert_eq!(
            p.key(KeyCode::Enter, KeyModifiers::NONE, None)
                .unwrap()
                .unwrap()["scope"],
            vec!["CLI"]
        );
        let mut p = panel(false);
        p.key(KeyCode::Down, KeyModifiers::NONE, None);
        p.insert("café");
        p.key(KeyCode::Char('j'), KeyModifiers::CONTROL, None);
        p.insert("second");
        p.key(KeyCode::Enter, KeyModifiers::NONE, None);
        assert_eq!(
            p.key(KeyCode::Enter, KeyModifiers::NONE, None)
                .unwrap()
                .unwrap()["scope"],
            vec!["café\nsecond"]
        );
        assert_eq!(
            panel(false).key(KeyCode::Esc, KeyModifiers::NONE, None),
            Some(None)
        );
    }
}
