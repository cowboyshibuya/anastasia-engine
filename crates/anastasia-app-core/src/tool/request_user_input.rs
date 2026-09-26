use super::{Tool, ToolContext, ToolOutput};
use crate::protocol::ServerEvent;
use anastasia_session_types::{
    UserAnswers, UserQuestion, validate_user_answers, validate_user_questions,
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use tokio::sync::{mpsc, oneshot};

struct Connection {
    owner: String,
    events: mpsc::UnboundedSender<ServerEvent>,
    pending: Option<Pending>,
}
struct Pending {
    id: String,
    questions: Vec<UserQuestion>,
    response: oneshot::Sender<Result<UserAnswers, String>>,
}
static CONNECTIONS: LazyLock<Mutex<HashMap<String, Connection>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// A connection owns its response channel. Dropping it cancels any pending
/// question, including when the client elects to keep generation alive.
pub struct QuestionConnection {
    pub session_id: String,
    owner: String,
}
impl Drop for QuestionConnection {
    fn drop(&mut self) {
        let mut connections = CONNECTIONS.lock().unwrap_or_else(|e| e.into_inner());
        if connections
            .get(&self.session_id)
            .is_some_and(|c| c.owner == self.owner)
        {
            connections.remove(&self.session_id);
        }
    }
}
pub fn connect(session_id: &str, events: mpsc::UnboundedSender<ServerEvent>) -> QuestionConnection {
    let owner = uuid::Uuid::new_v4().to_string();
    if let Some(old) = CONNECTIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(
            session_id.into(),
            Connection {
                owner: owner.clone(),
                events,
                pending: None,
            },
        )
    {
        if let Some(pending) = &old.pending {
            let _ = old.events.send(ServerEvent::QuestionClosed {
                session_id: session_id.into(),
                request_id: pending.id.clone(),
            });
        }
    }
    QuestionConnection {
        session_id: session_id.into(),
        owner,
    }
}

pub fn respond(
    connection: &QuestionConnection,
    request_id: &str,
    answers: Option<UserAnswers>,
) -> Result<()> {
    let mut connections = CONNECTIONS.lock().unwrap_or_else(|e| e.into_inner());
    let c = connections
        .get_mut(&connection.session_id)
        .filter(|c| c.owner == connection.owner)
        .ok_or_else(|| anyhow!("Question connection is no longer active"))?;
    let p = c
        .pending
        .as_ref()
        .filter(|p| p.id == request_id)
        .ok_or_else(|| anyhow!("Stale or unknown question request"))?;
    if let Some(answers) = &answers {
        validate_user_answers(&p.questions, answers).map_err(|e| anyhow!(e))?;
    }
    let pending = c.pending.take().expect("validated pending request");
    let _ = pending
        .response
        .send(answers.ok_or_else(|| "User cancelled the question".into()));
    let _ = c.events.send(ServerEvent::QuestionClosed {
        session_id: connection.session_id.clone(),
        request_id: request_id.into(),
    });
    Ok(())
}

struct PendingGuard {
    session_id: String,
    request_id: String,
}
impl Drop for PendingGuard {
    fn drop(&mut self) {
        let mut connections = CONNECTIONS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = connections.get_mut(&self.session_id)
            && c.pending.as_ref().is_some_and(|p| p.id == self.request_id)
        {
            c.pending.take();
            let _ = c.events.send(ServerEvent::QuestionClosed {
                session_id: self.session_id.clone(),
                request_id: self.request_id.clone(),
            });
        }
    }
}

pub struct RequestUserInputTool;
#[derive(Deserialize)]
struct Input {
    questions: Vec<UserQuestion>,
}
#[async_trait]
impl Tool for RequestUserInputTool {
    fn name(&self) -> &str {
        "request_user_input"
    }
    fn description(&self) -> &str {
        "Ask 1–3 material clarification questions with selectable options or custom text. Waits for explicit user answers. Explore discoverable facts first; do not ask unnecessary questions. Available only when an interactive client supports questions."
    }
    fn parameters_schema(&self) -> Value {
        json!({"type":"object","required":["questions"],"properties":{"questions":{"type":"array","minItems":1,"maxItems":3,"items":{"type":"object","required":["id","header","question","options","multi_select"],"properties":{"id":{"type":"string"},"header":{"type":"string"},"question":{"type":"string"},"multi_select":{"type":"boolean"},"options":{"type":"array","maxItems":12,"items":{"type":"object","required":["label","description"],"properties":{"label":{"type":"string"},"description":{"type":"string"}}}}}}}}})
    }
    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let input: Input = serde_json::from_value(input)?;
        validate_user_questions(&input.questions).map_err(|e| anyhow!(e))?;
        let answers = ask(&ctx, input.questions.clone()).await?;
        Ok(ToolOutput::new(serde_json::to_string(
            &json!({"questions":input.questions,"answers":answers}),
        )?))
    }
}

/// Whether an interactive client can answer questions for this session.
pub(crate) fn has_client(session_id: &str) -> bool {
    CONNECTIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains_key(session_id)
}

/// Ask the session's interactive client and wait for the answers. Shared by
/// `request_user_input` and the tool permission gate.
pub(crate) async fn ask(ctx: &ToolContext, questions: Vec<UserQuestion>) -> Result<UserAnswers> {
    let request_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();
    {
        let mut connections = CONNECTIONS.lock().unwrap_or_else(|e| e.into_inner());
        let c = connections.get_mut(&ctx.session_id).ok_or_else(|| {
            anyhow!("Selectable questions are unavailable: no supporting interactive client")
        })?;
        if c.pending.is_some() {
            return Err(anyhow!(
                "A question batch is already pending for this session"
            ));
        }
        c.events
            .send(ServerEvent::QuestionRequest {
                session_id: ctx.session_id.clone(),
                request_id: request_id.clone(),
                tool_call_id: ctx.tool_call_id.clone(),
                questions: questions.clone(),
            })
            .map_err(|_| anyhow!("Question client disconnected"))?;
        c.pending = Some(Pending {
            id: request_id.clone(),
            questions,
            response: tx,
        });
    }
    let _guard = PendingGuard {
        session_id: ctx.session_id.clone(),
        request_id,
    };
    let answers = tokio::select! {
        response = rx => response.map_err(|_| anyhow!("Question client disconnected"))?.map_err(|e| anyhow!(e))?,
        _ = async {
            match &ctx.graceful_shutdown_signal {
                Some(signal) => signal.notified().await,
                None => futures::future::pending::<()>().await,
            }
        } => return Err(anyhow!("Question cancelled by interruption")),
    };
    Ok(answers)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context(session: &str) -> ToolContext {
        ToolContext {
            session_id: session.into(),
            message_id: "m".into(),
            tool_call_id: "call".into(),
            working_dir: None,
            stdin_request_tx: None,
            graceful_shutdown_signal: None,
            execution_mode: super::super::ToolExecutionMode::AgentTurn,
        }
    }
    fn input() -> Value {
        json!({"questions":[{"id":"scope","header":"Scope","question":"Which?","options":[{"label":"CLI","description":"Terminal"}],"multi_select":false}]})
    }
    #[tokio::test]
    async fn explicit_answers_ownership_cancellation_and_disconnect() {
        let session = uuid::Uuid::new_v4().to_string();
        assert!(
            RequestUserInputTool
                .execute(input(), context(&session))
                .await
                .unwrap_err()
                .to_string()
                .contains("unavailable")
        );
        let (events, mut rx) = mpsc::unbounded_channel();
        let connection = connect(&session, events);
        let ctx = context(&session);
        let task = tokio::spawn(async move { RequestUserInputTool.execute(input(), ctx).await });
        let ServerEvent::QuestionRequest {
            request_id,
            tool_call_id,
            ..
        } = rx.recv().await.unwrap()
        else {
            panic!("question event")
        };
        assert_eq!(tool_call_id, "call");
        assert!(
            RequestUserInputTool
                .execute(input(), context(&session))
                .await
                .unwrap_err()
                .to_string()
                .contains("already pending")
        );
        let (events, _) = mpsc::unbounded_channel();
        let foreign = connect(&uuid::Uuid::new_v4().to_string(), events);
        let answers: UserAnswers = [("scope".into(), vec!["CLI".into()])].into();
        assert!(respond(&foreign, &request_id, Some(answers.clone())).is_err());
        assert!(respond(&connection, "stale", Some(answers.clone())).is_err());
        assert!(respond(&connection, &request_id, Some(UserAnswers::new())).is_err());
        respond(&connection, &request_id, Some(answers.clone())).unwrap();
        assert!(respond(&connection, &request_id, Some(answers)).is_err());
        assert!(task.await.unwrap().unwrap().output.contains("CLI"));
        assert!(matches!(
            rx.recv().await,
            Some(ServerEvent::QuestionClosed { .. })
        ));
        let ctx = context(&session);
        let task = tokio::spawn(async move { RequestUserInputTool.execute(input(), ctx).await });
        let ServerEvent::QuestionRequest { request_id, .. } = rx.recv().await.unwrap() else {
            panic!("question")
        };
        respond(&connection, &request_id, None).unwrap();
        assert!(
            task.await
                .unwrap()
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
        rx.recv().await.unwrap();
        let ctx = context(&session);
        let task = tokio::spawn(async move { RequestUserInputTool.execute(input(), ctx).await });
        rx.recv().await.unwrap();
        drop(connection);
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err()
                .to_string()
                .contains("disconnected")
        );
    }
    #[tokio::test]
    async fn interruption_releases_the_pending_batch() {
        let session = uuid::Uuid::new_v4().to_string();
        let (events, mut rx) = mpsc::unbounded_channel();
        let connection = connect(&session, events);
        let signal = anastasia_agent_runtime::InterruptSignal::new();
        let mut ctx = context(&session);
        ctx.graceful_shutdown_signal = Some(signal.clone());
        let task = tokio::spawn(async move { RequestUserInputTool.execute(input(), ctx).await });
        rx.recv().await.unwrap();
        signal.fire();
        assert!(
            task.await
                .unwrap()
                .unwrap_err()
                .to_string()
                .contains("interruption")
        );
        assert!(matches!(
            rx.recv().await,
            Some(ServerEvent::QuestionClosed { .. })
        ));
        assert!(respond(&connection, "old", None).is_err());
    }
}
