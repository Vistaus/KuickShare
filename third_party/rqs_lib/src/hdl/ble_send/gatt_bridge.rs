//! Hardware-free bridge between a GATT link (we are the central) and the byte stream an
//! `OutboundRequest` uses; the GATT counterpart of `bridge::CentralBridge`.
//!
//! Us to peripheral: the stream carries `[4-byte BE len][OfflineFrame]`; every complete
//! frame becomes one socket data message, fragmented into weave packets of at most the
//! link's `max_packet`; each packet is one write.
//! Peripheral to us: indicated packets go through the [`WeaveCentral`]; data goes to the
//! stream.

use std::collections::VecDeque;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until};

use super::bridge::{BridgeExit, FIRST_DATA_TIMEOUT, HANDSHAKE_TIMEOUT};
use super::weave_central::{Event, WeaveCentral};
use crate::hdl::ble_receiver::weave::MAX_MESSAGE_SIZE;

/// How long a closing bridge keeps handing over what it already holds.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(5);
/// Bytes from the peripheral waiting for the session to read them.
const MAX_PENDING_TO_STREAM: usize = 1 << 20;
/// Packets queued before the stream is no longer read (BLE is the slow side).
const MAX_OUTBOX: usize = 1024;
const READ_CHUNK: usize = 16 * 1024;
/// Largest frame accepted from the session: the weave message limit minus hash and length.
const MAX_FRAME: usize = MAX_MESSAGE_SIZE - 7;

/// What the dial side (`gatt_dial`) hands to the bridge: the two characteristics of the
/// phone's 0xFEF3 service as channels.
#[derive(Debug)]
pub struct GattLink {
    /// Packets for ToPeripheral (`...0101`, ATT write with response). The dial side awaits
    /// each write, so a small channel capacity gives the backpressure.
    pub writes: mpsc::Sender<Vec<u8>>,
    /// Packets indicated on FromPeripheral (`...0102`). Closed when the link is gone.
    pub notifications: mpsc::Receiver<Vec<u8>>,
    /// Largest weave packet one write can carry: `min(ATT MTU - 3, 509)`.
    pub max_packet: usize,
}

#[derive(Debug)]
pub struct GattCentralBridge {
    link: GattLink,
    stream: DuplexStream,
    handshake_timeout: Duration,
    first_data_timeout: Duration,
}

impl GattCentralBridge {
    /// Dropping the bridge drops `stream`, which the session sees as end of stream.
    pub fn new(link: GattLink, stream: DuplexStream) -> Self {
        Self {
            link,
            stream,
            handshake_timeout: HANDSHAKE_TIMEOUT,
            first_data_timeout: FIRST_DATA_TIMEOUT,
        }
    }

    #[cfg(test)]
    pub fn with_handshake_timeout(mut self, t: Duration) -> Self {
        self.handshake_timeout = t;
        self
    }

    #[cfg(test)]
    pub fn with_first_data_timeout(mut self, t: Duration) -> Self {
        self.first_data_timeout = t;
        self
    }

    /// Runs the weave handshake and then the session. Packets already produced are written
    /// first, so the peripheral gets the last frame (e.g. the plaintext DISCONNECTION of an
    /// upgrade) and our socket DISCONNECTION.
    pub async fn run(self) -> BridgeExit {
        let Self {
            link:
                GattLink {
                    writes,
                    mut notifications,
                    max_packet,
                },
            stream,
            handshake_timeout,
            first_data_timeout,
        } = self;
        let (mut reader, mut writer) = tokio::io::split(stream);
        let mut core = Core::new(max_packet);
        let mut chunk = vec![0u8; READ_CHUNK];
        let handshake_at = Instant::now() + handshake_timeout;
        let mut silent_at = handshake_at;
        let mut flush_at: Option<Instant> = None;

        // Every write is one branch of the loop, not an await inside another one: the session
        // may be blocked writing to us while we would be blocked writing to it, and the same
        // goes for the link.
        while !core.is_done() {
            if core.exit.is_some() && flush_at.is_none() {
                flush_at = Some(Instant::now() + FLUSH_TIMEOUT);
            }
            tokio::select! {
                packet = notifications.recv(),
                    if core.exit.is_none() && core.to_stream.len() < MAX_PENDING_TO_STREAM =>
                {
                    match packet {
                        Some(packet) => {
                            silent_at = Instant::now() + first_data_timeout;
                            core.on_packet(&packet);
                        }
                        None => core.end(BridgeExit::LinkClosed),
                    }
                }
                read = reader.read(&mut chunk),
                    if core.exit.is_none()
                        && core.session.is_connected()
                        && core.outbox.len() < MAX_OUTBOX =>
                {
                    match read {
                        Ok(n) if n > 0 => core.on_stream_bytes(&chunk[..n]),
                        _ => core.end(BridgeExit::StreamClosed),
                    }
                }
                written = writer.write(&core.to_stream), if !core.to_stream.is_empty() => {
                    match written {
                        Ok(n) => {
                            core.to_stream.drain(..n);
                        }
                        Err(_) => core.end(BridgeExit::StreamClosed),
                    }
                }
                permit = writes.reserve(), if !core.outbox.is_empty() => {
                    match (permit, core.outbox.pop_front()) {
                        (Ok(permit), Some(packet)) => permit.send(packet),
                        (Err(_), _) => core.abort_link(),
                        (Ok(_), None) => {}
                    }
                }
                () = sleep_until(handshake_at),
                    if !core.session.is_connected() && core.exit.is_none() =>
                {
                    core.end(BridgeExit::HandshakeTimeout);
                }
                () = sleep_until(silent_at),
                    if core.session.is_connected() && !core.got_data && core.exit.is_none() =>
                {
                    core.end(BridgeExit::SilentPeer);
                }
                () = sleep_until(flush_at.unwrap_or(handshake_at)), if flush_at.is_some() => {
                    break;
                }
            }
        }
        core.exit.unwrap_or(BridgeExit::StreamClosed)
    }
}

/// The synchronous part of the bridge: no I/O, so every transition can be tested directly.
#[derive(Debug)]
struct Core {
    session: WeaveCentral,
    /// The peripheral has sent at least one data message.
    got_data: bool,
    /// Peripheral data not yet written to the stream.
    to_stream: Vec<u8>,
    /// Partial frame read from the stream.
    from_stream: Vec<u8>,
    /// Packets not yet written to the link.
    outbox: VecDeque<Vec<u8>>,
    exit: Option<BridgeExit>,
}

impl Core {
    /// Starts with the connection request queued.
    fn new(max_packet: usize) -> Self {
        let session = WeaveCentral::new(max_packet);
        let outbox = VecDeque::from([session.request()]);
        Self {
            session,
            got_data: false,
            to_stream: Vec::new(),
            from_stream: Vec::new(),
            outbox,
            exit: None,
        }
    }

    fn is_done(&self) -> bool {
        self.exit.is_some() && self.outbox.is_empty() && self.to_stream.is_empty()
    }

    fn abort_link(&mut self) {
        self.outbox.clear();
        self.end(BridgeExit::LinkClosed);
    }

    /// The first reason wins. What is still worth delivering depends on who ended it.
    fn end(&mut self, exit: BridgeExit) {
        if self.exit.is_some() {
            return;
        }
        match exit {
            // Let the session read the rest of what the peripheral sent.
            BridgeExit::PhoneDisconnected | BridgeExit::LinkClosed => self.outbox.clear(),
            // The session is gone, but the peripheral still gets its last frames and a
            // socket DISCONNECTION.
            BridgeExit::StreamClosed => {
                self.to_stream.clear();
                self.outbox.extend(self.session.goodbye());
            }
            _ => {
                self.to_stream.clear();
                self.outbox.clear();
            }
        }
        self.exit = Some(exit);
    }

    fn on_packet(&mut self, packet: &[u8]) {
        for event in self.session.feed(packet) {
            match event {
                Event::Connected { packet_size } => {
                    debug!("gatt bridge: weave connected, packet size {packet_size}");
                    self.outbox.extend(self.session.introduction());
                }
                Event::Introduction => debug!("gatt bridge: socket introduction"),
                Event::Data(d) => {
                    self.got_data = true;
                    self.to_stream.extend_from_slice(&d);
                }
                Event::Disconnected => self.end(BridgeExit::PhoneDisconnected),
                Event::Failed(reason) => {
                    warn!("gatt bridge: {reason}");
                    self.end(BridgeExit::Protocol(reason));
                }
            }
        }
    }

    fn on_stream_bytes(&mut self, bytes: &[u8]) {
        self.from_stream.extend_from_slice(bytes);
        while let Some(prefix) = self.from_stream.first_chunk::<4>() {
            let len = u32::from_be_bytes(*prefix) as usize;
            if len > MAX_FRAME {
                self.end(BridgeExit::Protocol(
                    "oversized frame from the session".into(),
                ));
                return;
            }
            if self.from_stream.len() < 4 + len {
                return;
            }
            let frame: Vec<u8> = self.from_stream.drain(..4 + len).collect();
            self.outbox.extend(self.session.queue_data(&frame));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hdl::ble_receiver::socket::{self, SocketMessage};
    use crate::hdl::ble_receiver::weave::{self, Packetizer, Reassembler};

    fn connected_core(packet_size: usize) -> Core {
        let mut core = Core::new(packet_size);
        core.outbox.clear();
        core.on_packet(&weave::conn_confirm(packet_size));
        core
    }

    fn messages(packets: impl IntoIterator<Item = Vec<u8>>) -> Vec<Vec<u8>> {
        let mut rx = Reassembler::new();
        packets
            .into_iter()
            .filter_map(|p| rx.feed(&p).unwrap())
            .collect()
    }

    #[test]
    fn the_request_is_the_first_packet_and_the_introduction_follows_the_confirm() {
        let mut core = Core::new(509);
        assert_eq!(core.outbox, [core.session.request()]);
        core.outbox.clear();
        core.on_packet(&weave::conn_confirm(509));
        assert_eq!(
            messages(core.outbox.drain(..)),
            [socket::introduction_message()]
        );
    }

    #[test]
    fn frames_become_one_message_each_however_they_arrive() {
        let mut core = connected_core(100);
        let frame = |n: usize| {
            let mut f = (n as u32).to_be_bytes().to_vec();
            f.extend(vec![7u8; n]);
            f
        };
        let joined = [frame(10), frame(300), frame(3)].concat();
        for part in [&joined[..2], &joined[2..200], &joined[200..]] {
            core.on_stream_bytes(part);
        }
        assert!(core.outbox.iter().all(|p| p.len() <= 100));
        let mut expected = vec![socket::introduction_message()];
        expected.extend([10, 300, 3].map(|n| socket::data_message(&frame(n))));
        assert_eq!(messages(core.outbox.drain(..)), expected);
    }

    #[test]
    fn an_oversized_frame_is_a_protocol_error() {
        let mut core = connected_core(509);
        core.on_stream_bytes(&[0xff; 4]);
        assert!(matches!(core.exit, Some(BridgeExit::Protocol(_))));
    }

    #[test]
    fn peripheral_data_goes_to_the_stream_and_disconnection_keeps_it() {
        let mut core = connected_core(509);
        let mut phone = Packetizer::new();
        for p in phone.packetize(&socket::data_message(b"hello"), 509) {
            core.on_packet(&p);
        }
        assert!(core.got_data);
        core.on_stream_bytes(&[0, 0, 0, 1, 9]);
        for p in phone.packetize(&socket::disconnection_message(), 509) {
            core.on_packet(&p);
        }
        assert_eq!(core.exit, Some(BridgeExit::PhoneDisconnected));
        assert_eq!(core.to_stream, b"hello");
        assert!(core.outbox.is_empty(), "nothing is sent to a closed peer");
    }

    #[test]
    fn a_closed_stream_still_delivers_the_queued_frames_and_the_goodbye() {
        let mut core = connected_core(509);
        core.on_stream_bytes(&[0, 0, 0, 1, 9]);
        core.end(BridgeExit::StreamClosed);
        assert!(!core.is_done());
        assert_eq!(
            messages(core.outbox.drain(..)),
            [
                socket::introduction_message(),
                socket::data_message(&[0, 0, 0, 1, 9]),
                socket::disconnection_message()
            ]
        );
        assert!(core.is_done());
    }

    #[test]
    fn failures_drop_everything_and_the_first_reason_wins() {
        let mut core = connected_core(509);
        core.on_stream_bytes(&[0, 0, 0, 1, 9]);
        core.on_packet(&[0x82]);
        assert!(matches!(core.exit, Some(BridgeExit::Protocol(_))));
        assert!(core.outbox.is_empty() && core.is_done());
        core.end(BridgeExit::StreamClosed);
        assert!(matches!(core.exit, Some(BridgeExit::Protocol(_))));
    }

    #[test]
    fn a_bad_confirm_is_a_protocol_error() {
        let mut core = Core::new(509);
        core.on_packet(&[0x81, 0, 2, 0x01, 0xfd]);
        assert!(matches!(core.exit, Some(BridgeExit::Protocol(_))));
        assert!(!core.session.is_connected());
    }

    // --- The whole bridge against a hand-driven peripheral --------------------------------

    const STEP: Duration = Duration::from_secs(5);

    struct Harness {
        /// Packets the bridge wrote.
        writes: mpsc::Receiver<Vec<u8>>,
        /// Packets the peripheral indicates.
        notify: Option<mpsc::Sender<Vec<u8>>>,
        session_end: DuplexStream,
        bridge: tokio::task::JoinHandle<BridgeExit>,
        tx: Packetizer,
        rx: Reassembler,
        packet_size: usize,
    }

    impl Harness {
        fn start() -> Self {
            Self::start_with(|b| b)
        }

        fn start_with(tune: impl FnOnce(GattCentralBridge) -> GattCentralBridge) -> Self {
            let (writes_tx, writes) = mpsc::channel(1);
            let (notify_tx, notifications) = mpsc::channel(16);
            let (session_end, bridge_end) = tokio::io::duplex(64 * 1024);
            let link = GattLink {
                writes: writes_tx,
                notifications,
                max_packet: 100,
            };
            let bridge = tokio::spawn(tune(GattCentralBridge::new(link, bridge_end)).run());
            Self {
                writes,
                notify: Some(notify_tx),
                session_end,
                bridge,
                tx: Packetizer::new(),
                rx: Reassembler::new(),
                packet_size: 100,
            }
        }

        async fn next_write(&mut self) -> Option<Vec<u8>> {
            tokio::time::timeout(STEP, self.writes.recv())
                .await
                .expect("no write")
        }

        async fn notify(&self, packet: Vec<u8>) {
            self.notify.as_ref().unwrap().send(packet).await.unwrap();
        }

        /// Reads the request, confirms like a phone and returns our INTRODUCTION.
        async fn accept(&mut self) -> SocketMessage {
            let request = self.next_write().await.unwrap();
            assert_eq!(request, [0x80, 0, 1, 0, 1, 0, 100]);
            self.notify(weave::conn_confirm(self.packet_size)).await;
            self.next_message().await.unwrap()
        }

        async fn send_message(&mut self, message: &[u8]) {
            for p in self.tx.packetize(message, self.packet_size) {
                self.notify(p).await;
            }
        }

        async fn next_message(&mut self) -> Option<SocketMessage> {
            while let Some(packet) = self.next_write().await {
                assert!(packet.len() <= self.packet_size);
                if let Some(m) = self.rx.feed(&packet).unwrap() {
                    return Some(socket::parse_message(&m).unwrap());
                }
            }
            None
        }

        async fn exit(self) -> BridgeExit {
            tokio::time::timeout(STEP, self.bridge)
                .await
                .expect("bridge did not stop")
                .unwrap()
        }
    }

    async fn read_all(stream: &mut DuplexStream) -> Vec<u8> {
        let mut out = Vec::new();
        tokio::time::timeout(STEP, stream.read_to_end(&mut out))
            .await
            .expect("stream was not closed")
            .unwrap();
        out
    }

    #[tokio::test]
    async fn handshake_then_introduction() {
        let mut h = Harness::start();
        assert_eq!(
            h.accept().await,
            SocketMessage::Control(socket::Control::Introduction)
        );
    }

    #[tokio::test]
    async fn the_stream_is_not_read_before_the_handshake_is_done() {
        let mut h = Harness::start();
        let early = [0, 0, 0, 2, 1, 2];
        h.session_end.write_all(&early).await.unwrap();
        assert_eq!(h.next_write().await.unwrap()[0], 0x80);
        tokio::time::sleep(Duration::from_millis(100)).await;
        h.notify(weave::conn_confirm(100)).await;
        assert!(matches!(
            h.next_message().await,
            Some(SocketMessage::Control(socket::Control::Introduction))
        ));
        assert_eq!(
            h.next_message().await,
            Some(SocketMessage::Data(early.to_vec()))
        );
    }

    #[tokio::test]
    async fn big_frames_are_split_into_bounded_packets_and_phone_data_arrives() {
        let mut h = Harness::start();
        h.accept().await;
        let mut big = 50_000u32.to_be_bytes().to_vec();
        big.extend((0..50_000u32).map(|i| i as u8));
        h.session_end.write_all(&big).await.unwrap();
        assert_eq!(h.next_message().await, Some(SocketMessage::Data(big)));

        // The peripheral's INTRODUCTION is tolerated, its data reaches the stream.
        h.send_message(&socket::introduction_message()).await;
        h.send_message(&socket::data_message(&[4u8; 3000])).await;
        let mut got = vec![0u8; 3000];
        h.session_end.read_exact(&mut got).await.unwrap();
        assert_eq!(got, [4u8; 3000]);
    }

    #[tokio::test]
    async fn peer_disconnection_delivers_earlier_data_first() {
        let mut h = Harness::start();
        h.accept().await;
        h.send_message(&socket::data_message(&[5u8; 1000])).await;
        h.send_message(&socket::disconnection_message()).await;
        assert_eq!(read_all(&mut h.session_end).await, [5u8; 1000]);
        assert_eq!(h.exit().await, BridgeExit::PhoneDisconnected);
    }

    #[tokio::test]
    async fn closing_the_stream_delivers_the_last_frame_then_disconnects() {
        let mut h = Harness::start();
        h.accept().await;
        let mut last = 2000u32.to_be_bytes().to_vec();
        last.extend([9u8; 2000]);
        h.session_end.write_all(&last).await.unwrap();
        h.session_end.shutdown().await.unwrap();
        assert_eq!(h.next_message().await, Some(SocketMessage::Data(last)));
        assert_eq!(
            h.next_message().await,
            Some(SocketMessage::Control(socket::Control::Disconnection))
        );
        assert_eq!(h.exit().await, BridgeExit::StreamClosed);
    }

    #[tokio::test]
    async fn no_confirm_times_out() {
        let h = Harness::start_with(|b| b.with_handshake_timeout(Duration::from_millis(50)));
        assert_eq!(h.exit().await, BridgeExit::HandshakeTimeout);
    }

    #[tokio::test]
    async fn a_silent_connected_peer_times_out_but_only_before_its_first_data() {
        let mut h = Harness::start_with(|b| b.with_first_data_timeout(Duration::from_millis(50)));
        h.accept().await;
        assert_eq!(h.exit().await, BridgeExit::SilentPeer);

        let mut h = Harness::start_with(|b| b.with_first_data_timeout(Duration::from_millis(100)));
        h.accept().await;
        h.send_message(&socket::data_message(b"hi")).await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(!h.bridge.is_finished());
    }

    #[tokio::test]
    async fn garbage_ends_the_session() {
        let mut h = Harness::start();
        h.next_write().await.unwrap();
        h.notify(vec![0x82]).await;
        assert!(matches!(h.exit().await, BridgeExit::Protocol(_)));

        let mut h = Harness::start();
        h.accept().await;
        h.notify(vec![0x7c, 1, 2]).await; // counter 7, expected 1
        assert!(read_all(&mut h.session_end).await.is_empty());
        assert!(matches!(h.exit().await, BridgeExit::Protocol(_)));
    }

    #[tokio::test]
    async fn a_bogus_frame_length_from_the_session_ends_it() {
        let mut h = Harness::start();
        h.accept().await;
        h.session_end.write_all(&[0xff; 4]).await.unwrap();
        assert!(matches!(h.exit().await, BridgeExit::Protocol(_)));
    }

    #[tokio::test]
    async fn losing_the_link_delivers_what_arrived_and_ends_the_session() {
        // Notifications stop.
        let mut h = Harness::start();
        h.accept().await;
        h.send_message(&socket::data_message(b"last words")).await;
        h.notify = None;
        assert_eq!(read_all(&mut h.session_end).await, b"last words");
        assert_eq!(h.exit().await, BridgeExit::LinkClosed);

        // The write side is gone.
        let mut h = Harness::start();
        h.accept().await;
        let Harness {
            writes,
            mut session_end,
            bridge,
            notify: _notify,
            ..
        } = h;
        drop(writes);
        session_end.write_all(&[0, 0, 0, 1, 1]).await.unwrap();
        assert_eq!(
            tokio::time::timeout(STEP, bridge).await.unwrap().unwrap(),
            BridgeExit::LinkClosed
        );
    }
}
