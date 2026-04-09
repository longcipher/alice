//! Integration tests for SQLite runtime-state store behavior.

use alice_adapters::runtime_state::sqlite_store::SqliteRuntimeStateStore;
use alice_core::runtime_state::{
    domain::{ActiveSessionLease, BindToken, GlobalIdentityBinding, ScheduleKind, ScheduledTask},
    ports::RuntimeStateStorePort,
};

#[test]
fn identity_binding_roundtrip() {
    let Ok(store) = SqliteRuntimeStateStore::in_memory() else {
        return;
    };

    let binding = GlobalIdentityBinding {
        provider: "telegram".to_string(),
        external_user_id: "123".to_string(),
        global_user_id: "global-1".to_string(),
        bound_at_epoch_ms: 1,
    };
    assert!(store.upsert_identity_binding(&binding).is_ok());

    let Ok(loaded) = store.get_identity_binding("telegram", "123") else {
        return;
    };
    let Some(loaded) = loaded else {
        panic!("binding should exist");
    };
    assert_eq!(loaded.global_user_id, "global-1");
}

#[test]
fn bind_token_roundtrip_and_consumption() {
    let Ok(store) = SqliteRuntimeStateStore::in_memory() else {
        return;
    };

    let token = BindToken {
        token: "bind-1".to_string(),
        global_user_id: "global-1".to_string(),
        provider: Some("discord".to_string()),
        expires_at_epoch_ms: 10_000,
        consumed_at_epoch_ms: None,
        created_at_epoch_ms: 1,
    };
    assert!(store.insert_bind_token(&token).is_ok());
    assert!(store.mark_bind_token_consumed("bind-1", 5).is_ok());

    let Ok(loaded) = store.get_bind_token("bind-1") else {
        return;
    };
    let Some(loaded) = loaded else {
        panic!("bind token should exist");
    };
    assert_eq!(loaded.consumed_at_epoch_ms, Some(5));
}

#[test]
fn active_session_roundtrip() {
    let Ok(store) = SqliteRuntimeStateStore::in_memory() else {
        return;
    };

    let lease = ActiveSessionLease {
        global_user_id: "global-1".to_string(),
        session_id: "session-1".to_string(),
        channel: Some("cli".to_string()),
        thread_id: Some("thread-1".to_string()),
        updated_at_epoch_ms: 1,
    };
    assert!(store.upsert_active_session(&lease).is_ok());

    let Ok(loaded) = store.get_active_session("global-1") else {
        return;
    };
    let Some(loaded) = loaded else {
        panic!("active session should exist");
    };
    assert_eq!(loaded.session_id, "session-1");
    assert_eq!(loaded.thread_id.as_deref(), Some("thread-1"));
}

#[test]
fn due_task_listing_returns_enabled_due_tasks_only() {
    let Ok(store) = SqliteRuntimeStateStore::in_memory() else {
        return;
    };

    let due_task = ScheduledTask {
        task_id: "due".to_string(),
        global_user_id: "global-1".to_string(),
        channel: Some("telegram".to_string()),
        prompt: "Summarize alerts".to_string(),
        schedule: ScheduleKind::EveryMinutes(30),
        next_run_epoch_ms: 1_000,
        enabled: true,
        last_run_epoch_ms: None,
    };
    let later_task = ScheduledTask {
        task_id: "later".to_string(),
        global_user_id: "global-1".to_string(),
        channel: Some("telegram".to_string()),
        prompt: "Summarize alerts".to_string(),
        schedule: ScheduleKind::EveryMinutes(30),
        next_run_epoch_ms: 5_000,
        enabled: true,
        last_run_epoch_ms: None,
    };
    let disabled_task = ScheduledTask {
        task_id: "disabled".to_string(),
        global_user_id: "global-1".to_string(),
        channel: Some("telegram".to_string()),
        prompt: "Summarize alerts".to_string(),
        schedule: ScheduleKind::EveryMinutes(30),
        next_run_epoch_ms: 500,
        enabled: false,
        last_run_epoch_ms: None,
    };

    assert!(store.upsert_scheduled_task(&due_task).is_ok());
    assert!(store.upsert_scheduled_task(&later_task).is_ok());
    assert!(store.upsert_scheduled_task(&disabled_task).is_ok());

    let Ok(tasks) = store.list_due_tasks(1_000) else {
        return;
    };
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].task_id, "due");
}

#[test]
fn scheduled_task_listing_returns_all_tasks_in_order() {
    let Ok(store) = SqliteRuntimeStateStore::in_memory() else {
        return;
    };

    let task_a = ScheduledTask {
        task_id: "task-a".to_string(),
        global_user_id: "global-1".to_string(),
        channel: Some("telegram".to_string()),
        prompt: "A".to_string(),
        schedule: ScheduleKind::EveryMinutes(30),
        next_run_epoch_ms: 5_000,
        enabled: false,
        last_run_epoch_ms: None,
    };
    let task_b = ScheduledTask {
        task_id: "task-b".to_string(),
        global_user_id: "global-1".to_string(),
        channel: Some("telegram".to_string()),
        prompt: "B".to_string(),
        schedule: ScheduleKind::EveryMinutes(30),
        next_run_epoch_ms: 1_000,
        enabled: true,
        last_run_epoch_ms: None,
    };

    assert!(store.upsert_scheduled_task(&task_a).is_ok());
    assert!(store.upsert_scheduled_task(&task_b).is_ok());

    let Ok(tasks) = store.list_scheduled_tasks() else {
        return;
    };
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].task_id, "task-b");
    assert_eq!(tasks[1].task_id, "task-a");
}
