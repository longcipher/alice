//! Identity resolution helpers for cross-channel continuity.

use alice_core::runtime_state::domain::{ActiveSessionLease, BindToken};

use crate::context::AliceRuntimeContext;

const CLI_PROVIDER: &str = "cli";
const DEFAULT_CLI_USER_ID: &str = "local";

/// Resolved turn identity used by runtime entrypoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTurnIdentity {
    /// Effective session id that should be used for the turn.
    pub session_id: String,
    /// Profile id used for long-term memory and user modeling.
    pub profile_id: String,
    /// Stable global user id when one is known.
    pub global_user_id: Option<String>,
}

/// Result of attempting to consume a `/bind` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindCommandOutcome {
    /// Bound global user id when the command succeeded.
    pub global_user_id: Option<String>,
    /// User-facing response message that should be posted back to the channel.
    pub message: String,
}

/// Runtime helper for resolving global identity, bind commands, and session reuse.
#[derive(Debug, Clone, Copy)]
pub struct IdentityResolver<'a> {
    context: &'a AliceRuntimeContext,
}

impl<'a> IdentityResolver<'a> {
    /// Create a new resolver for the given runtime context.
    #[must_use]
    pub const fn new(context: &'a AliceRuntimeContext) -> Self {
        Self { context }
    }

    /// Issue a one-time bind token for a global user id.
    pub fn issue_bind_token(
        &self,
        global_user_id: &str,
        provider: Option<&str>,
        ttl_ms: i64,
    ) -> eyre::Result<BindToken> {
        self.context
            .runtime_state_service()
            .issue_bind_token(global_user_id, provider, ttl_ms)
            .map_err(eyre::Error::from)
    }

    /// Resolve identity for a direct CLI turn.
    pub fn resolve_cli_turn(
        &self,
        requested_session_id: &str,
        explicit_global_user_id: Option<&str>,
    ) -> eyre::Result<ResolvedTurnIdentity> {
        let Some(global_user_id) = explicit_global_user_id.map(ToString::to_string) else {
            return Ok(ResolvedTurnIdentity {
                session_id: requested_session_id.to_string(),
                profile_id: requested_session_id.to_string(),
                global_user_id: None,
            });
        };

        let session_id =
            self.preferred_session_for_global_user(&global_user_id, requested_session_id)?;

        Ok(ResolvedTurnIdentity {
            session_id,
            profile_id: global_user_id.clone(),
            global_user_id: Some(global_user_id),
        })
    }

    /// Resolve identity for a provider message event.
    pub fn resolve_message_turn(
        &self,
        provider: &str,
        external_user_id: &str,
        thread_id: &str,
    ) -> eyre::Result<ResolvedTurnIdentity> {
        let channel_session_id = default_channel_session_id(provider, thread_id);
        let global_user_id = self.resolve_global_user_id(provider, external_user_id)?;

        let Some(global_user_id) = global_user_id else {
            return Ok(ResolvedTurnIdentity {
                session_id: channel_session_id,
                profile_id: format!("{provider}-user-{external_user_id}"),
                global_user_id: None,
            });
        };

        let session_id =
            self.preferred_session_for_global_user(&global_user_id, &channel_session_id)?;

        Ok(ResolvedTurnIdentity {
            session_id,
            profile_id: global_user_id.clone(),
            global_user_id: Some(global_user_id),
        })
    }

    /// Persist the latest active session lease when a global user id is known.
    pub fn remember_active_session(
        &self,
        identity: &ResolvedTurnIdentity,
        channel: Option<&str>,
    ) -> eyre::Result<Option<ActiveSessionLease>> {
        self.remember_active_session_with_thread_id(identity, channel, None)
    }

    /// Persist the latest active session lease when a global user id is known.
    pub fn remember_active_session_with_thread_id(
        &self,
        identity: &ResolvedTurnIdentity,
        channel: Option<&str>,
        thread_id: Option<&str>,
    ) -> eyre::Result<Option<ActiveSessionLease>> {
        let Some(global_user_id) = identity.global_user_id.as_deref() else {
            return Ok(None);
        };

        self.context
            .runtime_state_service()
            .upsert_active_session_with_thread_id(
                global_user_id,
                &identity.session_id,
                channel,
                thread_id.or(Some(identity.session_id.as_str())),
            )
            .map(Some)
            .map_err(eyre::Error::from)
    }

    /// Handle a `/bind <token>` command for a channel identity.
    pub fn consume_bind_command(
        &self,
        provider: &str,
        external_user_id: &str,
        input: &str,
    ) -> eyre::Result<Option<BindCommandOutcome>> {
        let Some(token) = parse_bind_command(input) else {
            return Ok(None);
        };

        if token.is_empty() {
            return Ok(Some(BindCommandOutcome {
                global_user_id: None,
                message: "Usage: /bind <token>".to_string(),
            }));
        }

        let maybe_binding = self.context.runtime_state_service().consume_bind_token(
            token,
            provider,
            external_user_id,
        )?;

        let Some(binding) = maybe_binding else {
            return Ok(Some(BindCommandOutcome {
                global_user_id: None,
                message: "Bind token is invalid, expired, or already used.".to_string(),
            }));
        };

        Ok(Some(BindCommandOutcome {
            global_user_id: Some(binding.global_user_id.clone()),
            message: format!(
                "Bound this {provider} account to global user '{}'.",
                binding.global_user_id
            ),
        }))
    }

    fn resolve_global_user_id(
        &self,
        provider: &str,
        external_user_id: &str,
    ) -> eyre::Result<Option<String>> {
        if provider == CLI_PROVIDER && external_user_id != DEFAULT_CLI_USER_ID {
            return Ok(Some(external_user_id.to_string()));
        }

        self.context
            .runtime_state_service()
            .resolve_global_user_id(provider, external_user_id)
            .map_err(eyre::Error::from)
    }

    fn preferred_session_for_global_user(
        &self,
        global_user_id: &str,
        fallback_session_id: &str,
    ) -> eyre::Result<String> {
        Ok(self
            .context
            .runtime_state_service()
            .get_active_session(global_user_id)?
            .map_or_else(|| fallback_session_id.to_string(), |lease| lease.session_id))
    }
}

fn parse_bind_command(input: &str) -> Option<&str> {
    let trimmed = input.trim();
    let remainder = trimmed.strip_prefix("/bind")?;
    Some(remainder.trim())
}

fn default_channel_session_id(provider: &str, thread_id: &str) -> String {
    if provider == CLI_PROVIDER || thread_id.starts_with(provider) {
        thread_id.to_string()
    } else {
        format!("{provider}-{thread_id}")
    }
}

#[cfg(test)]
mod tests {
    use super::{default_channel_session_id, parse_bind_command};

    #[test]
    fn parse_bind_command_extracts_token() {
        assert_eq!(parse_bind_command("/bind abc123"), Some("abc123"));
        assert_eq!(parse_bind_command("/bind    abc123   "), Some("abc123"));
        assert_eq!(parse_bind_command("hello"), None);
    }

    #[test]
    fn default_cli_session_id_uses_thread_directly() {
        assert_eq!(default_channel_session_id("cli", "alice-session"), "alice-session");
    }

    #[test]
    fn default_channel_session_id_avoids_double_prefix() {
        assert_eq!(default_channel_session_id("telegram", "telegram-chat-1"), "telegram-chat-1");
        assert_eq!(default_channel_session_id("discord", "thread-1"), "discord-thread-1");
    }
}
