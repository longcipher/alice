//! Reusable scheduler tick executor.
//!
//! The executor intentionally does not own any background loop or clock.
//! Callers pass `now_epoch_ms` explicitly so ticks are deterministic and
//! straightforward to test.

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use alice_core::runtime_state::domain::ScheduledTask;

use crate::{context::AliceRuntimeContext, memory_context::run_turn_with_memory};

/// Source of the effective session id used for a task execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerSessionSource {
    /// An active session lease existed for the global user.
    ActiveLease,
    /// No active lease existed, so a deterministic fallback session id was used.
    Fallback,
}

/// Outcome for a single scheduled task during one tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerTickOutcome {
    /// Task ran successfully and was marked executed.
    Executed,
    /// Task failed while resolving the session or running the turn.
    Failed {
        /// Human-readable failure detail for logs and tests.
        error: String,
    },
}

/// Per-task execution record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerTaskExecution {
    /// Stable task identifier.
    pub task_id: String,
    /// Stable global user id.
    pub global_user_id: String,
    /// Effective session id used for the turn.
    pub session_id: String,
    /// Where the session id came from.
    pub session_source: SchedulerSessionSource,
    /// Final outcome for the task.
    pub outcome: SchedulerTickOutcome,
}

/// Summary of one scheduler tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerTickReport {
    /// Timestamp used to evaluate due tasks.
    pub now_epoch_ms: i64,
    /// Number of due tasks loaded from runtime-state storage.
    pub due_task_count: usize,
    /// Per-task execution results in due-task order.
    pub executions: Vec<SchedulerTaskExecution>,
}

/// Stateless scheduler tick executor.
#[derive(Debug, Default, Clone, Copy)]
pub struct SchedulerTickExecutor;

impl SchedulerTickExecutor {
    /// Create a new executor.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Execute one scheduler tick.
    ///
    /// Loads due tasks, resolves their effective session ids, runs each task
    /// through the memory-aware turn pipeline, and marks successful tasks as
    /// executed.
    pub async fn run(
        &self,
        context: &AliceRuntimeContext,
        now_epoch_ms: i64,
    ) -> eyre::Result<SchedulerTickReport> {
        let due_tasks = context.runtime_state_service().list_due_tasks(now_epoch_ms)?;
        let due_task_count = due_tasks.len();
        let mut executions = Vec::with_capacity(due_task_count);

        for task in due_tasks {
            executions.push(self.run_task(context, &task, now_epoch_ms).await);
        }

        Ok(SchedulerTickReport { now_epoch_ms, due_task_count, executions })
    }

    async fn run_task(
        &self,
        context: &AliceRuntimeContext,
        task: &ScheduledTask,
        now_epoch_ms: i64,
    ) -> SchedulerTaskExecution {
        let (session_id, session_source, lease_channel, lease_thread_id) =
            match context.runtime_state_service().get_active_session(&task.global_user_id) {
                Ok(Some(lease)) => (
                    lease.session_id,
                    SchedulerSessionSource::ActiveLease,
                    lease.channel,
                    lease.thread_id,
                ),
                Ok(None) => (
                    fallback_session_id(&task.global_user_id),
                    SchedulerSessionSource::Fallback,
                    None,
                    None,
                ),
                Err(error) => {
                    return SchedulerTaskExecution {
                        task_id: task.task_id.clone(),
                        global_user_id: task.global_user_id.clone(),
                        session_id: fallback_session_id(&task.global_user_id),
                        session_source: SchedulerSessionSource::Fallback,
                        outcome: SchedulerTickOutcome::Failed {
                            error: format!("failed to load active session: {error}"),
                        },
                    };
                }
            };

        let turn_result =
            run_turn_with_memory(context, &session_id, Some(&task.global_user_id), &task.prompt)
                .await;
        let outcome = match turn_result {
            Ok(response) => {
                if let (Some(channel), Some(thread_id)) = (
                    delivery_channel(task.channel.as_deref(), lease_channel.as_deref()),
                    lease_thread_id.as_deref(),
                ) {
                    match context
                        .channel_dispatcher()
                        .post_text(channel, thread_id, &response.content)
                        .await
                    {
                        Ok(true) => {
                            tracing::info!(
                                task_id = %task.task_id,
                                global_user_id = %task.global_user_id,
                                channel = %channel,
                                thread_id = %thread_id,
                                "scheduled task result delivered"
                            );
                        }
                        Ok(false) => {
                            tracing::debug!(
                                task_id = %task.task_id,
                                global_user_id = %task.global_user_id,
                                channel = %channel,
                                thread_id = %thread_id,
                                "scheduled task channel not currently registered"
                            );
                        }
                        Err(error) => {
                            tracing::warn!(
                                task_id = %task.task_id,
                                global_user_id = %task.global_user_id,
                                channel = %channel,
                                thread_id = %thread_id,
                                error = %error,
                                "failed to deliver scheduled task result"
                            );
                        }
                    }
                }

                match context
                    .runtime_state_service()
                    .mark_task_executed(&task.task_id, now_epoch_ms)
                {
                    Ok(Some(_)) => SchedulerTickOutcome::Executed,
                    Ok(None) => SchedulerTickOutcome::Failed {
                        error: format!(
                            "task '{}' disappeared before it could be marked executed",
                            task.task_id
                        ),
                    },
                    Err(error) => SchedulerTickOutcome::Failed {
                        error: format!("failed to mark task '{}' executed: {error}", task.task_id),
                    },
                }
            }
            Err(error) => SchedulerTickOutcome::Failed {
                error: format!("failed to run scheduled task '{}': {error}", task.task_id),
            },
        };

        SchedulerTaskExecution {
            task_id: task.task_id.clone(),
            global_user_id: task.global_user_id.clone(),
            session_id,
            session_source,
            outcome,
        }
    }
}

fn fallback_session_id(global_user_id: &str) -> String {
    format!("scheduled-{global_user_id}")
}

fn delivery_channel<'a>(
    preferred_channel: Option<&'a str>,
    lease_channel: Option<&'a str>,
) -> Option<&'a str> {
    match (preferred_channel, lease_channel) {
        (Some(preferred), Some(active)) if preferred == active => Some(active),
        (Some(_), Some(_)) => None,
        (Some(_), None) => None,
        (None, Some(active)) => Some(active),
        (None, None) => None,
    }
}

/// Spawn a background scheduler loop on the current Tokio runtime.
#[must_use]
pub fn spawn_scheduler_worker(
    context: Arc<AliceRuntimeContext>,
    poll_interval_ms: u64,
) -> tokio::task::JoinHandle<()> {
    let poll_interval = Duration::from_millis(poll_interval_ms.max(1_000));
    tokio::spawn(async move {
        let executor = SchedulerTickExecutor::new();
        loop {
            let now_epoch_ms = current_time_millis();
            match executor.run(&context, now_epoch_ms).await {
                Ok(report) => {
                    for execution in report.executions {
                        match execution.outcome {
                            SchedulerTickOutcome::Executed => {
                                tracing::info!(
                                    task_id = %execution.task_id,
                                    global_user_id = %execution.global_user_id,
                                    session_id = %execution.session_id,
                                    "scheduled task executed"
                                );
                            }
                            SchedulerTickOutcome::Failed { error } => {
                                tracing::warn!(
                                    task_id = %execution.task_id,
                                    global_user_id = %execution.global_user_id,
                                    session_id = %execution.session_id,
                                    error = %error,
                                    "scheduled task execution failed"
                                );
                            }
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!("scheduler tick failed: {error}");
                }
            }

            tokio::time::sleep(poll_interval).await;
        }
    })
}

fn current_time_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
}
