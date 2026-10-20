//! Weave transport: packet header, fragmentation and reassembly.
//!
//! Header byte: `C ccc F L mm`, bit 7 control, bits 6-4 packet counter, bit 3 first,
//! bit 2 last. Control packets carry a 4-bit command in the low nibble instead.

use std::fmt;

pub const MIN_PACKET_SIZE: usize = 20;
pub const MAX_PACKET_SIZE: usize = 509;
/// Upper bound for one reassembled message.
pub const MAX_MESSAGE_SIZE: usize = 4 << 20;
pub const PROTOCOL_VERSION: u16 = 1;

const CONTROL: u8 = 0x80;
const COUNTER_MASK: u8 = 0x70;
const FIRST: u8 = 0x08;
const LAST: u8 = 0x04;
const CMD_MASK: u8 = 0x0f;
const CMD_REQUEST: u8 = 0;
const CMD_CONFIRM: u8 = 1;
const CMD_ERROR: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WeaveError {
    EmptyPacket,
    UnexpectedControl,
    BadCounter { expected: u8, got: u8 },
    UnexpectedFirst,
    MissingFirst,
    MessageTooLarge,
    BadConnRequest,
    UnsupportedVersion,
    PacketSizeTooSmall,
    PeerError,
}

impl fmt::Display for WeaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPacket => write!(f, "empty weave packet"),
            Self::UnexpectedControl => write!(f, "unexpected weave control packet"),
            Self::BadCounter { expected, got } => {
                write!(f, "weave packet counter {got}, expected {expected}")
            }
            Self::UnexpectedFirst => write!(f, "weave first packet inside a message"),
            Self::MissingFirst => write!(f, "weave continuation without a first packet"),
            Self::MessageTooLarge => write!(f, "weave message too large"),
            Self::BadConnRequest => write!(f, "malformed weave connection request"),
            Self::UnsupportedVersion => write!(f, "no common weave protocol version"),
            Self::PacketSizeTooSmall => write!(f, "weave packet size below the minimum"),
            Self::PeerError => write!(f, "weave peer sent an error packet"),
        }
    }
}

impl std::error::Error for WeaveError {}

/// Kind of a control packet, as far as the receiver cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Request,
    Confirm,
    Error,
    Other(u8),
}

/// Returns the control command if `packet` is a control packet.
pub fn control_kind(packet: &[u8]) -> Option<Control> {
    let h = *packet.first()?;
    if h & CONTROL == 0 {
        return None;
    }
    Some(match h & CMD_MASK {
        CMD_REQUEST => Control::Request,
        CMD_CONFIRM => Control::Confirm,
        CMD_ERROR => Control::Error,
        other => Control::Other(other),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnRequest {
    pub min_version: u16,
    pub max_version: u16,
    pub max_packet_size: u16,
}

/// Parses `80 min(2) max(2) max_packet(2)`.
pub fn parse_conn_request(packet: &[u8]) -> Result<ConnRequest, WeaveError> {
    if control_kind(packet) != Some(Control::Request) || packet.len() < 7 {
        return Err(WeaveError::BadConnRequest);
    }
    let u16_at = |i: usize| u16::from_be_bytes([packet[i], packet[i + 1]]);
    Ok(ConnRequest {
        min_version: u16_at(1),
        max_version: u16_at(3),
        max_packet_size: u16_at(5),
    })
}

/// Picks our protocol version and the packet size (the phone's maximum, capped at 509).
/// A maximum below 20 is refused rather than raised: a packet larger than the phone allows
/// would be broken, and a tiny one could not hold a header and payload.
pub fn negotiate(req: &ConnRequest) -> Result<usize, WeaveError> {
    if !(req.min_version..=req.max_version).contains(&PROTOCOL_VERSION) {
        return Err(WeaveError::UnsupportedVersion);
    }
    let size = usize::from(req.max_packet_size);
    if size < MIN_PACKET_SIZE {
        return Err(WeaveError::PacketSizeTooSmall);
    }
    Ok(size.min(MAX_PACKET_SIZE))
}

/// `81 version(2) packet_size(2)`; its counter is 0, so data packets start at 1.
pub fn conn_confirm(packet_size: usize) -> Vec<u8> {
    let mut p = vec![CONTROL | CMD_CONFIRM];
    p.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    p.extend_from_slice(&(packet_size as u16).to_be_bytes());
    p
}

/// Splits messages into data packets with a rolling 3-bit counter.
#[derive(Debug, Clone)]
pub struct Packetizer {
    counter: u8,
}

impl Default for Packetizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Packetizer {
    /// The first data packet carries counter 1 (the connection confirm used 0).
    pub fn new() -> Self {
        Self { counter: 1 }
    }

    /// An empty message yields a single header-only packet.
    pub fn packetize(&mut self, message: &[u8], max_packet_size: usize) -> Vec<Vec<u8>> {
        let payload = max_packet_size.clamp(MIN_PACKET_SIZE, MAX_PACKET_SIZE) - 1;
        let chunks: Vec<&[u8]> = if message.is_empty() {
            vec![message]
        } else {
            message.chunks(payload).collect()
        };
        let n = chunks.len();
        chunks
            .into_iter()
            .enumerate()
            .map(|(i, chunk)| {
                let mut h = self.counter << 4;
                if i == 0 {
                    h |= FIRST;
                }
                if i + 1 == n {
                    h |= LAST;
                }
                self.counter = (self.counter + 1) & 0x07;
                let mut p = Vec::with_capacity(1 + chunk.len());
                p.push(h);
                p.extend_from_slice(chunk);
                p
            })
            .collect()
    }
}

/// Reassembles data packets into messages, enforcing the counter sequence.
#[derive(Debug, Clone)]
pub struct Reassembler {
    expected: u8,
    buf: Vec<u8>,
    in_message: bool,
}

impl Default for Reassembler {
    fn default() -> Self {
        Self::new()
    }
}

impl Reassembler {
    /// The phone's connection request used counter 0, so data starts at 1.
    pub fn new() -> Self {
        Self {
            expected: 1,
            buf: Vec::new(),
            in_message: false,
        }
    }

    /// Returns the full message once its last packet arrives. Any error is fatal for the
    /// session: the state is not recoverable.
    pub fn feed(&mut self, packet: &[u8]) -> Result<Option<Vec<u8>>, WeaveError> {
        let (&h, payload) = packet.split_first().ok_or(WeaveError::EmptyPacket)?;
        if h & CONTROL != 0 {
            return Err(WeaveError::UnexpectedControl);
        }
        let got = (h & COUNTER_MASK) >> 4;
        if got != self.expected {
            return Err(WeaveError::BadCounter {
                expected: self.expected,
                got,
            });
        }
        let first = h & FIRST != 0;
        if first && self.in_message {
            return Err(WeaveError::UnexpectedFirst);
        }
        if !first && !self.in_message {
            return Err(WeaveError::MissingFirst);
        }
        if self.buf.len() + payload.len() > MAX_MESSAGE_SIZE {
            return Err(WeaveError::MessageTooLarge);
        }
        self.expected = (self.expected + 1) & 0x07;
        self.buf.extend_from_slice(payload);
        if h & LAST != 0 {
            self.in_message = false;
            return Ok(Some(std::mem::take(&mut self.buf)));
        }
        self.in_message = true;
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(msg: &[u8], size: usize) -> Vec<Vec<u8>> {
        let packets = Packetizer::new().packetize(msg, size);
        let mut r = Reassembler::new();
        let mut out = None;
        for (i, p) in packets.iter().enumerate() {
            assert!(p.len() <= size);
            let res = r.feed(p).unwrap();
            if i + 1 == packets.len() {
                out = res;
            } else {
                assert!(res.is_none());
            }
        }
        assert_eq!(out.as_deref(), Some(msg));
        packets
    }

    #[test]
    fn parses_the_captured_conn_request() {
        let req = parse_conn_request(&[0x80, 0, 1, 0, 1, 0x01, 0xfd]).unwrap();
        assert_eq!(
            (req.min_version, req.max_version, req.max_packet_size),
            (1, 1, 509)
        );
        assert_eq!(negotiate(&req), Ok(509));
    }

    #[test]
    fn builds_the_captured_conn_confirm() {
        assert_eq!(conn_confirm(509), [0x81, 0, 1, 0x01, 0xfd]);
    }

    #[test]
    fn negotiation_clamps_and_checks_version() {
        let mut req = ConnRequest {
            min_version: 1,
            max_version: 1,
            max_packet_size: 5,
        };
        assert_eq!(negotiate(&req), Err(WeaveError::PacketSizeTooSmall));
        req.max_packet_size = 0;
        assert_eq!(negotiate(&req), Err(WeaveError::PacketSizeTooSmall));
        req.max_packet_size = 20;
        assert_eq!(negotiate(&req), Ok(20));
        req.max_packet_size = 4000;
        assert_eq!(negotiate(&req), Ok(509));
        req.min_version = 2;
        req.max_version = 3;
        assert_eq!(negotiate(&req), Err(WeaveError::UnsupportedVersion));
    }

    #[test]
    fn rejects_bad_conn_requests() {
        assert!(parse_conn_request(&[]).is_err());
        assert!(parse_conn_request(&[0x80, 0, 1, 0, 1, 1]).is_err());
        assert!(parse_conn_request(&[0x81, 0, 1, 0, 1, 1, 0xfd]).is_err());
        assert!(parse_conn_request(&[0x00, 0, 1, 0, 1, 1, 0xfd]).is_err());
    }

    #[test]
    fn small_message_is_one_packet() {
        let p = roundtrip(b"hello", 20);
        assert_eq!(p, [[0x1c, b'h', b'e', b'l', b'l', b'o']]);
    }

    #[test]
    fn fragments_at_20_bytes() {
        let msg: Vec<u8> = (0..100).collect();
        let packets = roundtrip(&msg, 20);
        assert_eq!(packets.len(), 6); // 19 bytes per packet
        assert_eq!(packets[0][0], 0x18); // counter 1, first
        assert_eq!(packets[1][0], 0x20);
        assert_eq!(packets[5][0], 0x64); // counter 6, last
        assert_eq!(packets[5].len(), 1 + 100 - 5 * 19);
    }

    #[test]
    fn fragments_at_509_bytes() {
        let msg = vec![0xab; 508 * 3];
        let packets = roundtrip(&msg, 509);
        assert_eq!(packets.len(), 3);
        assert!(packets.iter().all(|p| p.len() == 509));
        let msg = vec![0xab; 508 * 3 + 1];
        assert_eq!(roundtrip(&msg, 509).len(), 4);
    }

    #[test]
    fn counter_wraps_across_messages() {
        let mut tx = Packetizer::new();
        let mut rx = Reassembler::new();
        for i in 0..40u8 {
            for p in tx.packetize(&[i; 50], 20) {
                if let Some(m) = rx.feed(&p).unwrap() {
                    assert_eq!(m, [i; 50]);
                }
            }
        }
        let counters: Vec<u8> = Packetizer::new()
            .packetize(&[0; 19 * 9], 20)
            .iter()
            .map(|p| (p[0] & COUNTER_MASK) >> 4)
            .collect();
        assert_eq!(counters, [1, 2, 3, 4, 5, 6, 7, 0, 1]);
    }

    #[test]
    fn empty_message_roundtrips() {
        let p = roundtrip(&[], 20);
        assert_eq!(p, [[0x1c]]);
    }

    #[test]
    fn oversized_size_argument_is_clamped() {
        let packets = Packetizer::new().packetize(&[0; 2000], 100_000);
        assert!(packets.iter().all(|p| p.len() <= MAX_PACKET_SIZE));
        let packets = Packetizer::new().packetize(&[0; 40], 1);
        assert!(packets.iter().all(|p| p.len() <= MIN_PACKET_SIZE));
    }

    #[test]
    fn rejects_out_of_order_and_garbage() {
        let packets = Packetizer::new().packetize(&[1; 60], 20);
        // Skipping a packet breaks the counter.
        let mut r = Reassembler::new();
        r.feed(&packets[0]).unwrap();
        assert_eq!(
            r.feed(&packets[2]),
            Err(WeaveError::BadCounter {
                expected: 2,
                got: 3
            })
        );
        // Replay of the first packet.
        let mut r = Reassembler::new();
        r.feed(&packets[0]).unwrap();
        assert!(matches!(
            r.feed(&packets[0]),
            Err(WeaveError::BadCounter { .. })
        ));
        // Continuation with no first packet (counter right, flags wrong).
        assert_eq!(
            Reassembler::new().feed(&[0x10, 1]),
            Err(WeaveError::MissingFirst)
        );
        // Second first inside a message.
        let mut r = Reassembler::new();
        r.feed(&[0x18, 1]).unwrap();
        assert_eq!(r.feed(&[0x28, 1]), Err(WeaveError::UnexpectedFirst));
        // Empty and control packets.
        assert_eq!(Reassembler::new().feed(&[]), Err(WeaveError::EmptyPacket));
        assert_eq!(
            Reassembler::new().feed(&[0x80]),
            Err(WeaveError::UnexpectedControl)
        );
    }

    #[test]
    fn rejects_oversized_messages() {
        let mut r = Reassembler::new();
        let mut tx = Packetizer::new();
        let big = vec![0u8; MAX_MESSAGE_SIZE + 1];
        let res: Result<Vec<_>, _> = tx
            .packetize(&big, MAX_PACKET_SIZE)
            .iter()
            .map(|p| r.feed(p))
            .collect();
        assert_eq!(res, Err(WeaveError::MessageTooLarge));
    }

    #[test]
    fn classifies_control_packets() {
        assert_eq!(control_kind(&[0x80]), Some(Control::Request));
        assert_eq!(control_kind(&[0x81]), Some(Control::Confirm));
        assert_eq!(control_kind(&[0x82]), Some(Control::Error));
        assert_eq!(control_kind(&[0x85]), Some(Control::Other(5)));
        assert_eq!(control_kind(&[0x1c]), None);
        assert_eq!(control_kind(&[]), None);
    }
}
