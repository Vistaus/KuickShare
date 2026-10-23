//! BLE L2CAP CoC framing of the Nearby Connections BLE_L2CAP medium, central side.
//!
//! Every SDU is `[u32 BE length][payload]`. The central sends `[03]` (request data
//! connection) and the peripheral answers `[17]` (ready) or `[18]` (failure). After that the
//! BLE socket layer of [`ble_receiver::socket`](crate::hdl::ble_receiver::socket) runs:
//! control messages `00 00 00 <SocketControlFrame>` (INTRODUCTION first, one
//! PACKET_ACKNOWLEDGEMENT per received data message, DISCONNECTION at the end) and data
//! messages `fc 9f 5e <bytes of the [len][OfflineFrame] stream>`. A peripheral that is not
//! acknowledged stops sending.
//!
//! No I/O here: bytes from the link go into [`L2capSession::feed`], SDUs to write come out.

use crate::hdl::ble_receiver::socket::{self, Control, SERVICE_HASH, SocketMessage};

pub const CMD_REQUEST_DATA_CONNECTION: u8 = 0x03;
pub const CMD_DATA_CONNECTION_READY: u8 = 0x17;
pub const CMD_DATA_CONNECTION_FAILURE: u8 = 0x18;
/// Largest SDU payload we send. 64 KB breaks the link on the phones tried by other clients.
pub const MAX_SEND_SDU: usize = 16 * 1024;
/// Largest SDU payload accepted. Phones send at most 1024 B; this only bounds the buffer.
pub const MAX_RECV_SDU: usize = 64 * 1024;
const LEN_PREFIX: usize = 4;
const CONTROL_HASH: [u8; 3] = [0; 3];
const CTRL_PACKET_ACKNOWLEDGEMENT: u8 = 3;

/// `[len][payload]`.
pub fn encode_sdu(payload: &[u8]) -> Vec<u8> {
    let mut sdu = Vec::with_capacity(LEN_PREFIX + payload.len());
    sdu.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    sdu.extend_from_slice(payload);
    sdu
}

/// The first message after the handshake: type INTRODUCTION with {service_id_hash,
/// socket_version V2}. Same bytes as `ble_receiver::socket::introduction_message` (test-only
/// there; a test below keeps them equal).
pub fn introduction_message() -> Vec<u8> {
    let mut m = CONTROL_HASH.to_vec();
    m.extend_from_slice(&[0x08, 0x01, 0x12, 0x07, 0x0a, 0x03]);
    m.extend_from_slice(&SERVICE_HASH);
    m.extend_from_slice(&[0x10, 0x02]);
    m
}

pub fn disconnection_message() -> Vec<u8> {
    let mut m = CONTROL_HASH.to_vec();
    m.extend_from_slice(&[0x08, 0x02]);
    m
}

/// PACKET_ACKNOWLEDGEMENT { hash, size } for a data message that carried `received` bytes.
pub fn ack_message(received: usize) -> Vec<u8> {
    let mut inner = vec![0x0a, 0x03];
    inner.extend_from_slice(&SERVICE_HASH);
    inner.push(0x10);
    let mut v = received as u64;
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            inner.push(b);
            break;
        }
        inner.push(b | 0x80);
    }
    let mut m = CONTROL_HASH.to_vec();
    m.extend_from_slice(&[0x08, CTRL_PACKET_ACKNOWLEDGEMENT, 0x22, inner.len() as u8]);
    m.extend_from_slice(&inner);
    m
}

/// Data message(s) for `data`, each SDU at most [`MAX_SEND_SDU`] long.
pub fn data_sdus(data: &[u8]) -> Vec<Vec<u8>> {
    data.chunks(MAX_SEND_SDU - SERVICE_HASH.len())
        .map(|chunk| encode_sdu(&socket::data_message(chunk)))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    ZeroLength,
    TooLarge(usize),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroLength => f.write_str("empty L2CAP SDU"),
            Self::TooLarge(len) => write!(f, "L2CAP SDU of {len} bytes"),
        }
    }
}

impl std::error::Error for FrameError {}

/// Splits the byte stream of the link into SDUs, however it was cut by the reads.
#[derive(Debug, Default)]
pub struct SduDecoder {
    buf: Vec<u8>,
}

impl SduDecoder {
    /// Returns the SDU payloads completed by `bytes`. An invalid length cannot be skipped
    /// (the stream is out of step), so it is an error and the decoder is cleared.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, FrameError> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        let mut pos = 0;
        while let Some(prefix) = self.buf[pos..].first_chunk::<LEN_PREFIX>() {
            let len = u32::from_be_bytes(*prefix) as usize;
            if len == 0 || len > MAX_RECV_SDU {
                self.buf.clear();
                return Err(if len == 0 {
                    FrameError::ZeroLength
                } else {
                    FrameError::TooLarge(len)
                });
            }
            let end = pos + LEN_PREFIX + len;
            if self.buf.len() < end {
                break;
            }
            out.push(self.buf[pos + LEN_PREFIX..end].to_vec());
            pos = end;
        }
        self.buf.drain(..pos);
        Ok(out)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// SDU to write to the link.
    Send(Vec<u8>),
    /// The peripheral accepted the data connection.
    Ready,
    /// Chunk of the `[len][OfflineFrame]` stream (already acknowledged).
    Data(Vec<u8>),
    /// The peripheral closed the socket; the session is over.
    Disconnected,
    /// The peripheral answered the request with the failure byte; the session is over.
    Refused,
    /// Protocol failure; the session is over.
    Failed(String),
}

#[derive(Debug, PartialEq, Eq)]
enum State {
    Idle,
    Requested,
    Ready,
    Closed,
}

#[derive(Debug)]
pub struct L2capSession {
    state: State,
    decoder: SduDecoder,
}

impl Default for L2capSession {
    fn default() -> Self {
        Self::new()
    }
}

impl L2capSession {
    pub fn new() -> Self {
        Self {
            state: State::Idle,
            decoder: SduDecoder::default(),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.state == State::Ready
    }

    /// The SDU that opens the handshake.
    pub fn request(&mut self) -> Vec<u8> {
        self.state = State::Requested;
        encode_sdu(&[CMD_REQUEST_DATA_CONNECTION])
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Event> {
        let mut events = Vec::new();
        if self.state == State::Closed {
            return events;
        }
        match self.decoder.feed(bytes) {
            Ok(sdus) => {
                for sdu in sdus {
                    self.on_sdu(&sdu, &mut events);
                    if self.state == State::Closed {
                        break;
                    }
                }
            }
            Err(e) => self.fail(e.to_string(), &mut events),
        }
        events
    }

    /// SDUs carrying `data`; nothing before the peripheral is ready.
    pub fn queue_data(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        if self.state == State::Ready {
            data_sdus(data)
        } else {
            Vec::new()
        }
    }

    /// Closes the session and returns the DISCONNECTION SDU if the peripheral is to be told.
    pub fn goodbye(&mut self) -> Option<Vec<u8>> {
        let was_ready = self.state == State::Ready;
        self.state = State::Closed;
        was_ready.then(|| encode_sdu(&disconnection_message()))
    }

    fn fail(&mut self, reason: String, events: &mut Vec<Event>) {
        self.state = State::Closed;
        events.push(Event::Failed(reason));
    }

    fn on_sdu(&mut self, sdu: &[u8], events: &mut Vec<Event>) {
        match self.state {
            State::Requested => match sdu.first() {
                Some(&CMD_DATA_CONNECTION_READY) => {
                    self.state = State::Ready;
                    events.push(Event::Send(encode_sdu(&introduction_message())));
                    events.push(Event::Ready);
                }
                Some(&CMD_DATA_CONNECTION_FAILURE) => {
                    self.state = State::Closed;
                    events.push(Event::Refused);
                }
                other => self.fail(format!("unexpected handshake answer {other:?}"), events),
            },
            State::Ready => match socket::parse_message(sdu) {
                Ok(SocketMessage::Data(data)) => {
                    events.push(Event::Send(encode_sdu(&ack_message(data.len()))));
                    if !data.is_empty() {
                        events.push(Event::Data(data));
                    }
                }
                Ok(SocketMessage::Control(Control::Disconnection)) => {
                    self.state = State::Closed;
                    events.push(Event::Disconnected);
                }
                // Introduction and acknowledgements need no action; a message that is too
                // short or has another hash leaves the framing intact and is dropped.
                Ok(SocketMessage::Control(_)) | Err(_) => {
                    debug!("l2cap: ignored {} byte SDU", sdu.len())
                }
            },
            State::Idle => self.fail("data before the request".into(), events),
            State::Closed => {}
        }
    }
}

#[cfg(test)]
/// Decodes an acknowledgement message by hand, independently of `ack_message`.
pub(crate) fn parse_ack(m: &[u8]) -> Option<usize> {
    let rest = m.strip_prefix(&[0, 0, 0, 0x08, 0x03, 0x22])?;
    let (&len, inner) = rest.split_first()?;
    let inner = inner.strip_prefix(&[0x0a, 0x03, 0xfc, 0x9f, 0x5e, 0x10])?;
    assert_eq!(inner.len(), len as usize - 6);
    let mut size = 0usize;
    for (i, b) in inner.iter().enumerate() {
        size |= ((b & 0x7f) as usize) << (7 * i);
        if b & 0x80 == 0 {
            assert_eq!(i + 1, inner.len(), "trailing bytes");
            return Some(size);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_session() -> L2capSession {
        let mut s = L2capSession::new();
        s.request();
        s.feed(&encode_sdu(&[CMD_DATA_CONNECTION_READY]));
        s
    }

    #[test]
    fn introduction_matches_the_receiver_side_encoder() {
        assert_eq!(introduction_message(), socket::introduction_message());
        assert_eq!(disconnection_message(), socket::disconnection_message());
    }

    #[test]
    fn ack_matches_the_captured_bytes() {
        assert_eq!(
            ack_message(5),
            [
                0, 0, 0, 0x08, 0x03, 0x22, 0x07, 0x0a, 0x03, 0xfc, 0x9f, 0x5e, 0x10, 0x05
            ]
        );
        for n in [0, 1, 127, 128, 1024, 16_383, 16_384, 1 << 20] {
            assert_eq!(parse_ack(&ack_message(n)), Some(n), "{n}");
        }
    }

    #[test]
    fn sdus_have_a_big_endian_length() {
        assert_eq!(encode_sdu(&[3]), [0, 0, 0, 1, 3]);
    }

    #[test]
    fn decoder_handles_fragmented_and_merged_reads() {
        let all: Vec<u8> = [vec![1u8; 10], vec![2u8; 300], vec![3u8; 1]]
            .iter()
            .flat_map(|p| encode_sdu(p))
            .collect();
        for step in [1, 2, 3, 7, 100, all.len()] {
            let mut d = SduDecoder::default();
            let mut got = Vec::new();
            for part in all.chunks(step) {
                got.extend(d.feed(part).unwrap());
            }
            assert_eq!(got, [vec![1u8; 10], vec![2u8; 300], vec![3u8; 1]], "{step}");
        }
    }

    #[test]
    fn decoder_rejects_bad_lengths_without_buffering() {
        let mut d = SduDecoder::default();
        assert_eq!(d.feed(&[0, 0, 0, 0]), Err(FrameError::ZeroLength));
        assert_eq!(
            d.feed(&[0xff, 0xff, 0xff, 0xff]),
            Err(FrameError::TooLarge(u32::MAX as usize))
        );
        // Just over the cap is refused on the prefix alone, before any body arrived.
        let over = (MAX_RECV_SDU as u32 + 1).to_be_bytes();
        assert!(d.feed(&over).is_err());
        assert!(d.buf.is_empty());
        let at_cap = (MAX_RECV_SDU as u32).to_be_bytes();
        assert_eq!(d.feed(&at_cap), Ok(vec![]));
    }

    #[test]
    fn handshake_ready_sends_the_introduction() {
        let mut s = L2capSession::new();
        assert_eq!(s.request(), [0, 0, 0, 1, 3]);
        assert!(!s.is_ready());
        let events = s.feed(&[0, 0, 0, 1, 0x17]);
        assert_eq!(
            events,
            [
                Event::Send(encode_sdu(&introduction_message())),
                Event::Ready
            ]
        );
        assert!(s.is_ready());
    }

    #[test]
    fn handshake_failure_byte_ends_the_session() {
        let mut s = L2capSession::new();
        s.request();
        assert_eq!(s.feed(&[0, 0, 0, 1, 0x18]), [Event::Refused]);
        assert!(!s.is_ready());
        assert!(s.feed(&[0, 0, 0, 1, 0x17]).is_empty());
        assert!(s.queue_data(b"x").is_empty());
    }

    #[test]
    fn unexpected_handshake_answers_fail() {
        for answer in [vec![0x42], vec![0, 0, 0, 1, 0xfc]] {
            let mut s = L2capSession::new();
            s.request();
            let events = s.feed(&encode_sdu(&answer));
            assert!(
                matches!(events.as_slice(), [Event::Failed(_)]),
                "{answer:?}"
            );
        }
        let mut s = L2capSession::new();
        assert!(matches!(
            s.feed(&encode_sdu(&[0x17])).as_slice(),
            [Event::Failed(_)]
        ));
    }

    #[test]
    fn data_is_acknowledged_with_its_size_and_delivered() {
        let mut s = ready_session();
        let sdu = encode_sdu(&socket::data_message(&[9u8; 300]));
        let events = s.feed(&sdu);
        assert_eq!(
            events,
            [
                Event::Send(encode_sdu(&ack_message(300))),
                Event::Data(vec![9u8; 300])
            ]
        );
    }

    #[test]
    fn merged_and_fragmented_reads_give_the_same_events() {
        let sdus: Vec<u8> = [vec![1u8; 5], vec![2u8; 700]]
            .iter()
            .flat_map(|d| encode_sdu(&socket::data_message(d)))
            .collect();
        let whole = ready_session().feed(&sdus);
        let mut s = ready_session();
        let pieces: Vec<Event> = sdus.chunks(13).flat_map(|c| s.feed(c)).collect();
        assert_eq!(whole, pieces);
        assert_eq!(whole.len(), 4);
    }

    #[test]
    fn disconnection_ends_the_session_and_drops_the_rest() {
        let mut s = ready_session();
        let mut bytes = encode_sdu(&disconnection_message());
        bytes.extend(encode_sdu(&socket::data_message(b"late")));
        assert_eq!(s.feed(&bytes), [Event::Disconnected]);
        assert!(s.feed(&bytes).is_empty());
    }

    #[test]
    fn control_and_unknown_sdus_are_ignored() {
        let mut s = ready_session();
        for payload in [
            introduction_message(),
            ack_message(3),
            vec![1, 2],
            vec![1, 2, 3, 4],
            vec![0, 0, 0, 0x10],
        ] {
            assert!(s.feed(&encode_sdu(&payload)).is_empty(), "{payload:?}");
        }
        assert!(s.is_ready());
    }

    #[test]
    fn garbage_framing_fails_the_session() {
        let mut s = ready_session();
        let events = s.feed(&[0xde, 0xad, 0xbe, 0xef, 1, 2, 3]);
        assert!(matches!(events.as_slice(), [Event::Failed(_)]));
        assert!(s.feed(&encode_sdu(&socket::data_message(b"x"))).is_empty());
        let mut s = ready_session();
        assert!(matches!(
            s.feed(&[0, 0, 0, 0]).as_slice(),
            [Event::Failed(_)]
        ));
    }

    #[test]
    fn queued_data_is_split_into_bounded_sdus_and_reassembles() {
        let mut s = ready_session();
        let data: Vec<u8> = (0..50_000u32).map(|i| i as u8).collect();
        let sdus = s.queue_data(&data);
        assert_eq!(sdus.len(), 4);
        let mut joined = Vec::new();
        for sdu in &sdus {
            let payload = &sdu[LEN_PREFIX..];
            assert!(payload.len() <= MAX_SEND_SDU);
            assert_eq!(&sdu[..LEN_PREFIX], (payload.len() as u32).to_be_bytes());
            assert_eq!(payload[..3], SERVICE_HASH);
            joined.extend_from_slice(&payload[3..]);
        }
        assert_eq!(joined, data);
        assert_eq!(sdus[0].len(), LEN_PREFIX + MAX_SEND_SDU);
    }

    #[test]
    fn nothing_is_queued_before_ready_and_goodbye_only_after() {
        let mut s = L2capSession::new();
        assert!(s.queue_data(b"x").is_empty());
        assert_eq!(s.goodbye(), None);
        let mut s = ready_session();
        assert_eq!(s.goodbye(), Some(encode_sdu(&disconnection_message())));
        assert!(s.queue_data(b"x").is_empty());
    }
}
