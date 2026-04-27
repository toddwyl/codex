use crate::function_tool::FunctionCallError;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
use codex_protocol::config_types::ModeKind;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::plan_tool::UpdatePlanArgs;
use codex_protocol::protocol::EventMsg;
use serde_json::Value as JsonValue;

pub struct PlanHandler;

pub struct PlanToolOutput;

const PLAN_UPDATED_MESSAGE: &str = "Plan updated";

impl ToolOutput for PlanToolOutput {
    fn log_preview(&self) -> String {
        PLAN_UPDATED_MESSAGE.to_string()
    }

    fn success_for_logging(&self) -> bool {
        true
    }

    fn to_response_item(&self, call_id: &str, _payload: &ToolPayload) -> ResponseInputItem {
        let mut output = FunctionCallOutputPayload::from_text(PLAN_UPDATED_MESSAGE.to_string());
        output.success = Some(true);

        ResponseInputItem::FunctionCallOutput {
            call_id: call_id.to_string(),
            output,
        }
    }

    fn code_mode_result(&self, _payload: &ToolPayload) -> JsonValue {
        JsonValue::Object(serde_json::Map::new())
    }
}

impl ToolHandler for PlanHandler {
    type Output = PlanToolOutput;

    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<Self::Output, FunctionCallError> {
        let ToolInvocation {
            session,
            turn,
            call_id,
            payload,
            ..
        } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "update_plan handler received unsupported payload".to_string(),
                ));
            }
        };

        handle_update_plan(session.as_ref(), turn.as_ref(), arguments, call_id).await?;

        Ok(PlanToolOutput)
    }
}

/// This function doesn't do anything useful. However, it gives the model a structured way to record its plan that clients can read and render.
/// So it's the _inputs_ to this function that are useful to clients, not the outputs and neither are actually useful for the model other
/// than forcing it to come up and document a plan (TBD how that affects performance).
pub(crate) async fn handle_update_plan(
    session: &Session,
    turn_context: &TurnContext,
    arguments: String,
    _call_id: String,
) -> Result<String, FunctionCallError> {
    if turn_context.collaboration_mode.mode == ModeKind::Plan {
        return Err(FunctionCallError::RespondToModel(
            "update_plan is a TODO/checklist tool and is not allowed in Plan mode".to_string(),
        ));
    }
    let args = parse_update_plan_arguments(&arguments)?;
    session.set_active_todo_list(args.clone()).await;
    session
        .send_event(turn_context, EventMsg::PlanUpdate(args))
        .await;
    Ok("Plan updated".to_string())
}

fn parse_update_plan_arguments(arguments: &str) -> Result<UpdatePlanArgs, FunctionCallError> {
    serde_json::from_str::<UpdatePlanArgs>(arguments).map_err(|e| {
        FunctionCallError::RespondToModel(format!("failed to parse function arguments: {e}"))
    })
}

#[cfg(test)]
mod tests {
    use codex_protocol::plan_tool::StepStatus;
    use pretty_assertions::assert_eq;

    use super::handle_update_plan;
    use crate::session::tests::make_session_and_context;

    #[tokio::test]
    async fn update_plan_records_active_todo_list_in_session_state() {
        let (session, turn_context) = make_session_and_context().await;
        let arguments = serde_json::json!({
            "explanation": "Keep work ordered",
            "plan": [
                {"step": "Record todo state", "status": "in_progress"},
                {"step": "Inject reminder", "status": "pending"}
            ]
        })
        .to_string();

        handle_update_plan(&session, &turn_context, arguments, "call-1".to_string())
            .await
            .expect("update_plan should succeed");

        let active = session
            .active_todo_list()
            .await
            .expect("active todo list should be recorded");
        assert_eq!(active.explanation.as_deref(), Some("Keep work ordered"));
        assert_eq!(active.plan.len(), 2);
        assert_eq!(active.plan[0].step, "Record todo state");
        assert!(matches!(active.plan[0].status, StepStatus::InProgress));
        assert_eq!(active.plan[1].step, "Inject reminder");
        assert!(matches!(active.plan[1].status, StepStatus::Pending));
    }

    #[tokio::test]
    async fn update_plan_clears_session_todo_list_when_all_steps_are_completed() {
        let (session, turn_context) = make_session_and_context().await;
        let active = serde_json::json!({
            "plan": [
                {"step": "Record todo state", "status": "in_progress"}
            ]
        })
        .to_string();
        handle_update_plan(&session, &turn_context, active, "call-1".to_string())
            .await
            .expect("active update_plan should succeed");

        let completed = serde_json::json!({
            "plan": [
                {"step": "Record todo state", "status": "completed"}
            ]
        })
        .to_string();
        handle_update_plan(&session, &turn_context, completed, "call-2".to_string())
            .await
            .expect("completed update_plan should succeed");

        assert!(session.active_todo_list().await.is_none());
    }
}
