use super::*;
use std::sync::mpsc::{self, Receiver, TryRecvError};

pub(super) enum SessionEdit {
    Rename {
        session: SessionInfo,
        name: String,
        cursor: usize,
    },
    Delete {
        sessions: Vec<SessionInfo>,
        confirm: bool,
    },
    Pending(Receiver<Result<EditResult>>),
    Message(String),
}
pub(super) enum EditResult {
    Renamed(String, String),
    Deleted(Vec<String>, Option<String>),
}

fn local_id(session: &SessionInfo) -> Result<&str> {
    let ResumeTarget::AnastasiaSession { session_id } = &session.resume_target else {
        anyhow::bail!(
            "Only Anastasia transcripts can be edited here. External transcripts are unchanged."
        );
    };
    anyhow::ensure!(
        !session_id.is_empty()
            && session_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'),
        "Invalid session ID"
    );
    Ok(session_id)
}

fn check_local_path(id: &str) -> Result<std::path::PathBuf> {
    let path = crate::session::session_path(id)?;
    anyhow::ensure!(
        !std::fs::symlink_metadata(path.parent().unwrap())?
            .file_type()
            .is_symlink(),
        "Cannot edit a linked session directory"
    );
    anyhow::ensure!(
        !std::fs::symlink_metadata(&path)?.file_type().is_symlink(),
        "Cannot edit a linked transcript"
    );
    Ok(path)
}

fn clear_caches() {
    invalidate_session_list_cache();
    crate::session_list_cache::invalidate();
    if let Ok(home) = crate::storage::anastasia_dir() {
        let _ = std::fs::remove_file(home.join("cache/session-picker-list-v2.json"));
    }
}

fn rename(session: SessionInfo, name: String) -> Result<EditResult> {
    let id = local_id(&session)?.to_string();
    if crate::session::active_session_ids().contains(&id) {
        // Reuse the daemon's rename operation instead of overwriting a live agent's history.
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(rename_live(&session, &id, &name))?;
    } else {
        check_local_path(&id)?;
        let mut stored = Session::load(&id)?;
        anyhow::ensure!(stored.id == id, "Transcript identity mismatch");
        anyhow::ensure!(
            !crate::session::active_session_ids().contains(&id),
            "Session became active. Refresh the list before renaming."
        );
        stored.rename_title(Some(name.clone()));
        stored.save()?;
    }
    clear_caches();
    Ok(EditResult::Renamed(id, name))
}

async fn rename_live(session: &SessionInfo, id: &str, name: &str) -> Result<()> {
    use crate::protocol::{Request, ServerEvent};
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    let presence = crate::session::session_presence();
    let pid = presence
        .iter()
        .find(|session| session.session_id == id)
        .map(|session| session.pid);
    let servers = crate::registry::list_servers().await?;
    let server = servers
        .iter()
        .find(|server| {
            session.server_name.as_deref() == Some(server.name.as_str()) || Some(server.pid) == pid
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "This session is open but its daemon cannot be found. Close it before renaming."
            )
        })?;
    let operation = async {
        let stream = crate::transport::Stream::connect(&server.socket).await?;
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let request = Request::RenameSession {
            id: 2,
            title: Some(name.to_string()),
            target_session_id: Some(id.to_string()),
        };
        writer
            .write_all(format!("{}\n", serde_json::to_string(&request)?).as_bytes())
            .await?;
        let mut line = String::new();
        loop {
            line.clear();
            let size = (&mut reader).take(1024 * 1024).read_line(&mut line).await?;
            anyhow::ensure!(size > 0, "Daemon disconnected before renaming");
            anyhow::ensure!(size < 1024 * 1024, "Daemon event exceeded the size limit");
            match serde_json::from_str::<ServerEvent>(&line)? {
                ServerEvent::Done { id: 2 } => return Ok(()),
                ServerEvent::Error { id: 2, message, .. } => anyhow::bail!(message),
                _ => {}
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(15), operation)
        .await
        .map_err(|_| {
            anyhow::anyhow!("Daemon did not confirm the rename. Refresh the list before retrying.")
        })?
}

fn delete(sessions: Vec<SessionInfo>) -> Result<EditResult> {
    let active = crate::session::active_session_ids();
    let ids = sessions.iter().map(local_id).collect::<Result<Vec<_>>>()?;
    // Preflight the whole batch before removing any transcript.
    for id in &ids {
        anyhow::ensure!(
            !active.iter().any(|active_id| active_id == id),
            "Session {id} is open. Close it before deleting."
        );
        check_local_path(id)?;
    }
    let mut deleted = Vec::new();
    for id in ids {
        let path = crate::session::session_path(id)?;
        let removal = (|| -> Result<()> {
            // ponytail: presence is checked per deletion; shared lifecycle locks if strict cross-process serialization is needed.
            anyhow::ensure!(
                !crate::session::active_session_ids()
                    .iter()
                    .any(|active_id| active_id == id),
                "Session {id} became active. Close it before deleting."
            );
            check_local_path(id)?;
            let directory = path.parent().unwrap();
            let snapshot = format!("{id}.json");
            let snapshot_prefix = format!("{snapshot}.");
            let journal_prefix = format!("{id}.journal.");
            let mut files = Vec::new();
            for entry in std::fs::read_dir(directory)? {
                let entry = entry?;
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name == snapshot
                    || name.starts_with(&snapshot_prefix)
                    || name.starts_with(&journal_prefix)
                {
                    files.push(entry.path());
                }
            }
            // Remove the discoverable snapshot last so a partial failure remains visible.
            files.sort_by_key(|file| file == &path);
            for file in files {
                std::fs::remove_file(file)?;
            }
            Ok(())
        })();
        clear_caches();
        if let Err(error) = removal {
            return Ok(EditResult::Deleted(
                deleted,
                Some(format!(
                    "Could not delete {id}: {error:#}. Some files may have been removed."
                )),
            ));
        }
        deleted.push(id.to_string());
    }
    Ok(EditResult::Deleted(deleted, None))
}

impl SessionPicker {
    fn edit_sessions(&self) -> Vec<SessionInfo> {
        self.selection_or_current_targets()
            .iter()
            .filter_map(|target| self.session_for_target(target).cloned())
            .collect()
    }

    pub(super) fn begin_edit(&mut self, deleting: bool) {
        let sessions = self.edit_sessions();
        if sessions.is_empty() {
            return;
        }
        let validation = sessions
            .iter()
            .try_for_each(|session| local_id(session).map(|_| ()));
        self.edit = Some(match validation {
            Err(error) => SessionEdit::Message(error.to_string()),
            Ok(()) if deleting && sessions.iter().any(|session| self.session_is_current(session) || self.session_is_live(session)) =>
                SessionEdit::Message("Open sessions cannot be deleted. Close them before deleting.".into()),
            Ok(()) if deleting => SessionEdit::Delete { sessions, confirm: false },
            Ok(()) if sessions.len() != 1 => SessionEdit::Message("Select exactly one session to rename, or clear selections to rename the hovered session.".into()),
            Ok(()) => { let session = sessions.into_iter().next().unwrap(); let name = session.title.clone(); let cursor = name.chars().count(); SessionEdit::Rename { session, name, cursor } },
        });
    }

    fn launch_edit(&mut self, operation: impl FnOnce() -> Result<EditResult> + Send + 'static) {
        let (sender, receiver) = mpsc::channel();
        self.edit = Some(SessionEdit::Pending(receiver));
        std::thread::spawn(move || {
            let _ = sender.send(operation());
        });
    }

    pub fn poll_edit(&mut self) -> bool {
        let result = match self.edit.as_ref() {
            Some(SessionEdit::Pending(receiver)) => match receiver.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => {
                    Err(anyhow::anyhow!("Session editing stopped unexpectedly"))
                }
            },
            _ => return false,
        };
        self.edit = None;
        let change = match result {
            Ok(change) => change,
            Err(error) => {
                self.edit = Some(SessionEdit::Message(format!("{error:#}")));
                return true;
            }
        };
        let mut groups = self.all_server_groups.clone();
        let mut orphan = if groups.is_empty() {
            self.all_sessions.clone()
        } else {
            self.all_orphan_sessions.clone()
        };
        let update = |sessions: &mut Vec<SessionInfo>| match &change {
            EditResult::Renamed(id, title) => {
                for session in sessions.iter_mut().filter(|session| &session.id == id) {
                    session.title = title.clone();
                    session.search_index = build_search_index(
                        &session.id,
                        &session.short_name,
                        title,
                        session.working_dir.as_deref(),
                        session.save_label.as_deref(),
                        &session.messages_preview,
                    );
                }
            }
            EditResult::Deleted(ids, _) => sessions.retain(|session| !ids.contains(&session.id)),
        };
        update(&mut orphan);
        for group in &mut groups {
            update(&mut group.sessions);
        }
        self.preview_cache = None;
        self.reseed_grouped(groups, orphan);
        if let EditResult::Deleted(_, Some(error)) = change {
            self.edit = Some(SessionEdit::Message(error));
        }
        true
    }

    pub(super) fn handle_edit_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        let Some(edit) = self.edit.as_mut() else {
            return false;
        };
        if code == KeyCode::Esc
            || (code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL))
        {
            if !matches!(edit, SessionEdit::Pending(_)) {
                self.edit = None;
            }
            return true;
        }
        match edit {
            SessionEdit::Rename {
                session,
                name,
                cursor,
            } => match code {
                KeyCode::Enter
                    if !modifiers.contains(KeyModifiers::SHIFT) && !name.trim().is_empty() =>
                {
                    let session = session.clone();
                    let name = name.trim().to_string();
                    self.launch_edit(move || rename(session, name));
                }
                KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
                    name.clear();
                    *cursor = 0;
                }
                KeyCode::Left => *cursor = cursor.saturating_sub(1),
                KeyCode::Right => *cursor = (*cursor + 1).min(name.chars().count()),
                KeyCode::Home => *cursor = 0,
                KeyCode::End => *cursor = name.chars().count(),
                KeyCode::Backspace if *cursor > 0 => {
                    let end = name
                        .char_indices()
                        .nth(*cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(name.len());
                    let start = name.char_indices().nth(*cursor - 1).unwrap().0;
                    name.replace_range(start..end, "");
                    *cursor -= 1;
                }
                KeyCode::Delete if *cursor < name.chars().count() => {
                    let start = name.char_indices().nth(*cursor).unwrap().0;
                    let end = name
                        .char_indices()
                        .nth(*cursor + 1)
                        .map(|(i, _)| i)
                        .unwrap_or(name.len());
                    name.replace_range(start..end, "");
                }
                KeyCode::Char(c)
                    if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && !c.is_control()
                        && name.chars().count() < 256 =>
                {
                    let at = name
                        .char_indices()
                        .nth(*cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(name.len());
                    name.insert(at, c);
                    *cursor += 1;
                }
                _ => {}
            },
            SessionEdit::Delete { sessions, confirm } => match code {
                KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                    *confirm = !*confirm
                }
                KeyCode::Enter if *confirm && !modifiers.contains(KeyModifiers::SHIFT) => {
                    let sessions = sessions.clone();
                    self.launch_edit(move || delete(sessions));
                }
                KeyCode::Enter if !modifiers.contains(KeyModifiers::SHIFT) => self.edit = None,
                _ => {}
            },
            SessionEdit::Message(_) if code == KeyCode::Enter => self.edit = None,
            _ => {}
        }
        true
    }

    pub fn handle_edit_paste(&mut self, text: &str) -> bool {
        if self.edit.is_none() {
            return false;
        }
        for c in text.chars() {
            self.handle_edit_key(
                KeyCode::Char(if c.is_whitespace() { ' ' } else { c }),
                KeyModifiers::NONE,
            );
        }
        true
    }

    pub(super) fn render_edit(&self, frame: &mut Frame) {
        let Some(edit) = &self.edit else {
            return;
        };
        let accent = rgb(110, 210, 255);
        let danger = rgb(220, 120, 120);
        let warning = rgb(255, 193, 7);
        let muted = Style::default().fg(rgb(190, 190, 200));
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let border = match edit {
            SessionEdit::Delete { .. } => danger,
            SessionEdit::Message(_) => warning,
            _ => accent,
        };
        let (title, lines) = match edit {
            SessionEdit::Rename { name, cursor, .. } => {
                let at = name
                    .char_indices()
                    .nth(*cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(name.len());
                (
                    " Rename session ",
                    vec![
                        Line::from(vec![
                            Span::styled(name[..at].to_string(), bold.fg(Color::White)),
                            Span::styled("|", bold.fg(accent)),
                            Span::styled(name[at..].to_string(), bold.fg(Color::White)),
                        ]),
                        Line::from(""),
                        Line::from(vec![
                            Span::styled("Enter saves", bold.fg(rgb(140, 220, 160))),
                            Span::styled(" · Esc cancels · Ctrl+U clears", muted),
                        ]),
                    ],
                )
            }
            SessionEdit::Delete { sessions, confirm } => {
                let mut lines = vec![Line::from(Span::styled(
                    format!("Permanently delete {} session(s)?", sessions.len()),
                    bold.fg(danger),
                ))];
                for session in sessions.iter().take(4) {
                    lines.push(Line::from(format!("- {}", session.title)));
                }
                if sessions.len() > 4 {
                    lines.push(Line::from(format!("... and {} more", sessions.len() - 4)));
                }
                lines.push(Line::from(Span::styled(
                    "History and backups will be removed.",
                    Style::default().fg(warning),
                )));
                lines.push(Line::from(vec![
                    Span::styled(
                        if *confirm { " Cancel " } else { "[ Cancel ]" },
                        if *confirm {
                            muted
                        } else {
                            bold.fg(Color::Black).bg(accent)
                        },
                    ),
                    Span::raw("    "),
                    Span::styled(
                        if *confirm { "[ Delete ]" } else { " Delete " },
                        if *confirm {
                            bold.fg(Color::Black).bg(danger)
                        } else {
                            bold.fg(danger)
                        },
                    ),
                ]));
                lines.push(Line::from(Span::styled(
                    "Left/Right chooses · Enter confirms · Esc cancels",
                    muted,
                )));
                (" Delete sessions ", lines)
            }
            SessionEdit::Pending(_) => (
                " Session editing ",
                vec![Line::from(Span::styled(
                    "Saving changes...",
                    bold.fg(accent),
                ))],
            ),
            SessionEdit::Message(message) => (
                " Session editing ",
                vec![
                    Line::from(Span::styled(message.clone(), Style::default().fg(warning))),
                    Line::from(""),
                    Line::from(Span::styled("Enter / Esc returns to the list", muted)),
                ],
            ),
        };
        let full = frame.area();
        let width = full.width.saturating_sub(4).min(74);
        let content_width = usize::from(width.saturating_sub(2)).max(1);
        let content_height: usize = lines
            .iter()
            .map(|line| line.width().max(1).div_ceil(content_width))
            .sum();
        let height = full.height.saturating_sub(2).min(content_height as u16 + 4);
        let area = Rect::new(
            full.x + full.width.saturating_sub(width) / 2,
            full.y + full.height.saturating_sub(height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(ratatui::widgets::Wrap { trim: false })
                .block(
                    Block::default()
                        .title(Span::styled(title, bold.fg(border)))
                        .border_style(Style::default().fg(border))
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded),
                ),
            area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::session_picker::tests::make_session;

    fn row(id: &str) -> SessionInfo {
        make_session(id, id, false, SessionStatus::Closed)
    }

    #[test]
    fn selection_rename_confirmation_and_paste_contract() {
        let mut picker = SessionPicker::new(vec![row("edit_alpha"), row("edit_beta")]);
        picker.selected_session_ids.insert("edit_beta".into());
        picker
            .handle_overlay_key(KeyCode::Char('r'), KeyModifiers::NONE)
            .unwrap();
        let Some(SessionEdit::Rename { session, .. }) = &picker.edit else {
            panic!("rename missing");
        };
        assert_eq!(session.id, "edit_beta");
        picker.handle_edit_key(KeyCode::Char('u'), KeyModifiers::CONTROL);
        picker.handle_edit_paste("café\nnotes");
        let Some(SessionEdit::Rename { name, .. }) = &picker.edit else {
            panic!("paste submitted rename");
        };
        assert_eq!(name, "café notes");
        picker.handle_edit_key(KeyCode::Esc, KeyModifiers::NONE);
        picker.selected_session_ids.insert("edit_alpha".into());
        picker.begin_edit(false);
        assert!(matches!(picker.edit, Some(SessionEdit::Message(_))));
        picker.handle_edit_key(KeyCode::Esc, KeyModifiers::NONE);
        picker
            .handle_overlay_key(KeyCode::Delete, KeyModifiers::NONE)
            .unwrap();
        assert!(
            matches!(&picker.edit, Some(SessionEdit::Delete { sessions, confirm: false }) if sessions.len() == 2)
        );
        picker.handle_edit_paste("yes\n");
        assert!(matches!(
            picker.edit,
            Some(SessionEdit::Delete { confirm: false, .. })
        ));
        picker.handle_edit_key(KeyCode::Enter, KeyModifiers::NONE);
        assert!(picker.edit.is_none(), "Cancel must be the default");
        picker.clear_selected_sessions();
        picker.search_active = true;
        picker.search_query = "edit".into();
        picker
            .handle_overlay_key(KeyCode::Tab, KeyModifiers::NONE)
            .unwrap();
        assert!(!picker.search_active);
        assert_eq!(picker.search_query, "edit");
        picker.begin_edit(false);
        assert!(
            matches!(&picker.edit, Some(SessionEdit::Rename { session, .. }) if session.id == picker.selected_session().unwrap().id)
        );
        picker.handle_edit_key(KeyCode::Esc, KeyModifiers::NONE);
        picker.current_session_id = Some(picker.selected_session().unwrap().id.clone());
        picker.begin_edit(true);
        assert!(
            matches!(&picker.edit, Some(SessionEdit::Message(message)) if message.contains("Open sessions"))
        );
    }

    #[test]
    fn external_and_invalid_targets_are_rejected() {
        let mut external = row("edit_external");
        external.resume_target = ResumeTarget::CodexSession {
            session_id: "external".into(),
            session_path: "/tmp/external".into(),
        };
        assert!(local_id(&external).is_err());
        assert!(local_id(&row("../outside")).is_err());
        let mut picker = SessionPicker::new(vec![external]);
        picker.begin_edit(true);
        assert!(matches!(picker.edit, Some(SessionEdit::Message(_))));
    }

    #[test]
    fn completed_edits_update_rows_and_selection_without_resetting_filter() {
        let mut picker = SessionPicker::new(vec![row("edit_alpha"), row("edit_beta")]);
        picker.search_query = "new name".into();
        let (tx, rx) = mpsc::channel();
        picker.edit = Some(SessionEdit::Pending(rx));
        tx.send(Ok(EditResult::Renamed(
            "edit_beta".into(),
            "New name".into(),
        )))
        .unwrap();
        assert!(picker.poll_edit());
        assert_eq!(picker.search_query, "new name");
        assert_eq!(picker.visible_session_count(), 1);
        assert_eq!(picker.selected_session().unwrap().title, "New name");
        picker.selected_session_ids.insert("edit_beta".into());
        let (tx, rx) = mpsc::channel();
        picker.edit = Some(SessionEdit::Pending(rx));
        tx.send(Ok(EditResult::Deleted(vec!["edit_beta".into()], None)))
            .unwrap();
        assert!(picker.poll_edit());
        assert_eq!(picker.visible_session_count(), 0);
        assert!(picker.selected_session_ids.is_empty());
    }
    #[test]
    fn storage_rename_delete_and_live_session_protection() {
        let _lock = crate::storage::lock_test_env();
        let home = tempfile::tempdir().unwrap();
        struct Restore(Vec<(&'static str, Option<std::ffi::OsString>)>);
        impl Drop for Restore {
            fn drop(&mut self) {
                for (key, old) in &self.0 {
                    if let Some(old) = old {
                        crate::env::set_var(key, old);
                    } else {
                        crate::env::remove_var(key);
                    }
                }
            }
        }
        let _restore = Restore(
            ["ANASTASIA_CLI_HOME", "ANASTASIA_CLI_RUNTIME_DIR"]
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect(),
        );
        crate::env::set_var("ANASTASIA_CLI_HOME", home.path());
        crate::env::set_var("ANASTASIA_CLI_RUNTIME_DIR", home.path().join("run"));
        for id in ["edit_disk_alpha", "edit_disk_beta"] {
            let mut stored =
                Session::create_with_id(id.into(), None, Some("Original title".into()));
            stored.add_message(
                crate::message::Role::User,
                vec![crate::message::ContentBlock::Text {
                    text: "Preserved history".into(),
                    cache_control: None,
                }],
            );
            stored.save().unwrap();
        }
        let original = Session::load("edit_disk_alpha").unwrap();
        rename(row("edit_disk_alpha"), "Renamed café".into()).unwrap();
        let renamed = Session::load("edit_disk_alpha").unwrap();
        assert_eq!(renamed.custom_title.as_deref(), Some("Renamed café"));
        assert_eq!(renamed.messages.len(), original.messages.len());
        assert_eq!(renamed.title, original.title);
        crate::storage::register_active_pid("edit_disk_beta", std::process::id());
        assert!(delete(vec![row("edit_disk_alpha"), row("edit_disk_beta")]).is_err());
        assert!(crate::session::session_exists("edit_disk_alpha"));
        crate::storage::unregister_active_pid("edit_disk_beta");
        let snapshot = crate::session::session_path("edit_disk_alpha").unwrap();
        let backup = snapshot.with_extension("json.bak");
        std::fs::write(&backup, "backup").unwrap();
        let journal = crate::session::session_journal_path("edit_disk_alpha").unwrap();
        std::fs::write(&journal, "journal").unwrap();
        let unrelated = snapshot
            .parent()
            .unwrap()
            .join("edit_disk_alpha_other.json");
        std::fs::write(&unrelated, "unrelated").unwrap();
        let EditResult::Deleted(ids, None) =
            delete(vec![row("edit_disk_alpha"), row("edit_disk_beta")]).unwrap()
        else {
            panic!("delete failed");
        };
        assert_eq!(ids.len(), 2);
        assert!(!snapshot.exists() && !backup.exists() && !journal.exists());
        assert!(!crate::session::session_exists("edit_disk_beta"));
        assert!(unrelated.exists());
    }
}
