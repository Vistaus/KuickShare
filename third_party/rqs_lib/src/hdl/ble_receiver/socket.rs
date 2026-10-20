//! BLE socket layer on top of weave messages: `[service_id_hash(3)][data]`.
//!
//! Hash `00 00 00` marks a `SocketControlFrame` protobuf (`08 <type> ...`); the Nearby
//! Sharing hash marks data, a continuous `[4-byte BE len][OfflineFrame]` stream.

/// First 3 bytes of SHA-256("NearbySharing").
pub const SERVICE_HASH: [u8; 3] = [0xfc, 0x9f, 0x5e];
const CONTROL_HASH: [u8; 3] = [0; 3];
const CTRL_INTRODUCTION: u8 = 1;
const CTRL_DISCONNECTION: u8 = 2;
/// Socket version announced in our INTRODUCTION.
#[cfg(test)]
const SOCKET_VERSION: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Control {
    Introduction,
    Disconnection,
    /// Any other control type (e.g. packet acknowledgement); safe to ignore.
    Other(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketMessage {
    Control(Control),
    Data(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketError {
    TooShort,
    UnknownHash,
    BadControl,
}

impl std::fmt::Display for SocketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TooShort => "BLE socket message too short",
            Self::UnknownHash => "BLE socket message with unknown service hash",
            Self::BadControl => "malformed BLE socket control frame",
        })
    }
}

impl std::error::Error for SocketError {}

pub fn parse_message(message: &[u8]) -> Result<SocketMessage, SocketError> {
    let hash = message.get(..3).ok_or(SocketError::TooShort)?;
    let body = &message[3..];
    if hash == SERVICE_HASH {
        return Ok(SocketMessage::Data(body.to_vec()));
    }
    if hash != CONTROL_HASH {
        return Err(SocketError::UnknownHash);
    }
    match body {
        [0x08, kind, ..] => Ok(SocketMessage::Control(match *kind {
            CTRL_INTRODUCTION => Control::Introduction,
            CTRL_DISCONNECTION => Control::Disconnection,
            other => Control::Other(other),
        })),
        _ => Err(SocketError::BadControl),
    }
}

/// Wraps a chunk of the `[len][OfflineFrame]` stream.
pub fn data_message(data: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(3 + data.len());
    m.extend_from_slice(&SERVICE_HASH);
    m.extend_from_slice(data);
    m
}

/// `00 00 00 | 08 01 | 12 07 | 0a 03 <hash> | 10 02`: type INTRODUCTION with
/// {service_id_hash, socket_version}.
#[cfg(test)]
pub fn introduction_message() -> Vec<u8> {
    let mut m = CONTROL_HASH.to_vec();
    m.extend_from_slice(&[0x08, CTRL_INTRODUCTION, 0x12, 0x07, 0x0a, 0x03]);
    m.extend_from_slice(&SERVICE_HASH);
    m.extend_from_slice(&[0x10, SOCKET_VERSION]);
    m
}

#[cfg(test)]
pub fn disconnection_message() -> Vec<u8> {
    let mut m = CONTROL_HASH.to_vec();
    m.extend_from_slice(&[0x08, CTRL_DISCONNECTION]);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_captured_introduction() {
        let phone = [
            0, 0, 0, 0x08, 0x01, 0x12, 0x07, 0x0a, 0x03, 0xfc, 0x9f, 0x5e, 0x10, 0x02,
        ];
        assert_eq!(
            parse_message(&phone),
            Ok(SocketMessage::Control(Control::Introduction))
        );
        assert_eq!(introduction_message(), phone);
    }

    #[test]
    fn disconnection_roundtrips() {
        assert_eq!(disconnection_message(), [0, 0, 0, 0x08, 0x02]);
        assert_eq!(
            parse_message(&disconnection_message()),
            Ok(SocketMessage::Control(Control::Disconnection))
        );
        // Trailing protobuf fields are tolerated.
        assert_eq!(
            parse_message(&[0, 0, 0, 0x08, 0x02, 0x12, 0x00]),
            Ok(SocketMessage::Control(Control::Disconnection))
        );
    }

    #[test]
    fn unknown_control_type_is_other() {
        assert_eq!(
            parse_message(&[0, 0, 0, 0x08, 0x03]),
            Ok(SocketMessage::Control(Control::Other(3)))
        );
    }

    #[test]
    fn data_roundtrips() {
        let frame = [0, 0, 0, 2, 0xaa, 0xbb];
        let m = data_message(&frame);
        assert_eq!(&m[..3], SERVICE_HASH);
        assert_eq!(parse_message(&m), Ok(SocketMessage::Data(frame.to_vec())));
        assert_eq!(
            parse_message(&data_message(&[])),
            Ok(SocketMessage::Data(vec![]))
        );
    }

    #[test]
    fn rejects_malformed_messages() {
        assert_eq!(parse_message(&[]), Err(SocketError::TooShort));
        assert_eq!(parse_message(&[0xfc, 0x9f]), Err(SocketError::TooShort));
        assert_eq!(parse_message(&[1, 2, 3, 4]), Err(SocketError::UnknownHash));
        assert_eq!(parse_message(&[0, 0, 0]), Err(SocketError::BadControl));
        assert_eq!(
            parse_message(&[0, 0, 0, 0x10, 1]),
            Err(SocketError::BadControl)
        );
        assert_eq!(
            parse_message(&[0, 0, 0, 0x08]),
            Err(SocketError::BadControl)
        );
    }
}
