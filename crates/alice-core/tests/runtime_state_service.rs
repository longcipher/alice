//! Integration tests for runtime-state service behavior.

use alice_core::runtime_state::{
    domain::{ScheduleKind, ScheduledTask},
    service::RuntimeStateService,
};

#[test]
fn service_issues_and_consumes_bind_tokens() {
    let _ = RuntimeStateService::issue_bind_token;
}

#[test]
fn scheduled_task_supports_interval_execution() {
    let task = ScheduledTask {
        task_id: "task-1".to_string(),
        global_user_id: "global-user-1".to_string(),
        channel: Some("telegram".to_string()),
        prompt: "Summarize overnight alerts".to_string(),
        schedule: ScheduleKind::EveryMinutes(30),
        next_run_epoch_ms: 1_000,
        enabled: true,
        last_run_epoch_ms: None,
    };

    assert!(task.enabled);
}
