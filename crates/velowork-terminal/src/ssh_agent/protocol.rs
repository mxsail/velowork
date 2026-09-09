//! SSH Agent wire protocol encoding and decoding.
//!
//! Compliant with OpenSSH Agent Protocol and RFC specifications:
//! - Frame structure: `[length: u32][type: u8][payload...]`
//! - Identifies query: `SSH2_AGENTC_REQUEST_IDENTITIES` (11) -> `SSH2_AGENT_IDENTITIES_ANSWER` (12)
//! - Sign request: `SSH2_AGENTC_SIGN_REQUEST` (13) -> `SSH2_AGENT_SIGN_RESPONSE` (14)

use std::fmt;

pub const SSH_AGENT_FAILURE: u8 = 5;
pub const SSH_AGENT_SUCCESS: u8 = 6;
pub const SSH2_AGENTC_REQUEST_IDENTITIES: u8 = 11;
pub const SSH2_AGENT_IDENTITIES_ANSWER: u8 = 12;
pub const SSH2_AGENTC_SIGN_REQUEST: u8 = 13;
pub const SSH2_AGENT_SIGN_RESPONSE: u8 = 14;

// Sign flags
pub const SSH_AGENT_RSA_SHA2_256: u32 = 2;
pub const SSH_AGENT_RSA_SHA2_512: u32 = 4;

/// A public key identity stored in the SSH Agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentKey {
    /// Raw public key blob (as defined in SSH RFC 4253 / OpenSSH wire format).
    pub blob: Vec<u8>,
    /// Comment associated with the key (e.g. email or username@host).
    pub comment: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentProtocolError {
    MessageTooShort,
    UnexpectedMessageType { expected: u8, actual: u8 },
    AgentFailure,
    InvalidEncoding(String),
}

impl fmt::Display for AgentProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MessageTooShort => write!(f, "SSH agent message is too short"),
            Self::UnexpectedMessageType { expected, actual } => {
                write!(f, "Unexpected agent message type (expected {}, got {})", expected, actual)
            }
            Self::AgentFailure => write!(f, "SSH agent returned failure"),
            Self::InvalidEncoding(msg) => write!(f, "SSH agent invalid encoding: {}", msg),
        }
    }
}

impl std::error::Error for AgentProtocolError {}

/// Encode a frame with `[4-byte length][payload]`.
pub fn frame_message(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// Encode `SSH2_AGENTC_REQUEST_IDENTITIES` message.
pub fn encode_request_identities() -> Vec<u8> {
    frame_message(&[SSH2_AGENTC_REQUEST_IDENTITIES])
}

/// Decode `SSH2_AGENT_IDENTITIES_ANSWER` payload (excluding the 4-byte length prefix).
pub fn decode_identities_answer(data: &[u8]) -> Result<Vec<AgentKey>, AgentProtocolError> {
    if data.is_empty() {
        return Err(AgentProtocolError::MessageTooShort);
    }
    let msg_type = data[0];
    if msg_type == SSH_AGENT_FAILURE {
        return Err(AgentProtocolError::AgentFailure);
    }
    if msg_type != SSH2_AGENT_IDENTITIES_ANSWER {
        return Err(AgentProtocolError::UnexpectedMessageType {
            expected: SSH2_AGENT_IDENTITIES_ANSWER,
            actual: msg_type,
        });
    }

    let mut cursor = 1;
    if data.len() < cursor + 4 {
        return Err(AgentProtocolError::MessageTooShort);
    }
    let num_keys = u32::from_be_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
    cursor += 4;

    let mut keys = Vec::with_capacity(num_keys);
    for _ in 0..num_keys {
        // Read key blob string
        if data.len() < cursor + 4 {
            return Err(AgentProtocolError::MessageTooShort);
        }
        let blob_len = u32::from_be_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;
        if data.len() < cursor + blob_len {
            return Err(AgentProtocolError::MessageTooShort);
        }
        let blob = data[cursor..cursor + blob_len].to_vec();
        cursor += blob_len;

        // Read comment string
        if data.len() < cursor + 4 {
            return Err(AgentProtocolError::MessageTooShort);
        }
        let comment_len = u32::from_be_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;
        if data.len() < cursor + comment_len {
            return Err(AgentProtocolError::MessageTooShort);
        }
        let comment = String::from_utf8_lossy(&data[cursor..cursor + comment_len]).to_string();
        cursor += comment_len;

        keys.push(AgentKey { blob, comment });
    }

    Ok(keys)
}

/// Encode `SSH2_AGENTC_SIGN_REQUEST` message.
pub fn encode_sign_request(key_blob: &[u8], data_to_sign: &[u8], flags: u32) -> Vec<u8> {
    let mut payload = Vec::with_capacity(1 + 4 + key_blob.len() + 4 + data_to_sign.len() + 4);
    payload.push(SSH2_AGENTC_SIGN_REQUEST);

    // key blob
    payload.extend_from_slice(&(key_blob.len() as u32).to_be_bytes());
    payload.extend_from_slice(key_blob);

    // data
    payload.extend_from_slice(&(data_to_sign.len() as u32).to_be_bytes());
    payload.extend_from_slice(data_to_sign);

    // flags
    payload.extend_from_slice(&flags.to_be_bytes());

    frame_message(&payload)
}

/// Decode `SSH2_AGENT_SIGN_RESPONSE` payload (excluding the 4-byte length prefix).
/// Returns the full signature blob (as required by SSH2 publickey auth).
pub fn decode_sign_response(data: &[u8]) -> Result<Vec<u8>, AgentProtocolError> {
    if data.is_empty() {
        return Err(AgentProtocolError::MessageTooShort);
    }
    let msg_type = data[0];
    if msg_type == SSH_AGENT_FAILURE {
        return Err(AgentProtocolError::AgentFailure);
    }
    if msg_type != SSH2_AGENT_SIGN_RESPONSE {
        return Err(AgentProtocolError::UnexpectedMessageType {
            expected: SSH2_AGENT_SIGN_RESPONSE,
            actual: msg_type,
        });
    }

    let mut cursor = 1;
    if data.len() < cursor + 4 {
        return Err(AgentProtocolError::MessageTooShort);
    }
    let sig_len = u32::from_be_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
    cursor += 4;

    if data.len() < cursor + sig_len {
        return Err(AgentProtocolError::MessageTooShort);
    }

    Ok(data[cursor..cursor + sig_len].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_request_identities() {
        let msg = encode_request_identities();
        assert_eq!(msg, vec![0, 0, 0, 1, 11]);
    }

    #[test]
    fn test_decode_identities_answer() {
        // identities answer with 1 key
        let mut body = vec![SSH2_AGENT_IDENTITIES_ANSWER];
        body.extend_from_slice(&1u32.to_be_bytes()); // 1 key
        let blob = b"test-key-blob";
        body.extend_from_slice(&(blob.len() as u32).to_be_bytes());
        body.extend_from_slice(blob);
        let comment = b"user@example.com";
        body.extend_from_slice(&(comment.len() as u32).to_be_bytes());
        body.extend_from_slice(comment);

        let keys = decode_identities_answer(&body).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].blob, blob);
        assert_eq!(keys[0].comment, "user@example.com");
    }

    #[test]
    fn test_encode_decode_sign() {
        let key_blob = b"sample-key";
        let data = b"sample-data";
        let req = encode_sign_request(key_blob, data, SSH_AGENT_RSA_SHA2_256);
        assert_eq!(req[4], SSH2_AGENTC_SIGN_REQUEST);

        let sig = b"sample-signature";
        let mut resp = vec![SSH2_AGENT_SIGN_RESPONSE];
        resp.extend_from_slice(&(sig.len() as u32).to_be_bytes());
        resp.extend_from_slice(sig);

        let decoded = decode_sign_response(&resp).unwrap();
        assert_eq!(decoded, sig);
    }
}
