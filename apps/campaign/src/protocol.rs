use loro_protocol::{BatchId, CrdtType, Permission, ProtocolMessage, decode, encode};
use tracing::warn;

pub fn decode_frame(data: &[u8]) -> Result<ProtocolMessage, String> {
    decode(data).map_err(|e| format!("protocol decode error: {e}"))
}

pub fn encode_message(msg: &ProtocolMessage) -> Vec<u8> {
    encode(msg).unwrap_or_else(|e| {
        warn!(%e, "failed to encode protocol message");
        vec![]
    })
}

pub fn join_response_ok(room_id: &str, server_version: Vec<u8>) -> ProtocolMessage {
    ProtocolMessage::JoinResponseOk {
        crdt: CrdtType::Loro,
        room_id: room_id.to_string(),
        permission: Permission::Write,
        version: server_version,
        extra: None,
    }
}

pub fn join_error(room_id: &str, message: &str) -> ProtocolMessage {
    use loro_protocol::JoinErrorCode;
    ProtocolMessage::JoinError {
        crdt: CrdtType::Loro,
        room_id: room_id.to_string(),
        code: JoinErrorCode::AppError,
        message: message.to_string(),
        receiver_version: None,
        app_code: None,
    }
}

fn random_batch_id() -> BatchId {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    BatchId(nanos.to_le_bytes())
}

pub fn doc_update(room_id: &str, updates: Vec<Vec<u8>>) -> ProtocolMessage {
    ProtocolMessage::DocUpdate {
        crdt: CrdtType::Loro,
        room_id: room_id.to_string(),
        updates,
        batch_id: random_batch_id(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextFrame {
    Ping,
    Pong,
    Unknown(String),
}

impl TextFrame {
    pub fn parse(s: &str) -> Self {
        match s {
            "ping" => Self::Ping,
            "pong" => Self::Pong,
            other => Self::Unknown(other.to_string()),
        }
    }

    pub fn response(&self) -> Option<&'static str> {
        match self {
            Self::Ping => Some("pong"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_join_request() {
        let msg = ProtocolMessage::JoinRequest {
            crdt: CrdtType::Loro,
            room_id: "test-room".to_string(),
            auth: vec![],
            version: vec![1, 2, 3],
        };
        let bytes = encode_message(&msg);
        assert!(!bytes.is_empty());
        let decoded = decode_frame(&bytes).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn round_trip_doc_update() {
        let msg = doc_update("room-1", vec![vec![10, 20, 30]]);
        let bytes = encode_message(&msg);
        let decoded = decode_frame(&bytes).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn text_frame_ping_pong() {
        assert_eq!(TextFrame::parse("ping"), TextFrame::Ping);
        assert_eq!(TextFrame::Ping.response(), Some("pong"));
        assert_eq!(TextFrame::parse("pong"), TextFrame::Pong);
        assert_eq!(TextFrame::Pong.response(), None);
    }

    #[test]
    fn text_frame_case_sensitive() {
        assert_eq!(
            TextFrame::parse("PING"),
            TextFrame::Unknown("PING".to_string())
        );
    }
}
