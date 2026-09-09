pub mod client;
pub mod forwarding;
pub mod protocol;

pub use client::{AgentClient, AgentClientError, AgentStream, DEFAULT_WIN_OPENSSH_PIPE};
pub use forwarding::{
    handle_agent_forwarding_channel, AGENT_FORWARDING_CHANNEL_TYPE, AGENT_FORWARDING_REQ_TYPE,
};
pub use protocol::{AgentKey, AgentProtocolError};

use russh::client::Handler;
use russh::keys::agent::AgentIdentity;
use russh::keys::HashAlg;
use ssh_encoding::Encode;

impl russh::Signer for AgentClient {
    type Error = AgentClientError;

    fn auth_sign(
        &mut self,
        key: &AgentIdentity,
        hash_alg: Option<HashAlg>,
        to_sign: Vec<u8>,
    ) -> impl std::future::Future<Output = Result<Vec<u8>, Self::Error>> + Send {
        async move {
            let pk = key.public_key();
            let mut key_bytes = Vec::new();
            pk.key_data()
                .encode(&mut key_bytes)
                .map_err(|e| AgentClientError::Protocol(protocol::AgentProtocolError::InvalidEncoding(e.to_string())))?;

            let flags = match hash_alg {
                Some(HashAlg::Sha512) => protocol::SSH_AGENT_RSA_SHA2_512,
                Some(HashAlg::Sha256) => protocol::SSH_AGENT_RSA_SHA2_256,
                _ => 0,
            };

            let sig = self.sign_request(&key_bytes, &to_sign, flags).await?;
            Ok(sig)
        }
    }
}

/// Authenticate against an SSH server using keys loaded in the local SSH Agent.
///
/// Iterates through all public keys returned by the agent and attempts publickey authentication.
/// For RSA keys, respects negotiated server-sig-algs (e.g. `rsa-sha2-512` / `rsa-sha2-256`).
pub async fn authenticate_with_agent<H: Handler>(
    session: &mut russh::client::Handle<H>,
    username: &str,
    socket_path: Option<&str>,
) -> Result<bool, AgentClientError> {
    let mut client = AgentClient::connect(socket_path).await?;
    let keys = client.request_identities().await?;

    if keys.is_empty() {
        return Ok(false);
    }

    let rsa_hash = match session.best_supported_rsa_hash().await {
        Ok(Some(Some(h))) => Some(h),
        Ok(Some(None)) => None,
        _ => Some(HashAlg::Sha512),
    };

    for key in keys {
        let public_key = match ssh_key::PublicKey::from_bytes(&key.blob) {
            Ok(pk) => pk,
            Err(e) => {
                log::debug!("[terminal:ssh_agent] Skipping invalid SSH agent key ({}) | error: {:#}", key.comment, e);
                continue;
            }
        };

        let hash_alg = if matches!(public_key.algorithm(), ssh_key::Algorithm::Rsa { .. }) {
            rsa_hash
        } else {
            None
        };

        match session
            .authenticate_publickey_with(username, public_key, hash_alg, &mut client)
            .await
        {
            Ok(auth) if auth.success() => {
                log::info!("[terminal:ssh_agent] Successfully authenticated via SSH Agent key | comment={}", key.comment);
                return Ok(true);
            }
            Ok(_) => {
                log::debug!("[terminal:ssh_agent] SSH Agent key rejected by server | comment={}", key.comment);
            }
            Err(e) => {
                log::debug!("[terminal:ssh_agent] SSH Agent publickey attempt error | error: {:#}", e);
            }
        }
    }

    Ok(false)
}
