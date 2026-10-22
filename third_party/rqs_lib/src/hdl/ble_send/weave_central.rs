//! Central side of the weave handshake and framing (we send the connection request).
//!
//! The counterpart of `ble_receiver::session::WeaveSession`: transport-agnostic, no I/O.
//! Packets from FromPeripheral go into [`WeaveCentral::feed`]; packets for ToPeripheral
//! come out of `request`, `queue_data`, `introduction` and `goodbye`.

use crate::hdl::ble_receiver::socket::{self, Control, SocketMessage};
use crate::hdl::ble_receiver::weave::{
    self, Control as WeaveControl, MAX_PACKET_SIZE, MIN_PACKET_SIZE, Packetizer, Reassembler,
    WeaveError,
};

/// `80 | min_version(2) | max_version(2) | max_packet(2)`.
const CONN_REQUEST: u8 = 0x80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Weave handshake done; `packet_size` is the confirmed maximum packet size.
    Connected { packet_size: usize },
    /// Socket INTRODUCTION of the peripheral (informational; it may never come).
    Introduction,
    /// Chunk of the `[len][OfflineFrame]` stream.
    Data(Vec<u8>),
    /// The peripheral closed the socket; the session is over.
    Disconnected,
    /// Protocol failure; the session is over.
    Failed(String),
}

#[derive(Debug)]
enum State {
    /// The request is out (or about to be), the confirm is not in yet.
    Requested,
    Connected {
        packet_size: usize,
    },
    Closed,
}

#[derive(Debug)]
pub struct WeaveCentral {
    state: State,
    reassembler: Reassembler,
    packetizer: Packetizer,
    /// The packet size we ask for; the peripheral may confirm a smaller one.
    max_packet: usize,
}

impl WeaveCentral {
    /// `max_packet` is what one write can carry; it is clamped to the weave limits.
    pub fn new(max_packet: usize) -> Self {
        Self {
            state: State::Requested,
            reassembler: Reassembler::new(),
            packetizer: Packetizer::new(),
            max_packet: max_packet.clamp(MIN_PACKET_SIZE, MAX_PACKET_SIZE),
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self.state, State::Connected { .. })
    }

    /// The CONNECTION_REQUEST packet (counter 0, so data packets start at 1).
    pub fn request(&self) -> Vec<u8> {
        let version = weave::PROTOCOL_VERSION.to_be_bytes();
        let mut p = vec![CONN_REQUEST];
        p.extend_from_slice(&version);
        p.extend_from_slice(&version);
        p.extend_from_slice(&(self.max_packet as u16).to_be_bytes());
        p
    }

    /// Handles one packet indicated on FromPeripheral.
    pub fn feed(&mut self, packet: &[u8]) -> Vec<Event> {
        match self.state {
            State::Closed => Vec::new(),
            State::Requested => self.feed_requested(packet),
            State::Connected { .. } => self.feed_connected(packet),
        }
    }

    /// Our socket INTRODUCTION as packets. Empty before the handshake or after close.
    pub fn introduction(&mut self) -> Vec<Vec<u8>> {
        self.queue_message(&socket::introduction_message())
    }

    /// Our socket DISCONNECTION as packets, once: the session is closed afterwards. Empty
    /// unless connected.
    pub fn goodbye(&mut self) -> Vec<Vec<u8>> {
        let packets = self.queue_message(&socket::disconnection_message());
        self.state = State::Closed;
        packets
    }

    /// Fragments a complete socket message (hash included) into packets to write.
    /// Empty before the handshake or after close.
    pub fn queue_message(&mut self, message: &[u8]) -> Vec<Vec<u8>> {
        match self.state {
            State::Connected { packet_size } => self.packetizer.packetize(message, packet_size),
            _ => Vec::new(),
        }
    }

    /// Like [`queue_message`](Self::queue_message) for a chunk of the frame stream.
    pub fn queue_data(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        self.queue_message(&socket::data_message(data))
    }

    fn fail(&mut self, reason: String) -> Vec<Event> {
        self.state = State::Closed;
        vec![Event::Failed(reason)]
    }

    /// `81 | version(2) | packet_size(2)`.
    fn parse_confirm(&self, packet: &[u8]) -> Result<usize, WeaveError> {
        let &[_, v0, v1, s0, s1, ..] = packet else {
            return Err(WeaveError::BadConnRequest);
        };
        if u16::from_be_bytes([v0, v1]) != weave::PROTOCOL_VERSION {
            return Err(WeaveError::UnsupportedVersion);
        }
        let size = usize::from(u16::from_be_bytes([s0, s1]));
        if size < MIN_PACKET_SIZE {
            return Err(WeaveError::PacketSizeTooSmall);
        }
        // A packet larger than we offered could exceed the ATT payload.
        if size > self.max_packet {
            return Err(WeaveError::BadConnRequest);
        }
        Ok(size)
    }

    fn feed_requested(&mut self, packet: &[u8]) -> Vec<Event> {
        match weave::control_kind(packet) {
            Some(WeaveControl::Confirm) => match self.parse_confirm(packet) {
                Ok(packet_size) => {
                    self.state = State::Connected { packet_size };
                    vec![Event::Connected { packet_size }]
                }
                Err(e) => self.fail(e.to_string()),
            },
            Some(WeaveControl::Error) => self.fail(WeaveError::PeerError.to_string()),
            _ => self.fail("weave packet before the connection confirm".into()),
        }
    }

    fn feed_connected(&mut self, packet: &[u8]) -> Vec<Event> {
        if let Some(kind) = weave::control_kind(packet) {
            return match kind {
                WeaveControl::Error => self.fail(WeaveError::PeerError.to_string()),
                _ => Vec::new(),
            };
        }
        let message = match self.reassembler.feed(packet) {
            Ok(Some(m)) => m,
            Ok(None) => return Vec::new(),
            Err(e) => return self.fail(e.to_string()),
        };
        match socket::parse_message(&message) {
            Ok(SocketMessage::Data(d)) => vec![Event::Data(d)],
            Ok(SocketMessage::Control(Control::Introduction)) => vec![Event::Introduction],
            Ok(SocketMessage::Control(Control::Disconnection)) => {
                self.state = State::Closed;
                vec![Event::Disconnected]
            }
            Ok(SocketMessage::Control(Control::Other(_))) => Vec::new(),
            Err(e) => self.fail(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connected(max_packet: usize) -> WeaveCentral {
        let mut c = WeaveCentral::new(max_packet);
        assert_eq!(
            c.feed(&weave::conn_confirm(max_packet)),
            [Event::Connected {
                packet_size: max_packet
            }]
        );
        c
    }

    #[test]
    fn request_bytes_match_the_captured_one() {
        assert_eq!(
            WeaveCentral::new(509).request(),
            [0x80, 0, 1, 0, 1, 0x01, 0xfd]
        );
        assert_eq!(WeaveCentral::new(20).request(), [0x80, 0, 1, 0, 1, 0, 20]);
        // Out-of-range sizes are clamped.
        assert_eq!(WeaveCentral::new(5).request()[5..], [0, 20]);
        assert_eq!(WeaveCentral::new(4000).request()[5..], [0x01, 0xfd]);
    }

    #[test]
    fn confirm_connects_and_a_smaller_size_is_honoured() {
        let mut c = WeaveCentral::new(509);
        assert!(!c.is_connected());
        assert!(c.queue_data(b"x").is_empty() && c.introduction().is_empty());
        assert_eq!(
            c.feed(&[0x81, 0, 1, 0, 100]),
            [Event::Connected { packet_size: 100 }]
        );
        assert!(c.is_connected());
        let packets = c.queue_message(&[0xee; 500]);
        assert!(packets.len() > 4);
        assert!(packets.iter().all(|p| p.len() <= 100));
    }

    #[test]
    fn bad_confirms_fail_the_session() {
        for confirm in [
            &[0x81, 0, 2, 0x01, 0xfd][..], // version
            &[0x81, 0, 1, 0, 19],          // below the minimum
            &[0x81, 0, 1, 0x01, 0xfe],     // above what we asked
            &[0x81, 0, 1, 0x01],           // truncated
        ] {
            let mut c = WeaveCentral::new(509);
            assert!(
                matches!(c.feed(confirm).as_slice(), [Event::Failed(_)]),
                "{confirm:02x?}"
            );
            assert!(!c.is_connected());
            assert!(c.feed(&weave::conn_confirm(509)).is_empty());
        }
        // Above the size this central asked for.
        let mut c = WeaveCentral::new(100);
        assert!(matches!(
            c.feed(&weave::conn_confirm(101)).as_slice(),
            [Event::Failed(_)]
        ));
    }

    #[test]
    fn error_packet_or_data_before_the_confirm_fails() {
        let mut c = WeaveCentral::new(509);
        assert!(matches!(c.feed(&[0x82]).as_slice(), [Event::Failed(_)]));
        let mut c = WeaveCentral::new(509);
        assert!(matches!(
            c.feed(&[0x1c, 1, 2]).as_slice(),
            [Event::Failed(_)]
        ));
        assert!(c.queue_data(b"x").is_empty());
    }

    #[test]
    fn peer_error_after_the_confirm_fails() {
        let mut c = connected(509);
        assert!(matches!(c.feed(&[0x82]).as_slice(), [Event::Failed(_)]));
        assert!(!c.is_connected());
    }

    #[test]
    fn data_to_the_peripheral_is_fragmented_and_reassembles() {
        let mut c = connected(100);
        let data: Vec<u8> = (0..2000u32).map(|i| (i * 7) as u8).collect();
        let packets = c.queue_data(&data);
        assert!(packets.len() > 20);
        assert!(packets.iter().all(|p| p.len() <= 100));
        // Counters start at 1: the request was counter 0.
        assert_eq!((packets[0][0] >> 4) & 7, 1);

        let mut rx = Reassembler::new();
        let msg = packets.iter().find_map(|p| rx.feed(p).unwrap()).unwrap();
        assert_eq!(socket::parse_message(&msg), Ok(SocketMessage::Data(data)));
    }

    #[test]
    fn introduction_precedes_data_in_the_counter_sequence() {
        let mut c = connected(509);
        let mut rx = Reassembler::new();
        let mut messages = Vec::new();
        for p in c.introduction().iter().chain(&c.queue_data(b"abc")) {
            messages.extend(rx.feed(p).unwrap());
        }
        assert_eq!(
            messages,
            [socket::introduction_message(), socket::data_message(b"abc")]
        );
    }

    #[test]
    fn data_from_the_peripheral_is_reassembled() {
        let mut c = connected(509);
        let mut phone = Packetizer::new();
        let stream: Vec<u8> = (0..3000u32).map(|i| i as u8).collect();
        let mut events = Vec::new();
        // The peripheral's own INTRODUCTION is optional but accepted.
        for message in [
            socket::introduction_message(),
            socket::data_message(&stream),
        ] {
            for p in phone.packetize(&message, 509) {
                events.extend(c.feed(&p));
            }
        }
        assert_eq!(events, [Event::Introduction, Event::Data(stream)]);
    }

    #[test]
    fn peripheral_disconnection_closes_the_session() {
        let mut c = connected(509);
        let mut phone = Packetizer::new();
        let mut events = Vec::new();
        for p in phone.packetize(&socket::disconnection_message(), 509) {
            events.extend(c.feed(&p));
        }
        assert_eq!(events, [Event::Disconnected]);
        assert!(c.feed(&[0x1c]).is_empty());
        assert!(c.queue_data(b"x").is_empty());
        assert!(c.goodbye().is_empty());
    }

    #[test]
    fn goodbye_is_one_disconnection_message_and_only_once() {
        let mut c = connected(509);
        let packets = c.goodbye();
        let msg = Reassembler::new().feed(&packets[0]).unwrap().unwrap();
        assert_eq!(msg, socket::disconnection_message());
        assert!(c.goodbye().is_empty());
        assert!(c.queue_data(b"x").is_empty());

        // Nothing to say before the handshake.
        assert!(WeaveCentral::new(509).goodbye().is_empty());
    }

    #[test]
    fn bad_counter_and_unknown_hash_fail() {
        let mut c = connected(509);
        let ev = c.feed(&[0x38 | 0x04, 0, 0, 0, 1]); // counter 3, expected 1
        assert!(matches!(ev.as_slice(), [Event::Failed(_)]));

        let mut c = connected(509);
        let bad = Packetizer::new().packetize(&[1, 2, 3, 4], 509);
        assert!(matches!(c.feed(&bad[0]).as_slice(), [Event::Failed(_)]));
    }
}
