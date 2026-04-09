//! Domain types for runtime-state persistence.

use serde::{Deserialize, Serialize};

use crate::runtime_state::error::RuntimeStateValidationError;

/// Binding from an external channel identity to a global Alice user id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalIdentityBinding {
    /// Channel/provider name such as `cli`, `telegram`, or `discord`.
    pub provider: String,
    /// External user identifier from the provider.
    pub external_user_id: String,
    /// Stable Alice-global user id.
    pub global_user_id: String,
    /// Unix epoch milliseconds for the binding creation/update time.
    pub bound_at_epoch_ms: i64,
}

/// One-time token used to link a new provider identity to a global user id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindToken {
    /// Unique token value.
    pub token: String,
    /// The global user this token links to.
    pub global_user_id: String,
    /// Optional provider restriction. When set, only that provider may consume it.
    pub provider: Option<String>,
    /// Unix epoch milliseconds when this token expires.
    pub expires_at_epoch_ms: i64,
    /// Unix epoch milliseconds when this token was consumed.
    pub consumed_at_epoch_ms: Option<i64>,
    /// Unix epoch milliseconds when this token was created.
    pub created_at_epoch_ms: i64,
}

/// Leased active session for a global user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveSessionLease {
    /// The stable global user id.
    pub global_user_id: String,
    /// The effective Alice session id currently associated with the user.
    pub session_id: String,
    /// Optional channel label where this session was last active.
    pub channel: Option<String>,
    /// Optional external thread or conversation id associated with the session.
    pub thread_id: Option<String>,
    /// Unix epoch milliseconds of the last session update.
    pub updated_at_epoch_ms: i64,
}

/// Supported schedule representations for recurring tasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScheduleKind {
    /// Execute every `n` minutes.
    EveryMinutes(u32),
    /// Execute once per hour at the given minute.
    Hourly {
        /// Minute within the hour in `0..=59`.
        minute: u8,
    },
    /// Execute once per day at `hour:minute` in local time.
    DailyAt {
        /// Hour within the day in `0..=23`.
        hour: u8,
        /// Minute within the hour in `0..=59`.
        minute: u8,
    },
}

impl ScheduleKind {
    /// Validate a schedule kind.
    pub const fn validate(&self) -> Result<(), RuntimeStateValidationError> {
        match self {
            Self::EveryMinutes(minutes) if *minutes == 0 => {
                Err(RuntimeStateValidationError::InvalidScheduleMinutes)
            }
            Self::EveryMinutes(_) => Ok(()),
            Self::Hourly { minute } if *minute > 59 => {
                Err(RuntimeStateValidationError::InvalidScheduleMinute)
            }
            Self::Hourly { .. } => Ok(()),
            Self::DailyAt { hour, .. } if *hour > 23 => {
                Err(RuntimeStateValidationError::InvalidScheduleHour)
            }
            Self::DailyAt { minute, .. } if *minute > 59 => {
                Err(RuntimeStateValidationError::InvalidScheduleMinute)
            }
            Self::DailyAt { .. } => Ok(()),
        }
    }

    /// Compute the next run after a successful execution timestamp.
    #[must_use]
    pub fn next_run_after(&self, ran_at_epoch_ms: i64) -> i64 {
        match self {
            Self::EveryMinutes(minutes) => ran_at_epoch_ms + i64::from(*minutes) * 60_000,
            Self::Hourly { minute } => align_hourly(ran_at_epoch_ms, *minute),
            Self::DailyAt { hour, minute } => align_daily(ran_at_epoch_ms, *hour, *minute),
        }
    }
}

/// Persisted scheduled background task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledTask {
    /// Stable task identifier.
    pub task_id: String,
    /// Global user id that owns this task.
    pub global_user_id: String,
    /// Preferred output channel for results.
    pub channel: Option<String>,
    /// Prompt executed when the task runs.
    pub prompt: String,
    /// Recurrence strategy.
    pub schedule: ScheduleKind,
    /// Unix epoch milliseconds when the task should next run.
    pub next_run_epoch_ms: i64,
    /// Whether the task is enabled.
    pub enabled: bool,
    /// Unix epoch milliseconds of the most recent successful execution.
    pub last_run_epoch_ms: Option<i64>,
}

impl ScheduledTask {
    /// Validate a task before persistence.
    pub fn validate(&self) -> Result<(), RuntimeStateValidationError> {
        if self.prompt.trim().is_empty() {
            return Err(RuntimeStateValidationError::EmptyTaskPrompt);
        }
        self.schedule.validate()
    }
}

const HOUR_MS: i64 = 60 * 60 * 1_000;
const MINUTE_MS: i64 = 60 * 1_000;
const DAY_MS: i64 = 24 * HOUR_MS;

fn align_hourly(ran_at_epoch_ms: i64, minute: u8) -> i64 {
    let current_hour = (ran_at_epoch_ms / HOUR_MS) * HOUR_MS;
    let candidate = current_hour + i64::from(minute) * MINUTE_MS;
    if candidate > ran_at_epoch_ms { candidate } else { candidate + HOUR_MS }
}

fn align_daily(ran_at_epoch_ms: i64, hour: u8, minute: u8) -> i64 {
    let current_day = (ran_at_epoch_ms / DAY_MS) * DAY_MS;
    let candidate = current_day + i64::from(hour) * HOUR_MS + i64::from(minute) * MINUTE_MS;
    if candidate > ran_at_epoch_ms { candidate } else { candidate + DAY_MS }
}

#[cfg(test)]
mod tests {
    use super::{GlobalIdentityBinding, ScheduleKind, ScheduledTask};

    #[test]
    fn global_identity_binding_stores_fields() {
        let binding = GlobalIdentityBinding {
            provider: "telegram".to_string(),
            external_user_id: "123".to_string(),
            global_user_id: "global-1".to_string(),
            bound_at_epoch_ms: 42,
        };

        assert_eq!(binding.provider, "telegram");
        assert_eq!(binding.global_user_id, "global-1");
    }

    #[test]
    fn every_minutes_next_run_advances_by_interval() {
        let schedule = ScheduleKind::EveryMinutes(30);
        assert_eq!(schedule.next_run_after(1_000), 1_801_000);
    }

    #[test]
    fn scheduled_task_validate_rejects_blank_prompt() {
        let task = ScheduledTask {
            task_id: "task-1".to_string(),
            global_user_id: "global-1".to_string(),
            channel: None,
            prompt: "   ".to_string(),
            schedule: ScheduleKind::EveryMinutes(5),
            next_run_epoch_ms: 1_000,
            enabled: true,
            last_run_epoch_ms: None,
        };

        assert!(task.validate().is_err());
    }
}
