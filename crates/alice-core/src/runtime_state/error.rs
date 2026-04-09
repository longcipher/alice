//! Error types for the runtime-state subsystem.

/// Validation failures for runtime-state inputs.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum RuntimeStateValidationError {
    /// Schedule interval must be positive.
    #[error("schedule interval must be greater than zero minutes")]
    InvalidScheduleMinutes,
    /// Hour must be in 0..=23.
    #[error("schedule hour must be between 0 and 23")]
    InvalidScheduleHour,
    /// Minute must be in 0..=59.
    #[error("schedule minute must be between 0 and 59")]
    InvalidScheduleMinute,
    /// Bind token TTL must be positive.
    #[error("bind token ttl must be greater than zero")]
    InvalidBindTokenTtl,
    /// Prompt text cannot be blank.
    #[error("scheduled task prompt cannot be blank")]
    EmptyTaskPrompt,
}

/// Storage adapter failures.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeStateStoreError {
    /// Database operation failed.
    #[error("database error: {0}")]
    Database(String),
    /// Serialization or deserialization failed.
    #[error("serialization error: {0}")]
    Serialization(String),
    /// Input validation failed.
    #[error(transparent)]
    Validation(#[from] RuntimeStateValidationError),
}

/// Service-level failures.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeStateServiceError {
    /// Validation error.
    #[error(transparent)]
    Validation(#[from] RuntimeStateValidationError),
    /// Storage error.
    #[error(transparent)]
    Store(#[from] RuntimeStateStoreError),
}

impl From<serde_json::Error> for RuntimeStateStoreError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value.to_string())
    }
}
