use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::plan_tool::StepStatus;
use codex_protocol::plan_tool::UpdatePlanArgs;
use codex_protocol::protocol::RolloutItem;

pub(super) fn append_todo_reminder(
    mut input: Vec<ResponseItem>,
    todo: Option<UpdatePlanArgs>,
) -> Vec<ResponseItem> {
    let Some(todo) = todo else {
        return input;
    };
    if !todo
        .plan
        .iter()
        .any(|item| !matches!(item.status, StepStatus::Completed))
    {
        return input;
    }

    let mut text = String::from(
        "Current todo list reminder:\n\
         Continue working on the in_progress item before starting pending items. \
         Call update_plan when the todo list changes.\n",
    );
    if let Some(explanation) = todo.explanation.as_deref() {
        text.push_str("\nExplanation: ");
        text.push_str(explanation);
        text.push('\n');
    }
    text.push('\n');
    for item in todo.plan {
        let status = match item.status {
            StepStatus::Pending => "pending",
            StepStatus::InProgress => "in_progress",
            StepStatus::Completed => "completed",
        };
        text.push_str("- [");
        text.push_str(status);
        text.push_str("] ");
        text.push_str(&item.step);
        text.push('\n');
    }

    input.push(ResponseItem::Message {
        id: None,
        role: "developer".to_string(),
        content: vec![ContentItem::InputText { text }],
        end_turn: None,
        phase: None,
    });
    input
}

pub(super) fn active_todo_list_from_rollout_items(
    rollout_items: &[RolloutItem],
) -> Option<UpdatePlanArgs> {
    rollout_items.iter().rev().find_map(|item| {
        let RolloutItem::ResponseItem(ResponseItem::FunctionCall {
            name,
            namespace,
            arguments,
            ..
        }) = item
        else {
            return None;
        };
        if namespace.is_some() || name != "update_plan" {
            return None;
        }
        let update = serde_json::from_str::<UpdatePlanArgs>(arguments).ok()?;
        update
            .plan
            .iter()
            .any(|item| !matches!(item.status, StepStatus::Completed))
            .then_some(update)
    })
}

#[cfg(test)]
mod tests {
    use codex_protocol::models::ContentItem;
    use codex_protocol::models::ResponseItem;
    use codex_protocol::plan_tool::PlanItemArg;
    use codex_protocol::plan_tool::StepStatus;
    use codex_protocol::plan_tool::UpdatePlanArgs;
    use codex_protocol::protocol::RolloutItem;
    use pretty_assertions::assert_eq;

    use super::active_todo_list_from_rollout_items;
    use super::append_todo_reminder;

    #[test]
    fn append_todo_reminder_adds_current_todo_as_developer_message() {
        let input = vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "continue".to_string(),
            }],
            end_turn: None,
            phase: None,
        }];
        let todo = UpdatePlanArgs {
            explanation: Some("Keep the implementation ordered".to_string()),
            plan: vec![
                PlanItemArg {
                    step: "Record the todo list in session state".to_string(),
                    status: StepStatus::InProgress,
                },
                PlanItemArg {
                    step: "Inject a reminder before sampling".to_string(),
                    status: StepStatus::Pending,
                },
            ],
        };

        let output = append_todo_reminder(input.clone(), Some(todo));

        assert_eq!(output.len(), input.len() + 1);
        let ResponseItem::Message { role, content, .. } = output.last().expect("reminder item")
        else {
            panic!("expected reminder message");
        };
        assert_eq!(role, "developer");
        let [ContentItem::InputText { text }] = content.as_slice() else {
            panic!("expected single text reminder");
        };
        assert!(text.contains("Current todo list reminder"));
        assert!(text.contains("[in_progress] Record the todo list in session state"));
        assert!(text.contains("[pending] Inject a reminder before sampling"));
        assert!(text.contains("Call update_plan when the todo list changes"));
    }

    #[test]
    fn append_todo_reminder_skips_empty_or_completed_lists() {
        let input = vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "continue".to_string(),
            }],
            end_turn: None,
            phase: None,
        }];

        let empty = UpdatePlanArgs {
            explanation: None,
            plan: Vec::new(),
        };
        assert_eq!(
            append_todo_reminder(input.clone(), Some(empty)),
            input,
            "empty todo lists should not add reminders"
        );

        let completed = UpdatePlanArgs {
            explanation: None,
            plan: vec![PlanItemArg {
                step: "Already done".to_string(),
                status: StepStatus::Completed,
            }],
        };
        assert_eq!(
            append_todo_reminder(input.clone(), Some(completed)),
            input,
            "all-completed todo lists should not add reminders"
        );

        assert_eq!(append_todo_reminder(input.clone(), None), input);
    }

    #[test]
    fn active_todo_list_from_rollout_items_restores_last_active_update_plan() {
        let rollout_items = vec![
            RolloutItem::ResponseItem(ResponseItem::FunctionCall {
                id: None,
                name: "update_plan".to_string(),
                namespace: None,
                arguments: serde_json::json!({
                    "plan": [
                        {"step": "Old item", "status": "completed"}
                    ]
                })
                .to_string(),
                call_id: "call-1".to_string(),
            }),
            RolloutItem::ResponseItem(ResponseItem::FunctionCall {
                id: None,
                name: "update_plan".to_string(),
                namespace: None,
                arguments: serde_json::json!({
                    "explanation": "Resume active work",
                    "plan": [
                        {"step": "Restore todo state", "status": "in_progress"}
                    ]
                })
                .to_string(),
                call_id: "call-2".to_string(),
            }),
        ];

        let active = active_todo_list_from_rollout_items(&rollout_items)
            .expect("active update_plan should be restored");

        assert_eq!(active.explanation.as_deref(), Some("Resume active work"));
        assert_eq!(active.plan.len(), 1);
        assert_eq!(active.plan[0].step, "Restore todo state");
        assert!(matches!(active.plan[0].status, StepStatus::InProgress));
    }

    #[test]
    fn active_todo_list_from_rollout_items_skips_completed_update_plan() {
        let rollout_items = vec![RolloutItem::ResponseItem(ResponseItem::FunctionCall {
            id: None,
            name: "update_plan".to_string(),
            namespace: None,
            arguments: serde_json::json!({
                "plan": [
                    {"step": "Finished item", "status": "completed"}
                ]
            })
            .to_string(),
            call_id: "call-1".to_string(),
        })];

        assert!(active_todo_list_from_rollout_items(&rollout_items).is_none());
    }
}
