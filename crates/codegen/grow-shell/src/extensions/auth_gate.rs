use agent_client_protocol as acp;

use crate::auth::{AuthManager, ProviderAuth};

/// Require xAI auth from a sync context, accepting tokens in the client-side buffer window.
pub(crate) fn require_xai_auth(
    auth_manager: &AuthManager,
    missing_message: &'static str,
    third_party_message: &'static str,
) -> Result<ProviderAuth, acp::Error> {
    let auth = auth_manager
        .current_or_expired()
        .ok_or_else(|| acp::Error::auth_required().data(missing_message))?;
    if !auth.is_service_auth() {
        return Err(acp::Error::auth_required().data(third_party_message));
    }
    Ok(auth)
}
