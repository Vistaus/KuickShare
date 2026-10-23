//! Transport-agnostic weave session: feed GATT writes in, get events and packets out.

use super::socket::{self, Control, SocketMessage};
use super::weave::{self, Control as WeaveControl, Packetizer, Reassembler, WeaveError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Packet to notify on FromPeripheral.
    Send(Vec<u8>),
    /// Weave handshake done; `packet_size` is the negotiated maximum packet size.
    Connected { packet_size: usize },
    /// Socket INTRODUCTION control frame (informational).
    Introduction,
    /// Chunk of the `[len][OfflineFrame]` stream.
    Data(Vec<u8>),
    /// The phone closed the socket; the session is over.
    Disconnected,
    /// Protocol failure; the session is over.
    Failed(String),
}

#[derive(Debug)]
enum State {
    Idle,
    Connected { packet_size: usize },
    Closed,
}

#[derive(Debug)]
pub struct WeaveSession {
    state: State,
    reassembler: Reassembler,
    packetizer: Packetizer,
    /// Upper bound for the negotiated packet size (the ATT payload of the link).
    packet_limit: usize,
}

impl Default for WeaveSession {
    fn default() -> Self {
        Self::new()
    }
}

impl WeaveSession {
    pub fn new() -> Self {
        Self {
            state: State::Idle,
            reassembler: Reassembler::new(),
            packetizer: Packetizer::new(),
            packet_limit: weave::MAX_PACKET_SIZE,
        }
    }

    /// Caps the packet size agreed in the handshake; only effective before it.
    pub fn set_packet_limit(&mut self, limit: usize) {
        self.packet_limit = limit.clamp(weave::MIN_PACKET_SIZE, weave::MAX_PACKET_SIZE);
    }

    #[cfg(test)]
    pub fn is_closed(&self) -> bool {
        matches!(self.state, State::Closed)
    }

    /// Handles one write to ToPeripheral.
    pub fn feed_write(&mut self, packet: &[u8]) -> Vec<Event> {
        match self.state {
            State::Closed => Vec::new(),
            State::Idle => self.feed_idle(packet),
            State::Connected { .. } => self.feed_connected(packet),
        }
    }

    /// Fragments a complete socket message (hash included) into packets to notify.
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

    fn fail(&mut self, err: WeaveError) -> Vec<Event> {
        self.state = State::Closed;
        vec![Event::Failed(err.to_string())]
    }

    fn feed_idle(&mut self, packet: &[u8]) -> Vec<Event> {
        // Anything before a connection request is noise.
        if weave::control_kind(packet) != Some(WeaveControl::Request) {
            return Vec::new();
        }
        let negotiated = weave::parse_conn_request(packet).and_then(|r| weave::negotiate(&r));
        match negotiated.map(|size| size.min(self.packet_limit)) {
            Ok(packet_size) => {
                self.state = State::Connected { packet_size };
                vec![
                    Event::Send(weave::conn_confirm(packet_size)),
                    Event::Connected { packet_size },
                ]
            }
            Err(e) => self.fail(e),
        }
    }

    fn feed_connected(&mut self, packet: &[u8]) -> Vec<Event> {
        if let Some(kind) = weave::control_kind(packet) {
            return match kind {
                WeaveControl::Error => self.fail(WeaveError::PeerError),
                _ => Vec::new(),
            };
        }
        let message = match self.reassembler.feed(packet) {
            Ok(Some(m)) => m,
            Ok(None) => return Vec::new(),
            Err(e) => return self.fail(e),
        };
        match socket::parse_message(&message) {
            Ok(SocketMessage::Data(d)) => vec![Event::Data(d)],
            Ok(SocketMessage::Control(Control::Introduction)) => vec![Event::Introduction],
            Ok(SocketMessage::Control(Control::Disconnection)) => {
                self.state = State::Closed;
                vec![Event::Disconnected]
            }
            Ok(SocketMessage::Control(Control::Other(_))) => Vec::new(),
            Err(e) => {
                self.state = State::Closed;
                vec![Event::Failed(e.to_string())]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUEST: [u8; 7] = [0x80, 0, 1, 0, 1, 0x01, 0xfd];

    fn connect(s: &mut WeaveSession) {
        let ev = s.feed_write(&REQUEST);
        assert_eq!(
            ev,
            [
                Event::Send(vec![0x81, 0, 1, 0x01, 0xfd]),
                Event::Connected { packet_size: 509 }
            ]
        );
    }

    /// Phone side: packetize with its own counter starting at 1.
    fn phone_send(tx: &mut Packetizer, message: &[u8], size: usize) -> Vec<Vec<u8>> {
        tx.packetize(message, size)
    }

    #[test]
    fn handshake_then_introduction_and_multi_packet_data() {
        let mut s = WeaveSession::new();
        connect(&mut s);
        let mut phone = Packetizer::new();

        let mut events = Vec::new();
        for p in phone_send(&mut phone, &socket::introduction_message(), 509) {
            events.extend(s.feed_write(&p));
        }
        assert_eq!(events, [Event::Introduction]);

        let stream: Vec<u8> = (0..2000u32).map(|i| i as u8).collect();
        let mut events = Vec::new();
        for p in phone_send(&mut phone, &socket::data_message(&stream), 509) {
            events.extend(s.feed_write(&p));
        }
        assert_eq!(events, [Event::Data(stream)]);
    }

    #[test]
    fn multi_packet_message_to_the_phone() {
        let mut s = WeaveSession::new();
        connect(&mut s);
        let data: Vec<u8> = (0..1500u32).map(|i| (i * 7) as u8).collect();
        let packets = s.queue_data(&data);
        assert_eq!(packets.len(), 3);
        assert!(packets.iter().all(|p| p.len() <= 509));

        let mut rx = Reassembler::new();
        let msg = packets.iter().find_map(|p| rx.feed(p).unwrap()).unwrap();
        assert_eq!(socket::parse_message(&msg), Ok(SocketMessage::Data(data)));
    }

    #[test]
    fn negotiated_packet_size_is_honoured() {
        let mut s = WeaveSession::new();
        // Phone asks for 20-byte packets.
        s.feed_write(&[0x80, 0, 1, 0, 1, 0, 20]);
        let packets = s.queue_message(&[0xee; 100]);
        assert!(packets.len() > 5);
        assert!(packets.iter().all(|p| p.len() <= 20));
    }

    #[test]
    fn packet_limit_caps_the_confirmed_size() {
        let mut s = WeaveSession::new();
        s.set_packet_limit(100);
        let ev = s.feed_write(&REQUEST);
        assert_eq!(ev[1], Event::Connected { packet_size: 100 });
        assert_eq!(ev[0], Event::Send(weave::conn_confirm(100)));

        // The limit never raises what the phone asked for, nor goes below 20.
        let mut s = WeaveSession::new();
        s.set_packet_limit(1);
        let ev = s.feed_write(&[0x80, 0, 1, 0, 1, 0, 50]);
        assert_eq!(ev[1], Event::Connected { packet_size: 20 });
    }

    #[test]
    fn disconnection_closes_the_session() {
        let mut s = WeaveSession::new();
        connect(&mut s);
        let mut phone = Packetizer::new();
        let mut events = Vec::new();
        for p in phone_send(&mut phone, &socket::disconnection_message(), 509) {
            events.extend(s.feed_write(&p));
        }
        assert_eq!(events, [Event::Disconnected]);
        assert!(s.is_closed());
        assert!(s.feed_write(&REQUEST).is_empty());
        assert!(s.queue_data(b"x").is_empty());
    }

    #[test]
    fn noise_before_the_request_is_ignored() {
        let mut s = WeaveSession::new();
        assert!(s.feed_write(&[0x18, 1, 2]).is_empty());
        assert!(s.feed_write(&[]).is_empty());
        assert!(s.queue_data(b"x").is_empty());
        connect(&mut s);
    }

    #[test]
    fn bad_counter_fails_the_session() {
        let mut s = WeaveSession::new();
        connect(&mut s);
        let ev = s.feed_write(&[0x38 | 0x04, 0, 0, 0, 1]); // counter 3, expected 1
        assert!(matches!(ev.as_slice(), [Event::Failed(_)]));
        assert!(s.is_closed());
    }

    #[test]
    fn peer_error_and_bad_request_fail() {
        let mut s = WeaveSession::new();
        connect(&mut s);
        assert!(matches!(
            s.feed_write(&[0x82]).as_slice(),
            [Event::Failed(_)]
        ));

        let mut s = WeaveSession::new();
        assert!(matches!(
            s.feed_write(&[0x80, 0, 1]).as_slice(),
            [Event::Failed(_)]
        ));
        let mut s = WeaveSession::new();
        let ev = s.feed_write(&[0x80, 0, 2, 0, 3, 1, 0xfd]);
        assert!(matches!(ev.as_slice(), [Event::Failed(_)]));
    }

    #[test]
    fn unknown_hash_fails_and_ack_control_is_ignored() {
        let mut s = WeaveSession::new();
        connect(&mut s);
        let mut phone = Packetizer::new();
        let ack = phone_send(&mut phone, &[0, 0, 0, 0x08, 0x03], 509);
        assert!(s.feed_write(&ack[0]).is_empty());
        let bad = phone_send(&mut phone, &[1, 2, 3, 4], 509);
        assert!(matches!(
            s.feed_write(&bad[0]).as_slice(),
            [Event::Failed(_)]
        ));
    }
}
