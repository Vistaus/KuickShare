use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use prost::Message as _;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;

use super::*;
use crate::channel::{self, ChannelMessage, TransferAction};
use crate::hdl::ble_receiver::socket::{
    self, SocketMessage, data_message, disconnection_message, introduction_message,
};
use crate::hdl::ble_receiver::weave::{Packetizer, Reassembler};
use crate::hdl::{
    InboundRequest, MigratableStream, OutboundPayload, OutboundRequest, TransferState,
};
use crate::manager::drive_inbound;
use crate::upgrade::tests::{FakeLink, SERIAL};
use crate::upgrade::{self, WifiLink};
use crate::utils::RemoteDeviceInfo;

fn conn_request(max_packet: u16) -> Vec<u8> {
    let mut p = vec![0x80, 0, 1, 0, 1];
    p.extend_from_slice(&max_packet.to_be_bytes());
    p
}

fn frame(body: &[u8]) -> Vec<u8> {
    let mut f = (body.len() as u32).to_be_bytes().to_vec();
    f.extend_from_slice(body);
    f
}

/// Phone side of a bridge, driven by hand.
struct Harness {
    writes: Option<mpsc::Sender<GattWrite>>,
    notify: mpsc::Receiver<Vec<u8>>,
    session_end: DuplexStream,
    bridge: JoinHandle<BridgeExit>,
    tx: Packetizer,
    rx: Reassembler,
    mtu: u16,
    packet_size: usize,
}

impl Harness {
    fn start(mtu: u16) -> Self {
        Self::start_with(mtu, CONNECT_TIMEOUT)
    }

    fn start_with(mtu: u16, connect_timeout: Duration) -> Self {
        let (writes, writes_rx) = mpsc::channel(16);
        let (notify_tx, notify) = mpsc::channel(64);
        let (session_end, bridge_end) = tokio::io::duplex(64 * 1024);
        let bridge = tokio::spawn(
            BleBridge::new(writes_rx, notify_tx, bridge_end)
                .with_connect_timeout(connect_timeout)
                .run(),
        );
        Self {
            writes: Some(writes),
            notify,
            session_end,
            bridge,
            tx: Packetizer::new(),
            rx: Reassembler::new(),
            mtu,
            packet_size: 0,
        }
    }

    async fn write_raw(&self, data: Vec<u8>) {
        let write = GattWrite {
            data,
            mtu: self.mtu,
        };
        self.writes.as_ref().unwrap().send(write).await.unwrap();
    }

    /// Returns the packet size the bridge confirmed.
    async fn connect(&mut self, max_packet: u16) -> usize {
        self.write_raw(conn_request(max_packet)).await;
        let confirm = self.notify.recv().await.unwrap();
        assert_eq!(&confirm[..3], [0x81, 0, 1]);
        self.packet_size = u16::from_be_bytes([confirm[3], confirm[4]]) as usize;
        self.packet_size
    }

    async fn send_message(&mut self, message: &[u8]) {
        for packet in self.tx.packetize(message, self.packet_size) {
            self.write_raw(packet).await;
        }
    }

    async fn next_message(&mut self) -> Option<SocketMessage> {
        while let Some(packet) = self.notify.recv().await {
            assert!(packet.len() <= self.packet_size);
            if let Some(message) = self.rx.feed(&packet).unwrap() {
                return Some(socket::parse_message(&message).unwrap());
            }
        }
        None
    }

    async fn exit(self) -> BridgeExit {
        tokio::time::timeout(Duration::from_secs(5), self.bridge)
            .await
            .expect("bridge did not stop")
            .unwrap()
    }
}

async fn read_all(stream: &mut DuplexStream) -> Vec<u8> {
    let mut out = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut out))
        .await
        .expect("stream was not closed")
        .unwrap();
    out
}

#[tokio::test]
async fn phone_data_reaches_the_stream_and_disconnection_closes_it() {
    let mut h = Harness::start(512);
    assert_eq!(h.connect(509).await, 509);
    h.send_message(&introduction_message()).await;
    let payload: Vec<u8> = (0..3000u32).map(|i| i as u8).collect();
    h.send_message(&data_message(&payload)).await;
    h.send_message(&disconnection_message()).await;

    // Everything sent before the disconnection is delivered before the end of stream.
    assert_eq!(read_all(&mut h.session_end).await, payload);
    assert_eq!(h.exit().await, BridgeExit::PhoneDisconnected);
}

#[tokio::test]
async fn stream_frames_become_one_message_each_however_they_are_chunked() {
    let mut h = Harness::start(512);
    h.connect(509).await;
    let frames: Vec<Vec<u8>> = [10usize, 1200, 3]
        .iter()
        .map(|n| frame(&vec![7u8; *n]))
        .collect();
    let joined: Vec<u8> = frames.concat();
    // Awkward cuts: inside a length prefix, inside a body, and two frames at once.
    for part in [&joined[..2], &joined[2..700], &joined[700..]] {
        h.session_end.write_all(part).await.unwrap();
        h.session_end.flush().await.unwrap();
    }
    for expected in &frames {
        assert_eq!(
            h.next_message().await,
            Some(SocketMessage::Data(expected.clone()))
        );
    }
}

#[tokio::test]
async fn closing_the_stream_still_delivers_the_last_frame() {
    let mut h = Harness::start(512);
    h.connect(509).await;
    let last = frame(&[9u8; 2000]);
    h.session_end.write_all(&last).await.unwrap();
    h.session_end.shutdown().await.unwrap();

    assert_eq!(h.next_message().await, Some(SocketMessage::Data(last)));
    assert!(h.next_message().await.is_none(), "notify side must close");
    assert_eq!(h.exit().await, BridgeExit::StreamClosed);
}

#[tokio::test]
async fn the_link_mtu_caps_the_packet_size() {
    let mut h = Harness::start(23);
    // The phone would take 509, the link only fits 20.
    assert_eq!(h.connect(509).await, 20);
    let big = frame(&[1u8; 100]);
    h.session_end.write_all(&big).await.unwrap();
    assert_eq!(h.next_message().await, Some(SocketMessage::Data(big)));

    let mut h = Harness::start(0);
    assert_eq!(h.connect(100).await, 20);
}

#[tokio::test]
async fn no_connection_request_times_out() {
    let h = Harness::start_with(512, Duration::from_millis(50));
    assert_eq!(h.exit().await, BridgeExit::ConnectTimeout);
}

#[tokio::test]
async fn the_timeout_only_applies_before_the_handshake() {
    let mut h = Harness::start_with(512, Duration::from_millis(100));
    h.connect(509).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!h.bridge.is_finished());
}

#[tokio::test]
async fn a_silent_connected_peer_times_out() {
    let (writes, writes_rx) = mpsc::channel(16);
    let (notify_tx, _notify) = mpsc::channel(64);
    let (_session_end, bridge_end) = tokio::io::duplex(1024);
    let bridge = tokio::spawn(
        BleBridge::new(writes_rx, notify_tx, bridge_end)
            .with_first_frame_timeout(Duration::from_millis(50))
            .run(),
    );
    let request = GattWrite {
        data: conn_request(509),
        mtu: 512,
    };
    writes.send(request).await.unwrap();
    assert_eq!(bridge.await.unwrap(), BridgeExit::SilentPeer);
}

#[tokio::test]
async fn protocol_errors_end_the_session() {
    let mut h = Harness::start(512);
    h.connect(509).await;
    // Wrong packet counter.
    h.write_raw(vec![0x38 | 0x04, 0, 0, 0, 1]).await;
    assert_eq!(read_all(&mut h.session_end).await, Vec::<u8>::new());
    assert!(matches!(h.exit().await, BridgeExit::Protocol(_)));

    // A bogus frame length from the session.
    let mut h = Harness::start(512);
    h.connect(509).await;
    h.session_end.write_all(&[0xff; 4]).await.unwrap();
    assert!(matches!(h.exit().await, BridgeExit::Protocol(_)));
}

#[tokio::test]
async fn losing_the_gatt_side_ends_the_session() {
    let mut h = Harness::start(512);
    h.connect(509).await;
    h.writes = None;
    assert_eq!(read_all(&mut h.session_end).await, Vec::<u8>::new());
    assert_eq!(h.exit().await, BridgeExit::GattClosed);

    let mut h = Harness::start(512);
    h.connect(509).await;
    h.session_end.write_all(&frame(&[1u8; 64])).await.unwrap();
    drop(std::mem::replace(&mut h.notify, mpsc::channel(1).1));
    assert_eq!(h.exit().await, BridgeExit::GattClosed);
}

#[tokio::test]
async fn a_stalled_peer_cannot_deadlock_the_bridge() {
    // The session sends a lot while the phone sends a lot, and neither reads for a while.
    let mut h = Harness::start(512);
    h.connect(509).await;
    let (mut read_half, mut write_half) = tokio::io::split(std::mem::replace(
        &mut h.session_end,
        tokio::io::duplex(1).0,
    ));
    let session = tokio::spawn(async move {
        let body = vec![5u8; 200 * 1024];
        write_half.write_all(&frame(&body)).await.unwrap();
        let mut got = vec![0u8; 300 * 1024];
        read_half.read_exact(&mut got).await.unwrap();
        got
    });
    let payload = vec![6u8; 300 * 1024];
    h.send_message(&data_message(&payload)).await;
    let got = tokio::time::timeout(Duration::from_secs(10), session)
        .await
        .expect("deadlock")
        .unwrap();
    assert_eq!(got, payload);
}

// --- A whole Quick Share session through the bridge -------------------------------------

#[derive(Default)]
struct Stats {
    up: AtomicUsize,
    down: AtomicUsize,
}

struct Outcome {
    sent: Vec<u8>,
    received: Vec<u8>,
    up: usize,
    down: usize,
    last_down: Vec<u8>,
    exit: BridgeExit,
}

const SMALL_FILE: usize = 100 * 1024;
const BIG_FILE: usize = 3 * 1024 * 1024;

/// Phone: drives `OutboundRequest` on `stream`, talking weave to the bridge only through
/// the two channels. The uplink sends whatever chunks it reads (not frame aligned).
fn spawn_phone(
    stream: DuplexStream,
    writes: mpsc::Sender<GattWrite>,
    mut notify: mpsc::Receiver<Vec<u8>>,
    mtu: u16,
    max_packet: u16,
    stats: Arc<Stats>,
    last_down: Arc<StdMutex<Vec<u8>>>,
) -> (JoinHandle<()>, JoinHandle<()>) {
    let (mut reader, mut writer) = tokio::io::split(stream);
    let (size_tx, size_rx) = oneshot::channel::<usize>();
    let (down_done_tx, down_done_rx) = oneshot::channel::<()>();
    let down_stats = stats.clone();

    let downlink = tokio::spawn(async move {
        let confirm = notify.recv().await.unwrap();
        assert_eq!(confirm[0], 0x81);
        let size = u16::from_be_bytes([confirm[3], confirm[4]]) as usize;
        size_tx.send(size).unwrap();
        let mut rx = Reassembler::new();
        while let Some(packet) = notify.recv().await {
            assert!(packet.len() <= size, "packet over the negotiated size");
            down_stats.down.fetch_add(packet.len(), Ordering::SeqCst);
            let Some(message) = rx.feed(&packet).unwrap() else {
                continue;
            };
            if let SocketMessage::Data(data) = socket::parse_message(&message).unwrap() {
                *last_down.lock().unwrap() = data.clone();
                // The sender may be gone already; keep draining so `last_down` is final.
                let _ = writer.write_all(&data).await;
            }
        }
        let _ = writer.shutdown().await;
        let _ = down_done_tx.send(());
    });

    let uplink = tokio::spawn(async move {
        let send = |data: Vec<u8>| GattWrite { data, mtu };
        writes.send(send(conn_request(max_packet))).await.unwrap();
        let size = size_rx.await.unwrap();
        let mut tx = Packetizer::new();
        let mut messages = vec![introduction_message()];
        let mut chunk = vec![0u8; 16 * 1024];
        loop {
            for message in messages.drain(..) {
                for packet in tx.packetize(&message, size) {
                    stats.up.fetch_add(packet.len(), Ordering::SeqCst);
                    if writes.send(send(packet)).await.is_err() {
                        return;
                    }
                }
            }
            match reader.read(&mut chunk).await {
                Ok(n) if n > 0 => messages.push(data_message(&chunk[..n])),
                _ => {
                    // Like a phone that has what it needs: say goodbye once the session
                    // has had its say (or after a moment, when it is waiting for us).
                    let _ = tokio::time::timeout(Duration::from_millis(300), down_done_rx).await;
                    messages.push(disconnection_message());
                    for message in messages.drain(..) {
                        for packet in tx.packetize(&message, size) {
                            let _ = writes.send(send(packet)).await;
                        }
                    }
                    return;
                }
            }
        }
    });
    (uplink, downlink)
}

async fn try_session(
    link: Option<Arc<FakeLink>>,
    file_size: usize,
    mtu: u16,
    max_packet: u16,
) -> Option<Outcome> {
    let _guard = SERIAL.lock().await;
    let dyn_link: Option<Arc<dyn WifiLink>> = link.map(|l| l as Arc<dyn WifiLink>);
    *crate::WIFI_LINK.write().unwrap() = dyn_link;

    let dir = std::env::temp_dir().join(format!("rqs_ble_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (src_dir, dst_dir) = (dir.join("src"), dir.join("dst"));
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::create_dir_all(&dst_dir).unwrap();
    *crate::CUSTOM_DOWNLOAD.write().unwrap() = Some(dst_dir.clone());

    let data: Vec<u8> = (0..file_size).map(|i| (i * 31 % 251) as u8).collect();
    let src = src_dir.join("payload.bin");
    std::fs::write(&src, &data).unwrap();

    // Receiver: InboundRequest <-> duplex <-> bridge.
    let (inbound_end, bridge_end) = tokio::io::duplex(64 * 1024);
    let (writes_tx, writes_rx) = mpsc::channel(16);
    let (notify_tx, notify_rx) = mpsc::channel(64);
    let bridge = tokio::spawn(BleBridge::new(writes_rx, notify_tx, bridge_end).run());
    let (rx_bus, mut rx_events) = broadcast::channel::<ChannelMessage>(256);
    let inbound = InboundRequest::new_over(
        MigratableStream::Duplex(inbound_end),
        "ble-1".into(),
        rx_bus.clone(),
    );
    let rx_task = tokio::spawn(drive_inbound(inbound, rx_bus.clone()));

    // Sender and phone-side weave.
    let (app_end, phone_end) = tokio::io::duplex(64 * 1024);
    let stats = Arc::new(Stats::default());
    let last_down = Arc::new(StdMutex::new(Vec::new()));
    let (uplink, downlink) = spawn_phone(
        phone_end,
        writes_tx,
        notify_rx,
        mtu,
        max_packet,
        stats.clone(),
        last_down.clone(),
    );
    let (tx_bus, _tx_events) = broadcast::channel::<ChannelMessage>(256);
    let mut outbound = OutboundRequest::new_over(
        *b"SEND",
        MigratableStream::Duplex(app_end),
        "ble-tx".into(),
        tx_bus,
        OutboundPayload::Files(vec![src.to_string_lossy().into_owned()]),
        RemoteDeviceInfo {
            device_type: crate::DeviceType::Unknown,
            name: "test".into(),
        },
    );
    let tx_task = tokio::spawn(async move {
        outbound.send_connection_request().await?;
        outbound.send_ukey2_client_init().await?;
        while outbound.handle().await.is_ok() {}
        anyhow::Ok(())
    });

    let run = async {
        loop {
            let msg = rx_events.recv().await.unwrap();
            let Some(client) = msg.msg.as_client() else {
                continue;
            };
            match client.state {
                Some(TransferState::WaitingForUserConsent) => {
                    rx_bus
                        .send(ChannelMessage {
                            id: msg.id.clone(),
                            msg: channel::Message::Lib {
                                action: TransferAction::ConsentAccept,
                            },
                        })
                        .unwrap();
                }
                Some(TransferState::Finished) => break true,
                Some(TransferState::Disconnected | TransferState::Cancelled) => break false,
                _ => {}
            }
        }
    };
    let finished = tokio::time::timeout(Duration::from_secs(120), run)
        .await
        .expect("transfer did not finish");
    let _ = tokio::time::timeout(Duration::from_secs(20), tx_task).await;
    let _ = tokio::time::timeout(Duration::from_secs(20), rx_task).await;
    let _ = tokio::time::timeout(Duration::from_secs(20), uplink).await;
    let _ = tokio::time::timeout(Duration::from_secs(20), downlink).await;
    let exit = tokio::time::timeout(Duration::from_secs(20), bridge)
        .await
        .expect("bridge did not stop")
        .unwrap();

    let received = std::fs::read(dst_dir.join("payload.bin")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    *crate::WIFI_LINK.write().unwrap() = None;
    let last_down = last_down.lock().unwrap().clone();
    finished.then(|| Outcome {
        sent: data,
        received,
        up: stats.up.load(Ordering::SeqCst),
        down: stats.down.load(Ordering::SeqCst),
        last_down,
        exit,
    })
}

/// The lib mis-encodes public keys with a leading zero byte in about 1 in 70 sessions
/// (see upgrade/tests.rs); such a session is repeated.
async fn session(
    link: Option<Arc<FakeLink>>,
    file_size: usize,
    mtu: u16,
    max_packet: u16,
) -> Outcome {
    for _ in 0..5 {
        if let Some(outcome) = try_session(link.clone(), file_size, mtu, max_packet).await {
            return outcome;
        }
    }
    panic!("session kept aborting");
}

fn closed_normally(exit: &BridgeExit) -> bool {
    matches!(
        exit,
        BridgeExit::PhoneDisconnected | BridgeExit::StreamClosed
    )
}

async fn plain_session(file_size: usize, mtu: u16, max_packet: u16) {
    let out = session(None, file_size, mtu, max_packet).await;
    assert_eq!(out.sent, out.received);
    assert!(out.up >= file_size, "payload must cross the BLE link");
    assert!(closed_normally(&out.exit), "{:?}", out.exit);
}

async fn upgraded_session(file_size: usize, mtu: u16, max_packet: u16) {
    let link = Arc::new(FakeLink::default());
    let out = session(Some(link.clone()), file_size, mtu, max_packet).await;
    assert_eq!(out.sent, out.received);
    assert_eq!(link.hosts.load(Ordering::SeqCst), 1);
    assert_eq!(link.joins.load(Ordering::SeqCst), 1);
    // Only the handshake and the hand-over crossed the BLE link, not the file.
    assert!(out.up < file_size / 4, "{} bytes up", out.up);
    assert!(out.down < 64 * 1024, "{} bytes down", out.down);
    // The last thing the phone got is the plaintext DISCONNECTION, then the link closed.
    let expected = frame(&upgrade::plain_disconnection_frame().encode_to_vec());
    assert!(
        out.last_down == expected,
        "last message to the phone: {} bytes, starts {:02x?}",
        out.last_down.len(),
        &out.last_down[..out.last_down.len().min(16)]
    );
    assert!(closed_normally(&out.exit), "{:?}", out.exit);
}

#[tokio::test(flavor = "multi_thread")]
async fn small_file_with_509_byte_packets() {
    plain_session(SMALL_FILE, 512, 509).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn small_file_with_20_byte_packets() {
    plain_session(SMALL_FILE, 23, 20).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn small_file_when_the_link_mtu_is_below_the_phones_request() {
    plain_session(SMALL_FILE, 185, 509).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn hotspot_upgrade_with_509_byte_packets() {
    upgraded_session(BIG_FILE, 512, 509).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn hotspot_upgrade_with_20_byte_packets() {
    upgraded_session(BIG_FILE, 23, 20).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_hotspot_falls_back_to_the_ble_link() {
    let link = Arc::new(FakeLink {
        join_fails: true,
        ..Default::default()
    });
    let out = session(Some(link.clone()), SMALL_FILE, 512, 509).await;
    assert_eq!(out.sent, out.received);
    assert!(out.up >= SMALL_FILE);
    assert!(closed_normally(&out.exit), "{:?}", out.exit);
}
